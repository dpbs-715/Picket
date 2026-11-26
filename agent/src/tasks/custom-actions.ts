import { normalizeAppLocale, type AppLocale } from '../../../shared/language-protocol';
import type {
  GenerateCustomActionCommand,
  PiActionLink,
  PiCustomActionDraft,
  RunCustomActionCommand,
} from '../../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import { PICKET_DESKTOP_OVERLAY, type SkillRegistry } from '../skills';
import { extractJsonObject, formatPlanFilesPromptBlock, stripReasoningBlocks } from './shared';

export function buildCustomActionPrompts(
  command: RunCustomActionCommand,
  registry: SkillRegistry,
): { systemPrompt: string; userPrompt: string } {
  const locale = normalizeAppLocale(command.locale, 'en');
  const commitLang = normalizeAppLocale(command.commitLanguage, 'en');
  const uiLanguage = locale === 'zh' ? 'Chinese' : 'English';
  const commitLangNote = commitLang === 'zh'
    ? 'Write Git commit subjects & `- ` bullets in Chinese (keeping Conventional Commit tokens in English).'
    : 'Write Git commit subjects & `- ` bullets in English.';

  const systemPrompt = [
    'You are Picket\'s Git & Collaboration Agent executing a user-triggered Custom Action Button inside the currently open repository.',
    '',
    '<available_skills_index>',
    registry.formatIndex(),
    '</available_skills_index>',
    '',
    '<picket_desktop_overlay>',
    PICKET_DESKTOP_OVERLAY.customActionOverlay,
    '</picket_desktop_overlay>',
    '',
    `UI / user-facing interaction language: ${uiLanguage}. Write all questions, explanations, and your final \`summary\` in concise ${uiLanguage}.`,
    `Commit message language preference: ${commitLangNote}`,
  ].join('\n');

  const parts: string[] = [
    `Custom Action Button: ${command.actionLabel}`,
    `Current branch: ${command.detached ? '(detached HEAD)' : command.branch}`,
  ];
  if (command.remoteBranches.length > 0) {
    parts.push(`Remote branches (${command.remoteBranches.length}): ${command.remoteBranches.slice(0, 30).join(', ')}`);
  }
  parts.push(`Action preset prompt / command:\n${command.actionPrompt}`);
  if (command.extraPrompt.trim()) {
    parts.push(`User one-off extra instructions (+ Prompt):\n${command.extraPrompt.trim()}`);
  }
  if (command.selectedFiles.length > 0) {
    parts.push(`Authorized selected files (${command.selectedFiles.length}):\n${formatPlanFilesPromptBlock(command.selectedFiles)}`);
  } else {
    parts.push('Authorized selected files: 0 (no files are currently checked; do NOT commit local modifications unless the user selects them, though read-only actions, tagging, PR review, or creating PRs/issues from existing commits are completely fine).');
  }
  if (command.unselectedPaths.length > 0) {
    parts.push(`Unselected worktree files (off-limits for local commits):\n${command.unselectedPaths.slice(0, 40).map((p) => `- ${p}`).join('\n')}`);
  }
  if (command.localKeepPaths.length > 0) {
    parts.push(`Local Keep paths (excluded from commits):\n${command.localKeepPaths.slice(0, 30).map((p) => `- ${p}`).join('\n')}`);
  }
  if (command.previousCheckpoint?.trim()) {
    parts.push(
      [
        '<previous_session_checkpoint>',
        command.previousCheckpoint.trim(),
        '</previous_session_checkpoint>',
        'A previous session recorded the verified checkpoint above. Reconcile these Issues, branches, isolated worktrees, and PRs/MRs against live Git and gh/glab BEFORE creating new resources, and resume cleanly from the first unfinished step without duplicating already-created resources.',
      ].join('\n'),
    );
  }
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}

export function buildGenerateCustomActionPrompts(
  command: GenerateCustomActionCommand,
  registry: SkillRegistry,
): { systemPrompt: string; userPrompt: string } {
  const locale = normalizeAppLocale(command.locale, 'en');
  const uiLanguage = locale === 'zh' ? 'Chinese' : 'English';
  const systemPrompt = [
    'You are Picket\'s Custom Action designer (`Create with Picket`).',
    '',
    '<available_skills_index>',
    registry.formatIndex(),
    '</available_skills_index>',
    '',
    '<picket_desktop_overlay>',
    PICKET_DESKTOP_OVERLAY.createActionOverlay,
    '</picket_desktop_overlay>',
    '',
    `Write \`label\`, \`summary\`, and any clarifying \`ask_user_question\` text in concise ${uiLanguage}.`,
  ].join('\n');

  const parts = [
    `Current branch: ${command.branch}`,
    command.existingLabels.length > 0
      ? `Existing custom button labels in this repository (avoid duplicates): ${command.existingLabels.join(', ')}`
      : 'Existing custom button labels in this repository: none',
    `User request:\n${command.userRequest}`,
  ];
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}

export function parseCustomActionResult(
  raw: string,
  locale: AppLocale = 'en',
): { summary: string; links: PiActionLink[] } {
  const msg = getSidecarMessages(locale);
  const trimmed = stripReasoningBlocks(raw);
  try {
    const parsed = extractJsonObject(trimmed);
    if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
      const rec = parsed as Record<string, unknown>;
      const summary = typeof rec.summary === 'string' && rec.summary.trim()
        ? rec.summary.trim().slice(0, 1200)
        : '';
      const links: PiActionLink[] = [];
      if (Array.isArray(rec.links)) {
        for (const item of rec.links) {
          if (typeof item !== 'object' || item === null || Array.isArray(item)) continue;
          const entry = item as Record<string, unknown>;
          const label = typeof entry.label === 'string' ? entry.label.trim().slice(0, 80) : '';
          const url = typeof entry.url === 'string' ? entry.url.trim() : '';
          if (label && /^https?:\/\//i.test(url)) {
            links.push({ label, url });
          }
        }
      }
      if (summary) {
        return { summary, links };
      }
    }
  } catch {
    // Fall back to clean plain text summary when model finishes with prose instead of JSON.
  }
  const fallback = trimmed.slice(0, 1200) || msg.defaultCustomActionSummary;
  return { summary: fallback, links: [] };
}

export function parseCustomActionDraft(raw: string, locale: AppLocale = 'en'): PiCustomActionDraft {
  const msg = getSidecarMessages(locale);
  const parsed = extractJsonObject(raw);
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new LocalizedSidecarError(msg.invalidCustomActionDraftJson);
  }
  const rec = parsed as Record<string, unknown>;
  const label = typeof rec.label === 'string' ? rec.label.trim().slice(0, 48) : '';
  const prompt = typeof rec.prompt === 'string' ? rec.prompt.replace(/\r\n/g, '\n').trim().slice(0, 4000) : '';
  const summary = typeof rec.summary === 'string' ? rec.summary.trim().slice(0, 300) : '';
  if (!label || !prompt) {
    throw new LocalizedSidecarError(msg.missingCustomActionDraftFields);
  }
  return {
    label,
    prompt,
    summary: summary || msg.defaultCustomActionDraftSummary,
  };
}
