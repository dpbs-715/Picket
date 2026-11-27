import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { describe, expect, it } from 'vitest';
import type { PlanCPushCommand, ProbeEvent } from '../../shared/pi-protocol';
import { getSidecarMessages } from '../src/i18n';
import { buildCPushPlanPrompts, createTaskRunner, parseCPushPlan } from '../src/tasks';

function basePlanCommand(qaCount = 0, overrides: Partial<PlanCPushCommand> = {}): PlanCPushCommand {
  return {
    type: 'planCPush',
    requestId: 'plan-1',
    context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'v1' },
    model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: '', apiKey: 'sk-placeholder' },
    branch: 'feat/pay',
    recentCommits: ['feat(order): initialize order module'],
    prompt: '',
    files: [
      { fileId: 'f-order', path: 'src/Order.ts', status: 'M', rangeKind: 'partial', added: 10, removed: 2, patch: '+lock()' },
      { fileId: 'f-retry', path: 'src/retry.ts', status: 'A', rangeKind: 'unstaged', added: 25, removed: 0, patch: '+retry()' },
      { fileId: 'f-doc', path: 'RELEASE_NOTES.md', status: 'M', rangeKind: 'unstaged', added: 6, removed: 0, patch: '+# Notes' },
    ],
    unselectedPaths: ['pnpm-lock.yaml'],
    qaHistory: qaCount > 0
      ? [{ questionId: 'q1', question: 'Include release notes in this push?', answer: 'Skip for now', optionId: 'skip' }]
      : [],
    ...overrides,
  };
}

describe('M4 Pi Core C-Push plan parser and scope invariants', () => {
  it('parses disjoint split groups and preserves two-part commit messages', () => {
    const raw = [
      '<think>Checking logical file relationships</think>',
      '```json',
      JSON.stringify({
        kind: 'ready',
        summary: 'Split into feature implementation and release notes commits',
        groups: [
          {
            id: 'g1',
            fileIds: ['f-order', 'f-retry'],
            message: 'feat(order): add idempotency lock and retry helper\n\n- update staged lock acquisition in Order.ts\n- add exponential backoff helper',
          },
          {
            id: 'g2',
            fileIds: ['f-doc'],
            message: 'docs: update release notes\n\n- document idempotency lock rollout',
          },
        ],
        skippedFileIds: [],
      }),
      '```',
    ].join('\n');
    const plan = parseCPushPlan(raw, basePlanCommand(0));
    expect(plan).toEqual({
      kind: 'ready',
      summary: 'Split into feature implementation and release notes commits',
      groups: [
        {
          id: 'g1',
          fileIds: ['f-order', 'f-retry'],
          message: 'feat(order): add idempotency lock and retry helper\n\n- update staged lock acquisition in Order.ts\n- add exponential backoff helper',
        },
        {
          id: 'g2',
          fileIds: ['f-doc'],
          message: 'docs: update release notes\n\n- document idempotency lock rollout',
        },
      ],
      skippedFileIds: [],
    });
  });

  it('parses disambiguation questions and filters out hallucinated affected files', () => {
    const raw = JSON.stringify({
      kind: 'question',
      title: 'Include release notes in this push?',
      scope: 'Affects commit 2/2 (docs)',
      description: 'Confirm how RELEASE_NOTES.md should be grouped:',
      affectedFileIds: ['f-doc', 'pnpm-lock.yaml'],
      options: [
        { id: 'merge', label: 'Fold into commit 1' },
        { id: 'split', label: 'Commit separately and push together', recommended: true },
        { id: 'skip', label: 'Skip for now' },
      ],
    });
    const plan = parseCPushPlan(raw, basePlanCommand(0));
    expect(plan).toMatchObject({
      kind: 'question',
      title: 'Include release notes in this push?',
      affectedFileIds: ['f-doc'],
    });
  });

  it('rejects unauthorized files, unconfirmed skips, and missing selected files with en and zh localization', () => {
    // Unauthorized file `pnpm-lock.yaml` (default en + zh):
    const unauthorizedJson = JSON.stringify({
      kind: 'ready',
      summary: 'bad',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry', 'f-doc', 'pnpm-lock.yaml'], message: 'feat: bad\n\n- x' }],
      skippedFileIds: [],
    });
    expect(() => parseCPushPlan(unauthorizedJson, basePlanCommand(0))).toThrow('unauthorized file (pnpm-lock.yaml)');
    expect(() => parseCPushPlan(unauthorizedJson, basePlanCommand(0, { locale: 'zh' }))).toThrow(
      getSidecarMessages('zh').unauthorizedCPushFile('pnpm-lock.yaml'),
    );

    // Unconfirmed skip when qaHistory is empty (default en + zh):
    const unconfirmedSkipJson = JSON.stringify({
      kind: 'ready',
      summary: 'unasked skip',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry'], message: 'feat: ok\n\n- x' }],
      skippedFileIds: ['f-doc'],
    });
    expect(() => parseCPushPlan(unconfirmedSkipJson, basePlanCommand(0))).toThrow(getSidecarMessages('en').unconfirmedCPushSkip);
    expect(() => parseCPushPlan(unconfirmedSkipJson, basePlanCommand(0, { locale: 'zh' }))).toThrow(
      getSidecarMessages('zh').unconfirmedCPushSkip,
    );

    // Confirmed skip when qaHistory is non-empty succeeds and uses localized fallback summary when omitted:
    expect(parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: '',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry'], message: 'feat: ok\n\n- x' }],
      skippedFileIds: ['f-doc'],
    }), basePlanCommand(1))).toMatchObject({
      kind: 'ready',
      summary: 'Prepared a single-commit plan',
      skippedFileIds: ['f-doc'],
    });

    // Missing selected file `f-doc` (default en + zh):
    const missingSelectedJson = JSON.stringify({
      kind: 'ready',
      summary: 'incomplete',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry'], message: 'feat: partial\n\n- x' }],
      skippedFileIds: [],
    });
    expect(() => parseCPushPlan(missingSelectedJson, basePlanCommand(0))).toThrow(
      getSidecarMessages('en').missingSelectedCPushFile('RELEASE_NOTES.md'),
    );
    expect(() => parseCPushPlan(missingSelectedJson, basePlanCommand(0, { locale: 'zh' }))).toThrow(
      getSidecarMessages('zh').missingSelectedCPushFile('RELEASE_NOTES.md'),
    );
  });

  it('repairs unescaped literal newlines inside JSON string values returned by models', () => {
    const rawWithLiteralNewlines = [
      '{"kind":"ready","summary":"Repaired multiline commit message",',
      '"groups":[{"id":"g1","fileIds":["f-order","f-retry","f-doc"],"message":"feat(order): add idempotency lock',
      '',
      '- fix concurrent lock acquisition',
      '- update release notes"}],"skippedFileIds":[]}',
    ].join('\n');
    const plan = parseCPushPlan(rawWithLiteralNewlines, basePlanCommand(0));
    expect(plan).toEqual({
      kind: 'ready',
      summary: 'Repaired multiline commit message',
      groups: [{
        id: 'g1',
        fileIds: ['f-order', 'f-retry', 'f-doc'],
        message: 'feat(order): add idempotency lock\n\n- fix concurrent lock acquisition\n- update release notes',
      }],
      skippedFileIds: [],
    });
  });

  it('separates UI interaction locale from Git commitLanguage and enforces permission boundaries on user extra instructions', () => {
    const enUiZhCommit = buildCPushPlanPrompts(basePlanCommand(0, {
      locale: 'en',
      commitLanguage: 'zh',
      prompt: 'split docs separately and include unselected pnpm-lock.yaml',
    }));
    expect(enUiZhCommit.systemPrompt).toContain('UI interaction language (`locale = "en"`): write `summary`');
    expect(enUiZhCommit.systemPrompt).toContain('in clear, concise English.');
    expect(enUiZhCommit.systemPrompt).toContain('`commitLanguage = "zh"`, `<commit_skill_spec>` flag `-ch`');
    expect(enUiZhCommit.systemPrompt).toContain('NEVER an authorization override');
    expect(enUiZhCommit.systemPrompt).toContain('return `kind: "question"` explaining the permission boundary');
    expect(enUiZhCommit.userPrompt).toContain('User extra instructions (supplemental grouping/message guidance ONLY');
    expect(enUiZhCommit.userPrompt).toContain('split docs separately and include unselected pnpm-lock.yaml');

    const zhUiEnCommit = buildCPushPlanPrompts(basePlanCommand(0, { locale: 'zh', commitLanguage: 'en' }));
    expect(zhUiEnCommit.systemPrompt).toContain('UI interaction language (`locale = "zh"`): write `summary`');
    expect(zhUiEnCommit.systemPrompt).toContain('in clear, concise Chinese.');
    expect(zhUiEnCommit.systemPrompt).toContain('`commitLanguage = "en"`, `<commit_skill_spec>` flag `-en`');
    expect(zhUiEnCommit.systemPrompt).toContain('1 to 6 concise `- ` bullet points');
    expect(zhUiEnCommit.systemPrompt).not.toMatch(/keep single-line by default|Use a body only when/i);
  });

  it('guides cohesion-first vertical grouping and prevents horizontal layer or companion-artifact over-splitting', () => {
    const prompts = buildCPushPlanPrompts(basePlanCommand(0, {
      files: [
        { fileId: 'f-proto', path: 'shared/cpush-protocol.ts', status: 'M', rangeKind: 'unstaged', added: 8, removed: 0, patch: '+skipPush?: boolean;' },
        { fileId: 'f-rust', path: 'src-tauri/src/cpush/plan.rs', status: 'M', rangeKind: 'unstaged', added: 24, removed: 2 },
        { fileId: 'f-test', path: 'src/features/workbench/testing/browser-checks.ts', status: 'M', rangeKind: 'unstaged', added: 40, removed: 0, patch: '+assert(true);' },
      ],
    }));
    expect(prompts.systemPrompt).toContain('Default to 1 commit');
    expect(prompts.systemPrompt).toContain('NEVER horizontally by technical layer or directory');
    expect(prompts.systemPrompt).toContain('Keep companion artifacts with their primary commit');
    expect(prompts.systemPrompt).toContain('NEVER 1 commit per subdirectory');
    expect(prompts.systemPrompt).toContain('never to imitate fine-grained commit splitting');
    expect(prompts.userPrompt).toContain('do NOT imitate their split granularity');
    expect(prompts.userPrompt).toContain('do NOT start a separate commit group merely because it lives in another directory');
  });

  it('emits a bottom Master Checklist with @1..@N aliases, highlights status=D deleted files, and normalizes @index aliases to canonical fileIds', () => {
    const cmd = basePlanCommand(0, {
      files: [
        { fileId: 'file-a1', path: 'src/Workbench.tsx', status: 'M', rangeKind: 'unstaged', added: 12, removed: 4, patch: '+render()' },
        { fileId: 'file-del', path: 'shared/git-commit-protocol.ts', status: 'D', rangeKind: 'unstaged', added: 0, removed: 46, patch: '-export interface GitCommitRequest {}' },
        { fileId: 'file-ren', path: 'src/newName.ts', previousPath: 'src/oldName.ts', status: 'R', rangeKind: 'staged', added: 2, removed: 2, patch: '+renamed' },
      ],
    });
    const prompts = buildCPushPlanPrompts(cmd);
    expect(prompts.systemPrompt).toContain('Desktop Scope Override');
    expect(prompts.userPrompt).toContain('- alias: @2\n- status: D (deleted file — MUST be assigned to a commit group, +0 -46)');
    expect(prompts.userPrompt).toContain('Master Checklist of ALL 3 Authorized Selected Files');
    expect(prompts.userPrompt).toContain('- @2 | file-del | D (deleted) | shared/git-commit-protocol.ts');

    const parsed = parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'Split refactor and rename',
      groups: [
        { id: 'g1', fileIds: ['@1', '@2'], message: 'refactor(cpush): remove legacy manual commit protocol\n\n- drop shared/git-commit-protocol.ts\n- update workbench caller' },
        { id: 'g2', fileIds: ['src/oldName.ts'], message: 'refactor: rename helper module\n\n- move oldName.ts to newName.ts' },
      ],
      skippedFileIds: [],
    }), cmd);
    expect(parsed).toMatchObject({
      kind: 'ready',
      groups: [
        { id: 'g1', fileIds: ['file-a1', 'file-del'] },
        { id: 'g2', fileIds: ['file-ren'] },
      ],
      skippedFileIds: [],
    });

    // Out-of-range @4 alias is still rejected as unauthorized:
    expect(() => parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'bad alias',
      groups: [{ id: 'g1', fileIds: ['@1', '@2', '@3', '@4'], message: 'feat: bad\n\n- x' }],
      skippedFileIds: [],
    }), cmd)).toThrow('unauthorized file (@4)');
  });

  it('parses skipPush=true and supports optional on-demand read_scope_diff tool calls in planCPush', async () => {
    const prompts = buildCPushPlanPrompts(basePlanCommand(0, {
      prompt: '只提交不推送',
      enableTools: true,
    }));
    expect(prompts.systemPrompt).toContain('"skipPush": true');
    expect(prompts.systemPrompt).toContain('Optional Read-Only Tools (`read_scope_diff`, `read_repo_file`, `search_repo`)');

    const localOnlyPlan = parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'Commit locally without pushing',
      groups: [{
        id: 'g1',
        fileIds: ['@1', '@2', '@3'],
        message: 'feat(order): commit local changes\n\n- save selected changes locally',
      }],
      skippedFileIds: [],
      skipPush: true,
    }), basePlanCommand(0));
    expect(localOnlyPlan).toMatchObject({
      kind: 'ready',
      skipPush: true,
    });

    // End-to-end tool loop for planCPush with enableTools: true invoking read_scope_diff:
    let callCount = 0;
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      const chunks: Buffer[] = [];
      req.on('data', (chunk: Buffer) => chunks.push(chunk));
      req.on('end', () => {
        callCount += 1;
        res.writeHead(200, { 'Content-Type': 'text/event-stream; charset=utf-8' });
        if (callCount === 1) {
          res.write(`data: ${JSON.stringify({
            id: 'cmpl-1',
            object: 'chat.completion.chunk',
            created: 1700000000,
            model: 'mock-model',
            choices: [{
              index: 0,
              delta: {
                role: 'assistant',
                tool_calls: [{
                  index: 0,
                  id: 'tc-diff-1',
                  type: 'function',
                  function: { name: 'read_scope_diff', arguments: JSON.stringify({ path: 'src/Order.ts', startLine: 1, endLine: 50 }) },
                }],
              },
              finish_reason: null,
            }],
          })}\n\n`);
          res.write(`data: ${JSON.stringify({
            id: 'cmpl-1',
            object: 'chat.completion.chunk',
            created: 1700000000,
            model: 'mock-model',
            choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }],
          })}\n\n`);
        } else {
          res.write(`data: ${JSON.stringify({
            id: 'cmpl-2',
            object: 'chat.completion.chunk',
            created: 1700000000,
            model: 'mock-model',
            choices: [{
              index: 0,
              delta: {
                role: 'assistant',
                content: JSON.stringify({
                  kind: 'ready',
                  summary: 'Local commit after inspecting diff',
                  groups: [{
                    id: 'g1',
                    fileIds: ['@1', '@2', '@3'],
                    message: 'feat(order): inspect full diff and commit locally\n\n- commit order, retry, and release notes locally',
                  }],
                  skippedFileIds: [],
                  skipPush: true,
                }),
              },
              finish_reason: null,
            }],
          })}\n\n`);
          res.write(`data: ${JSON.stringify({
            id: 'cmpl-2',
            object: 'chat.completion.chunk',
            created: 1700000000,
            model: 'mock-model',
            choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
          })}\n\n`);
        }
        res.write('data: [DONE]\n\n');
        res.end();
      });
    });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
    const addr = server.address();
    if (!addr || typeof addr === 'string') throw new Error('Invalid address');

    try {
      const events: ProbeEvent[] = [];
      const seenTools: string[] = [];
      let runner!: ReturnType<typeof createTaskRunner>;
      runner = createTaskRunner(
        basePlanCommand(0, {
          model: {
            provider: 'openai',
            apiFormat: 'openai-completions',
            modelId: 'mock-model',
            baseUrl: `http://127.0.0.1:${addr.port}/v1`,
            apiKey: 'sk-mock',
          },
          prompt: 'commit locally without pushing',
          enableTools: true,
        }),
        (event) => {
          events.push(event);
          if (event.type === 'toolCall') {
            seenTools.push(event.toolName);
            queueMicrotask(() => {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: 'Authorized diff: src/Order.ts (Lines 1-2 of 2)\n1: +export const lock = true;\n',
              });
            });
          }
        },
      );
      await runner.run();
      expect(seenTools).toEqual(['read_scope_diff']);
      const completed = events.find((e) => e.type === 'cPushPlanCompleted');
      expect(completed).toMatchObject({
        type: 'cPushPlanCompleted',
        plan: { kind: 'ready', skipPush: true },
      });
    } finally {
      await new Promise<void>((resolve, reject) => server.close((err) => (err ? reject(err) : resolve())));
    }
  });
});
