import {
  normalizeRunIcon,
  type RunCommandIconId,
} from '../../../shared/run-protocol';

export const RUN_ICON_IDS: readonly RunCommandIconId[] = [
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

export function inferRunCommandIcon(command: string, label = ''): RunCommandIconId {
  const lowerCmd = command.trim().toLowerCase();
  const lowerLabel = label.trim().toLowerCase();
  const combined = `${lowerCmd} ${lowerLabel}`;
  const firstToken = lowerCmd.split(/[\s&|;]+/)[0] ?? '';

  if (/\bspring[-_\s]?boot\b/.test(combined) || lowerLabel.includes('spring')) {
    return 'spring';
  }
  if (
    firstToken === 'mvn'
    || firstToken === './mvnw'
    || firstToken === 'gradle'
    || firstToken === './gradlew'
    || firstToken === 'java'
    || firstToken === 'javac'
    || /\b(mvnw|gradlew|spring-boot|tomcat|quarkus|micronaut)\b/.test(lowerCmd)
  ) {
    return 'java';
  }
  if (
    firstToken === 'docker'
    || firstToken === 'docker-compose'
    || firstToken === 'podman'
    || /\bdocker\s+compose\b/.test(lowerCmd)
  ) {
    return 'docker';
  }
  if (
    firstToken === 'psql'
    || firstToken === 'postgres'
    || firstToken === 'mysql'
    || firstToken === 'redis-server'
    || firstToken === 'mongod'
    || /\b(prisma\s+studio|drizzle-kit|pg_ctl|redis-server|mongod)\b/.test(lowerCmd)
  ) {
    return 'database';
  }
  if (
    firstToken === 'tauri'
    || firstToken === 'electron'
    || /\b(tauri|electron|desktop\s+dev)\b/.test(lowerCmd)
  ) {
    return 'desktop';
  }
  if (firstToken === 'cargo' || firstToken === 'rustc' || firstToken === 'rustup') {
    return 'rust';
  }
  if (
    firstToken === 'python'
    || firstToken === 'python3'
    || firstToken === 'uv'
    || firstToken === 'poetry'
    || firstToken === 'uvicorn'
    || firstToken === 'gunicorn'
    || firstToken === 'pytest'
    || firstToken === 'flask'
    || firstToken === 'fastapi'
    || /\bmanage\.py\b/.test(lowerCmd)
  ) {
    return 'python';
  }
  if (firstToken === 'go' || firstToken === 'air') {
    return 'go';
  }
  if (
    firstToken === 'vite'
    || firstToken === 'next'
    || firstToken === 'nuxt'
    || firstToken === 'astro'
    || firstToken === 'webpack'
    || /\b(vite|next\s+dev|nuxt\s+dev|astro\s+dev|storybook)\b/.test(lowerCmd)
  ) {
    return 'vite';
  }
  if (firstToken === 'pnpm') {
    return 'pnpm';
  }
  if (
    firstToken === 'node'
    || firstToken === 'npm'
    || firstToken === 'npx'
    || firstToken === 'yarn'
    || firstToken === 'bun'
    || firstToken === 'tsx'
    || firstToken === 'deno'
  ) {
    return 'node';
  }
  return 'terminal';
}

export function resolveRunCommandIcon(
  icon?: string | null,
  command = '',
  label = '',
): RunCommandIconId {
  return normalizeRunIcon(icon) ?? inferRunCommandIcon(command, label);
}

export function formatLocalPortChip(rawUrl: string): string {
  try {
    const parsed = new URL(rawUrl);
    if (parsed.port) {
      return `:${parsed.port}`;
    }
    return parsed.host;
  } catch {
    return rawUrl.replace(/^https?:\/\//i, '').replace(/\/$/, '');
  }
}

export function formatElapsedDuration(startedAtMs: number, nowMs: number): string {
  const totalSeconds = Math.max(0, Math.floor((nowMs - startedAtMs) / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (value: number) => String(value).padStart(2, '0');
  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }
  return `${pad(minutes)}:${pad(seconds)}`;
}
