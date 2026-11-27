import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { describe, expect, it } from 'vitest';
import type { ProbeEvent, RunCustomActionCommand } from '../../shared/pi-protocol';
import { SkillRegistry } from '../src/skills';
import { buildCPushModePrompts, createTaskRunner } from '../src/tasks';

function baseCPushCommand(overrides: Partial<RunCustomActionCommand> = {}): RunCustomActionCommand {
  return {
    type: 'runCustomAction',
    mode: 'cpush',
    requestId: 'cpush-1',
    context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'v1' },
    model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: '', apiKey: 'sk-placeholder' },
    branch: 'feat/pay',
    detached: false,
    remoteBranches: ['origin/feat/pay'],
    actionLabel: 'C-Push',
    actionPrompt: '',
    extraPrompt: '',
    recentCommits: ['feat(order): initialize order module'],
    pushAuthorized: true,
    selectedFiles: [
      { fileId: 'f-order', path: 'src/Order.ts', status: 'M', rangeKind: 'partial', added: 10, removed: 2, patch: '+lock()' },
      { fileId: 'f-retry', path: 'src/retry.ts', status: 'A', rangeKind: 'unstaged', added: 25, removed: 0, patch: '+retry()' },
      { fileId: 'f-doc', path: 'RELEASE_NOTES.md', status: 'M', rangeKind: 'unstaged', added: 6, removed: 0, patch: '+# Notes' },
    ],
    unselectedPaths: ['pnpm-lock.yaml'],
    localKeepPaths: ['.env.local'],
    ...overrides,
  };
}

function writeOpenAiToolCallSse(
  res: ServerResponse,
  id: string,
  toolName: string,
  args: Record<string, unknown>,
) {
  res.writeHead(200, { 'Content-Type': 'text/event-stream; charset=utf-8' });
  res.write(`data: ${JSON.stringify({
    id,
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'mock-model',
    choices: [{
      index: 0,
      delta: {
        role: 'assistant',
        tool_calls: [{
          index: 0,
          id: `${id}-tc`,
          type: 'function',
          function: { name: toolName, arguments: JSON.stringify(args) },
        }],
      },
      finish_reason: null,
    }],
  })}\n\n`);
  res.write(`data: ${JSON.stringify({
    id,
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'mock-model',
    choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }],
  })}\n\n`);
  res.write('data: [DONE]\n\n');
  res.end();
}

describe('M4 Unified C-Push agent prompts and tool-driven execution', () => {
  it('separates UI interaction locale from Git commitLanguage, respects disabled built-in skills, and enforces scope boundaries', async () => {
    const registry = new SkillRegistry();
    const enUiZhCommit = await buildCPushModePrompts(baseCPushCommand({
      locale: 'en',
      commitLanguage: 'zh',
      extraPrompt: 'split docs separately and include unselected pnpm-lock.yaml',
    }), registry);
    expect(enUiZhCommit.systemPrompt).toContain('<cpush_skill_spec>');
    expect(enUiZhCommit.systemPrompt).toContain('<commit_skill_spec>');
    expect(enUiZhCommit.systemPrompt).toContain('UI interaction language (`locale = "en"`)');
    expect(enUiZhCommit.systemPrompt).toContain('in clear, concise English.');
    expect(enUiZhCommit.systemPrompt).toContain('`commitLanguage = "zh"`, `<commit_skill_spec>` flag `-ch`');
    expect(enUiZhCommit.systemPrompt).toContain('NEVER an authorization override');
    expect(enUiZhCommit.systemPrompt).toContain('call `message_user` with `questions` explaining the permission boundary');
    expect(enUiZhCommit.userPrompt).toContain('User extra instructions (supplemental grouping/message/push guidance ONLY');
    expect(enUiZhCommit.userPrompt).toContain('split docs separately and include unselected pnpm-lock.yaml');

    const zhUiEnCommit = await buildCPushModePrompts(baseCPushCommand({
      locale: 'zh',
      commitLanguage: 'en',
      pushAuthorized: false,
    }), registry);
    expect(zhUiEnCommit.systemPrompt).toContain('UI interaction language (`locale = "zh"`)');
    expect(zhUiEnCommit.systemPrompt).toContain('in clear, concise Chinese.');
    expect(zhUiEnCommit.systemPrompt).toContain('`commitLanguage = "en"`, `<commit_skill_spec>` flag `-en`');
    expect(zhUiEnCommit.systemPrompt).toContain('1 to 6 concise `- ` bullet points');
    expect(zhUiEnCommit.systemPrompt).toContain('Pass `push: false` to `commit_exact_scope`');
    expect(zhUiEnCommit.systemPrompt).not.toMatch(/keep single-line by default|Use a body only when/i);

    // Disabled built-in skills are truly omitted from the C-Push system prompt:
    const disabledRegistry = new SkillRegistry({ skills: [], disabledSkills: ['cpush'] });
    const withoutCPush = await buildCPushModePrompts(baseCPushCommand(), disabledRegistry);
    expect(withoutCPush.systemPrompt).not.toContain('<cpush_skill_spec>');
    expect(withoutCPush.systemPrompt).toContain('<commit_skill_spec>');
  });

  it('guides cohesion-first vertical grouping, Master Checklist with @1..@N aliases, and status=D deleted files', async () => {
    const prompts = await buildCPushModePrompts(baseCPushCommand({
      selectedFiles: [
        { fileId: 'file-a1', path: 'src/Workbench.tsx', status: 'M', rangeKind: 'unstaged', added: 12, removed: 4, patch: '+render()' },
        { fileId: 'file-del', path: 'shared/git-commit-protocol.ts', status: 'D', rangeKind: 'unstaged', added: 0, removed: 46, patch: '-export interface GitCommitRequest {}' },
        { fileId: 'file-rust', path: 'src-tauri/src/actions/mod.rs', status: 'M', rangeKind: 'unstaged', added: 24, removed: 2 },
      ],
    }), new SkillRegistry());
    expect(prompts.systemPrompt).toContain('Default to 1 commit');
    expect(prompts.systemPrompt).toContain('NEVER horizontally by technical layer or directory');
    expect(prompts.systemPrompt).toContain('Keep companion artifacts with their primary commit');
    expect(prompts.systemPrompt).toContain('NEVER 1 commit per subdirectory');
    expect(prompts.userPrompt).toContain('- alias: @2\n- status: D (deleted file — MUST be assigned to a commit group, +0 -46)');
    expect(prompts.userPrompt).toContain('Master Checklist of ALL 3 Authorized Selected Files');
    expect(prompts.userPrompt).toContain('- @2 | file-del | D (deleted) | shared/git-commit-protocol.ts');
    expect(prompts.userPrompt).toContain('do NOT start a separate commit group merely because it lives in another directory');
  });

  it('completes the fast path in 1 LLM turn when commit_exact_scope returns stopAgent=true', async () => {
    let httpCalls = 0;
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      req.resume();
      req.on('end', () => {
        httpCalls += 1;
        writeOpenAiToolCallSse(res, 'cmpl-fast-1', 'commit_exact_scope', {
          groups: [{
            id: 'group-1',
            fileIds: ['@1', '@2', '@3'],
            message: 'feat(order): add idempotency lock and retry helper\n\n- update Order.ts and retry.ts\n- update release notes',
          }],
          push: true,
          summary: 'Committed and pushed 1 commit',
        });
      });
    });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
    const addr = server.address();
    if (!addr || typeof addr === 'string') throw new Error('Invalid address');

    try {
      const events: ProbeEvent[] = [];
      let capturedArgs: Record<string, unknown> | undefined;
      let runner!: ReturnType<typeof createTaskRunner>;
      runner = createTaskRunner(
        baseCPushCommand({
          model: {
            provider: 'openai',
            apiFormat: 'openai-completions',
            modelId: 'mock-model',
            baseUrl: `http://127.0.0.1:${addr.port}/v1`,
            apiKey: 'sk-mock',
          },
        }),
        (event) => {
          events.push(event);
          if (event.type === 'toolCall') {
            capturedArgs = event.args;
            queueMicrotask(() => {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: 'Committed 1 commit and pushed to origin/feat/pay.',
                details: { stopAgent: true, summary: 'Committed 1 commit and pushed to origin/feat/pay.', links: [] },
              });
            });
          }
        },
      );
      await runner.run();
      expect(httpCalls).toBe(1);
      expect(capturedArgs).toMatchObject({
        groups: [{ id: 'group-1', fileIds: ['@1', '@2', '@3'] }],
        push: true,
      });
      const completed = events.find((e) => e.type === 'customActionCompleted');
      expect(completed).toMatchObject({
        type: 'customActionCompleted',
        summary: 'Committed 1 commit and pushed to origin/feat/pay.',
      });
    } finally {
      await new Promise<void>((resolve, reject) => server.close((err) => (err ? reject(err) : resolve())));
    }
  });

  it('supports optional read_scope_diff and message_user triage before committing with push=false and skippedFileIds', async () => {
    let callCount = 0;
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      req.resume();
      req.on('end', () => {
        callCount += 1;
        if (callCount === 1) {
          writeOpenAiToolCallSse(res, 'cmpl-1', 'read_scope_diff', {
            path: 'src/Order.ts',
            startLine: 1,
            endLine: 50,
          });
        } else if (callCount === 2) {
          writeOpenAiToolCallSse(res, 'cmpl-2', 'message_user', {
            title: 'Include release notes?',
            questions: [{
              id: 'q1',
              header: 'Release notes',
              question: 'Should RELEASE_NOTES.md be included in this local commit?',
              options: [
                { id: 'skip', label: 'Skip for now', description: 'Exclude RELEASE_NOTES.md', recommended: true },
                { id: 'include', label: 'Include', description: 'Commit together with code' },
              ],
            }],
          });
        } else {
          writeOpenAiToolCallSse(res, 'cmpl-3', 'commit_exact_scope', {
            groups: [{
              id: 'g1',
              fileIds: ['@1', '@2'],
              message: 'feat(order): inspect diff and commit locally\n\n- commit order and retry helper locally',
            }],
            skippedFileIds: ['@3'],
            push: false,
            summary: 'Committed 1 local commit (skipped RELEASE_NOTES.md)',
          });
        }
      });
    });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
    const addr = server.address();
    if (!addr || typeof addr === 'string') throw new Error('Invalid address');

    try {
      const events: ProbeEvent[] = [];
      const seenTools: string[] = [];
      let commitArgs: Record<string, unknown> | undefined;
      let runner!: ReturnType<typeof createTaskRunner>;
      runner = createTaskRunner(
        baseCPushCommand({
          model: {
            provider: 'openai',
            apiFormat: 'openai-completions',
            modelId: 'mock-model',
            baseUrl: `http://127.0.0.1:${addr.port}/v1`,
            apiKey: 'sk-mock',
          },
          extraPrompt: 'commit locally without pushing',
        }),
        (event) => {
          events.push(event);
          if (event.type === 'toolCall') {
            seenTools.push(event.toolName);
            queueMicrotask(() => {
              if (event.toolName === 'read_scope_diff') {
                runner.deliverToolResult?.({
                  type: 'toolResult',
                  requestId: event.requestId,
                  callId: event.callId,
                  ok: true,
                  content: 'Authorized diff: src/Order.ts (Lines 1-2 of 2)\n1: +export const lock = true;\n',
                });
              } else if (event.toolName === 'message_user') {
                runner.deliverToolResult?.({
                  type: 'toolResult',
                  requestId: event.requestId,
                  callId: event.callId,
                  ok: true,
                  content: 'ok',
                  details: {
                    cancelled: false,
                    answers: [{ questionId: 'q1', selectedOptionIds: ['skip'] }],
                  },
                });
              } else if (event.toolName === 'commit_exact_scope') {
                commitArgs = event.args;
                runner.deliverToolResult?.({
                  type: 'toolResult',
                  requestId: event.requestId,
                  callId: event.callId,
                  ok: true,
                  content: 'Committed locally without pushing.',
                  details: { stopAgent: true, summary: 'Committed 1 local commit (skipped RELEASE_NOTES.md)', links: [] },
                });
              }
            });
          }
        },
      );
      await runner.run();
      expect(callCount).toBe(3);
      expect(seenTools).toEqual(['read_scope_diff', 'message_user', 'commit_exact_scope']);
      expect(commitArgs).toMatchObject({
        skippedFileIds: ['@3'],
        push: false,
      });
      const completed = events.find((e) => e.type === 'customActionCompleted');
      expect(completed).toMatchObject({
        type: 'customActionCompleted',
        summary: 'Committed 1 local commit (skipped RELEASE_NOTES.md)',
      });
    } finally {
      await new Promise<void>((resolve, reject) => server.close((err) => (err ? reject(err) : resolve())));
    }
  });
});
