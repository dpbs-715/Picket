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

  it('runs a multi-turn custom action tool loop (check_cli_env -> run_git -> ask_user_question -> completed with links)', async () => {
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
            name: 'ask_user_question',
            arguments: {
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
            if (event.toolName === 'ask_user_question') {
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
    expect(seenTools).toEqual(['check_cli_env', 'run_git', 'ask_user_question']);
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
});
