import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { afterEach, describe, expect, it } from 'vitest';
import type { ProbeEvent } from '../../shared/pi-protocol';
import { createTaskRunner } from '../src/tasks';

async function startMockOpenAiServer(
  handler: (body: Record<string, unknown>, callIndex: number) => {
    toolCalls?: Array<{ id: string; name: string; arguments: Record<string, unknown> }>;
    text?: string;
  },
): Promise<{ url: string; close: () => Promise<void> }> {
  let count = 0;
  const server = createServer((req: IncomingMessage, res: ServerResponse) => {
    const chunks: Buffer[] = [];
    req.on('data', (chunk: Buffer) => chunks.push(chunk));
    req.on('end', () => {
      count += 1;
      const parsed = JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>;
      const reply = handler(parsed, count);
      res.writeHead(200, {
        'Content-Type': 'text/event-stream; charset=utf-8',
        'Cache-Control': 'no-cache',
      });
      const id = `chatcmpl-${count}`;
      if (reply.toolCalls?.length) {
        const tcPayload = reply.toolCalls.map((tc, idx) => ({
          index: idx,
          id: tc.id,
          type: 'function',
          function: { name: tc.name, arguments: JSON.stringify(tc.arguments) },
        }));
        res.write(`data: ${JSON.stringify({
          id,
          object: 'chat.completion.chunk',
          created: 1700000000,
          model: 'mock-model',
          choices: [{ index: 0, delta: { role: 'assistant', tool_calls: tcPayload }, finish_reason: null }],
        })}\n\n`);
        res.write(`data: ${JSON.stringify({
          id,
          object: 'chat.completion.chunk',
          created: 1700000000,
          model: 'mock-model',
          choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }],
        })}\n\n`);
      } else {
        res.write(`data: ${JSON.stringify({
          id,
          object: 'chat.completion.chunk',
          created: 1700000000,
          model: 'mock-model',
          choices: [{ index: 0, delta: { role: 'assistant', content: reply.text ?? '' }, finish_reason: null }],
        })}\n\n`);
        res.write(`data: ${JSON.stringify({
          id,
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
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Invalid server address');
  return {
    url: `http://127.0.0.1:${address.port}/v1`,
    close: () => new Promise<void>((resolve, reject) => server.close((err) => (err ? reject(err) : resolve()))),
  };
}

describe('Phase 2 Pi sidecar runCustomAction & generateCustomAction tool loops', () => {
  const closers: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (closers.length > 0) {
      await closers.pop()!();
    }
  });

  it('runs a multi-turn custom action tool loop (check_cli_env -> run_git -> message_user -> completed with links)', async () => {
    const srv = await startMockOpenAiServer((_body, callIndex) => {
      if (callIndex === 1) {
        return {
          toolCalls: [{ id: 'c1', name: 'check_cli_env', arguments: {} }],
        };
      }
      if (callIndex === 2) {
        return {
          toolCalls: [{ id: 'c2', name: 'run_git', arguments: { args: ['tag', '-a', 'v0.2.0', '-m', 'Release v0.2.0'] } }],
        };
      }
      if (callIndex === 3) {
        return {
          toolCalls: [{
            id: 'c3',
            name: 'message_user',
            arguments: {
              title: 'Push Release Tag',
              questions: [{
                header: 'Push Tag',
                question: 'Push v0.2.0 to origin?',
                options: [
                  { id: 'yes', label: 'Push now', description: 'Push v0.2.0 to origin', recommended: true },
                  { id: 'no', label: 'Keep local', description: 'Do not push yet' },
                ],
              }],
            },
          }],
        };
      }
      return {
        text: JSON.stringify({
          summary: 'Tagged v0.2.0 and pushed to origin.',
          links: [{ label: 'Release v0.2.0', url: 'https://github.com/acme/demo/releases/tag/v0.2.0' }],
        }),
      };
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    const seenTools: string[] = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'runCustomAction',
        requestId: 'req-act-1',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
        model: {
          provider: 'openai',
          apiFormat: 'openai-completions',
          modelId: 'mock-model',
          baseUrl: srv.url,
          apiKey: 'sk-mock',
        },
        branch: 'main',
        detached: false,
        remoteBranches: ['origin/main'],
        actionLabel: 'Release Tag',
        actionPrompt: 'git tag v0.2.0 and push to origin',
        extraPrompt: '',
        selectedFiles: [],
        unselectedPaths: [],
        localKeepPaths: [],
        locale: 'en',
        commitLanguage: 'en',
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          seenTools.push(event.toolName);
          queueMicrotask(() => {
            if (event.toolName === 'message_user') {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: '{}',
                details: {
                  cancelled: false,
                  answers: [{
                    questionIndex: 0,
                    questionId: 'q-1',
                    header: 'Push Tag',
                    selectedOptionIds: ['yes'],
                    selectedLabels: ['Push now'],
                  }],
                  unanswered: [],
                },
              });
            } else {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: 'ok',
              });
            }
          });
        }
      },
    );

    await runner.run();
    expect(seenTools).toEqual(['check_cli_env', 'run_git', 'message_user']);
    const done = events.find((e) => e.type === 'customActionCompleted');
    expect(done).toMatchObject({
      type: 'customActionCompleted',
      summary: 'Tagged v0.2.0 and pushed to origin.',
      links: [{ label: 'Release v0.2.0', url: 'https://github.com/acme/demo/releases/tag/v0.2.0' }],
    });
  });

  it('drafts a structured custom action button via Create with Pi (read-only inspection + ask_user_question)', async () => {
    let generatorSystemPrompt = '';
    const srv = await startMockOpenAiServer((body, callIndex) => {
      if (callIndex === 1) {
        const messages = Array.isArray(body.messages) ? body.messages as Array<{ role?: string; content?: string }> : [];
        generatorSystemPrompt = messages.find((m) => m.role === 'system')?.content ?? '';
        return {
          toolCalls: [{ id: 'g1', name: 'run_git', arguments: { args: ['tag', '-l'] } }],
        };
      }
      return {
        text: JSON.stringify({
          label: 'Release Tag',
          prompt: 'Bump the patch semver tag from the latest v0.x.y tag and push it to origin.',
          summary: 'Creates and pushes the next semver release tag.',
        }),
      };
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'generateCustomAction',
        requestId: 'req-gen-1',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
        model: {
          provider: 'openai',
          apiFormat: 'openai-completions',
          modelId: 'mock-model',
          baseUrl: srv.url,
          apiKey: 'sk-mock',
        },
        branch: 'main',
        existingLabels: ['Review PR'],
        userRequest: 'Build a release tag button',
        locale: 'en',
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          queueMicrotask(() => {
            runner.deliverToolResult?.({
              type: 'toolResult',
              requestId: event.requestId,
              callId: event.callId,
              ok: true,
              content: 'v0.1.0\nv0.1.1',
            });
          });
        }
      },
    );

    await runner.run();
    expect(generatorSystemPrompt).toContain('edit_repo_file');
    expect(generatorSystemPrompt).toContain('run_repo_command');
    expect(generatorSystemPrompt).toContain('commit_exact_scope');
    const drafted = events.find((e) => e.type === 'customActionDrafted');
    expect(drafted).toMatchObject({
      type: 'customActionDrafted',
      draft: {
        label: 'Release Tag',
        prompt: 'Bump the patch semver tag from the latest v0.x.y tag and push it to origin.',
        summary: 'Creates and pushes the next semver release tag.',
      },
    });
  });

  it('executes a full patch-release tool loop (edit_repo_file -> run_repo_command -> commit_exact_scope(editedPaths) -> tag -> atomic push)', async () => {
    let seenSystemPrompt = '';
    const srv = await startMockOpenAiServer((body, callIndex) => {
      if (callIndex === 1) {
        const messages = Array.isArray(body.messages) ? body.messages as Array<{ role?: string; content?: string }> : [];
        seenSystemPrompt = messages.find((m) => m.role === 'system')?.content ?? '';
        return {
          toolCalls: [{
            id: 'e1',
            name: 'edit_repo_file',
            arguments: {
              path: 'package.json',
              oldText: '"version": "0.2.0"',
              newText: '"version": "0.2.1"',
            },
          }],
        };
      }
      if (callIndex === 2) {
        return {
          toolCalls: [{
            id: 'r1',
            name: 'run_repo_command',
            arguments: {
              command: 'node',
              args: ['scripts/package-dmg.mjs', '--check-version-only', '--tag', 'v0.2.1'],
            },
          }],
        };
      }
      if (callIndex === 3) {
        return {
          toolCalls: [{
            id: 'cm1',
            name: 'commit_exact_scope',
            arguments: {
              editedPaths: ['package.json'],
              message: 'chore(release): bump version to 0.2.1\n\n- Bump version manifest to 0.2.1',
            },
          }],
        };
      }
      if (callIndex === 4) {
        return {
          toolCalls: [{
            id: 'g1',
            name: 'run_git',
            arguments: { args: ['push', '--atomic', 'origin', 'main', 'refs/tags/v0.2.1'] },
          }],
        };
      }
      return {
        text: JSON.stringify({
          summary: 'Released v0.2.1 and triggered GitHub Actions.',
          links: [{ label: 'Actions #101', url: 'https://github.com/dpbs-715/Picket/actions/runs/101' }],
        }),
      };
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    const toolCalls: Array<{ name: string; args: Record<string, unknown> }> = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'runCustomAction',
        requestId: 'req-rel-loop-1',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
        model: {
          provider: 'openai',
          apiFormat: 'openai-completions',
          modelId: 'mock-model',
          baseUrl: srv.url,
          apiKey: 'sk-mock',
        },
        branch: 'main',
        detached: false,
        remoteBranches: ['origin/main'],
        actionLabel: 'Patch Release',
        actionPrompt: 'Bump patch version, verify, commit, tag, and atomic push.',
        extraPrompt: '',
        selectedFiles: [],
        unselectedPaths: [],
        localKeepPaths: [],
        locale: 'en',
        commitLanguage: 'en',
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          toolCalls.push({ name: event.toolName, args: event.args });
          queueMicrotask(() => {
            runner.deliverToolResult?.({
              type: 'toolResult',
              requestId: event.requestId,
              callId: event.callId,
              ok: true,
              content: '{"ok":true}',
            });
          });
        }
      },
    );

    await runner.run();
    expect(seenSystemPrompt).toContain('edit_repo_file');
    expect(seenSystemPrompt).toContain('run_repo_command');
    expect(toolCalls.map((tc) => tc.name)).toEqual([
      'edit_repo_file',
      'run_repo_command',
      'commit_exact_scope',
      'run_git',
    ]);
    expect(toolCalls[2]?.args).toMatchObject({
      editedPaths: ['package.json'],
    });
    const done = events.find((e) => e.type === 'customActionCompleted');
    expect(done).toMatchObject({
      type: 'customActionCompleted',
      summary: 'Released v0.2.1 and triggered GitHub Actions.',
      links: [{ label: 'Actions #101', url: 'https://github.com/dpbs-715/Picket/actions/runs/101' }],
    });
  });

  it('supports write_repo_file and injects <user_follow_up_instruction> with <previous_session_checkpoint>', async () => {
    let seenUserPrompt = '';
    const srv = await startMockOpenAiServer((body, callIndex) => {
      if (callIndex === 1) {
        const messages = Array.isArray(body.messages) ? body.messages as Array<{ role?: string; content?: unknown }> : [];
        const userContent = messages.find((m) => m.role === 'user')?.content;
        seenUserPrompt = typeof userContent === 'string' ? userContent : JSON.stringify(userContent ?? '');
        return {
          toolCalls: [{
            id: 'w1',
            name: 'write_repo_file',
            arguments: {
              path: 'CHANGELOG.md',
              content: '# Changelog\n\n- v0.2.2\n',
            },
          }],
        };
      }
      if (callIndex === 2) {
        return {
          toolCalls: [{
            id: 'c1',
            name: 'commit_exact_scope',
            arguments: {
              editedPaths: ['CHANGELOG.md'],
              message: 'docs(release): add changelog for v0.2.2\n\n- record v0.2.2 release notes',
            },
          }],
        };
      }
      return {
        text: JSON.stringify({
          summary: 'Created CHANGELOG.md and committed it.',
          links: [],
        }),
      };
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    const seenTools: string[] = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'runCustomAction',
        requestId: 'req-followup-1',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
        model: {
          provider: 'openai',
          apiFormat: 'openai-completions',
          modelId: 'mock-model',
          baseUrl: srv.url,
          apiKey: 'sk-mock',
        },
        branch: 'main',
        detached: false,
        remoteBranches: ['origin/main'],
        actionLabel: 'Patch Release',
        actionPrompt: 'Bump patch version and release.',
        extraPrompt: '',
        followUpPrompt: 'Also create CHANGELOG.md and commit it.',
        previousCheckpoint: 'Previous runId=run-prev action="Patch Release" status=done',
        selectedFiles: [],
        unselectedPaths: [],
        localKeepPaths: [],
        locale: 'en',
        commitLanguage: 'en',
      },
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
              content: '{"ok":true}',
            });
          });
        }
      },
    );

    await runner.run();
    expect(seenUserPrompt).toContain('<user_follow_up_instruction>');
    expect(seenUserPrompt).toContain('Also create CHANGELOG.md and commit it.');
    expect(seenUserPrompt).toContain('<previous_session_checkpoint>');
    expect(seenTools).toEqual(['write_repo_file', 'commit_exact_scope']);
    const done = events.find((e) => e.type === 'customActionCompleted');
    expect(done).toMatchObject({
      type: 'customActionCompleted',
      summary: 'Created CHANGELOG.md and committed it.',
    });
  });

  it('injects upstream and recentCommits into custom actions, exposes read_scope_diff & multi-group commit_exact_scope, and stops immediately on handoff_to_recovery', async () => {
    let seenUserPrompt = '';
    let seenToolNames: string[] = [];
    let httpCalls = 0;
    const srv = await startMockOpenAiServer((body, callIndex) => {
      httpCalls = callIndex;
      if (callIndex === 1) {
        const messages = Array.isArray(body.messages) ? body.messages as Array<{ role?: string; content?: unknown }> : [];
        const userContent = messages.find((m) => m.role === 'user')?.content;
        seenUserPrompt = typeof userContent === 'string' ? userContent : JSON.stringify(userContent ?? '');
        const tools = Array.isArray(body.tools) ? body.tools as Array<{ function?: { name?: string } }> : [];
        seenToolNames = tools.map((t) => t.function?.name ?? '').filter(Boolean);
        return {
          toolCalls: [{
            id: 'd1',
            name: 'read_scope_diff',
            arguments: { path: 'src/app.ts', startLine: 1, endLine: 40 },
          }],
        };
      }
      if (callIndex === 2) {
        return {
          toolCalls: [{
            id: 'cm1',
            name: 'commit_exact_scope',
            arguments: {
              groups: [
                { fileIds: ['@1'], message: 'feat(app): update runtime flow\n\n- refine app logic' },
                { fileIds: ['@2'], message: 'test(app): update unit test\n\n- cover runtime flow' },
              ],
              push: true,
            },
          }],
        };
      }
      if (callIndex === 3) {
        return {
          toolCalls: [{
            id: 'hr1',
            name: 'handoff_to_recovery',
            arguments: {
              handleId: 'rh-1',
              summary: 'Commits landed locally; handing push failure off to host recovery.',
            },
          }],
        };
      }
      throw new Error(`Unexpected extra LLM turn #${callIndex} after stopAgent=true`);
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    const executedTools: Array<{ name: string; args: Record<string, unknown> }> = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'runCustomAction',
        requestId: 'req-unified-act-1',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
        model: {
          provider: 'openai',
          apiFormat: 'openai-completions',
          modelId: 'mock-model',
          baseUrl: srv.url,
          apiKey: 'sk-mock',
        },
        branch: 'feat/unified',
        detached: false,
        upstream: 'origin/feat/unified (ahead 0, behind 0)',
        remoteBranches: ['origin/main', 'origin/feat/unified'],
        recentCommits: ['feat: previous baseline commit'],
        actionLabel: 'Smart Commit & Push',
        actionPrompt: 'Split code and tests into separate commits and push.',
        extraPrompt: '',
        selectedFiles: [
          { fileId: 'f-1', path: 'src/app.ts', status: 'M', rangeKind: 'unstaged', added: 5, removed: 1 },
          { fileId: 'f-2', path: 'src/app.test.ts', status: 'M', rangeKind: 'unstaged', added: 10, removed: 0 },
        ],
        unselectedPaths: [],
        localKeepPaths: [],
        locale: 'en',
        commitLanguage: 'en',
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          executedTools.push({ name: event.toolName, args: event.args });
          queueMicrotask(() => {
            if (event.toolName === 'read_scope_diff') {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: '1: +export const unified = true;',
              });
            } else if (event.toolName === 'commit_exact_scope') {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: JSON.stringify({
                  committed: true,
                  commitCount: 2,
                  pushed: false,
                  pushError: 'remote rejected',
                  recoveryHandle: { handleId: 'rh-1', kind: 'pushFailed', summary: 'remote rejected' },
                }),
              });
            } else if (event.toolName === 'handoff_to_recovery') {
              runner.deliverToolResult?.({
                type: 'toolResult',
                requestId: event.requestId,
                callId: event.callId,
                ok: true,
                content: 'Commits landed locally; handing push failure off to host recovery.',
                details: {
                  stopAgent: true,
                  summary: 'Commits landed locally; handing push failure off to host recovery.',
                  links: [],
                },
              });
            }
          });
        }
      },
    );

    await runner.run();
    expect(httpCalls).toBe(3);
    expect(seenToolNames).toContain('read_scope_diff');
    expect(seenToolNames).toContain('commit_exact_scope');
    expect(seenToolNames).toContain('handoff_to_recovery');
    expect(seenUserPrompt).toContain('origin/feat/unified (ahead 0, behind 0)');
    expect(seenUserPrompt).toContain('feat: previous baseline commit');
    expect(executedTools.map((t) => t.name)).toEqual([
      'read_scope_diff',
      'commit_exact_scope',
      'handoff_to_recovery',
    ]);
    expect(executedTools[1]?.args).toMatchObject({
      groups: [
        { fileIds: ['@1'], message: expect.stringContaining('feat(app)') },
        { fileIds: ['@2'], message: expect.stringContaining('test(app)') },
      ],
      push: true,
    });
    const done = events.find((e) => e.type === 'customActionCompleted');
    expect(done).toMatchObject({
      type: 'customActionCompleted',
      summary: 'Commits landed locally; handing push failure off to host recovery.',
    });
  });

  it('fences subsequent tool calls in the same turn after message_user returns stopAgent and injects approvalMode guidance', async () => {
    let seenSystemPrompt = '';
    const srv = await startMockOpenAiServer((body, callIndex) => {
      if (callIndex === 1) {
        const messages = Array.isArray(body.messages) ? body.messages as Array<{ role?: string; content?: string }> : [];
        seenSystemPrompt = messages.find((m) => m.role === 'system')?.content ?? '';
        return {
          toolCalls: [
            {
              id: 'm1',
              name: 'message_user',
              arguments: {
                title: 'Confirm Action',
                questions: [{
                  header: 'Confirm',
                  question: 'Proceed?',
                  options: [
                    { id: 'yes', label: 'Yes', description: 'Continue execution' },
                    { id: 'no', label: 'No', description: 'Stop now' },
                  ],
                }],
              },
            },
            {
              id: 'g-after-stop',
              name: 'run_git',
              arguments: { args: ['status'] },
            },
          ],
        };
      }
      throw new Error(`Unexpected extra LLM turn #${callIndex} after stopAgent=true`);
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    const executedTools: string[] = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'runCustomAction',
        requestId: 'req-fence-1',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
        model: {
          provider: 'openai',
          apiFormat: 'openai-completions',
          modelId: 'mock-model',
          baseUrl: srv.url,
          apiKey: 'sk-mock',
        },
        branch: 'main',
        detached: false,
        remoteBranches: ['origin/main'],
        actionLabel: 'Fence Test',
        actionPrompt: 'Ask user then run git status.',
        extraPrompt: '',
        selectedFiles: [],
        unselectedPaths: [],
        localKeepPaths: [],
        locale: 'en',
        commitLanguage: 'en',
        approvalMode: 'ask',
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          executedTools.push(event.toolName);
          queueMicrotask(() => {
            runner.deliverToolResult?.({
              type: 'toolResult',
              requestId: event.requestId,
              callId: event.callId,
              ok: true,
              content: 'User declined or stopped.',
              details: {
                stopAgent: true,
                summary: 'Stopped cleanly after user decision.',
                links: [],
              },
            });
          });
        }
      },
    );

    await runner.run();
    expect(seenSystemPrompt).toContain('Host write-approval mode is `ask`');
    expect(executedTools).toEqual(['message_user']);
    const done = events.find((e) => e.type === 'customActionCompleted');
    expect(done).toMatchObject({
      type: 'customActionCompleted',
      summary: 'Stopped cleanly after user decision.',
    });
  });
});
