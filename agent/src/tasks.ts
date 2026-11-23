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

function extractJsonObject(raw: string): unknown {
  const trimmed = stripReasoningBlocks(raw);
  const fenced = /```(?:json)?\s*\r?\n([\s\S]*?)\r?\n```/i.exec(trimmed);
  const candidate = fenced ? fenced[1].trim() : trimmed;
  try {
    return JSON.parse(candidate);
  } catch {
    const firstBrace = candidate.indexOf('{');
    const lastBrace = candidate.lastIndexOf('}');
    if (firstBrace >= 0 && lastBrace > firstBrace) {
      return JSON.parse(candidate.slice(firstBrace, lastBrace + 1));
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
    'You are Picket\'s Git commit message generator.',
    'Write ONE Conventional Commits message strictly for the supplied diff scope using this exact two-part format (first-line title, one blank line, then concise bullet points):',
    '<type>(<optional scope>): <concise summary>',
    '',
    '- <specific change or intent 1>',
    '- <specific change or intent 2>',
    '',
    'Rules:',
    '1. ALWAYS include BOTH the first-line title AND a blank line followed by 1 to 4 concise `- ` bullet points detailing the concrete changes and their purpose/impact, even when only one file changed or recent commit subjects are single-line. Do not repeat the title verbatim in the bullets.',
    '2. Match the natural language (e.g. Chinese vs English) and `<type>(<scope>)` naming convention of recent commit subjects when provided; if none are provided, write the summary and bullet points in concise Chinese after the Conventional Commit prefix.',
    '3. Describe ONLY the supplied diff changes. For partially staged files, only the staged diff is supplied; never guess or invent unstaged edits.',
    '4. If the user already wrote draft notes, preserve their intent and key details in the generated title and bullet points.',
    '5. Output ONLY the raw commit message text (title + blank line + `- ` bullet list) without markdown code fences, headings, or extra commentary.',
  ].join('\n');

  const parts: string[] = [`Target branch: ${command.branch}`];
  if (command.recentCommits.length > 0) {
    parts.push(
      `Recent commit subjects (reference for language and <type>(<scope>) naming only — you MUST still include the blank line and \`- \` bullet body below the title):\n${command.recentCommits.map((subject) => `- ${subject}`).join('\n')}`,
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
    'You are Picket\'s C-Push commit splitting planner.',
    'Analyze ONLY the authorized selected files and either:',
    '(A) Return a ready execution plan (`kind: "ready"`) when grouping and commit attribution are clear — DO NOT ask unnecessary questions when the files clearly belong to 1 commit or cleanly split into logical commits;',
    '(B) Return a concise user question (`kind: "question"`) ONLY when attribution is genuinely ambiguous (e.g. whether release notes / changelog / mixed docs should merge into a feature commit, split into a separate commit, or be skipped; OR when an existing manual user draft has ambiguous attribution across multiple split commits; OR when the user\'s latest answer is unclear or requests an unauthorized/unselected file or unsupported action).',
    '',
    'Strict Rules:',
    '1. File-level grouping ONLY: each authorized `fileId` is an indivisible unit (partially staged files represent ONLY their staged diff). Never invent files outside the authorized list.',
    '2. When `qaHistory` is empty, `skippedFileIds` MUST be `[]` and EVERY authorized `fileId` MUST appear in exactly one group of `groups`.',
    '3. When `qaHistory` is non-empty, follow the user\'s answer within the authorized scope. Only place a `fileId` in `skippedFileIds` if the user explicitly chose/asked to skip it (`暂不提交`). If the user\'s free-text reply asks to include an unselected file (listed under `Unselected worktree files`), modify code, force-push, or is contradictory/unclear, MUST return `kind: "question"` explaining the boundary clearly in Chinese and offering valid choices within the authorized files.',
    '4. Existing user draft (`draft`): use it as an intent/writing constraint for the relevant commit(s), NEVER copy the same raw manual note blindly onto multiple unrelated commits.',
    '5. Every `group.message` MUST use two-part Conventional Commits format: `<type>(<optional scope>): <concise summary>\\n\\n- <bullet 1>\\n- <bullet 2>` (1–4 concise bullets, matching recent commit language or Chinese by default).',
    '6. Return ONLY a single raw JSON object (no prose outside JSON) matching ONE of these two schemas:',
    'Schema Ready: {"kind":"ready","summary":"简短规划说明","groups":[{"id":"group-1","fileIds":["<exact fileId>"],"message":"feat(scope): 标题\\n\\n- 要点 1\\n- 要点 2"}],"skippedFileIds":[]}',
    'Schema Question: {"kind":"question","title":"简短问题标题","scope":"影响提交 2/2「docs」","description":"说明归属分歧及影响文件","affectedFileIds":["<exact fileId>"],"options":[{"id":"merge","label":"合并进提交 1"},{"id":"split","label":"单独提交并一起推送","recommended":true},{"id":"skip","label":"暂不提交"}]}',
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

function runAgentTask(
  command: TestConnectionRequest | GenerateCommitCommand | ReviewScopeCommand | PlanCPushCommand | ResolveConflictCommand,
  emit: (event: ProbeEvent) => void,
) {
  const envelope = { requestId: command.requestId, context: command.context };
  let cancelled = false;
  let activeAgent: Agent | undefined;

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
            maxTokens: isTest ? 24 : command.type === 'planCPush' || command.type === 'resolveConflict' ? 4_096 : 2_048,
            temperature: command.type === 'reviewScope' || command.type === 'planCPush' || command.type === 'resolveConflict' ? 0.1 : 0.2,
            maxRetries: isTest ? 0 : 1,
            timeoutMs: isTest ? 15_000 : 45_000,
            cacheRetention: 'none',
            disableContinuation: true,
          }),
        });
        activeAgent = agent;
        unsubscribe = agent.subscribe((event) => {
          if (event.type === 'message_update' && event.assistantMessageEvent.type === 'text_delta') {
            emit({ ...envelope, type: 'textDelta', delta: event.assistantMessageEvent.delta });
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
        if (command.type === 'testConnection') {
          emit({ ...envelope, type: 'completed', text: rawText.trim() || 'OK' });
        } else if (command.type === 'generateCommit') {
          emit({ ...envelope, type: 'completed', text: cleanCommitMessage(rawText) });
        } else if (command.type === 'reviewScope') {
          const findings = parseReviewFindings(rawText, command.files);
          emit({ ...envelope, type: 'reviewCompleted', findings });
        } else if (command.type === 'planCPush') {
          const plan = parseCPushPlan(rawText, command);
          emit({ ...envelope, type: 'cPushPlanCompleted', plan });
        } else {
          const resolution = parseConflictResolution(rawText, command);
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
