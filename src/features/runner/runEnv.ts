import {
  RUN_COMMAND_LIMITS,
  isValidRunEnvKey,
  type RunEnvEntry,
} from '../../../shared/run-protocol';

export function formatRunEnvEntries(entries?: readonly RunEnvEntry[]): string {
  if (!entries?.length) return '';
  return entries.map((item) => `${item.key}=${item.value}`).join('\n');
}

export function parseRunEnvText(raw: string): { ok: true; entries: RunEnvEntry[] } | { ok: false } {
  const entries: RunEnvEntry[] = [];
  const seen = new Set<string>();
  const lines = raw.replace(/\r\n/g, '\n').split('\n');
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const eqIndex = trimmed.indexOf('=');
    if (eqIndex <= 0) return { ok: false };
    const key = trimmed.slice(0, eqIndex).trim();
    const value = trimmed.slice(eqIndex + 1);
    if (!isValidRunEnvKey(key) || value.includes('\0') || value.length > RUN_COMMAND_LIMITS.maxEnvValueChars) {
      return { ok: false };
    }
    if (!seen.has(key)) {
      seen.add(key);
      entries.push({ key, value });
    }
  }
  if (entries.length > RUN_COMMAND_LIMITS.maxEnvEntries) return { ok: false };
  return { ok: true, entries };
}

export function appendRunEnvPreset(currentText: string, key: string, defaultValue: string): string {
  const normalized = currentText.replace(/\r\n/g, '\n');
  const lines = normalized.split('\n');
  const alreadyPresent = lines.some((line) => {
    const trimmed = line.trim();
    const eqIndex = trimmed.indexOf('=');
    return eqIndex > 0 && trimmed.slice(0, eqIndex).trim() === key;
  });
  if (alreadyPresent) return currentText;
  const trimmedEnd = normalized.trimEnd();
  const nextLine = `${key}=${defaultValue}`;
  return trimmedEnd ? `${trimmedEnd}\n${nextLine}` : nextLine;
}
