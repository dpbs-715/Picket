import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { afterEach, describe, expect, it } from 'vitest';
import { decodeCommand, type PiEvent } from '../../shared/pi-protocol';

const ROOT = process.cwd();
const DEADLINE_MS = 7_000;
const context = (id: string) => ({ repositoryId: 'foundation', worktreeId: 'temporary', sessionId: id, scopeVersion: 'foundation-v1' });
interface SidecarTestProcess {
  child: ChildProcessWithoutNullStreams;
  events: PiEvent[];
  exited: Promise<number | null>;
  send(command: unknown): void;
  waitFor(predicate: (event: PiEvent) => boolean): Promise<PiEvent>;
  close(): Promise<number | null>;
}
const probes: SidecarTestProcess[] = [];

function spawnSidecar(): SidecarTestProcess {
  const directory = mkdtempSync(`${tmpdir()}/picket-sidecar-test-`);
  const child = spawn(resolve(ROOT, 'src-tauri/binaries/picket-node-aarch64-apple-darwin'),
    [resolve(ROOT, 'src-tauri/resources/generated/pi-sidecar.mjs')], {
      cwd: directory, env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin' }, stdio: ['pipe', 'pipe', 'pipe'],
    });
  const bus = new EventEmitter();
  const events: PiEvent[] = [];
  let stderr = '';
  child.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
  const reader = createInterface({ input: child.stdout });
  reader.on('line', (line) => { const event = JSON.parse(line) as PiEvent; events.push(event); bus.emit('event', event); });
  const exited = new Promise<number | null>((yes, no) => { child.once('error', no); child.once('exit', yes); });
  const instance = {
    child, events, exited,
    send(command: unknown) { child.stdin.write(JSON.stringify(command) + '\n'); },
    async waitFor(predicate: (event: PiEvent) => boolean): Promise<PiEvent> {
      const existing = events.find(predicate);
      if (existing) return existing;
      return new Promise((yes, no) => {
        const receive = (event: PiEvent) => { if (predicate(event)) { clearTimeout(timer); bus.off('event', receive); yes(event); } };
        const timer = setTimeout(() => { bus.off('event', receive); no(new Error(`Sidecar event timed out: ${stderr}`)); }, DEADLINE_MS);
        bus.on('event', receive);
      });
    },
    async close() {
      if (child.exitCode === null && child.signalCode === null) child.stdin.end();
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        return await Promise.race([exited, new Promise<never>((_, no) => { timer = setTimeout(() => { child.kill('SIGKILL'); no(new Error('Sidecar did not exit after stdin EOF')); }, DEADLINE_MS); })]);
      } finally { clearTimeout(timer); reader.close(); rmSync(directory, { recursive: true, force: true }); }
    },
  };
  probes.push(instance);
  return instance;
}

afterEach(async () => { for (const probe of probes.splice(0)) await probe.close(); });

describe('bundled Pi process', () => {
  it('uses the real SDK and a bundled runtime without project cwd or Node on PATH', async () => {
    const probe = spawnSidecar();
    const ready = await probe.waitFor((event) => event.type === 'ready');
    expect(ready).toMatchObject({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: '24.17.0' });
    probe.send({ type: 'probe', requestId: 'complete', context: context('complete') });
    const completed = await probe.waitFor((event) => event.type === 'completed');
    expect(completed).toMatchObject({ requestId: 'complete', context: context('complete') });
    expect(probe.events.filter((event) => event.type === 'textDelta').length).toBeGreaterThan(1);
    expect(await probe.close()).toBe(0);
  });
  it('cancels an in-flight request and emits exactly one terminal event', async () => {
    const probe = spawnSidecar();
    await probe.waitFor((event) => event.type === 'ready');
    probe.send({ type: 'probe', requestId: 'cancel', context: context('cancel') });
    await probe.waitFor((event) => event.type === 'textDelta');
    probe.send({ type: 'cancel', requestId: 'cancel' });
    await probe.waitFor((event) => event.type === 'cancelled');
    expect(await probe.close()).toBe(0);
    expect(probe.events.filter((event) => ['completed', 'cancelled', 'failed'].includes(event.type))).toHaveLength(1);
  });
  it('preserves cancellation sent before Agent.prompt starts', async () => {
    const probe = spawnSidecar();
    await probe.waitFor((event) => event.type === 'ready');
    probe.child.stdin.write(JSON.stringify({ type: 'probe', requestId: 'early', context: context('early') })
      + '\n' + JSON.stringify({ type: 'cancel', requestId: 'early' }) + '\n');
    await probe.waitFor((event) => event.type === 'cancelled');
    expect(probe.events.some((event) => event.type === 'completed')).toBe(false);
  });
  it('parent EOF aborts active work and exits instead of leaving a child behind', async () => {
    const probe = spawnSidecar();
    await probe.waitFor((event) => event.type === 'ready');
    probe.send({ type: 'probe', requestId: 'parent-eof', context: context('parent-eof') });
    await probe.waitFor((event) => event.type === 'textDelta');
    expect(await probe.close()).toBe(0);
    expect(probe.events.some((event) => event.type === 'cancelled')).toBe(true);
  });
  it('rejects a different scope, unknown commands, invalid locales, and duplicate accepted identities', async () => {
    expect(() => decodeCommand(JSON.stringify({ type: 'probe', requestId: 'unsafe', context: { ...context('unsafe'), worktreeId: '/real/repository' } }))).toThrow('isolated');
    expect(() => decodeCommand(JSON.stringify({ type: 'shell', requestId: 'unsafe' }))).toThrow('Unknown');
    expect(() => decodeCommand(JSON.stringify({
      type: 'testConnection',
      requestId: 'bad-locale',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v1' },
      model: { provider: 'openai', modelId: 'gpt-4.1', baseUrl: 'https://api.openai.com/v1', apiKey: 'sk-test' },
      locale: 'fr',
    }))).toThrow('Invalid locale');
    const probe = spawnSidecar();
    await probe.waitFor((event) => event.type === 'ready');
    const command = { type: 'probe', requestId: 'duplicate', context: context('duplicate') };
    probe.send(command);
    probe.send(command);
    await probe.waitFor((event) => event.type === 'protocolError');
    await probe.close();
    expect(probe.events.filter((event) => event.type === 'started')).toHaveLength(1);
  });
});
