import { Agent, type AgentTool } from '@earendil-works/pi-agent-core';
import { normalizeAppLocale, type AppLocale } from '../../shared/language-protocol';
import { MODEL_GLOBAL_DEFAULTS } from '../../shared/model-defaults';
import type {
  GenerateCustomActionCommand,
  ListModelsRequest,
  PiActionLink,
  PiConflictFile,
  PiConflictResolution,
  PiCPushCommitGroup,
  PiCPushPlan,
  PiCPushQuestionOption,
  PiCustomActionDraft,
  PiDiffFile,
  PiPlanFileEntry,
  PiResolvedConflictFile,
  PiReviewFinding,
  PiTaskRequest,
  PiToolResultCommand,
  PlanCPushCommand,
  ProbeEvent,
  ResolveConflictCommand,
  ReviewScopeCommand,
  RunCustomActionCommand,
  TestConnectionRequest,
} from '../../shared/pi-protocol';
import { COMMIT_SKILL_SPEC, CPUSH_SKILL_SPEC, DCR_SKILL_SPEC } from './git-skills';
import { LocalizedSidecarError, getSidecarMessages } from './i18n';
import { createProbe } from './probe';
import { createIsolatedModelRuntime, fetchProviderModels, sanitizeProviderError } from './providers';
import {
  PICKET_DESKTOP_OVERLAY,
  SkillRegistry,
  adaptCommitSkillForDesktop,
} from './skills';
import {
  ToolBridge,
  createAskUserQuestionTool,
  createCheckCliEnvTool,
  createCleanupSubmitDuplicatesTool,
  createCommitExactScopeTool,
  createInstallPlatformCliTool,
  createManageSubmitWorktreeTool,
  createReadRepoFileTool,
  createReadScopeDiffTool,
  createReadSkillDocTool,
  createRecordActionCheckpointTool,
  createRunGitTool,
  createRunPlatformCliTool,
  createSearchRepoTool,
  createSyncAndPushTool,
} from './tools';

function formatDiffPromptBlock(files: PiDiffFile[], skippedPaths: string[]): string {
  const blocks = files.map((file) => {
    const rename = file.previousPath ? ` (renamed from ${file.previousPath})` : '';
    return [
      `### File: ${file.path}${rename}`,
      `- fileId: ${file.fileId}`,
      `- status: ${file.status} (${file.rangeKind}, +${file.added} -${file.removed})`,
      '```diff',
      file.patch.trimEnd(),
      '```',
    ].join('\n');
  });
  if (skippedPaths.length > 0) {
    blocks.push(`### Additional selected entries not expanded inline:\n${skippedPaths.map((item) => `- ${item}`).join('\n')}`);
  }
  return blocks.join('\n\n');
}

function stripReasoningBlocks(raw: string): string {
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

function extractJsonObject(raw: string): unknown {
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

class RecoverablePlanValidationError extends LocalizedSidecarError {
  readonly repairPrompt: string;

  constructor(message: string, repairPrompt: string) {
    super(message);
    this.name = 'RecoverablePlanValidationError';
    this.repairPrompt = repairPrompt;
  }
}

function formatPlanFilesPromptBlock(files: PiPlanFileEntry[]): string {
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
      header.push(`(Patch body omitted; assign ${alias} / ${file.fileId} to its logical commit group based on path/status.)`);
    }
    return header.join('\n');
  }).join('\n\n');
}

function formatMasterChecklistBlock(files: PiPlanFileEntry[]): string {
  const lines = files.map((file, index) => {
    const statusTag = file.status === 'D' ? 'D (deleted)' : file.status;
    return `- @${index + 1} | ${file.fileId} | ${statusTag} | ${file.path}`;
  });
  return [
    `Master Checklist of ALL ${files.length} Authorized Selected Files (when returning kind="ready", 100% of @1..@${files.length} — including every D deleted file and patch-omitted file — MUST appear once in groups[].fileIds${files.length > 1 ? ' across your groups' : ''}; you may use either "@index" aliases like "@1" or exact fileIds in fileIds):`,
    ...lines,
  ].join('\n');
}

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

const TEST_CONNECTION_HTTP_TIMEOUT_MS = 45_000;
const TASK_HTTP_CEILING_TIMEOUT_MS = 600_000;
const ACTIVITY_HEARTBEAT_INTERVAL_MS = 1_000;
const MAX_REVIEW_TOOL_CALLS = 24;
const MAX_CUSTOM_ACTION_TOOL_CALLS = 80;
const MAX_GENERATE_ACTION_TOOL_CALLS = 16;

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

export interface TaskRunnerHandle {
  cancel: () => void;
  deliverToolResult?: (result: PiToolResultCommand) => void;
  run: () => Promise<void>;
}

function runAgentTask(
  command:
    | TestConnectionRequest
    | ReviewScopeCommand
    | PlanCPushCommand
    | ResolveConflictCommand
    | RunCustomActionCommand
    | GenerateCustomActionCommand,
  emit: (event: ProbeEvent) => void,
): TaskRunnerHandle {
  const envelope = { requestId: command.requestId, context: command.context };
  const locale = normalizeAppLocale(command.locale, 'en');
  const msg = getSidecarMessages(locale);
  const bridge = new ToolBridge(envelope, emit);
  let cancelled = false;
  let activeAgent: Agent | undefined;
  let lastActivitySentAt = 0;

  const emitActivity = (force = false) => {
    const now = Date.now();
    if (force || now - lastActivitySentAt >= ACTIVITY_HEARTBEAT_INTERVAL_MS) {
      lastActivitySentAt = now;
      emit({ ...envelope, type: 'activity' });
    }
  };

  return {
    cancel: () => {
      cancelled = true;
      bridge.abortAll('Task cancelled');
      activeAgent?.abort();
    },
    deliverToolResult: (result) => {
      bridge.deliverToolResult(result);
    },
    async run() {
      emit({ ...envelope, type: 'started' });
      let unsubscribe: (() => void) | undefined;
      try {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        const { models, model } = createIsolatedModelRuntime(command.model, locale);
        const isTest = command.type === 'testConnection';

        const overrideCatalog = command.type === 'reviewScope'
          || command.type === 'planCPush'
          || command.type === 'runCustomAction'
          || command.type === 'generateCustomAction'
          ? command.skillOverrides
          : undefined;
        const registry = new SkillRegistry(overrideCatalog, async (skill, path) => {
          const response = await bridge.invokeHostTool('read_skill_override_doc', { skill, path });
          return response.ok && !response.isError ? response.content : null;
        });

        let prompts: { systemPrompt: string; userPrompt: string };
        let tools: AgentTool<any>[] = [];
        let toolBudget = 0;

        if (isTest) {
          prompts = { systemPrompt: 'Reply with OK only.', userPrompt: 'Connection check.' };
        } else if (command.type === 'reviewScope') {
          const dcrDoc = await registry.readDoc('dcr', 'SKILL.md');
          prompts = buildReviewPrompts(command, dcrDoc.content);
          if (command.enableTools) {
            tools = [
              createReadScopeDiffTool(bridge),
              createReadRepoFileTool(bridge),
              createSearchRepoTool(bridge),
              createReadSkillDocTool(registry),
            ];
            toolBudget = MAX_REVIEW_TOOL_CALLS;
          }
        } else if (command.type === 'planCPush') {
          const [cpushDoc, commitDoc] = await Promise.all([
            registry.readDoc('cpush', 'SKILL.md'),
            registry.readDoc('commit', 'SKILL.md'),
          ]);
          prompts = buildCPushPlanPrompts(command, {
            cpush: cpushDoc.content,
            commit: commitDoc.content,
          });
        } else if (command.type === 'runCustomAction') {
          prompts = buildCustomActionPrompts(command, registry);
          tools = [
            createReadSkillDocTool(registry),
            createCheckCliEnvTool(bridge),
            createInstallPlatformCliTool(bridge),
            createRunGitTool(bridge, false),
            createRunPlatformCliTool(bridge),
            createCommitExactScopeTool(bridge),
            createSyncAndPushTool(bridge),
            createManageSubmitWorktreeTool(bridge),
            createCleanupSubmitDuplicatesTool(bridge),
            createRecordActionCheckpointTool(bridge),
            createReadRepoFileTool(bridge),
            createSearchRepoTool(bridge),
            createAskUserQuestionTool(bridge),
          ];
          toolBudget = MAX_CUSTOM_ACTION_TOOL_CALLS;
        } else if (command.type === 'generateCustomAction') {
          prompts = buildGenerateCustomActionPrompts(command, registry);
          tools = [
            createReadSkillDocTool(registry),
            createCheckCliEnvTool(bridge),
            createRunGitTool(bridge, true),
            createReadRepoFileTool(bridge),
            createSearchRepoTool(bridge),
            createAskUserQuestionTool(bridge),
          ];
          toolBudget = MAX_GENERATE_ACTION_TOOL_CALLS;
        } else {
          prompts = buildResolveConflictPrompts(command);
        }

        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }

        let toolCallsExecuted = 0;
        const agent = new Agent({
          sessionId: command.context.sessionId,
          identity: { attribution: null },
          initialState: {
            model,
            tools,
            thinkingLevel: 'off',
            systemPrompt: prompts.systemPrompt,
          },
          beforeToolCall: toolBudget > 0
            ? async () => {
                if (toolCallsExecuted >= toolBudget) {
                  return {
                    block: true,
                    reason: `Tool call budget (${toolBudget} calls) reached. Do not call more tools; return your final JSON response now using the evidence already collected.`,
                  };
                }
                toolCallsExecuted += 1;
                return undefined;
              }
            : undefined,
          streamFn: (targetModel, context, options) => models.streamSimple(targetModel, context, {
            ...options,
            maxTokens: isTest
              ? Math.min(MODEL_GLOBAL_DEFAULTS.testConnectionMaxOutputTokens, targetModel.maxTokens)
              : targetModel.maxTokens,
            ...(targetModel.api === 'openai-responses'
              && /(?:kimi|k2|k3|\bo1\b|\bo3\b|\bo4\b|gpt-5|reasoner|codex)/i.test(targetModel.id)
              ? { samplingParams: { reasoning: { effort: 'low' } } }
              : {}),
            temperature: isTest ? 0.2 : 0.1,
            maxRetries: isTest ? 0 : 1,
            timeoutMs: isTest ? TEST_CONNECTION_HTTP_TIMEOUT_MS : TASK_HTTP_CEILING_TIMEOUT_MS,
            cacheRetention: 'none',
            disableContinuation: true,
            onResponse: () => {
              emitActivity(true);
            },
            onProviderStreamEvent: () => {
              emitActivity(false);
            },
          }),
        });
        activeAgent = agent;
        unsubscribe = agent.subscribe((event) => {
          if (event.type === 'message_update') {
            if (event.assistantMessageEvent.type === 'text_delta') {
              lastActivitySentAt = Date.now();
              emit({ ...envelope, type: 'textDelta', delta: event.assistantMessageEvent.delta });
            } else {
              emitActivity(false);
            }
          } else if (
            event.type === 'tool_execution_start'
            || event.type === 'tool_execution_update'
            || event.type === 'tool_execution_end'
          ) {
            emitActivity(true);
          }
        });

        const runPromptTurn = async (promptText: string) => {
          if (cancelled) return { status: 'cancelled' as const };
          await agent.prompt(promptText);
          await agent.waitForIdle();
          if (cancelled) return { status: 'cancelled' as const };
          const last = agent.state.messages.findLast((message) => message.role === 'assistant');
          if (!last || last.role !== 'assistant') {
            throw new LocalizedSidecarError(msg.noResponseMessage);
          }
          if (last.stopReason === 'aborted') {
            return { status: 'cancelled' as const };
          }
          if (last.stopReason === 'error') {
            throw new Error(last.errorMessage ?? msg.providerReturnedError);
          }
          const rawText = last.content.flatMap((block) => (block.type === 'text' ? [block.text] : [])).join('');
          const hasThinkingOnly = !rawText.trim() && last.content.some((block) => block.type === 'thinking');
          if (hasThinkingOnly && last.stopReason === 'length') {
            throw new LocalizedSidecarError(msg.thinkingTokensExhausted);
          }
          return { status: 'ok' as const, stopReason: last.stopReason, rawText };
        };

        const wrapLengthTruncation = <T>(stopReason: string | undefined, parseFn: () => T): T => {
          try {
            return parseFn();
          } catch (err) {
            if (stopReason === 'length') {
              throw new LocalizedSidecarError(msg.lengthTruncatedJson);
            }
            throw err;
          }
        };

        const firstTurn = await runPromptTurn(prompts.userPrompt);
        if (firstTurn.status === 'cancelled') {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }

        if (command.type === 'testConnection') {
          emit({ ...envelope, type: 'completed', text: firstTurn.rawText.trim() || 'OK' });
        } else if (command.type === 'reviewScope') {
          const findings = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseReviewFindings(firstTurn.rawText, command.files, locale),
          );
          emit({ ...envelope, type: 'reviewCompleted', findings });
        } else if (command.type === 'planCPush') {
          let plan: PiCPushPlan;
          try {
            plan = wrapLengthTruncation(firstTurn.stopReason, () =>
              parseCPushPlan(firstTurn.rawText, command, locale),
            );
          } catch (err) {
            if (!(err instanceof RecoverablePlanValidationError) || firstTurn.stopReason === 'length') {
              throw err;
            }
            emitActivity(true);
            const repairTurn = await runPromptTurn(err.repairPrompt);
            if (repairTurn.status === 'cancelled') {
              emit({ ...envelope, type: 'cancelled' });
              return;
            }
            plan = wrapLengthTruncation(repairTurn.stopReason, () =>
              parseCPushPlan(repairTurn.rawText, command, locale),
            );
          }
          emit({ ...envelope, type: 'cPushPlanCompleted', plan });
        } else if (command.type === 'runCustomAction') {
          const result = parseCustomActionResult(firstTurn.rawText, locale);
          emit({ ...envelope, type: 'customActionCompleted', summary: result.summary, links: result.links });
        } else if (command.type === 'generateCustomAction') {
          const draft = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseCustomActionDraft(firstTurn.rawText, locale),
          );
          emit({ ...envelope, type: 'customActionDrafted', draft });
        } else {
          const resolution = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseConflictResolution(firstTurn.rawText, command, locale),
          );
          emit({ ...envelope, type: 'conflictResolved', resolution });
        }
      } catch (error) {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'failed', message: sanitizeProviderError(error, command.model, locale) });
      } finally {
        unsubscribe?.();
      }
    },
  };
}

function runListModelsTask(
  command: ListModelsRequest,
  emit: (event: ProbeEvent) => void,
): TaskRunnerHandle {
  const envelope = { requestId: command.requestId, context: command.context };
  const locale = normalizeAppLocale(command.locale, 'en');
  const controller = new AbortController();
  let cancelled = false;

  return {
    cancel: () => {
      cancelled = true;
      controller.abort();
    },
    async run() {
      emit({ ...envelope, type: 'started' });
      const timer = setTimeout(() => controller.abort(), 30_000);
      try {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        const models = await fetchProviderModels(command.model, controller.signal, locale);
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'modelsListed', models });
      } catch (error) {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'failed', message: sanitizeProviderError(error, command.model, locale) });
      } finally {
        clearTimeout(timer);
      }
    },
  };
}

export function createTaskRunner(command: PiTaskRequest, emit: (event: ProbeEvent) => void): TaskRunnerHandle {
  if (command.type === 'probe') {
    return createProbe(command, emit);
  }
  if (command.type === 'listModels') {
    return runListModelsTask(command, emit);
  }
  return runAgentTask(command, emit);
}
