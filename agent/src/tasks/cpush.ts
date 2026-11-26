import { normalizeAppLocale, type AppLocale } from '../../../shared/language-protocol';
import type {
  PiCPushCommitGroup,
  PiCPushPlan,
  PiCPushQuestionOption,
  PiPlanFileEntry,
  PlanCPushCommand,
} from '../../../shared/pi-protocol';
import { COMMIT_SKILL_SPEC, CPUSH_SKILL_SPEC } from '../git-skills';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import { PICKET_DESKTOP_OVERLAY, adaptCommitSkillForDesktop } from '../skills';
import {
  RecoverablePlanValidationError,
  cleanCommitMessage,
  extractJsonObject,
  formatMasterChecklistBlock,
  formatPlanFilesPromptBlock,
} from './shared';

export function parseCPushPlan(
  raw: string,
  command: PlanCPushCommand,
  locale: AppLocale = normalizeAppLocale(command.locale, 'en'),
): PiCPushPlan {
  const resolvedLocale = normalizeAppLocale(locale, 'en');
  const msg = getSidecarMessages(resolvedLocale);
  let parsed: unknown;
  try {
    parsed = extractJsonObject(raw);
  } catch {
    throw new LocalizedSidecarError(msg.invalidCPushPlanJson);
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new LocalizedSidecarError(msg.cPushPlanNotObject);
  }
  const record = parsed as Record<string, unknown>;
  const byId = new Map(command.files.map((file) => [file.fileId, file]));
  const byPath = new Map(command.files.map((file) => [file.path, file]));
  const byAlias = new Map(command.files.map((file, idx) => [`@${idx + 1}`, file]));
  const aliasById = new Map(command.files.map((file, idx) => [file.fileId, `@${idx + 1}`]));
  const byPrevPath = new Map<string, PiPlanFileEntry>();
  for (const file of command.files) {
    if (file.previousPath && !byPath.has(file.previousPath)) {
      byPrevPath.set(file.previousPath, file);
    }
  }
  const matchAuthorizedFile = (clean: string): PiPlanFileEntry | undefined =>
    byId.get(clean) ?? byPath.get(clean) ?? byAlias.get(clean) ?? byPrevPath.get(clean);
  const resolveAuthorizedId = (token: unknown): string => {
    if (typeof token !== 'string' || !token.trim()) {
      throw new LocalizedSidecarError(msg.emptyCPushFileRef);
    }
    const clean = token.trim();
    const matched = matchAuthorizedFile(clean);
    if (!matched) {
      throw new LocalizedSidecarError(msg.unauthorizedCPushFile(clean));
    }
    return matched.fileId;
  };

  if (record.kind === 'question') {
    const title = typeof record.title === 'string' ? record.title.trim().slice(0, 180) : '';
    const scope = typeof record.scope === 'string' ? record.scope.trim().slice(0, 120) : '';
    const description = typeof record.description === 'string' ? record.description.trim().slice(0, 800) : '';
    if (!title || !description) {
      throw new LocalizedSidecarError(msg.missingCPushQuestionText);
    }
    if (!Array.isArray(record.options) || record.options.length === 0 || record.options.length > 6) {
      throw new LocalizedSidecarError(msg.invalidCPushOptionCount);
    }
    const seenOptionIds = new Set<string>();
    const options: PiCPushQuestionOption[] = record.options.map((rawOpt, index) => {
      if (typeof rawOpt !== 'object' || rawOpt === null || Array.isArray(rawOpt)) {
        throw new LocalizedSidecarError(msg.invalidCPushOptionFormat);
      }
      const opt = rawOpt as Record<string, unknown>;
      const id = typeof opt.id === 'string' && opt.id.trim() ? opt.id.trim().slice(0, 64) : `option-${index + 1}`;
      const label = typeof opt.label === 'string' ? opt.label.trim().slice(0, 120) : '';
      if (!label || seenOptionIds.has(id)) {
        throw new LocalizedSidecarError(msg.invalidCPushOptionLabelOrDuplicateId);
      }
      seenOptionIds.add(id);
      return {
        id,
        label,
        ...(typeof opt.recommended === 'boolean' ? { recommended: opt.recommended } : {}),
      };
    });
    const rawAffected = Array.isArray(record.affectedFileIds) ? record.affectedFileIds : [];
    const affectedFileIds = [...new Set(
      rawAffected.flatMap((token) => {
        if (typeof token !== 'string') return [];
        const matched = matchAuthorizedFile(token.trim());
        return matched ? [matched.fileId] : [];
      }),
    )];
    return {
      kind: 'question',
      title,
      scope,
      description,
      affectedFileIds,
      options,
    };
  }

  if (record.kind !== 'ready') {
    throw new LocalizedSidecarError(msg.invalidCPushPlanKind);
  }
  if (!Array.isArray(record.groups)) {
    throw new LocalizedSidecarError(msg.missingCPushGroups);
  }
  const rawSkipped = Array.isArray(record.skippedFileIds) ? record.skippedFileIds : [];
  if (rawSkipped.length > 0 && command.qaHistory.length === 0) {
    throw new RecoverablePlanValidationError(
      msg.unconfirmedCPushSkip,
      [
        'Validation error: `skippedFileIds` MUST be `[]` when `qaHistory` is empty — selected files cannot be skipped without asking the user first.',
        `Either assign ALL ${command.files.length} authorized files (@1 through @${command.files.length}) to commit groups in \`groups[].fileIds\` with \`"skippedFileIds": []\`, OR return \`"kind": "question"\` so the user can confirm whether to skip them.`,
        'Return ONLY the corrected raw JSON object (no prose outside JSON).',
      ].join('\n'),
    );
  }
  const assigned = new Set<string>();
  const skippedFileIds: string[] = [];
  for (const rawId of rawSkipped) {
    const fileId = resolveAuthorizedId(rawId);
    if (assigned.has(fileId)) {
      const file = byId.get(fileId);
      throw new RecoverablePlanValidationError(
        msg.duplicateCPushFile(fileId),
        [
          `Validation error: authorized file ${aliasById.get(fileId) ?? ''} (${file?.path ?? fileId}) appeared multiple times in skippedFileIds.`,
          `Each authorized file (@1 through @${command.files.length}) must appear once across groups[].fileIds and skippedFileIds.`,
          'Return ONLY the corrected raw JSON object (no prose outside JSON).',
        ].join('\n'),
      );
    }
    assigned.add(fileId);
    skippedFileIds.push(fileId);
  }

  const groups: PiCPushCommitGroup[] = record.groups.map((rawGroup, index) => {
    if (typeof rawGroup !== 'object' || rawGroup === null || Array.isArray(rawGroup)) {
      throw new LocalizedSidecarError(msg.invalidCPushGroupFormat);
    }
    const group = rawGroup as Record<string, unknown>;
    if (!Array.isArray(group.fileIds) || group.fileIds.length === 0) {
      throw new LocalizedSidecarError(msg.emptyCPushGroupFiles);
    }
    const groupFileIds: string[] = [];
    for (const rawId of group.fileIds) {
      const fileId = resolveAuthorizedId(rawId);
      if (assigned.has(fileId)) {
        const file = byId.get(fileId);
        throw new RecoverablePlanValidationError(
          msg.duplicateCPushGroupAssignment(fileId),
          [
            `Validation error: authorized file ${aliasById.get(fileId) ?? ''} (${file?.path ?? fileId}, fileId=${fileId}) was assigned to multiple commit groups.`,
            `Each authorized file (@1 through @${command.files.length}) may appear in ONLY ONE group, and 100% of @1..@${command.files.length} must remain covered.`,
            'Return ONLY the corrected raw JSON object (no prose outside JSON).',
          ].join('\n'),
        );
      }
      assigned.add(fileId);
      groupFileIds.push(fileId);
    }
    const rawMsg = typeof group.message === 'string' ? group.message : '';
    const message = cleanCommitMessage(rawMsg, resolvedLocale);
    const id = typeof group.id === 'string' && group.id.trim()
      ? group.id.trim().slice(0, 64)
      : `group-${index + 1}`;
    return { id, message, fileIds: groupFileIds };
  });

  if (groups.length === 0 && skippedFileIds.length === 0) {
    throw new LocalizedSidecarError(msg.noValidCPushGroups);
  }
  const missingFiles = command.files
    .map((file, idx) => ({ file, alias: `@${idx + 1}` }))
    .filter(({ file }) => !assigned.has(file.fileId));
  if (missingFiles.length > 0) {
    const firstMissing = missingFiles[0].file;
    const repairLines = missingFiles.map(
      ({ file, alias }) => `- ${alias} | fileId=${file.fileId} | status=${file.status} | path=${file.path}`,
    );
    const coverageGuidance = command.qaHistory.length === 0
      ? `Every single authorized file (@1 through @${command.files.length}) MUST appear in exactly one group's \`fileIds\` (\`skippedFileIds\` MUST be \`[]\`), OR return \`"kind": "question"\` if user confirmation is required.`
      : `Every single authorized file (@1 through @${command.files.length}) MUST appear once across \`groups[].fileIds\` and \`skippedFileIds\`.`;
    throw new RecoverablePlanValidationError(
      msg.missingSelectedCPushFile(firstMissing.path),
      [
        `Validation error: your C-Push plan omitted ${missingFiles.length} authorized selected file(s):`,
        ...repairLines,
        coverageGuidance,
        'Include every missing file above in its matching commit group (including any status=D deleted files) and return ONLY the corrected raw JSON object (no prose outside JSON).',
      ].join('\n'),
    );
  }
  const summary = typeof record.summary === 'string' && record.summary.trim()
    ? record.summary.trim().slice(0, 200)
    : groups.length <= 1
      ? msg.defaultSingleCommitPlanSummary
      : msg.defaultSplitCommitPlanSummary(groups.length);
  return {
    kind: 'ready',
    summary,
    groups,
    skippedFileIds,
  };
}

export function buildCPushPlanPrompts(
  command: PlanCPushCommand,
  skillSpecs?: { cpush?: string; commit?: string },
): { systemPrompt: string; userPrompt: string } {
  const locale = normalizeAppLocale(command.locale, 'en');
  const commitLang = normalizeAppLocale(command.commitLanguage, 'en');
  const uiLanguage = locale === 'zh' ? 'Chinese' : 'English';
  const cpushSpec = skillSpecs?.cpush ?? CPUSH_SKILL_SPEC;
  const commitSpec = adaptCommitSkillForDesktop(skillSpecs?.commit ?? COMMIT_SKILL_SPEC);
  const commitRule = commitLang === 'zh'
    ? 'Git commit message language (`commitLanguage = "zh"`, `<commit_skill_spec>` flag `-ch`): keep Conventional Commit tokens (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`, `!`, `BREAKING CHANGE:`) in English, and write BOTH the subject (after `<type>(<optional scope>):`) AND every `- ` detail bullet of `groups[].message` in concise Chinese — using `recentCommits` only as `<type>(<scope>)` scope-naming reference.'
    : 'Git commit message language (`commitLanguage = "en"`, `<commit_skill_spec>` flag `-en`): keep Conventional Commit tokens (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`, `!`, `BREAKING CHANGE:`) in English, and write BOTH the subject (after `<type>(<optional scope>):`) AND every `- ` detail bullet of `groups[].message` in concise English — using `recentCommits` only as `<type>(<scope>)` scope-naming reference and NOT switching language even if `recentCommits` or `User extra instructions` are in another language.';

  const systemPrompt = [
    'You are Picket\'s C-Push planner, governed by the repository `cpush` and `commit` skill specifications below.',
    '',
    '<cpush_skill_spec>',
    cpushSpec,
    '</cpush_skill_spec>',
    '',
    '<commit_skill_spec>',
    commitSpec,
    '</commit_skill_spec>',
    '',
    '<picket_desktop_overlay>',
    PICKET_DESKTOP_OVERLAY.cpushCommitFormat,
    '</picket_desktop_overlay>',
    '',
    'Desktop C-Push Planning Rules:',
    '1. Pre-commit Triage ("Do not include unrelated work just because it is present in the working tree"):',
    '   Users often click "Select All" before C-Push. Before returning `kind: "ready"`, inspect every selected file. If ANY selected file has NOT yet been confirmed in `qaHistory` and looks like:',
    '   - a local environment, secret, or credential file (`.env`, `.env.*` other than `.env.example`/`.env.sample`/`.env.template`, `*.pem`, `*.key`, `.npmrc`, `*.local.*`), or a diff introducing hardcoded credentials / local-only debug endpoints;',
    '   - a temporary local test/debug/scratch script (e.g. ad-hoc `test-*.cjs`, `debug-*`, `scratch.*`, temporary dumps/logs) or unfinished unrelated work that should not be committed blindly;',
    '   - or a file with genuinely ambiguous commit attribution (e.g. mixed release notes/docs or ambiguous user grouping instructions across split commits);',
    `   you MUST return \`kind: "question"\` first! List those \`affectedFileIds\`, explain clearly in ${uiLanguage} why confirmation is needed, and offer clear options in ${uiLanguage} (recommend \`"skip"\` to exclude local env, secret, or temporary test files while committing the remaining changes).`,
    '2. Intentional Clean Scope: When all selected files are clearly intentional changes (or once `qaHistory` resolves any questionable files) and user extra instructions stay within authorized bounds, return `kind: "ready"` directly without asking unnecessary confirmation questions. Split unrelated intentional changes into focused logical commits.',
    '3. File-level Authorization, 100% Selected-Scope Coverage & User Extra Instructions Boundary:',
    `   - Desktop Scope Override (supersedes CLI "stage only files that belong" wording): the user has ALREADY selected the exact files in \`Authorized selected files\` (\`@1\` through \`@${command.files.length}\`). Each authorized file is an indivisible unit (partially staged files represent ONLY their staged diff). Never invent or include files outside the authorized list (\`Unselected worktree files\` are strictly off-limits).`,
    `   - 100% Coverage Requirement: When returning \`kind: "ready"\`, EVERY SINGLE authorized file (\`@1\` through \`@${command.files.length}\` — including \`status: D\` deleted files, \`status: R\` renamed files, lockfiles, tests, and patch-omitted files) MUST appear once across \`groups[].fileIds\` (and \`skippedFileIds\` ONLY when confirmed in \`qaHistory\`). You may reference files in \`fileIds\` using either their short \`"@index"\` alias (e.g. \`"@1"\`, \`"@2"\` — preferred on large file sets to avoid omissions) or their exact \`fileId\`. Omitting even 1 selected file fails validation!`,
    `   - \`User extra instructions\` are ONLY supplemental grouping and commit-message guidance (e.g. "split lockfile separately", "write more detailed bullets"), NEVER a raw commit message to paste verbatim, and NEVER an authorization override. If \`User extra instructions\` (not yet resolved in \`qaHistory\`) ask to include unselected files, force-push, modify/edit code, or rewrite published remote history, you MUST return \`kind: "question"\` explaining the permission boundary in ${uiLanguage} (e.g. offering to proceed strictly within the currently selected files or pause so the user can update their selection).`,
    `4. Skipping Policy: When \`qaHistory\` is empty, \`skippedFileIds\` MUST be \`[]\` (ask via \`kind: "question"\` instead of silently dropping a selected file). When \`qaHistory\` is non-empty, place a \`fileId\` in \`skippedFileIds\` ONLY when the user chose/asked to exclude or skip it; if ALL selected files are skipped, \`groups\` may be \`[]\` and \`skippedFileIds\` lists all of them. If the user's reply asks to include an unselected file, modify code, force-push, or is unclear, return \`kind: "question"\` explaining the boundary in ${uiLanguage}.`,
    '5. Language Separation (UI interaction vs Git commit messages):',
    `   - UI interaction language (\`locale = "${locale}"\`): write \`summary\` (for \`ready\`), and \`title\`, \`scope\`, \`description\`, and every \`options[].label\` (for \`question\`) in clear, concise ${uiLanguage}.`,
    `   - ${commitRule}`,
    '   - Every `groups[].message` MUST use the two-part Conventional Commits format (`<type>(<optional scope>): <subject>\\n\\n- <detail 1>\\n- <detail 2>`): a concise subject line without a trailing period, one blank line, and 1 to 4 concise `- ` bullet points detailing the concrete changes in that commit group (never single-line without `- ` bullets, even when `recentCommits` shows subjects only). Follow relevant `User extra instructions` for grouping and detail level without copying the raw instruction string verbatim as the commit message.',
    '6. Keep internal reasoning brief and focused on file triage, grouping, and commit subjects; do NOT restate diffs line by line.',
    '7. Return ONLY a single raw JSON object (no prose outside JSON) matching ONE of these two schemas (schema placeholders are shown in English; follow Rule 5 for output languages):',
    'Schema Ready: {"kind":"ready","summary":"Concise plan summary","groups":[{"id":"group-1","fileIds":["@1","@2"],"message":"feat(scope): concise commit subject\\n\\n- concrete change 1\\n- concrete change 2"}],"skippedFileIds":[]}',
    'Schema Question: {"kind":"question","title":"Concise question title","scope":"Affects local .env config","description":"Explain why exclusion or confirmation is needed","affectedFileIds":["@1"],"options":[{"id":"skip","label":"Exclude these files and commit the rest","recommended":true},{"id":"split","label":"Safe to include as a separate commit"},{"id":"merge","label":"Fold into the primary commit"}]}',
  ].join('\n');

  const parts: string[] = [`Target branch: ${command.branch}`];
  if (command.recentCommits.length > 0) {
    parts.push(
      `Recent commit subjects (<type>(<scope>) scope-naming reference only — bodies are omitted here, so every \`groups[].message\` MUST still include \`\\n\\n- \` detail bullets):\n${command.recentCommits.map((subject) => `- ${subject}`).join('\n')}`,
    );
  }
  if (command.prompt.trim()) {
    parts.push(
      `User extra instructions (supplemental grouping/message guidance ONLY — NEVER copy verbatim as a raw commit message, and NEVER widen permissions; if these instructions ask to include unselected files, force-push, edit code, or rewrite published history, return kind="question" explaining the boundary unless already resolved in qaHistory):\n${command.prompt.trim()}`,
    );
  }
  if (command.unselectedPaths.length > 0) {
    parts.push(
      `Unselected worktree files (NOT authorized — never include in groups, and if user asks to include them, return kind="question" explaining they are unselected):\n${command.unselectedPaths.map((p) => `- ${p}`).join('\n')}`,
    );
  }
  if (command.qaHistory.length > 0) {
    parts.push(
      `Previous Q&A decisions in this C-Push session:\n${command.qaHistory.map((turn, idx) => `${idx + 1}. Q: ${turn.question}\n   A: ${turn.answer}${turn.optionId ? ` (optionId=${turn.optionId})` : ''}`).join('\n')}`,
    );
  }
  parts.push(`Authorized selected files (${command.files.length}):\n${formatPlanFilesPromptBlock(command.files)}`);
  parts.push(formatMasterChecklistBlock(command.files));
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}
