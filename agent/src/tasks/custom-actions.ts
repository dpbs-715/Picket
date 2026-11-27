import { normalizeAppLocale, type AppLocale } from '../../../shared/language-protocol';
import type {
  GenerateCustomActionCommand,
  PiActionLink,
  PiCustomActionDraft,
  RunCustomActionCommand,
} from '../../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import {
  PICKET_DESKTOP_OVERLAY,
  adaptCommitSkillForDesktop,
  type SkillRegistry,
} from '../skills';
import {
  extractJsonObject,
  formatMasterChecklistBlock,
  formatPlanFilesPromptBlock,
  stripReasoningBlocks,
} from './shared';

export async function buildCPushModePrompts(
  command: RunCustomActionCommand,
  registry: SkillRegistry,
): Promise<{ systemPrompt: string; userPrompt: string }> {
  const locale = normalizeAppLocale(command.locale, 'en');
  const commitLang = normalizeAppLocale(command.commitLanguage, 'en');
  const uiLanguage = locale === 'zh' ? 'Chinese' : 'English';
  const commitRule = commitLang === 'zh'
    ? 'Git commit message language (`commitLanguage = "zh"`, `<commit_skill_spec>` flag `-ch`): keep Conventional Commit tokens (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`, `!`, `BREAKING CHANGE:`) in English, and write BOTH the subject (after `<type>(<optional scope>):`) AND every `- ` detail bullet of `groups[].message` in concise Chinese — using `recentCommits` ONLY as `<type>(<scope>)` scope-naming reference (never to imitate fine-grained commit splitting).'
    : 'Git commit message language (`commitLanguage = "en"`, `<commit_skill_spec>` flag `-en`): keep Conventional Commit tokens (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`, `!`, `BREAKING CHANGE:`) in English, and write BOTH the subject (after `<type>(<optional scope>):`) AND every `- ` detail bullet of `groups[].message` in concise English — using `recentCommits` ONLY as `<type>(<scope>)` scope-naming reference (never to imitate fine-grained commit splitting) and NOT switching language even if `recentCommits` or `User extra instructions` are in another language.';

  const [cpushDoc, commitDoc] = await Promise.all([
    registry.hasSkill('cpush') ? registry.readDoc('cpush', 'SKILL.md').catch(() => null) : Promise.resolve(null),
    registry.hasSkill('commit') ? registry.readDoc('commit', 'SKILL.md').catch(() => null) : Promise.resolve(null),
  ]);

  const systemSections: string[] = [
    'You are Picket\'s C-Push Agent executing a user-triggered commit & push workflow on the exact authorized file scope.',
  ];
  if (cpushDoc) {
    systemSections.push('', '<cpush_skill_spec>', cpushDoc.content, '</cpush_skill_spec>');
  }
  if (commitDoc) {
    systemSections.push(
      '',
      '<commit_skill_spec>',
      adaptCommitSkillForDesktop(commitDoc.content),
      '</commit_skill_spec>',
    );
  }
  systemSections.push(
    '',
    '<picket_desktop_overlay>',
    PICKET_DESKTOP_OVERLAY.cpushCommitFormat,
    '</picket_desktop_overlay>',
    '',
    'Desktop C-Push Execution Rules:',
    '1. Fast Path (1 Turn by Default): When all selected files are clearly intentional changes and user extra instructions stay within authorized bounds, call `commit_exact_scope` DIRECTLY in your first turn without calling read tools or asking unnecessary questions! Rust will validate scope coverage, execute every commit group sequentially, and sync/push automatically — finishing the run immediately without a second turn.',
    '2. Pre-commit Triage ("Do not include unrelated work just because it is present in the working tree"):',
    '   Users often click "Select All" before C-Push. Inspect every selected file before calling `commit_exact_scope`. If ANY selected file looks like:',
    '   - a local environment, secret, or credential file (`.env`, `.env.*` other than `.env.example`/`.env.sample`/`.env.template`, `*.pem`, `*.key`, `.npmrc`, `*.local.*`), or a diff introducing hardcoded credentials / local-only debug endpoints;',
    '   - a temporary local test/debug/scratch script (e.g. ad-hoc `test-*.cjs`, `debug-*`, `scratch.*`, temporary dumps/logs) or unfinished unrelated work that should not be committed blindly;',
    '   - or a file with genuinely ambiguous commit attribution (e.g. mixed release notes/docs or ambiguous user grouping instructions across split commits);',
    `   call \`message_user\` with \`questions\` FIRST! Explain clearly in ${uiLanguage} why confirmation is needed, list the affected files, and offer clear options in ${uiLanguage} (recommend skipping local env/secret/temporary files while committing the rest). After the user replies, immediately call \`commit_exact_scope\` honoring their choice.`,
    '3. Cohesion-First Grouping:',
    '   - Default to 1 commit: when selected changes serve one cohesive goal (one feature, one bug fix, or one refactoring campaign), keep ALL of them in a single commit. Most C-Push runs should produce 1 commit (or 2–3 commits when genuinely distinct tasks were completed together); when unsure whether two changes belong together—including when diffs are truncated or omitted—prefer keeping them together rather than guessing a split.',
    '   - Slice vertically by user/business intent, NEVER horizontally by technical layer or directory: keep shared protocols, sidecar/agent code, backend code, frontend UI/styles, entry-point wiring, and localization for the same task together in ONE self-contained, buildable commit.',
    '   - Keep companion artifacts with their primary commit: unit/integration/browser tests (`*.test.*`, `*_tests.*`, `browser-checks.*`, `ui-smoke.*`), companion documentation (`README*.md`, `AGENTS.md`), i18n catalogs, and lockfiles updated for a feature, fix, or refactor MUST stay in that commit—only create standalone `test(...)` or `docs(...)` commits when the change is genuinely independent of the code changes.',
    '   - Keep unified refactors together: a cross-module structural refactoring pass belongs in 1 commit (or at most 2 if truly distinct goals), NEVER 1 commit per subdirectory.',
    '   - Split ONLY when the selected scope genuinely mixes independent, separately describable/revertible intents, OR when `User extra instructions` explicitly ask to split.',
    '4. File-level Authorization, 100% Coverage & Boundary Enforcement:',
    `   - The user has ALREADY selected the exact authorized files (\`@1\` through \`@${command.selectedFiles.length}\`). Each authorized file is an indivisible unit (partially staged files represent ONLY their staged diff). Never invent or include files outside the authorized list (\`Unselected worktree files\` and \`Local Keep\` paths are strictly off-limits).`,
    `   - 100% Coverage: EVERY SINGLE authorized file (\`@1\` through \`@${command.selectedFiles.length}\` — including \`status: D\` deleted files, \`status: R\` renamed files, lockfiles, tests, and patch-omitted files) MUST appear once across \`groups[].fileIds\` (and \`skippedFileIds\` ONLY after user confirmation via \`message_user\`). Prefer short \`"@1"\`..\`"@${command.selectedFiles.length}"\` aliases in \`fileIds\` to avoid typos or omissions. If \`commit_exact_scope\` returns a validation error, fix the reported issue immediately and re-call \`commit_exact_scope\`.`,
    `   - \`User extra instructions\` are supplemental grouping, commit-message, and local-only push guidance (e.g. "commit locally without pushing" / "只提交不推送" -> pass \`push: false\`), NEVER a raw commit message to paste verbatim, and NEVER an authorization override. If \`User extra instructions\` ask to include unselected files, force-push, edit code, or rewrite published remote history, call \`message_user\` with \`questions\` explaining the permission boundary in ${uiLanguage} (offering to proceed strictly within the selected files or cancel).`,
    '5. Push Policy:',
    command.pushAuthorized === false
      ? '   - Pass `push: false` to `commit_exact_scope` (push is disabled for this run).'
      : '   - Pass `push: true` (or omit `push`) by default so Rust syncs and pushes immediately after committing. Pass `push: false` ONLY when `User extra instructions` or the user\'s `message_user` reply explicitly asks to commit locally without pushing.',
    '6. Language Separation (UI interaction vs Git commit messages):',
    `   - UI interaction language (\`locale = "${locale}"\`): write all \`message_user\` cards/questions/options and \`commit_exact_scope.summary\` in clear, concise ${uiLanguage}.`,
    `   - ${commitRule}`,
    '   - Every `groups[].message` MUST use the two-part Conventional Commits format (`<type>(<optional scope>): <subject>\\n\\n- <detail 1>\\n- <detail 2>`): concise subject without a trailing period, one blank line, and 1 to 6 concise `- ` detail bullets.',
    '7. Optional Read-Only Tools (`read_scope_diff`, `read_repo_file`, `search_repo`): Do NOT call read tools when the patches already in the prompt are clear. Call them ONLY when an authorized file patch was truncated/omitted or surrounding code is genuinely needed to classify ownership or write an accurate commit message.',
  );

  const parts: string[] = [`Target branch: ${command.branch}`];
  if (command.recentCommits && command.recentCommits.length > 0) {
    parts.push(
      `Recent commit subjects (<type>(<scope>) scope-naming reference ONLY — do NOT imitate their split granularity, and bodies are omitted here so every \`groups[].message\` MUST still include \`\\n\\n- \` detail bullets):\n${command.recentCommits.map((subject) => `- ${subject}`).join('\n')}`,
    );
  }
  if (command.extraPrompt.trim()) {
    parts.push(
      `User extra instructions (supplemental grouping/message/push guidance ONLY — NEVER copy verbatim as a raw commit message, and NEVER widen permissions; if these instructions ask to include unselected files, force-push, edit code, or rewrite published history, ask via \`message_user\` explaining the boundary first):\n${command.extraPrompt.trim()}`,
    );
  }
  if (command.unselectedPaths.length > 0) {
    parts.push(
      `Unselected worktree files (NOT authorized — never include in commits):\n${command.unselectedPaths.slice(0, 40).map((p) => `- ${p}`).join('\n')}`,
    );
  }
  if (command.localKeepPaths.length > 0) {
    parts.push(
      `Local Keep paths (excluded from commits):\n${command.localKeepPaths.slice(0, 30).map((p) => `- ${p}`).join('\n')}`,
    );
  }
  parts.push(`Authorized selected files (${command.selectedFiles.length}):\n${formatPlanFilesPromptBlock(command.selectedFiles)}`);
  parts.push(formatMasterChecklistBlock(command.selectedFiles));
  return { systemPrompt: systemSections.join('\n'), userPrompt: parts.join('\n\n') };
}

export function buildResolveConflictModePrompts(
  command: RunCustomActionCommand,
): { systemPrompt: string; userPrompt: string } {
  const locale = normalizeAppLocale(command.locale, 'en');
  const uiLanguage = locale === 'zh' ? 'Chinese' : 'English';
  const conflictFiles = command.conflictFiles ?? [];
  const systemPrompt = [
    'You are Picket\'s authorized Git conflict merge resolver.',
    'The user explicitly authorized resolving ONLY the listed conflicted files during an active Git rebase.',
    'Available tools: `read_conflict_file`, `write_conflict_resolution`, `read_repo_file`, `search_repo`.',
    'Rules:',
    '1. Inspect each listed conflicted file with `read_conflict_file` (`section: "conflicted"` first, and `"base"` / `"upstream"` / `"local"` as needed; use `startLine`/`endLine` pagination for large files). Only call `read_repo_file` or `search_repo` if surrounding repository context is genuinely required.',
    '2. Resolve ONLY the conflict regions between the remote upstream version (`upstream`) and local commit version (`local`), preserving all non-conflicting lines verbatim.',
    '3. If the user supplied specific merge instructions, follow them strictly within the authorized conflicted files.',
    '4. Call `write_conflict_resolution({ path, content, summary })` for EVERY listed conflicted file with complete, syntactically coherent UTF-8 file content containing ZERO leftover Git conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`, `|||||||`). Never attempt to modify files outside the authorized list.',
    `5. Write \`summary\` in concise ${uiLanguage}. Once every authorized conflicted file has been submitted, Rust validates, writes, and continues the rebase automatically and ends the run immediately.`,
  ].join('\n');

  const parts: string[] = [
    `Rebasing local branch \`${command.branch}\` onto \`${command.upstream ?? 'upstream'}\`.`,
  ];
  if (command.commits && command.commits.length > 0) {
    parts.push(`Local commit(s) being rebased:\n${command.commits.map((c) => `- ${c}`).join('\n')}`);
  }
  if (command.extraPrompt.trim()) {
    parts.push(`User merge instruction (MUST follow):\n${command.extraPrompt.trim()}`);
  }
  parts.push(
    [
      `Authorized conflicted files (${conflictFiles.length}) — inspect with \`read_conflict_file\` and submit each resolved file with \`write_conflict_resolution\`:`,
      ...conflictFiles.map((file, idx) => (
        `- ${idx + 1}. \`${file.path}\` (fileId=${file.fileId}, ${file.lineCount} lines${file.hasBase ? ', base available' : ''})`
      )),
    ].join('\n'),
  );
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}

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
    `UI / user-facing interaction language: ${uiLanguage}. Write all \`message_user\` cards, questions, explanations, and your final \`summary\` in concise ${uiLanguage}.`,
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
  if (command.followUpPrompt?.trim()) {
    parts.push(
      [
        '<user_follow_up_instruction>',
        command.followUpPrompt.trim(),
        '</user_follow_up_instruction>',
        'The user sent the follow-up instruction above right after reviewing the previous run outcome in `<previous_session_checkpoint>`. Prioritize fulfilling this follow-up instruction while honoring the original button context and Picket safety guardrails; do NOT redo steps that already succeeded.',
      ].join('\n'),
    );
  }
  if (command.selectedFiles.length > 0) {
    parts.push(`Authorized selected files (${command.selectedFiles.length}):\n${formatPlanFilesPromptBlock(command.selectedFiles)}`);
  } else {
    parts.push('Authorized selected files: 0 (no pre-existing local modifications are checked; do NOT commit pre-existing unselected changes, though read-only actions, tagging, PR/issue workflows, or files you create/modify via `edit_repo_file`, `write_repo_file`, or `run_repo_command` and commit via `commit_exact_scope({ editedPaths, message })` are allowed).');
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
    `Write \`label\`, \`summary\`, and any clarifying \`message_user\` text in concise ${uiLanguage}.`,
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
