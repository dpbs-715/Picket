import { normalizeAppLocale, type AppLocale } from '../../../shared/language-protocol';
import type { PiDiffFile, PiReviewFinding, ReviewScopeCommand } from '../../../shared/pi-protocol';
import { DCR_SKILL_SPEC } from '../git-skills';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import { PICKET_DESKTOP_OVERLAY } from '../skills';
import { extractJsonObject, formatDiffPromptBlock } from './shared';

export function parseReviewFindings(
  raw: string,
  files: PiDiffFile[],
  locale: AppLocale = 'en',
): PiReviewFinding[] {
  const msg = getSidecarMessages(locale);
  let parsed: unknown;
  try {
    parsed = extractJsonObject(raw);
  } catch {
    throw new LocalizedSidecarError(msg.invalidReviewJson);
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed) || !Array.isArray((parsed as { findings?: unknown }).findings)) {
    throw new LocalizedSidecarError(msg.missingReviewFindings);
  }
  const byId = new Map(files.map((file) => [file.fileId, file]));
  const byPath = new Map(files.map((file) => [file.path, file]));
  const findings: PiReviewFinding[] = [];

  for (const item of (parsed as { findings: unknown[] }).findings) {
    if (typeof item !== 'object' || item === null || Array.isArray(item)) {
      throw new LocalizedSidecarError(msg.invalidReviewFindingItem);
    }
    const record = item as Record<string, unknown>;
    const matched = (typeof record.fileId === 'string' ? byId.get(record.fileId) : undefined)
      ?? (typeof record.path === 'string' ? byPath.get(record.path) : undefined);
    // Ignore hallucinated findings pointing outside the explicitly authorized review files.
    if (!matched) continue;

    const rawSeverity = typeof record.severity === 'string' ? record.severity.toLowerCase().trim() : '';
    const severity: 'high' | 'suggestion' = rawSeverity === 'high'
      || rawSeverity === 'critical'
      || rawSeverity === 'error'
      || rawSeverity === 'p0'
      || rawSeverity === 'p1'
      ? 'high'
      : rawSeverity === 'suggestion'
        || rawSeverity === 'medium'
        || rawSeverity === 'low'
        || rawSeverity === 'warning'
        || rawSeverity === 'info'
        || rawSeverity === 'p2'
        || rawSeverity === 'p3'
        ? 'suggestion'
        : (() => { throw new LocalizedSidecarError(msg.unknownReviewSeverity); })();

    const line = typeof record.line === 'number' && Number.isFinite(record.line)
      ? Math.max(1, Math.trunc(record.line))
      : 1;
    const title = typeof record.title === 'string' ? record.title.trim().slice(0, 180) : '';
    const reason = typeof record.reason === 'string' ? record.reason.trim().slice(0, 800) : '';
    if (!title || !reason) {
      throw new LocalizedSidecarError(msg.missingReviewTitleOrReason);
    }
    findings.push({
      fileId: matched.fileId,
      path: matched.path,
      line,
      severity,
      title,
      reason,
    });
  }
  return findings;
}

export function buildReviewPrompts(
  command: ReviewScopeCommand,
  dcrSkillSpec: string = DCR_SKILL_SPEC,
): { systemPrompt: string; userPrompt: string } {
  const locale = normalizeAppLocale(command.locale, 'en');
  const languageDirective = locale === 'zh'
    ? 'Write every finding `title` and `reason` in clear, concise Chinese (the schema below uses English placeholders only).'
    : 'Write every finding `title` and `reason` in clear, concise English.';
  const toolsGuidance = command.enableTools
    ? 'Read-only repository tools (`read_scope_diff`, `read_repo_file`, `search_repo`, `read_skill_doc`) are enabled: use `read_scope_diff` to read omitted hunks of any truncated/deferred authorized file diff, and use `read_repo_file`/`search_repo` to inspect surrounding code, callers, types, schemas, or tests when needed to confirm a concrete bug or rule out a false positive. Keep tool calls focused and minimal.'
    : 'Review ONLY the supplied Git diff patches without inventing context outside them.';

  const systemPrompt = [
    'You are Picket\'s pre-commit code reviewer, governed by the `dcr` (Diff Code Review) skill specification and desktop overlay below.',
    '',
    '<dcr_skill_spec>',
    dcrSkillSpec,
    '</dcr_skill_spec>',
    '',
    '<picket_desktop_overlay>',
    PICKET_DESKTOP_OVERLAY.dcrReviewOverlay,
    '</picket_desktop_overlay>',
    '',
    toolsGuidance,
    'When `User review instructions / context` is provided, apply its focus and background strictly within the supplied patches without inventing findings on unselected files.',
    `Do NOT invent issues outside the supplied patches. ${languageDirective}`,
    'Return ONLY a single JSON object with this exact schema (no prose outside JSON):',
    '{"findings":[{"fileId":"<exact fileId from input>","path":"<exact path from input>","line":1,"severity":"high","title":"Concise issue title","reason":"Concrete cause, impact, and suggested fix or verification"}]}',
    'Map P0/P1 findings (real bugs, security/credential exposure, broken invariants, data corruption, broken behavior) to severity "high"; map P2 findings (edge-case bugs, risky missing tests, diff-introduced maintainability risks) to severity "suggestion". Return {"findings":[]} if no concrete issues are found.',
  ].join('\n');

  const parts: string[] = [`Branch: ${command.branch}`];
  if (command.prompt.trim()) {
    parts.push(`User review instructions / context:\n${command.prompt.trim()}`);
  }
  parts.push(`Reviewable diff patches:\n${formatDiffPromptBlock(command.files, command.skippedPaths)}`);
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}
