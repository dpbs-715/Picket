import { describe, expect, it } from 'vitest';
import { decodeCommand } from '../../../shared/pi-protocol';
import {
  decodeDiscoverRunCommandsReply,
  decodeRunCommandConfig,
  makeRunProcessId,
  normalizeRunCwd,
  normalizeRunGroup,
  sanitizeRunCommandConfig,
  type RunProcessSnapshot,
} from '../../../shared/run-protocol';
import {
  applyClearToBucket,
  applyLogBatchToBucket,
  mergeLoadedLogBucket,
} from './RunLogsWindow';
import { isNewerProcessSnapshot } from '../workbench/hooks/useRunProcesses';
import { decodeWorkbenchRestoreReply } from '../../../shared/workbench-persistence-protocol';
import {
  buildDiscoverRunCommandsPrompts,
  parseDiscoveredRunCommands,
} from '../../../agent/src/tasks/discover-run';
import { serializeWorkbenchPersistence } from '../workbench/state/persistence';
import { emptyWorkbench, runCommandsFor, runSharedEnvFor, workbenchReducer } from '../workbench/state/workbench';
import { appendRunEnvPreset, formatRunEnvEntries, parseRunEnvText } from './runEnv';
import {
  formatElapsedDuration,
  formatLocalPortChip,
  inferRunCommandIcon,
  resolveRunCommandIcon,
} from './runIcon';

describe('run-protocol and workbench run commands state', () => {
  it('validates cwd boundaries, env keys, and sanitizes run command configs', () => {
    expect(normalizeRunCwd(undefined)).toBeUndefined();
    expect(normalizeRunCwd('.')).toBeUndefined();
    expect(normalizeRunCwd('./apps/web/')).toBe('apps/web');
    expect(() => normalizeRunCwd('/etc')).toThrow();
    expect(() => normalizeRunCwd('../escape')).toThrow();
    expect(() => normalizeRunCwd('.git/hooks')).toThrow();

    const decoded = decodeRunCommandConfig({
      id: 'run-dev',
      label: '  Desktop Dev  ',
      command: 'pnpm desktop dev --no-watch\r\n',
      cwd: './apps/desktop/',
      group: '  Spring Boot  ',
      env: [{ key: 'PORT', value: '1420' }],
    });
    expect(decoded).toEqual({
      id: 'run-dev',
      label: 'Desktop Dev',
      command: 'pnpm desktop dev --no-watch',
      cwd: 'apps/desktop',
      group: 'Spring Boot',
      env: [{ key: 'PORT', value: '1420' }],
    });
    expect(normalizeRunGroup('  Maven  ')).toBe('Maven');
    expect(normalizeRunGroup('   ')).toBeUndefined();
    expect(() => normalizeRunGroup('Bad\nGroup')).toThrow();
    expect(makeRunProcessId('wt-1', 'run-dev')).toBe('wt-1::run-dev');

    const withPreset = appendRunEnvPreset('SPRING_PROFILES_ACTIVE=dev', 'JAVA_HOME', '');
    expect(withPreset).toBe('SPRING_PROFILES_ACTIVE=dev\nJAVA_HOME=');
    expect(appendRunEnvPreset(withPreset, 'JAVA_HOME', '/ignored')).toBe(withPreset);
    const parsedEnv = parseRunEnvText('# comment\nJAVA_HOME=/opt/jbr-25\nSPRING_PROFILES_ACTIVE=dev');
    expect(parsedEnv).toEqual({
      ok: true,
      entries: [
        { key: 'JAVA_HOME', value: '/opt/jbr-25' },
        { key: 'SPRING_PROFILES_ACTIVE', value: 'dev' },
      ],
    });
    expect(formatRunEnvEntries(parsedEnv.ok ? parsedEnv.entries : [])).toBe(
      'JAVA_HOME=/opt/jbr-25\nSPRING_PROFILES_ACTIVE=dev',
    );
  });

  it('saves, deduplicates discovered additions, preserves across repo edits, and serializes runCommands per repo', () => {
    let state = emptyWorkbench();
    state = workbenchReducer(state, {
      type: 'saveRunCommand',
      repositoryId: 'repo-picket',
      command: sanitizeRunCommandConfig({
        id: 'run-1',
        label: 'Desktop Dev',
        command: 'pnpm desktop dev --no-watch',
      }),
    });
    expect(runCommandsFor(state, 'repo-picket')).toHaveLength(1);

    // Adding discovered commands deduplicates identical command+cwd entries:
    state = workbenchReducer(state, {
      type: 'addRunCommands',
      repositoryId: 'repo-picket',
      commands: [
        { id: 'run-dup', label: 'Duplicate Dev', command: 'pnpm desktop dev --no-watch' },
        { id: 'run-2', label: 'Web Preview', command: 'pnpm dev', cwd: 'web' },
      ],
    });
    expect(runCommandsFor(state, 'repo-picket').map((c) => c.id)).toEqual(['run-1', 'run-2']);

    // Changing repository editor/commitLanguage preserves runCommands and runSharedEnv:
    state = workbenchReducer(state, {
      type: 'setRunSharedEnv',
      repositoryId: 'repo-picket',
      env: [
        { key: 'JAVA_HOME', value: '/Library/Java/jbr-25' },
        { key: 'SPRING_PROFILES_ACTIVE', value: 'dev' },
      ],
    });
    state = workbenchReducer(state, {
      type: 'editor',
      repositoryId: 'repo-picket',
      editor: 'rustrover',
    });
    state = workbenchReducer(state, {
      type: 'commitLanguage',
      repositoryId: 'repo-picket',
      commitLanguage: 'zh',
    });
    expect(runCommandsFor(state, 'repo-picket')).toHaveLength(2);
    expect(runSharedEnvFor(state, 'repo-picket')).toEqual([
      { key: 'JAVA_HOME', value: '/Library/Java/jbr-25' },
      { key: 'SPRING_PROFILES_ACTIVE', value: 'dev' },
    ]);

    // Serialize & decode round-trip through workbench persistence protocol:
    const saved = serializeWorkbenchPersistence(state, 1);
    const decoded = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: 'en',
      theme: 'dark',
      editor: 'idea',
      defaultCommitLanguage: 'en',
      repositorySettings: saved.repositorySettings,
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
    });
    expect(decoded.repositorySettings?.[0]?.runCommands).toHaveLength(2);
    expect(decoded.repositorySettings?.[0]?.runSharedEnv).toEqual([
      { key: 'JAVA_HOME', value: '/Library/Java/jbr-25' },
      { key: 'SPRING_PROFILES_ACTIVE', value: 'dev' },
    ]);

    // Deleting removes only the targeted command:
    state = workbenchReducer(state, {
      type: 'deleteRunCommand',
      repositoryId: 'repo-picket',
      commandId: 'run-1',
    });
    expect(runCommandsFor(state, 'repo-picket').map((c) => c.id)).toEqual(['run-2']);
  });

  it('decodes discoverRunCommands protocol frame and parses model JSON output', () => {
    const cmd = decodeCommand(JSON.stringify({
      type: 'discoverRunCommands',
      requestId: 'disc-123',
      context: {
        repositoryId: 'repo-1',
        worktreeId: 'wt-1',
        sessionId: 's-1',
        scopeVersion: 'sv-1',
      },
      model: {
        provider: 'openai',
        modelId: 'gpt-4o-mini',
        baseUrl: 'https://api.openai.com/v1',
        apiKey: 'sk-test',
      },
      repositoryName: 'Picket',
      projectFiles: ['package.json', 'src-tauri/Cargo.toml'],
      existingCommands: ['pnpm check'],
      locale: 'zh',
    }));
    expect(cmd.type).toBe('discoverRunCommands');
    if (cmd.type !== 'discoverRunCommands') throw new Error('unexpected');
    const prompts = buildDiscoverRunCommandsPrompts(cmd);
    expect(prompts.systemPrompt).toContain('Simplified Chinese');
    expect(prompts.userPrompt).toContain('package.json');

    const parsed = parseDiscoveredRunCommands(JSON.stringify({
      summary: 'Identified 2 common commands',
      commands: [
        {
          label: 'Desktop Dev',
          command: 'pnpm desktop dev --no-watch',
          cwd: '.',
          description: 'Start desktop dev environment',
          category: 'dev',
        },
        {
          label: 'Frontend Preview',
          command: 'pnpm dev',
          cwd: 'web',
          description: 'Start frontend Vite server',
          category: 'dev',
        },
      ],
    }));
    expect(parsed.summary).toBe('Identified 2 common commands');
    expect(parsed.commands).toEqual([
      {
        label: 'Desktop Dev',
        command: 'pnpm desktop dev --no-watch',
        description: 'Start desktop dev environment',
        category: 'dev',
      },
      {
        label: 'Frontend Preview',
        command: 'pnpm dev',
        cwd: 'web',
        description: 'Start frontend Vite server',
        category: 'dev',
      },
    ]);
  });

  it('infers built-in runtime icons, honors manual overrides, and formats port chips and elapsed time', () => {
    expect(inferRunCommandIcon('pnpm desktop dev --no-watch')).toBe('desktop');
    expect(inferRunCommandIcon('pnpm dev')).toBe('pnpm');
    expect(inferRunCommandIcon('vite --port 5173')).toBe('vite');
    expect(inferRunCommandIcon('./mvnw spring-boot:run')).toBe('spring');
    expect(inferRunCommandIcon('java -jar app.jar')).toBe('java');
    expect(inferRunCommandIcon('cargo run --bin api')).toBe('rust');
    expect(inferRunCommandIcon('python3 -m http.server')).toBe('python');
    expect(inferRunCommandIcon('go run ./cmd/server')).toBe('go');
    expect(inferRunCommandIcon('docker compose up')).toBe('docker');
    expect(inferRunCommandIcon('./scripts/start.sh')).toBe('terminal');

    expect(resolveRunCommandIcon('java', 'pnpm dev')).toBe('java');
    expect(resolveRunCommandIcon('auto', 'cargo run')).toBe('rust');
    expect(resolveRunCommandIcon('invalid-icon', 'node index.js')).toBe('node');

    const decodedWithIcon = decodeRunCommandConfig({
      id: 'run-java',
      label: 'Orders API',
      command: './mvnw spring-boot:run',
      icon: 'java',
    });
    expect(decodedWithIcon.icon).toBe('java');

    expect(formatLocalPortChip('http://localhost:1420/')).toBe(':1420');
    expect(formatLocalPortChip('http://127.0.0.1:8080/health')).toBe(':8080');
    expect(formatElapsedDuration(10_000, 148_000)).toBe('02:18');
    expect(formatElapsedDuration(0, 3_665_000)).toBe('1:01:05');
  });

  it('rejects NUL bytes, orders process snapshots monotonically by generation/status, and merges log buckets cleanly across restart and clear', () => {
    expect(() => sanitizeRunCommandConfig({
      id: 'run-nul',
      label: 'Bad\0Label',
      command: 'echo ok',
    })).toThrow();
    expect(() => decodeDiscoverRunCommandsReply({
      protocolVersion: 1,
      kind: 'discoverRunCommands',
      requestId: 'disc-1',
      status: 'done',
      summary: 'ok',
      commands: [{
        label: 'Dev',
        command: 'pnpm\0dev',
        description: '',
        category: 'dev',
      }],
    })).toThrow();

    const baseSnap: RunProcessSnapshot = {
      processId: 'wt-1::cmd-1',
      generation: 1,
      repositoryId: 'repo-1',
      repositoryName: 'demo',
      worktreeId: 'wt-1',
      worktreeLabel: 'main',
      commandId: 'cmd-1',
      label: 'Dev',
      command: 'pnpm dev',
      cwd: '.',
      status: 'running',
      startedAtMs: 1000,
      detectedUrls: [],
      logLineCount: 5,
    };
    // Older status ('starting') of same generation cannot overwrite 'running':
    expect(isNewerProcessSnapshot({ ...baseSnap, status: 'starting' }, baseSnap)).toBe(false);
    // Terminal 'exited' of same generation supersedes 'running':
    expect(isNewerProcessSnapshot({ ...baseSnap, status: 'exited' }, baseSnap)).toBe(true);
    // Newer generation supersedes exited older generation:
    expect(
      isNewerProcessSnapshot(
        { ...baseSnap, generation: 2, status: 'starting', startedAtMs: 2000 },
        { ...baseSnap, status: 'exited' },
      ),
    ).toBe(true);

    // Log bucket merges initial load with newer live lines without dropping them:
    const liveEarly = applyLogBatchToBucket(undefined, 1, [
      { seq: 3, stream: 'stdout', text: 'live-3', timestampMs: 3 },
      { seq: 4, stream: 'stdout', text: 'live-4', timestampMs: 4 },
    ]);
    const merged = mergeLoadedLogBucket(liveEarly, 1, [
      { seq: 1, stream: 'stdout', text: 'init-1', timestampMs: 1 },
      { seq: 2, stream: 'stdout', text: 'init-2', timestampMs: 2 },
    ]);
    expect(merged.lines.map((l) => l.seq)).toEqual([1, 2, 3, 4]);

    // Clear sets watermark and retains lines arriving after watermark:
    const withLine5 = applyLogBatchToBucket(merged, 1, [
      { seq: 5, stream: 'stdout', text: 'live-5', timestampMs: 5 },
    ]);
    const afterClear = applyClearToBucket(withLine5, 1, 4);
    expect(afterClear.clearedSeq).toBe(4);
    expect(afterClear.lines.map((l) => l.seq)).toEqual([5]);

    // Restart onto generation 2 resets bucket even when seq restarts at 1:
    const gen2 = applyLogBatchToBucket(afterClear, 2, [
      { seq: 1, stream: 'stdout', text: 'gen2-1', timestampMs: 10 },
    ]);
    expect(gen2?.generation).toBe(2);
    expect(gen2?.clearedSeq).toBe(0);
    expect(gen2?.lines.map((l) => l.text)).toEqual(['gen2-1']);

    // Stale generation-1 batch is ignored once generation 2 is active:
    const ignoredStale = applyLogBatchToBucket(gen2, 1, [
      { seq: 99, stream: 'stdout', text: 'stale-gen1', timestampMs: 99 },
    ]);
    expect(ignoredStale).toBe(gen2);
  });
});
