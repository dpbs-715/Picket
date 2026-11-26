import { normalizeAppLocale, type AppLocale } from '../../../shared/language-protocol';
import type {
  PiConflictFile,
  PiConflictResolution,
  PiResolvedConflictFile,
  ResolveConflictCommand,
} from '../../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import { extractJsonObject } from './shared';

const CONFLICT_MARKER_LINE = /^(?:<{7}(?: |$)|={7}\r?$|>{7}(?: |$)|\|{7}(?: |$))/m;

export function hasConflictMarkers(text: string): boolean {
  return CONFLICT_MARKER_LINE.test(text);
}

export function parseConflictResolution(
  raw: string,
  command: ResolveConflictCommand,
  locale: AppLocale = normalizeAppLocale(command.locale, 'en'),
): PiConflictResolution {
  const resolvedLocale = normalizeAppLocale(locale, 'en');
  const msg = getSidecarMessages(resolvedLocale);
  let parsed: unknown;
  try {
    parsed = extractJsonObject(raw);
  } catch {
    throw new LocalizedSidecarError(msg.invalidConflictJson);
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new LocalizedSidecarError(msg.conflictResultNotObject);
  }
  const record = parsed as Record<string, unknown>;
  if (!Array.isArray(record.resolvedFiles) || record.resolvedFiles.length === 0) {
    throw new LocalizedSidecarError(msg.missingResolvedFiles);
  }

  const byId = new Map(command.files.map((file) => [file.fileId, file]));
  const byPath = new Map(command.files.map((file) => [file.path, file]));
  const seen = new Set<string>();
  const resolvedFiles: PiResolvedConflictFile[] = record.resolvedFiles.map((entry) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      throw new LocalizedSidecarError(msg.invalidConflictFileEntry);
    }
    const item = entry as Record<string, unknown>;
    const matched = (typeof item.fileId === 'string' ? byId.get(item.fileId.trim()) : undefined)
      ?? (typeof item.path === 'string' ? byPath.get(item.path.trim()) : undefined);
    if (!matched) {
      const targetName = typeof item.path === 'string' ? item.path : String(item.fileId ?? 'unknown');
      throw new LocalizedSidecarError(msg.unauthorizedConflictFile(targetName));
    }
    if (seen.has(matched.fileId)) {
      throw new LocalizedSidecarError(msg.duplicateConflictFile(matched.path));
    }
    seen.add(matched.fileId);
    if (typeof item.content !== 'string') {
      throw new LocalizedSidecarError(msg.missingMergedContent(matched.path));
    }
    const baseHadMarkers = matched.baseContent ? hasConflictMarkers(matched.baseContent) : false;
    if (!baseHadMarkers && hasConflictMarkers(item.content)) {
      throw new LocalizedSidecarError(msg.leftoverConflictMarkers(matched.path));
    }
    if (
      !item.content.trim()
      && matched.upstreamContent.trim().length > 0
      && matched.localContent.trim().length > 0
    ) {
      throw new LocalizedSidecarError(msg.emptyMergedContent(matched.path));
    }
    return {
      fileId: matched.fileId,
      path: matched.path,
      content: item.content,
    };
  });

  if (seen.size !== command.files.length) {
    throw new LocalizedSidecarError(msg.missingConflictFiles);
  }
  const summary = typeof record.summary === 'string' && record.summary.trim()
    ? record.summary.trim().slice(0, 240)
    : msg.defaultConflictMergeSummary(resolvedFiles.length);
  return { summary, resolvedFiles };
}

function formatConflictFilesPromptBlock(files: PiConflictFile[]): string {
  return files.map((file) => {
    const sections = [
      `### Authorized Conflicted File: ${file.path}`,
      `- fileId: ${file.fileId}`,
    ];
    if (file.baseContent !== undefined) {
      sections.push('#### Common Ancestor (base):', '```', file.baseContent, '```');
    }
    sections.push(
      '#### Remote Upstream Version (onto):',
      '```',
      file.upstreamContent,
      '```',
      '#### Local Commit Version (being rebased):',
      '```',
      file.localContent,
      '```',
      '#### Working Tree With Conflict Markers:',
      '```',
      file.conflictedContent,
      '```',
    );
    return sections.join('\n');
  }).join('\n\n');
}

export function buildResolveConflictPrompts(command: ResolveConflictCommand): { systemPrompt: string; userPrompt: string } {
  const locale = normalizeAppLocale(command.locale, 'en');
  const summaryLanguage = locale === 'zh' ? 'Chinese' : 'English';
  const systemPrompt = [
    'You are Picket\'s authorized Git conflict merge resolver.',
    'The user explicitly authorized resolving ONLY the listed conflicted files during a Git rebase.',
    'Rules:',
    '1. Resolve ONLY the conflict regions between the Remote Upstream Version and the Local Commit Version, preserving all non-conflicting lines verbatim.',
    '2. Never modify or output any file outside the authorized conflicted files list.',
    '3. If the user supplied specific merge instructions, follow them strictly within the conflicted files.',
    '4. Ensure the merged file content is syntactically coherent and contains ZERO leftover Git conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`, `|||||||`).',
    `5. Write \`summary\` in concise ${summaryLanguage}.`,
    '6. Return ONLY a single raw JSON object (no prose outside JSON) with this exact schema:',
    '{"summary":"Concise merge summary","resolvedFiles":[{"fileId":"<exact fileId>","path":"<exact path>","content":"<complete resolved file content>"}]}',
  ].join('\n');

  const parts: string[] = [
    `Rebasing local branch \`${command.branch}\` onto \`${command.upstream}\`.`,
  ];
  if (command.commits.length > 0) {
    parts.push(`Local commit(s) being rebased:\n${command.commits.map((c) => `- ${c}`).join('\n')}`);
  }
  if (command.userInstruction.trim()) {
    parts.push(`User merge instruction (MUST follow):\n${command.userInstruction.trim()}`);
  }
  parts.push(`Authorized conflicted files (${command.files.length}):\n${formatConflictFilesPromptBlock(command.files)}`);
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}
