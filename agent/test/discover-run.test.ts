import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { mkdtempSync, rmSync } from 'node:fs';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { afterEach, describe, expect, it } from 'vitest';
import { decodeCommand, type PiEvent, type PiProviderSpec } from '../../shared/pi-protocol';
import { buildDiscoverRunCommandsPrompts, parseDiscoveredRunCommands } from '../src/tasks/discover-run';

const ROOT = process.cwd();
const DEADLINE_MS = 10_000;

interface SidecarHarness {
  child: ChildProcessWithoutNullStreams;
  events: PiEvent[];
  stderr: () => string;
  send(command: unknown): void;
  waitFor(predicate: (event: PiEvent) => boolean): Promise<PiEvent>;
  close(): Promise<number | null>;
}

const activeSidecars: SidecarHarness[] = [];
const activeServers: Server[] = [];

function spawnSidecar(): SidecarHarness {
  const directory = mkdtempSync(`${tmpdir()}/picket-discover-run-`);
  const child = spawn(
    resolve(ROOT, 'src-tauri/binaries/picket-node-aarch64-apple-darwin'),
    [resolve(ROOT, 'src-tauri/resources/generated/pi-sidecar.mjs')],
    {
      cwd: directory,
      env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin' },
      stdio: ['pipe', 'pipe', 'pipe'],
    },
  );
  const bus = new EventEmitter();
  const events: PiEvent[] = [];
  let stderr = '';
  child.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
  const reader = createInterface({ input: child.stdout });
  reader.on('line', (line) => {
    const event = JSON.parse(line) as PiEvent;
    events.push(event);
    bus.emit('event', event);
  });
  const exited = new Promise<number | null>((yes, no) => {
    child.once('error', no);
    child.once('exit', yes);
  });
  const harness: SidecarHarness = {
    child,
    events,
    stderr: () => stderr,
    send(command: unknown) { child.stdin.write(`${JSON.stringify(command)}\n`); },
    async waitFor(predicate: (event: PiEvent) => boolean): Promise<PiEvent> {
      const existing = events.find(predicate);
      if (existing) return existing;
      return new Promise((yes, no) => {
        const receive = (event: PiEvent) => {
          if (predicate(event)) {
            clearTimeout(timer);
            bus.off('event', receive);
            yes(event);
          }
        };
        const timer = setTimeout(() => {
          bus.off('event', receive);
          no(new Error(`Sidecar event timed out. Events=${JSON.stringify(events)} Stderr=${stderr}`));
        }, DEADLINE_MS);
        bus.on('event', receive);
      });
    },
    async close() {
      if (child.exitCode === null && child.signalCode === null) child.stdin.end();
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        return await Promise.race([
          exited,
          new Promise<never>((_, no) => {
            timer = setTimeout(() => {
              child.kill('SIGKILL');
              no(new Error('Sidecar did not exit after stdin EOF'));
            }, DEADLINE_MS);
          }),
        ]);
      } finally {
        clearTimeout(timer);
        reader.close();
        rmSync(directory, { recursive: true, force: true });
      }
    },
  };
  activeSidecars.push(harness);
  return harness;
}

async function startServer(
  handler: (req: IncomingMessage, body: string, res: ServerResponse) => void | Promise<void>,
): Promise<{ url: string; close: () => Promise<void> }> {
  const server = createServer((req, res) => {
    let body = '';
    req.setEncoding('utf8');
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => { void handler(req, body, res); });
  });
  activeServers.push(server);
  await new Promise<void>((yes) => server.listen(0, '127.0.0.1', yes));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Expected TCP address');
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () => new Promise<void>((yes) => server.close(() => yes())),
  };
}

function writeOpenAiToolCallSse(
  res: ServerResponse,
  id: string,
  toolName: string,
  args: Record<string, unknown>,
) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
  res.write(`data: ${JSON.stringify({
    id,
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'test-model',
    choices: [{
      index: 0,
      delta: {
        role: 'assistant',
        tool_calls: [{
          index: 0,
          id: `${id}-tc`,
          type: 'function',
          function: { name: toolName, arguments: JSON.stringify(args) },
        }],
      },
      finish_reason: null,
    }],
  })}\n\n`);
  res.write(`data: ${JSON.stringify({
    id,
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'test-model',
    choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }],
  })}\n\n`);
  res.write('data: [DONE]\n\n');
  res.end();
}

function writeOpenAiCompletionsSse(res: ServerResponse, text: string) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
  res.write(`data: ${JSON.stringify({
    id: 'chatcmpl-discover',
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'test-model',
    choices: [{ index: 0, delta: { content: text }, finish_reason: null }],
  })}\n\n`);
  res.write(`data: ${JSON.stringify({
    id: 'chatcmpl-discover',
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'test-model',
    choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
    usage: { prompt_tokens: 15, completion_tokens: 25, total_tokens: 40 },
  })}\n\n`);
  res.write('data: [DONE]\n\n');
  res.end();
}

afterEach(async () => {
  for (const sidecar of activeSidecars.splice(0)) await sidecar.close();
  for (const server of activeServers.splice(0)) await new Promise<void>((yes) => server.close(() => yes()));
});

describe('discoverRunCommands task', () => {
  it('builds localized prompts, parses fenced/reasoning JSON, and decodes wire commands', () => {
    const prompts = buildDiscoverRunCommandsPrompts({
      type: 'discoverRunCommands',
      requestId: 'req-discover-1',
      context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
      model: { provider: 'deepseek', modelId: 'deepseek-chat', baseUrl: 'https://api.deepseek.com/v1', apiKey: 'sk-1' },
      repositoryName: 'inspur-scm-service',
      projectFiles: ['pom.xml', 'mvnw'],
      workspaceModules: [
        'maven-root: path=. artifact=inspur-scm-service wrapper=./mvnw totalModules=4 runnableServices=2 libraryModules=1',
        'spring-boot-service: module=scm-modules/scm-business-starter artifact=inspur-scm-business-starter mainClass=BusinessServerApplication port=8082',
      ],
      existingCommands: ['pnpm dev'],
      locale: 'zh',
    });
    expect(prompts.systemPrompt).toContain('Simplified Chinese');
    expect(prompts.systemPrompt).toContain('MULTI-MODULE WORKSPACE DETECTED');
    expect(prompts.systemPrompt).toContain('NOT just a single install command');
    expect(prompts.userPrompt).toContain('pom.xml');

    const singlePkgPrompts = buildDiscoverRunCommandsPrompts({
      type: 'discoverRunCommands',
      requestId: 'req-discover-fe',
      context: { repositoryId: 'r2', worktreeId: 'w2', sessionId: 's2', scopeVersion: 'v1' },
      model: { provider: 'deepseek', modelId: 'deepseek-chat', baseUrl: 'https://api.deepseek.com/v1', apiKey: 'sk-1' },
      repositoryName: 'inspur-scm-static',
      projectFiles: ['package.json'],
      existingCommands: [],
      locale: 'zh',
    });
    expect(singlePkgPrompts.systemPrompt).toContain('INCLUDING ALL environment-specific variants');
    expect(singlePkgPrompts.systemPrompt).toContain('Do NOT artificially cap the command list');
    expect(prompts.userPrompt).toContain('BusinessServerApplication');
    expect(prompts.userPrompt).toContain('pnpm dev');

    const parsed = parseDiscoveredRunCommands(
      '<think>checking scripts</think>\n```json\n{"summary":"Detected 2 commands","commands":[{"label":"inspur-scm-service [install]","command":"./mvnw install -DskipTests","cwd":".","group":"Maven","icon":"java","description":"Install workspace dependencies skipping tests","category":"build"},{"label":"BusinessServerApplication","command":"./mvnw -pl scm-modules/scm-business-starter spring-boot:run","group":"Spring Boot","icon":"spring","description":"Start business microservice on port 8082","category":"dev"},{"label":"","command":"skip"}]}\n```',
    );
    expect(parsed.summary).toBe('Detected 2 commands');
    expect(parsed.commands).toEqual([
      {
        label: 'inspur-scm-service [install]',
        command: './mvnw install -DskipTests',
        group: 'Maven',
        icon: 'java',
        description: 'Install workspace dependencies skipping tests',
        category: 'build',
      },
      {
        label: 'BusinessServerApplication',
        command: './mvnw -pl scm-modules/scm-business-starter spring-boot:run',
        group: 'Spring Boot',
        icon: 'spring',
        description: 'Start business microservice on port 8082',
        category: 'dev',
      },
    ]);

    const decoded = decodeCommand(JSON.stringify({
      type: 'discoverRunCommands',
      requestId: 'req-wire',
      context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
      model: { provider: 'openai', modelId: 'gpt-4.1', baseUrl: 'http://127.0.0.1:9999/v1', apiKey: 'sk-x' },
      repositoryName: 'Demo',
      projectFiles: ['pom.xml'],
      workspaceModules: ['maven-root: path=. artifact=demo wrapper=mvn totalModules=2 runnableServices=1 libraryModules=0'],
      existingCommands: [],
      locale: 'en',
    }));
    expect(decoded.type).toBe('discoverRunCommands');
    if (decoded.type === 'discoverRunCommands') {
      expect(decoded.workspaceModules).toHaveLength(1);
    }
  });

  it('runs read-only read_repo_file tool roundtrip and emits runCommandsDiscovered in bundled sidecar', async () => {
    let turn = 0;
    const server = await startServer((_req, _body, res) => {
      turn += 1;
      if (turn === 1) {
        writeOpenAiToolCallSse(res, 'call-manifest', 'read_repo_file', { path: 'package.json' });
        return;
      }
      writeOpenAiCompletionsSse(res, JSON.stringify({
        summary: 'Found 2 local dev commands.',
        commands: [
          {
            label: 'Desktop Dev',
            command: 'pnpm desktop dev --no-watch',
            description: 'Start Tauri desktop dev mode without file watcher.',
            category: 'dev',
          },
          {
            label: 'Frontend Dev',
            command: 'pnpm dev',
            cwd: 'apps/web',
            description: 'Start Vite frontend dev server.',
            category: 'dev',
          },
        ],
      }));
    });

    const sidecar = spawnSidecar();
    await sidecar.waitFor((e) => e.type === 'ready');

    const model: PiProviderSpec = {
      provider: 'deepseek',
      modelId: 'test-model',
      baseUrl: `${server.url}/v1`,
      apiKey: 'sk-mock',
    };
    const context = { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'sv-1' };
    sidecar.send({
      type: 'discoverRunCommands',
      requestId: 'req-disc-e2e',
      context,
      model,
      repositoryName: 'Picket',
      projectFiles: ['package.json'],
      existingCommands: [],
      locale: 'en',
    });

    const toolCall = await sidecar.waitFor((e) => e.type === 'toolCall');
    if (toolCall.type !== 'toolCall') throw new Error('Expected toolCall');
    expect(toolCall.toolName).toBe('read_repo_file');
    expect(toolCall.args).toEqual({ path: 'package.json' });

    sidecar.send({
      type: 'toolResult',
      requestId: 'req-disc-e2e',
      callId: toolCall.callId,
      ok: true,
      content: '{"scripts":{"dev":"vite","desktop":"node scripts/desktop.mjs"}}',
    });

    const discovered = await sidecar.waitFor((e) => e.type === 'runCommandsDiscovered');
    if (discovered.type !== 'runCommandsDiscovered') throw new Error('Expected runCommandsDiscovered');
    expect(discovered.summary).toBe('Found 2 local dev commands.');
    expect(discovered.commands).toHaveLength(2);
    expect(discovered.commands[0]?.command).toBe('pnpm desktop dev --no-watch');
    expect(discovered.commands[1]?.cwd).toBe('apps/web');
  });
});
