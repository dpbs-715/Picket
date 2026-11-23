import { Agent } from '@earendil-works/pi-agent-core';
import type {
  GenerateCommitCommand,
  PiConflictFile,
  PiConflictResolution,
  PiCPushCommitGroup,
  PiCPushPlan,
  PiCPushQuestionOption,
  PiDiffFile,
  PiPlanFileEntry,
  PiResolvedConflictFile,
  PiReviewFinding,
  PiTaskRequest,
  PlanCPushCommand,
  ProbeEvent,
  ResolveConflictCommand,
  ReviewScopeCommand,
  TestConnectionRequest,
} from '../../shared/pi-protocol';
import { MODEL_GLOBAL_DEFAULTS } from '../../shared/model-defaults';
import { COMMIT_SKILL_SPEC, CPUSH_SKILL_SPEC } from './git-skills';
import { createProbe } from './probe';
import { createIsolatedModelRuntime, sanitizeProviderError } from './providers';

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

export function cleanCommitMessage(raw: string): string {
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
    throw new Error('模型返回了空的提交说明');
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
    throw new Error('模型未返回有效的结构化审查 JSON');
  }
}

function formatPlanFilesPromptBlock(files: PiPlanFileEntry[]): string {
  return files.map((file) => {
    const rename = file.previousPath ? ` (renamed from ${file.previousPath})` : '';
    const header = [
      `### Authorized File: ${file.path}${rename}`,
      `- fileId: ${file.fileId}`,
      `- status: ${file.status} (${file.rangeKind}, +${file.added} -${file.removed})`,
    ];
    if (file.skipReason) {
      header.push(`- note: ${file.skipReason}`);
    }
    if (file.patch && file.patch.trim()) {
      header.push('```diff', file.patch.trimEnd(), '```');
    } else {
      header.push('(Patch body omitted; assign this fileId to its logical commit group based on path/status.)');
    }
    return header.join('\n');
  }).join('\n\n');
}

export function parseCPushPlan(raw: string, command: PlanCPushCommand): PiCPushPlan {
  let parsed: unknown;
  try {
    parsed = extractJsonObject(raw);
  } catch {
    throw new Error('模型未返回有效的 C-Push 结构化规划 JSON');
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new Error('模型返回的 C-Push 规划不是 JSON 对象');
  }
  const record = parsed as Record<string, unknown>;
  const byId = new Map(command.files.map((file) => [file.fileId, file]));
  const byPath = new Map(command.files.map((file) => [file.path, file]));
  const resolveAuthorizedId = (token: unknown): string => {
    if (typeof token !== 'string' || !token.trim()) {
      throw new Error('C-Push 规划包含空的文件引用');
    }
    const clean = token.trim();
    const matched = byId.get(clean) ?? byPath.get(clean);
    if (!matched) {
      throw new Error(`模型尝试将未授权文件（${clean}）纳入 C-Push 计划`);
    }
    return matched.fileId;
  };

  if (record.kind === 'question') {
    const title = typeof record.title === 'string' ? record.title.trim().slice(0, 180) : '';
    const scope = typeof record.scope === 'string' ? record.scope.trim().slice(0, 120) : '';
    const description = typeof record.description === 'string' ? record.description.trim().slice(0, 800) : '';
    if (!title || !description) {
      throw new Error('C-Push 提问缺少标题或具体说明');
    }
    if (!Array.isArray(record.options) || record.options.length === 0 || record.options.length > 6) {
      throw new Error('C-Push 提问必须提供 1 至 6 个明确选项');
    }
    const seenOptionIds = new Set<string>();
    const options: PiCPushQuestionOption[] = record.options.map((rawOpt, index) => {
      if (typeof rawOpt !== 'object' || rawOpt === null || Array.isArray(rawOpt)) {
        throw new Error('C-Push 提问选项格式无效');
      }
      const opt = rawOpt as Record<string, unknown>;
      const id = typeof opt.id === 'string' && opt.id.trim() ? opt.id.trim().slice(0, 64) : `option-${index + 1}`;
      const label = typeof opt.label === 'string' ? opt.label.trim().slice(0, 120) : '';
      if (!label || seenOptionIds.has(id)) {
        throw new Error('C-Push 提问选项缺少标签或存在重复 ID');
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
        const matched = byId.get(token.trim()) ?? byPath.get(token.trim());
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
    throw new Error('C-Push 规划结果缺少有效的 kind（须为 ready 或 question）');
  }
  if (!Array.isArray(record.groups)) {
    throw new Error('C-Push 规划缺少 groups 数组');
  }
  const rawSkipped = Array.isArray(record.skippedFileIds) ? record.skippedFileIds : [];
  if (rawSkipped.length > 0 && command.qaHistory.length === 0) {
    throw new Error('未经用户确认，C-Push 不能擅自跳过已勾选文件');
  }
  const assigned = new Set<string>();
  const skippedFileIds: string[] = [];
  for (const rawId of rawSkipped) {
    const fileId = resolveAuthorizedId(rawId);
    if (assigned.has(fileId)) {
      throw new Error(`文件 ${fileId} 在 C-Push 计划中重复出现`);
    }
    assigned.add(fileId);
    skippedFileIds.push(fileId);
  }

  const groups: PiCPushCommitGroup[] = record.groups.map((rawGroup, index) => {
    if (typeof rawGroup !== 'object' || rawGroup === null || Array.isArray(rawGroup)) {
      throw new Error('C-Push 提交分组格式无效');
    }
    const group = rawGroup as Record<string, unknown>;
    if (!Array.isArray(group.fileIds) || group.fileIds.length === 0) {
      throw new Error('C-Push 每个提交分组至少需包含一个已授权文件');
    }
    const groupFileIds: string[] = [];
    for (const rawId of group.fileIds) {
      const fileId = resolveAuthorizedId(rawId);
      if (assigned.has(fileId)) {
        throw new Error(`文件 ${fileId} 被分配到了多个提交分组`);
      }
      assigned.add(fileId);
      groupFileIds.push(fileId);
    }
    const rawMsg = typeof group.message === 'string' ? group.message : '';
    const message = cleanCommitMessage(rawMsg);
    const id = typeof group.id === 'string' && group.id.trim()
      ? group.id.trim().slice(0, 64)
      : `group-${index + 1}`;
    return { id, message, fileIds: groupFileIds };
  });

  if (groups.length === 0 && skippedFileIds.length === 0) {
    throw new Error('C-Push 计划未包含任何有效分组');
  }
  for (const file of command.files) {
    if (!assigned.has(file.fileId)) {
      throw new Error(`已选文件 ${file.path} 未被纳入 C-Push 分组计划`);
    }
  }
  const summary = typeof record.summary === 'string' && record.summary.trim()
    ? record.summary.trim().slice(0, 200)
    : groups.length <= 1
      ? '已生成单笔提交计划'
      : `已规划拆分为 ${groups.length} 个独立提交`;
  return {
    kind: 'ready',
    summary,
    groups,
    skippedFileIds,
  };
}

export function parseReviewFindings(raw: string, files: PiDiffFile[]): PiReviewFinding[] {
  let parsed: unknown;
  try {
    parsed = extractJsonObject(raw);
  } catch {
    throw new Error('模型未返回有效的结构化审查结果');
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed) || !Array.isArray((parsed as { findings?: unknown }).findings)) {
    throw new Error('模型返回的审查结果缺少 findings 数组');
  }
  const byId = new Map(files.map((file) => [file.fileId, file]));
  const byPath = new Map(files.map((file) => [file.path, file]));
  const findings: PiReviewFinding[] = [];

  for (const item of (parsed as { findings: unknown[] }).findings) {
    if (typeof item !== 'object' || item === null || Array.isArray(item)) {
      throw new Error('审查结果项格式无效');
    }
    const record = item as Record<string, unknown>;
    const matched = (typeof record.fileId === 'string' ? byId.get(record.fileId) : undefined)
      ?? (typeof record.path === 'string' ? byPath.get(record.path) : undefined);
    // Ignore hallucinated findings pointing outside the explicitly authorized review files.
    if (!matched) continue;

    const rawSeverity = typeof record.severity === 'string' ? record.severity.toLowerCase().trim() : '';
    const severity: 'high' | 'suggestion' = rawSeverity === 'high' || rawSeverity === 'critical' || rawSeverity === 'error'
      ? 'high'
      : rawSeverity === 'suggestion' || rawSeverity === 'medium' || rawSeverity === 'low' || rawSeverity === 'warning' || rawSeverity === 'info'
        ? 'suggestion'
        : (() => { throw new Error('审查结果包含未知严重程度'); })();

    const line = typeof record.line === 'number' && Number.isFinite(record.line)
      ? Math.max(1, Math.trunc(record.line))
      : 1;
    const title = typeof record.title === 'string' ? record.title.trim().slice(0, 180) : '';
    const reason = typeof record.reason === 'string' ? record.reason.trim().slice(0, 800) : '';
    if (!title || !reason) {
      throw new Error('审查结果缺少标题或原因说明');
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

function buildGeneratePrompts(command: GenerateCommitCommand): { systemPrompt: string; userPrompt: string } {
  const systemPrompt = [
    'You are Picket\'s Git commit message generator, governed by the repository `commit` skill specification below.',
    '',
    '<commit_skill_spec>',
    COMMIT_SKILL_SPEC,
    '</commit_skill_spec>',
    '',
    'Desktop Adapter Rules:',
    '1. Write ONE concise Conventional Commits message (`<type>(<optional scope>): <subject>`, without a trailing period) strictly for the supplied diff scope.',
    '2. Follow the `commit` skill\'s body policy: keep single-line by default, and add a blank line + concise body ONLY when the reason, migration impact, or non-obvious context would be lost from the subject alone.',
    '3. Keep Conventional Commit tokens (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`, `!`, `BREAKING CHANGE:`) in English. Match the natural language (Chinese vs English) and `<type>(<scope>)` naming convention of recent commit subjects or the user draft; if neither indicates English, write the subject after the colon in concise Chinese.',
    '4. Describe ONLY the supplied diff changes. For partially staged files, only the staged diff is supplied; never guess or invent unstaged edits. Preserve user draft intent when provided.',
    '5. Output ONLY the raw commit message text (no `Committed <hash>:` wrapper, no markdown code fences, no extra commentary).',
  ].join('\n');

  const parts: string[] = [`Target branch: ${command.branch}`];
  if (command.recentCommits.length > 0) {
    parts.push(
      `Recent commit subjects (language and <type>(<scope>) naming reference):\n${command.recentCommits.map((subject) => `- ${subject}`).join('\n')}`,
    );
  }
  if (command.draft.trim()) {
    parts.push(`Existing user draft / notes:\n${command.draft.trim()}`);
  }
  parts.push(`Selected diff scope:\n${formatDiffPromptBlock(command.files, command.skippedPaths)}`);
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}

function buildReviewPrompts(command: ReviewScopeCommand): { systemPrompt: string; userPrompt: string } {
  const systemPrompt = [
    'You are Picket\'s pre-commit code reviewer.',
    'Review ONLY the supplied Git diff patches and report concrete defects, security/credential leaks, data loss hazards, concurrency/state bugs, or maintainability risks.',
    'Do NOT invent issues outside the supplied patches. Write titles and reasons in clear, concise Chinese.',
    'Return ONLY a single JSON object with this exact schema (no prose outside JSON):',
    '{"findings":[{"fileId":"<exact fileId from input>","path":"<exact path from input>","line":1,"severity":"high","title":"简短问题标题","reason":"具体原因与风险说明"}]}',
    'Use severity "high" only for real bugs, security/credential exposure, broken invariants, or data corruption; use "suggestion" for non-blocking improvements. Return {"findings":[]} if no issues are found.',
  ].join('\n');

  const parts: string[] = [`Branch: ${command.branch}`];
  if (command.draft.trim()) {
    parts.push(`Planned commit note:\n${command.draft.trim()}`);
  }
  parts.push(`Reviewable diff patches:\n${formatDiffPromptBlock(command.files, [])}`);
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}

function buildCPushPlanPrompts(command: PlanCPushCommand): { systemPrompt: string; userPrompt: string } {
  const systemPrompt = [
    'You are Picket\'s C-Push planner, governed by the repository `cpush` and `commit` skill specifications below.',
    '',
    '<cpush_skill_spec>',
    CPUSH_SKILL_SPEC,
    '</cpush_skill_spec>',
    '',
    '<commit_skill_spec>',
    COMMIT_SKILL_SPEC,
    '</commit_skill_spec>',
    '',
    'Desktop C-Push Planning Rules:',
    '1. Pre-commit Triage ("Do not include unrelated work just because it is present in the working tree"):',
    '   Users often click "Select All" before C-Push. Before returning `kind: "ready"`, inspect every selected file. If ANY selected file has NOT yet been confirmed in `qaHistory` and looks like:',
    '   - a local environment, secret, or credential file (`.env`, `.env.*` other than `.env.example`/`.env.sample`/`.env.template`, `*.pem`, `*.key`, `.npmrc`, `*.local.*`), or a diff introducing hardcoded credentials / local-only debug endpoints;',
    '   - a temporary local test/debug/scratch script (e.g. ad-hoc `test-*.cjs`, `debug-*`, `scratch.*`, temporary dumps/logs) or unfinished unrelated work that should not be committed blindly;',
    '   - or a file with genuinely ambiguous commit attribution (e.g. mixed release notes/docs or an ambiguous manual `draft` across split commits);',
    '   you MUST return `kind: "question"` first! List those `affectedFileIds`, explain clearly in Chinese why confirmation is needed, and offer clear options (recommend `"skip"` / `"排除这些文件，仅提交其余改动"` for local env, secret, or temporary test files).',
    '2. Intentional Clean Scope: When all selected files are clearly intentional changes (or once `qaHistory` resolves any questionable files), return `kind: "ready"` directly without asking unnecessary confirmation questions. Split unrelated intentional changes into focused logical commits.',
    '3. File-level Authorization Boundary: Each authorized `fileId` is an indivisible unit (partially staged files represent ONLY their staged diff). Never invent or include files outside the authorized list (`Unselected worktree files` are strictly off-limits).',
    '4. Skipping Policy: When `qaHistory` is empty, `skippedFileIds` MUST be `[]` (ask via `kind: "question"` instead of silently dropping a selected file). When `qaHistory` is non-empty, place a `fileId` in `skippedFileIds` ONLY when the user chose/asked to exclude or skip it; if ALL selected files are skipped, `groups` may be `[]` and `skippedFileIds` lists all of them. If the user\'s reply asks to include an unselected file, modify code, force-push, or is unclear, return `kind: "question"` explaining the boundary in Chinese.',
    '5. Commit Message Style (follow `<commit_skill_spec>`): Keep Conventional Commit tokens (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`, `!`, `BREAKING CHANGE:`) in English; match the subject language of `recentCommits` (or concise Chinese by default when recent commits are Chinese or absent). Write a concise `<type>(<optional scope>): <subject>` without a trailing period; include a blank line and brief body ONLY when the reason, migration impact, or non-obvious context would be lost from the subject alone. Preserve existing user `draft` intent on the relevant commit without duplicating it verbatim across unrelated commits.',
    '6. Keep internal reasoning brief and focused on file triage, grouping, and commit subjects; do NOT restate diffs line by line.',
    '7. Return ONLY a single raw JSON object (no prose outside JSON) matching ONE of these two schemas:',
    'Schema Ready: {"kind":"ready","summary":"简短规划说明","groups":[{"id":"group-1","fileIds":["<exact fileId>"],"message":"feat(scope): 简明提交标题"}],"skippedFileIds":[]}',
    'Schema Question: {"kind":"question","title":"简短问题标题","scope":"涉及本地配置 .env 等","description":"说明为何建议排除或需确认归属","affectedFileIds":["<exact fileId>"],"options":[{"id":"skip","label":"排除这些文件，仅提交其余改动","recommended":true},{"id":"split","label":"确认安全，单独提交并推送"},{"id":"merge","label":"合并进主提交"}]}',
  ].join('\n');

  const parts: string[] = [`Target branch: ${command.branch}`];
  if (command.recentCommits.length > 0) {
    parts.push(
      `Recent commit subjects (language and <type>(<scope>) naming reference):\n${command.recentCommits.map((subject) => `- ${subject}`).join('\n')}`,
    );
  }
  if (command.draft.trim()) {
    parts.push(`Existing manual user draft (preserve intent; do NOT duplicate verbatim across unrelated commits):\n${command.draft.trim()}`);
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
  return { systemPrompt, userPrompt: parts.join('\n\n') };
}

const CONFLICT_MARKER_LINE = /^(?:<{7}(?: |$)|={7}\r?$|>{7}(?: |$)|\|{7}(?: |$))/m;

export function hasConflictMarkers(text: string): boolean {
  return CONFLICT_MARKER_LINE.test(text);
}

export function parseConflictResolution(
  raw: string,
  command: ResolveConflictCommand,
): PiConflictResolution {
  let parsed: unknown;
  try {
    parsed = extractJsonObject(raw);
  } catch {
    throw new Error('模型未返回有效的冲突合并结构化 JSON');
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new Error('模型返回的冲突合并结果不是 JSON 对象');
  }
  const record = parsed as Record<string, unknown>;
  if (!Array.isArray(record.resolvedFiles) || record.resolvedFiles.length === 0) {
    throw new Error('模型未返回冲突文件的合并内容（缺少 resolvedFiles）');
  }

  const byId = new Map(command.files.map((file) => [file.fileId, file]));
  const byPath = new Map(command.files.map((file) => [file.path, file]));
  const seen = new Set<string>();
  const resolvedFiles: PiResolvedConflictFile[] = record.resolvedFiles.map((entry) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      throw new Error('冲突合并文件项格式无效');
    }
    const item = entry as Record<string, unknown>;
    const matched = (typeof item.fileId === 'string' ? byId.get(item.fileId.trim()) : undefined)
      ?? (typeof item.path === 'string' ? byPath.get(item.path.trim()) : undefined);
    if (!matched) {
      const targetName = typeof item.path === 'string' ? item.path : String(item.fileId ?? 'unknown');
      throw new Error(`模型尝试修改未授权的非冲突文件（${targetName}），已被拦截`);
    }
    if (seen.has(matched.fileId)) {
      throw new Error(`冲突文件 ${matched.path} 在合并结果中重复出现`);
    }
    seen.add(matched.fileId);
    if (typeof item.content !== 'string') {
      throw new Error(`冲突文件 ${matched.path} 缺少合并后的文本内容`);
    }
    const baseHadMarkers = matched.baseContent ? hasConflictMarkers(matched.baseContent) : false;
    if (!baseHadMarkers && hasConflictMarkers(item.content)) {
      throw new Error(`冲突文件 ${matched.path} 的合并结果仍残留冲突标记，已拒绝写入`);
    }
    if (
      !item.content.trim()
      && matched.upstreamContent.trim().length > 0
      && matched.localContent.trim().length > 0
    ) {
      throw new Error(`冲突文件 ${matched.path} 返回了空内容，为防止意外清空代码已中止合并`);
    }
    return {
      fileId: matched.fileId,
      path: matched.path,
      content: item.content,
    };
  });

  if (seen.size !== command.files.length) {
    throw new Error('模型未完整返回所有当前冲突文件的合并结果');
  }
  const summary = typeof record.summary === 'string' && record.summary.trim()
    ? record.summary.trim().slice(0, 240)
    : `已完成 ${resolvedFiles.length} 个冲突文件的合并`;
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

function buildResolveConflictPrompts(command: ResolveConflictCommand): { systemPrompt: string; userPrompt: string } {
  const systemPrompt = [
    'You are Picket\'s authorized Git conflict merge resolver.',
    'The user explicitly authorized resolving ONLY the listed conflicted files during a Git rebase.',
    'Rules:',
    '1. Resolve ONLY the conflict regions between the Remote Upstream Version and the Local Commit Version, preserving all non-conflicting lines verbatim.',
    '2. Never modify or output any file outside the authorized conflicted files list.',
    '3. If the user supplied specific merge instructions, follow them strictly within the conflicted files.',
    '4. Ensure the merged file content is syntactically coherent and contains ZERO leftover Git conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`, `|||||||`).',
    '5. Return ONLY a single raw JSON object (no prose outside JSON) with this exact schema:',
    '{"summary":"简短中文合并说明","resolvedFiles":[{"fileId":"<exact fileId>","path":"<exact path>","content":"<complete resolved file content>"}]}',
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

function runAgentTask(
  command: TestConnectionRequest | GenerateCommitCommand | ReviewScopeCommand | PlanCPushCommand | ResolveConflictCommand,
  emit: (event: ProbeEvent) => void,
) {
  const envelope = { requestId: command.requestId, context: command.context };
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
      activeAgent?.abort();
    },
    async run() {
      emit({ ...envelope, type: 'started' });
      let unsubscribe: (() => void) | undefined;
      try {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        const { models, model } = createIsolatedModelRuntime(command.model);
        const isTest = command.type === 'testConnection';
        const prompts = isTest
          ? { systemPrompt: 'Reply with OK only.', userPrompt: 'Connection check.' }
          : command.type === 'generateCommit'
            ? buildGeneratePrompts(command)
            : command.type === 'reviewScope'
              ? buildReviewPrompts(command)
              : command.type === 'planCPush'
                ? buildCPushPlanPrompts(command)
                : buildResolveConflictPrompts(command);

        const agent = new Agent({
          sessionId: command.context.sessionId,
          identity: { attribution: null },
          initialState: {
            model,
            tools: [],
            thinkingLevel: 'off',
            systemPrompt: prompts.systemPrompt,
          },
          streamFn: (targetModel, context, options) => models.streamSimple(targetModel, context, {
            ...options,
            maxTokens: isTest
              ? Math.min(MODEL_GLOBAL_DEFAULTS.testConnectionMaxOutputTokens, targetModel.maxTokens)
              : targetModel.maxTokens,
            ...(targetModel.api === 'openai-responses'
              && /(?:kimi|k2|k3|\bo1\b|\bo3\b|\bo4\b|gpt-5|reasoner|codex)/i.test(targetModel.id)
              ? { samplingParams: { reasoning: { effort: 'low' } } }
              : {}),
            temperature: command.type === 'reviewScope' || command.type === 'planCPush' || command.type === 'resolveConflict' ? 0.1 : 0.2,
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
          }
        });

        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        await agent.prompt(prompts.userPrompt);
        await agent.waitForIdle();
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }

        const last = agent.state.messages.findLast((message) => message.role === 'assistant');
        if (!last || last.role !== 'assistant') {
          throw new Error('模型未返回响应消息');
        }
        if (last.stopReason === 'aborted') {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        if (last.stopReason === 'error') {
          throw new Error(last.errorMessage ?? '模型服务返回错误');
        }

        const rawText = last.content.flatMap((block) => (block.type === 'text' ? [block.text] : [])).join('');
        const hasThinkingOnly = !rawText.trim() && last.content.some((block) => block.type === 'thinking');
        const wrapLengthTruncation = <T>(parseFn: () => T): T => {
          try {
            return parseFn();
          } catch (err) {
            if (last.stopReason === 'length') {
              throw new Error('模型输出达到长度上限被截断，未能生成完整的结构化结果；请缩小单次勾选文件范围后重试');
            }
            throw err;
          }
        };
        if (command.type === 'testConnection') {
          emit({ ...envelope, type: 'completed', text: rawText.trim() || 'OK' });
        } else if (hasThinkingOnly && last.stopReason === 'length') {
          throw new Error('模型思考内容耗尽了输出长度上限，未生成最终正文；请缩小所选变更范围或改用非深度思考模型重试');
        } else if (command.type === 'generateCommit') {
          emit({ ...envelope, type: 'completed', text: cleanCommitMessage(rawText) });
        } else if (command.type === 'reviewScope') {
          const findings = wrapLengthTruncation(() => parseReviewFindings(rawText, command.files));
          emit({ ...envelope, type: 'reviewCompleted', findings });
        } else if (command.type === 'planCPush') {
          const plan = wrapLengthTruncation(() => parseCPushPlan(rawText, command));
          emit({ ...envelope, type: 'cPushPlanCompleted', plan });
        } else {
          const resolution = wrapLengthTruncation(() => parseConflictResolution(rawText, command));
          emit({ ...envelope, type: 'conflictResolved', resolution });
        }
      } catch (error) {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'failed', message: sanitizeProviderError(error, command.model) });
      } finally {
        unsubscribe?.();
      }
    },
  };
}

export function createTaskRunner(command: PiTaskRequest, emit: (event: ProbeEvent) => void) {
  if (command.type === 'probe') {
    return createProbe(command, emit);
  }
  return runAgentTask(command, emit);
}
