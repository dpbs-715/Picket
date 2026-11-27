import type { AppLocale } from '../../../shared/language-protocol';
import type { PiPlanFileEntry } from '../../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';

export function formatPlanFilesPromptBlock(files: PiPlanFileEntry[]): string {
  return files.map((file, index) => {
    const alias = `@${index + 1}`;
    const rename = file.previousPath ? ` (renamed from ${file.previousPath})` : '';
    const statusLabel = file.status === 'D'
      ? `D (deleted file — MUST be assigned to a commit group, +${file.added} -${file.removed})`
      : `${file.status} (${file.rangeKind}, +${file.added} -${file.removed})`;
    const header = [
      `### Authorized File: ${file.path}${rename}`,
      `- fileId: ${file.fileId}`,
      `- alias: ${alias}`,
      `- status: ${statusLabel}`,
    ];
    if (file.skipReason) {
      header.push(`- note: ${file.skipReason}`);
    }
    if (file.patch && file.patch.trim()) {
      header.push('```diff', file.patch.trimEnd(), '```');
    } else {
      header.push(`(Patch body omitted; keep ${alias} / ${file.fileId} with the cohesive commit group it belongs to based on path/status—do NOT start a separate commit group merely because it lives in another directory.)`);
    }
    return header.join('\n');
  }).join('\n\n');
}

export function formatMasterChecklistBlock(files: PiPlanFileEntry[]): string {
  const lines = files.map((file, index) => {
    const statusTag = file.status === 'D' ? 'D (deleted)' : file.status;
    return `- @${index + 1} | ${file.fileId} | ${statusTag} | ${file.path}`;
  });
  return [
    `Master Checklist of ALL ${files.length} Authorized Selected Files (when returning kind="ready", 100% of @1..@${files.length} — including every D deleted file and patch-omitted file — MUST appear once in groups[].fileIds${files.length > 1 ? ' across your groups' : ''}; you may use either "@index" aliases like "@1" or exact fileIds in fileIds):`,
    ...lines,
  ].join('\n');
}

export function stripReasoningBlocks(raw: string): string {
  return raw.replace(/^(?:\s*<think>[\s\S]*?<\/think>\s*)+/i, '').trim();
}

export function cleanCommitMessage(raw: string, locale: AppLocale = 'en'): string {
  const msg = getSidecarMessages(locale);
  let trimmed = stripReasoningBlocks(raw);
  const fenced = /^```[a-zA-Z0-9_-]*[ \t]*\r?\n([\s\S]*?)\r?\n```$/.exec(trimmed);
  if (fenced) {
    trimmed = fenced[1].trim();
  }
  const normalized = trimmed
    .split(/\r?\n/)
    .map((line) => line.replace(/[ \t]+$/g, ''))
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
  if (!normalized) {
    throw new LocalizedSidecarError(msg.emptyCommitMessage);
  }
  return normalized;
}

function repairJsonControlChars(json: string): string {
  const validEscapes = new Set(['"', '\\', '/', 'b', 'f', 'n', 'r', 't']);
  let out = '';
  let inString = false;
  for (let i = 0; i < json.length; i += 1) {
    const ch = json[i];
    if (!inString) {
      out += ch;
      if (ch === '"') inString = true;
      continue;
    }
    if (ch === '"') {
      out += ch;
      inString = false;
      continue;
    }
    if (ch === '\\') {
      const next = json[i + 1];
      if (next === undefined) {
        out += '\\\\';
        continue;
      }
      if (next === 'u' && /^[0-9a-fA-F]{4}$/.test(json.slice(i + 2, i + 6))) {
        out += json.slice(i, i + 6);
        i += 5;
        continue;
      }
      if (validEscapes.has(next)) {
        out += `\\${next}`;
        i += 1;
        continue;
      }
      out += '\\\\';
      continue;
    }
    const code = ch.codePointAt(0) ?? 0;
    if (code <= 0x1f) {
      if (ch === '\n') out += '\\n';
      else if (ch === '\r') out += '\\r';
      else if (ch === '\t') out += '\\t';
      else out += `\\u${code.toString(16).padStart(4, '0')}`;
    } else {
      out += ch;
    }
  }
  return out;
}

function parseJsonCandidate(candidate: string): unknown {
  try {
    return JSON.parse(candidate);
  } catch {
    return JSON.parse(repairJsonControlChars(candidate));
  }
}

export function extractJsonObject(raw: string): unknown {
  const trimmed = stripReasoningBlocks(raw);
  const fenced = /```(?:json)?\s*\r?\n([\s\S]*?)\r?\n```/i.exec(trimmed);
  const candidate = fenced ? fenced[1].trim() : trimmed;
  try {
    return parseJsonCandidate(candidate);
  } catch {
    const firstBrace = candidate.indexOf('{');
    const lastBrace = candidate.lastIndexOf('}');
    if (firstBrace >= 0 && lastBrace > firstBrace) {
      return parseJsonCandidate(candidate.slice(firstBrace, lastBrace + 1));
    }
    throw new Error('Invalid JSON payload');
  }
}

export class RecoverablePlanValidationError extends LocalizedSidecarError {
  readonly repairPrompt: string;

  constructor(message: string, repairPrompt: string) {
    super(message);
    this.name = 'RecoverablePlanValidationError';
    this.repairPrompt = repairPrompt;
  }
}
