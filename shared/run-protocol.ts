import type {
  ClearRunLogsRequest,
  DiscoverRunCommandsProgressEvent,
  DiscoverRunCommandsReply,
  DiscoverRunCommandsRequest,
  DiscoveredRunCommand,
  GetRunLogsRequest,
  OpenRunLocalUrlRequest,
  OpenRunLogsWindowRequest,
  PersistedRunCommand,
  RestartRunCommandRequest,
  RunEnvEntry,
  RunLogBatchEvent,
  RunLogLine,
  RunLogStream,
  RunProcessLogsReply,
  RunProcessSnapshot,
  RunProcessStatus,
  StartRunCommandRequest,
  StopRunCommandRequest,
  UserEnvSummary,
} from './generated';

export const RUN_PROTOCOL_VERSION = 1;
export const RUN_STATE_EVENT = 'picket:run-state';
export const RUN_LOG_BATCH_EVENT = 'picket:run-log-batch';
export const RUN_LOGS_SELECT_EVENT = 'picket:run-logs-select';
export const RUN_DISCOVER_PROGRESS_EVENT = 'picket:run-discover-progress';

export const RUN_COMMAND_LIMITS = {
  maxPerRepo: 64,
  maxIdChars: 64,
  maxLabelChars: 48,
  maxGroupChars: 48,
  maxCommandChars: 1000,
  maxCwdChars: 256,
  maxEnvEntries: 32,
  maxEnvKeyChars: 128,
  maxEnvValueChars: 2000,
} as const;

export const RUN_COMMAND_ICON_IDS = [
  'node',
  'pnpm',
  'vite',
  'java',
  'spring',
  'rust',
  'python',
  'go',
  'docker',
  'desktop',
  'database',
  'terminal',
] as const;

export type RunCommandIconId = typeof RUN_COMMAND_ICON_IDS[number];

export function normalizeRunIcon(raw?: string | null): RunCommandIconId | undefined {
  if (!raw) return undefined;
  const trimmed = raw.trim().toLowerCase();
  if (!trimmed || trimmed === 'auto') return undefined;
  return (RUN_COMMAND_ICON_IDS as readonly string[]).includes(trimmed)
    ? (trimmed as RunCommandIconId)
    : undefined;
}

export function normalizeRunGroup(raw?: string | null): string | undefined {
  if (!raw) return undefined;
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  if (
    trimmed.includes('\0')
    || trimmed.includes('\r')
    || trimmed.includes('\n')
    || trimmed.length > RUN_COMMAND_LIMITS.maxGroupChars
  ) {
    throw new Error(`Invalid run command group: ${raw}`);
  }
  return trimmed;
}

export function sanitizeRunEnvEntries(raw?: readonly RunEnvEntry[] | null): RunEnvEntry[] {
  const seenKeys = new Set<string>();
  const env: RunEnvEntry[] = [];
  for (const entry of raw ?? []) {
    const key = entry.key.trim();
    if (!isValidRunEnvKey(key) || entry.value.includes('\0') || entry.value.length > RUN_COMMAND_LIMITS.maxEnvValueChars) {
      continue;
    }
    if (!seenKeys.has(key)) {
      seenKeys.add(key);
      env.push({ key, value: entry.value });
      if (env.length >= RUN_COMMAND_LIMITS.maxEnvEntries) break;
    }
  }
  return env;
}

export function decodeRunEnvEntries(value: unknown): RunEnvEntry[] {
  if (!Array.isArray(value)) {
    throw new Error('Invalid run environment list.');
  }
  if (value.length > RUN_COMMAND_LIMITS.maxEnvEntries) {
    throw new Error('Too many run environment entries.');
  }
  const seenKeys = new Set<string>();
  const env: RunEnvEntry[] = [];
  for (const item of value) {
    if (!isRecord(item)
      || typeof item.key !== 'string' || !isValidRunEnvKey(item.key)
      || typeof item.value !== 'string' || item.value.includes('\0') || item.value.length > RUN_COMMAND_LIMITS.maxEnvValueChars) {
      throw new Error('Invalid run environment entry.');
    }
    const key = item.key.trim();
    if (seenKeys.has(key)) {
      throw new Error(`Duplicate run environment key: ${key}`);
    }
    seenKeys.add(key);
    env.push({ key, value: item.value });
  }
  return env;
}

export type RunCommandConfig = PersistedRunCommand;

export type {
  ClearRunLogsRequest,
  DiscoverRunCommandsProgressEvent,
  DiscoverRunCommandsReply,
  DiscoverRunCommandsRequest,
  DiscoveredRunCommand,
  GetRunLogsRequest,
  OpenRunLocalUrlRequest,
  OpenRunLogsWindowRequest,
  PersistedRunCommand,
  RestartRunCommandRequest,
  RunEnvEntry,
  RunLogBatchEvent,
  RunLogLine,
  RunLogStream,
  RunProcessLogsReply,
  RunProcessSnapshot,
  RunProcessStatus,
  StartRunCommandRequest,
  StopRunCommandRequest,
  UserEnvSummary,
};

export function makeRunProcessId(worktreeId: string, commandId: string): string {
  return `${worktreeId}::${commandId}`;
}

export function isRunProcessActive(status: RunProcessStatus | undefined): boolean {
  return status === 'starting' || status === 'running' || status === 'stopping';
}

export function normalizeRunCwd(raw?: string | null): string | undefined {
  if (!raw) return undefined;
  const rawTrimmed = raw.trim();
  if (!rawTrimmed || rawTrimmed === '.') return undefined;
  if (
    rawTrimmed.startsWith('/')
    || rawTrimmed.startsWith('~')
    || rawTrimmed.includes('\0')
    || rawTrimmed.includes('\\')
  ) {
    throw new Error(`Invalid relative working directory: ${raw}`);
  }
  const stripped = rawTrimmed.replace(/^\.\//, '').replace(/\/+$/, '');
  if (!stripped || stripped === '.') return undefined;
  if (stripped.length > RUN_COMMAND_LIMITS.maxCwdChars) {
    throw new Error(`Working directory exceeds limit: ${raw}`);
  }
  const segments = stripped.split('/');
  for (const segment of segments) {
    if (!segment || segment === '.' || segment === '..' || segment.toLowerCase() === '.git') {
      throw new Error(`Invalid relative working directory: ${raw}`);
    }
  }
  return segments.join('/');
}

export function isValidRunEnvKey(key: string): boolean {
  const trimmed = key.trim();
  if (!trimmed || trimmed.length > RUN_COMMAND_LIMITS.maxEnvKeyChars) return false;
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(trimmed);
}

export function sanitizeRunCommandConfig(raw: RunCommandConfig): RunCommandConfig {
  if (raw.id.includes('\0') || raw.label.includes('\0') || raw.command.includes('\0')) {
    throw new Error('Run command fields must not contain NUL bytes.');
  }
  const id = raw.id.trim().slice(0, RUN_COMMAND_LIMITS.maxIdChars);
  const label = raw.label.trim().slice(0, RUN_COMMAND_LIMITS.maxLabelChars);
  const command = raw.command.replace(/\r\n/g, '\n').trim().slice(0, RUN_COMMAND_LIMITS.maxCommandChars);
  const cwd = normalizeRunCwd(raw.cwd);
  const group = normalizeRunGroup(raw.group);
  const icon = normalizeRunIcon(raw.icon);
  const env = sanitizeRunEnvEntries(raw.env);
  return {
    id,
    label,
    command,
    ...(cwd ? { cwd } : {}),
    ...(group ? { group } : {}),
    ...(icon ? { icon } : {}),
    ...(env.length > 0 ? { env } : {}),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function decodeRunCommandConfig(value: unknown): RunCommandConfig {
  if (!isRecord(value)
    || typeof value.id !== 'string' || !value.id.trim() || value.id.trim().length > RUN_COMMAND_LIMITS.maxIdChars || value.id.includes('\0')
    || typeof value.label !== 'string' || !value.label.trim() || value.label.trim().length > RUN_COMMAND_LIMITS.maxLabelChars || value.label.includes('\0')
    || typeof value.command !== 'string' || !value.command.trim() || value.command.trim().length > RUN_COMMAND_LIMITS.maxCommandChars || value.command.includes('\0')
    || !(value.cwd === undefined || value.cwd === null || typeof value.cwd === 'string')
    || !(value.group === undefined || value.group === null || typeof value.group === 'string')
    || !(value.icon === undefined || value.icon === null || typeof value.icon === 'string')
    || !(value.env === undefined || value.env === null || Array.isArray(value.env))) {
    throw new Error('Invalid run command configuration.');
  }
  const cwd = normalizeRunCwd(typeof value.cwd === 'string' ? value.cwd : undefined);
  const group = normalizeRunGroup(typeof value.group === 'string' ? value.group : undefined);
  const icon = normalizeRunIcon(typeof value.icon === 'string' ? value.icon : undefined);
  const env: RunEnvEntry[] = [];
  if (Array.isArray(value.env)) {
    if (value.env.length > RUN_COMMAND_LIMITS.maxEnvEntries) {
      throw new Error('Too many run command environment entries.');
    }
    for (const item of value.env) {
      if (!isRecord(item)
        || typeof item.key !== 'string' || !isValidRunEnvKey(item.key)
        || typeof item.value !== 'string' || item.value.includes('\0') || item.value.length > RUN_COMMAND_LIMITS.maxEnvValueChars) {
        throw new Error('Invalid run command environment entry.');
      }
      env.push({ key: item.key.trim(), value: item.value });
    }
  }
  return sanitizeRunCommandConfig({
    id: value.id,
    label: value.label,
    command: value.command,
    ...(cwd ? { cwd } : {}),
    ...(group ? { group } : {}),
    ...(icon ? { icon } : {}),
    ...(env.length > 0 ? { env } : {}),
  });
}

export function decodeDiscoverRunCommandsReply(value: unknown): DiscoverRunCommandsReply {
  if (!isRecord(value)
    || value.protocolVersion !== RUN_PROTOCOL_VERSION
    || value.kind !== 'discoverRunCommands'
    || typeof value.requestId !== 'string' || !value.requestId
    || (value.status !== 'done' && value.status !== 'cancelled')
    || typeof value.summary !== 'string'
    || !Array.isArray(value.commands)) {
    throw new Error('Discover run commands reply protocol mismatch.');
  }
  const commands: DiscoveredRunCommand[] = value.commands.map((item) => {
    if (!isRecord(item)
      || typeof item.label !== 'string' || !item.label.trim() || item.label.includes('\0')
      || typeof item.command !== 'string' || !item.command.trim() || item.command.includes('\0')
      || typeof item.description !== 'string'
      || typeof item.category !== 'string') {
      throw new Error('Invalid discovered run command entry.');
    }
    const cwd = typeof item.cwd === 'string' && item.cwd.trim() ? normalizeRunCwd(item.cwd) : undefined;
    let group: string | undefined;
    if (typeof item.group === 'string' && item.group.trim()) {
      try {
        group = normalizeRunGroup(item.group);
      } catch {
        group = undefined;
      }
    }
    const icon = typeof item.icon === 'string' ? normalizeRunIcon(item.icon) : undefined;
    return {
      label: item.label.trim(),
      command: item.command.trim(),
      ...(cwd ? { cwd } : {}),
      ...(group ? { group } : {}),
      ...(icon ? { icon } : {}),
      description: item.description.trim(),
      category: item.category.trim() || 'dev',
    };
  });
  return {
    protocolVersion: RUN_PROTOCOL_VERSION,
    kind: 'discoverRunCommands',
    requestId: value.requestId,
    status: value.status,
    summary: value.summary,
    commands,
  };
}
