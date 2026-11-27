import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { describe, expect, it } from 'vitest';
import type { ProbeEvent, RunCustomActionCommand } from '../../shared/pi-protocol';
import { buildResolveConflictModePrompts, createTaskRunner } from '../src/tasks';

function sampleConflictCommand(overrides: Partial<RunCustomActionCommand> = {}): RunCustomActionCommand {
  return {
    type: 'runCustomAction',
    mode: 'resolveConflict',
    requestId: 'req-conflict-1',
    context: {
      repositoryId: 'repo-1',
      worktreeId: 'wt-1',
      sessionId: 'sess-1',
      scopeVersion: 'scope-v1',
    },
    model: {
      provider: 'openai',
      apiFormat: 'openai-completions',
      modelId: 'gpt-4.1',
      baseUrl: '',
      apiKey: 'sk-test-only',
    },
    branch: 'feat/pay',
    detached: false,
    remoteBranches: ['origin/feat/pay'],
    actionLabel: 'Resolve Conflict',
    actionPrompt: '',
    extraPrompt: 'Preserve local idempotency lock and keep upstream traceId logging',
    selectedFiles: [],
    unselectedPaths: [],
    localKeepPaths: [],
    upstream: 'origin/feat/pay',
    commits: ['a3f91c2 feat(order): add callback idempotency lock'],
    conflictFiles: [
      {
        fileId: 'f-order',
        path: 'src/service/Order.ts',
        lineCount: 10,
        hasBase: true,
      },
    ],
    ...overrides,
  };
}

function writeToolSse(res: ServerResponse, id: string, toolName: string, args: Record<string, unknown>) {
  res.writeHead(200, { 'Content-Type': 'text/event-stream; charset=utf-8' });
  res.write(`data: ${JSON.stringify({
    id,
    object: 'chat.completion.chunk',
    created: 1700000000,
    model: 'gpt-4.1',
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
    model: 'gpt-4.1',
    choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }],
  })}\n\n`);
  res.write('data: [DONE]\n\n');
  res.end();
}

describe('M5 Tool-driven conflict resolution mode', () => {
  it('sends compact metadata-only prompts and switches summary language directive between English and Chinese', () => {
    const enPrompts = buildResolveConflictModePrompts(sampleConflictCommand());
    expect(enPrompts.systemPrompt).toContain('read_conflict_file');
    expect(enPrompts.systemPrompt).toContain('write_conflict_resolution');
    expect(enPrompts.systemPrompt).toContain('Write `summary` in concise English.');
    expect(enPrompts.userPrompt).toContain('`src/service/Order.ts` (fileId=f-order, 10 lines, base available)');
    expect(enPrompts.userPrompt).toContain('Preserve local idempotency lock and keep upstream traceId logging');
    expect(enPrompts.userPrompt).not.toContain('<<<<<<<');

    const zhPrompts = buildResolveConflictModePrompts(sampleConflictCommand({ locale: 'zh' }));
    expect(zhPrompts.systemPrompt).toContain('Write `summary` in concise Chinese.');
  });

  it('reads conflicted files on demand, self-repairs rejected conflict markers, and stops immediately on stopAgent=true', async () => {
    let callCount = 0;
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      req.resume();
      req.on('end', () => {
        callCount += 1;
        if (callCount === 1) {
          writeToolSse(res, 'conf-1', 'read_conflict_file', {
            path: 'src/service/Order.ts',
            section: 'conflicted',
          });
        } else if (callCount === 2) {
          // Simulate a bad attempt with leftover markers rejected by host:
          writeToolSse(res, 'conf-2', 'write_conflict_resolution', {
            path: 'src/service/Order.ts',
            content: '<<<<<<< HEAD\nbad\n=======\nstill bad\n>>>>>>> a3f91c2\n',
            summary: 'Attempt 1',
          });
        } else {
          // Clean self-repaired resolution:
          writeToolSse(res, 'conf-3', 'write_conflict_resolution', {
            path: 'src/service/Order.ts',
            content: 'export function handlePay(traceId: string) {\n  log(traceId);\n  acquireLock();\n  return ok();\n}\n',
            summary: 'Preserved local idempotency lock and merged upstream traceId logging',
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
      let writeAttempts = 0;
      let runner!: ReturnType<typeof createTaskRunner>;
      runner = createTaskRunner(
        sampleConflictCommand({
          model: {
            provider: 'openai',
            apiFormat: 'openai-completions',
            modelId: 'gpt-4.1',
            baseUrl: `http://127.0.0.1:${addr.port}/v1`,
            apiKey: 'sk-test-only',
          },
        }),
        (event) => {
          events.push(event);
          if (event.type === 'toolCall') {
            seenTools.push(event.toolName);
            queueMicrotask(() => {
              if (event.toolName === 'read_conflict_file') {
                runner.deliverToolResult?.({
                  type: 'toolResult',
                  requestId: event.requestId,
                  callId: event.callId,
                  ok: true,
                  content: '<<<<<<< HEAD\nexport function handlePay(traceId: string) {\n=======\nexport function handlePay() {\n>>>>>>> a3f91c2\n',
                });
              } else if (event.toolName === 'write_conflict_resolution') {
                writeAttempts += 1;
                if (writeAttempts === 1) {
                  runner.deliverToolResult?.({
                    type: 'toolResult',
                    requestId: event.requestId,
                    callId: event.callId,
                    ok: false,
                    isError: true,
                    content: 'Merged result for conflicted file src/service/Order.ts still contains leftover conflict markers; write rejected.',
                  });
                } else {
                  runner.deliverToolResult?.({
                    type: 'toolResult',
                    requestId: event.requestId,
                    callId: event.callId,
                    ok: true,
                    content: 'Applied all conflict resolutions and continued rebase.',
                    details: {
                      stopAgent: true,
                      summary: 'Preserved local idempotency lock and merged upstream traceId logging',
                      links: [],
                    },
                  });
                }
              }
            });
          }
        },
      );
      await runner.run();
      expect(callCount).toBe(3);
      expect(seenTools).toEqual([
        'read_conflict_file',
        'write_conflict_resolution',
        'write_conflict_resolution',
      ]);
      const completed = events.find((e) => e.type === 'customActionCompleted');
      expect(completed).toMatchObject({
        type: 'customActionCompleted',
        summary: 'Preserved local idempotency lock and merged upstream traceId logging',
      });
    } finally {
      await new Promise<void>((resolve, reject) => server.close((err) => (err ? reject(err) : resolve())));
    }
  });
});
