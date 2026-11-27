import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { afterEach, describe, expect, it } from 'vitest';
import { decodeCommand, type ProbeEvent, type RunCustomActionCommand } from '../../shared/pi-protocol';
import { SkillRegistry } from '../src/skills';
import { buildCustomActionPrompts, createTaskRunner } from '../src/tasks';

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

const HEAD_SHA = '1111111111111111111111111111111111111111';
const PROMO_SHA = '2222222222222222222222222222222222222222';

describe('Phase 3 submit full-chain sidecar tools and resume checkpoint', () => {
  const closers: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (closers.length > 0) {
      await closers.pop()!();
    }
  });

  it('decodes previousCheckpoint and injects <previous_session_checkpoint> into custom action prompts', () => {
    const raw: RunCustomActionCommand = {
      type: 'runCustomAction',
      requestId: 'req-resume-decode',
      context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
      model: {
        provider: 'openai',
        apiFormat: 'openai-completions',
        modelId: 'mock-model',
        baseUrl: 'http://127.0.0.1:9999/v1',
        apiKey: 'sk-test',
      },
      branch: 'main',
      detached: false,
      remoteBranches: ['origin/main', 'origin/v1.9.0', 'origin/test'],
      actionLabel: 'Submit to v1.9.0 then test',
      actionPrompt: 'submit -ch to v1.9.0 then test',
      extraPrompt: '',
      selectedFiles: [],
      unselectedPaths: [],
      localKeepPaths: [],
      locale: 'zh',
      commitLanguage: 'zh',
      previousCheckpoint: 'Previous runId=run-old status=interrupted\nPR #101: https://github.com/acme/demo/pull/101',
    };
    const decoded = decodeCommand(JSON.stringify(raw)) as RunCustomActionCommand;
    expect(decoded.previousCheckpoint).toContain('run-old');

    const { systemPrompt, userPrompt } = buildCustomActionPrompts(decoded, new SkillRegistry());
    expect(systemPrompt).toContain('manage_submit_worktree');
    expect(systemPrompt).toContain('cleanup_submit_duplicates');
    expect(systemPrompt).toContain('record_action_checkpoint');
    expect(userPrompt).toContain('<previous_session_checkpoint>');
    expect(userPrompt).toContain('https://github.com/acme/demo/pull/101');
  });

  it('executes a full submit two-stage promotion workflow tool loop end-to-end', async () => {
    const srv = await startMockOpenAiServer((_body, callIndex) => {
      switch (callIndex) {
        case 1:
          return {
            toolCalls: [
              { id: 'c1', name: 'read_skill_doc', arguments: { skill: 'submit', path: 'SKILL.md' } },
              { id: 'c2', name: 'read_skill_doc', arguments: { skill: 'submit', path: 'references/merge-chain.md' } },
              { id: 'c3', name: 'check_cli_env', arguments: {} },
            ],
          };
        case 2:
          return {
            toolCalls: [{
              id: 'c4',
              name: 'manage_submit_worktree',
              arguments: {
                op: 'create',
                groupId: 'group-1',
                title: 'Auth bug fix',
                branch: 'fix/issue-42-auth',
                baseRef: 'origin/v1.9.0',
                fileIds: ['f-auth'],
              },
            }],
          };
        case 3:
          return {
            toolCalls: [{
              id: 'c5',
              name: 'manage_submit_worktree',
              arguments: {
                op: 'commit',
                groupId: 'group-1',
                message: 'fix(auth): validate expired session tokens\n\n- reject expired bearer tokens before refresh',
                push: true,
                remote: 'origin',
              },
            }],
          };
        case 4:
          return {
            toolCalls: [{
              id: 'c6',
              name: 'run_platform_cli',
              arguments: {
                cli: 'gh',
                args: ['issue', 'create', '--title', 'Fix expired token refresh'],
                bodyText: '## Problem\nExpired tokens were accepted.',
                bodyFlag: '--body-file',
              },
            }],
          };
        case 5:
          return {
            toolCalls: [{
              id: 'c7',
              name: 'run_platform_cli',
              arguments: {
                cli: 'gh',
                args: ['pr', 'create', '--base', 'v1.9.0', '--head', 'fix/issue-42-auth', '--title', 'fix(auth): validate expired session tokens'],
                bodyText: 'Fixes #42',
                bodyFlag: '--body-file',
              },
            }],
          };
        case 6:
          return {
            toolCalls: [{
              id: 'c8',
              name: 'record_action_checkpoint',
              arguments: {
                workflowKind: 'submitPromote',
                firstTarget: 'v1.9.0',
                finalTarget: 'test',
                group: {
                  groupId: 'group-1',
                  issueUrl: 'https://github.com/acme/demo/issues/42',
                  issueNumber: '42',
                  branch: 'fix/issue-42-auth',
                  verifiedHeadSha: HEAD_SHA,
                  prUrl: 'https://github.com/acme/demo/pull/101',
                  prNumber: '101',
                  prState: 'open',
                },
              },
            }],
          };
        case 7:
          return {
            toolCalls: [{
              id: 'c9',
              name: 'message_user',
              arguments: {
                questions: [{
                  header: 'Step 5 Cleanup',
                  question: 'Clean up verified submitted duplicate changes in the original checkout (backed up first)?',
                  options: [
                    { id: 'cleanup-yes', label: 'Back up & clean duplicates', description: 'Verify remote tip and back up to .git/submit-backup first', recommended: true },
                    { id: 'cleanup-no', label: 'Keep local changes', description: 'Leave original checkout untouched' },
                  ],
                }],
              },
            }],
          };
        case 8:
          return {
            toolCalls: [{
              id: 'c10',
              name: 'cleanup_submit_duplicates',
              arguments: { userConfirmed: true, groupIds: ['group-1'] },
            }],
          };
        case 9:
          return {
            toolCalls: [
              {
                id: 'c11',
                name: 'run_platform_cli',
                arguments: {
                  cli: 'gh',
                  args: ['pr', 'merge', '101', '--squash', '--match-head-commit', HEAD_SHA],
                },
              },
              {
                id: 'c12',
                name: 'run_platform_cli',
                arguments: {
                  cli: 'gh',
                  args: ['pr', 'merge', '102', '--merge', '--match-head-commit', PROMO_SHA],
                },
              },
              {
                id: 'c13',
                name: 'run_platform_cli',
                arguments: {
                  cli: 'gh',
                  args: ['issue', 'close', '42'],
                },
              },
            ],
          };
        case 10:
          return {
            toolCalls: [
              {
                id: 'c14',
                name: 'run_git',
                arguments: {
                  args: ['push', 'origin', `--force-with-lease=refs/heads/fix/issue-42-auth:${HEAD_SHA}`, ':refs/heads/fix/issue-42-auth'],
                },
              },
              {
                id: 'c15',
                name: 'manage_submit_worktree',
                arguments: {
                  op: 'remove',
                  groupId: 'group-1',
                  expectedHeadSha: HEAD_SHA,
                  deleteLocalBranch: true,
                },
              },
            ],
          };
        default:
          return {
            text: JSON.stringify({
              summary: 'Submitted group-1 to v1.9.0 and promoted to test; closed #42 and cleaned verified temporary worktree.',
              links: [
                { label: 'Issue #42', url: 'https://github.com/acme/demo/issues/42' },
                { label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' },
                { label: 'PR #102 (v1.9.0 -> test)', url: 'https://github.com/acme/demo/pull/102' },
              ],
            }),
          };
      }
    });
    closers.push(srv.close);

    const events: ProbeEvent[] = [];
    const hostTools: Array<{ name: string; args: Record<string, unknown> }> = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(
      {
        type: 'runCustomAction',
        requestId: 'req-phase3-submit',
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
        remoteBranches: ['origin/main', 'origin/v1.9.0', 'origin/test'],
        actionLabel: 'Submit -> v1.9.0 -> test',
        actionPrompt: 'submit -ch to v1.9.0 then test',
        extraPrompt: '',
        selectedFiles: [{
          fileId: 'f-auth',
          path: 'src/auth.ts',
          status: 'M',
          rangeKind: 'unstaged',
          added: 4,
          removed: 1,
          patch: '@@ -1 +1 @@\n-old\n+new',
        }],
        unselectedPaths: ['src/unselected.ts'],
        localKeepPaths: ['.env.local'],
        locale: 'en',
        commitLanguage: 'zh',
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          hostTools.push({ name: event.toolName, args: event.args });
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
                    header: 'Step 5 Cleanup',
                    selectedOptionIds: ['cleanup-yes'],
                    selectedLabels: ['Back up & clean duplicates'],
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
                content: JSON.stringify({ ok: true, oid: HEAD_SHA }),
              });
            }
          });
        }
      },
    );

    await runner.run();
    const names = hostTools.map((t) => t.name);
    expect(names).toEqual([
      'check_cli_env',
      'manage_submit_worktree',
      'manage_submit_worktree',
      'run_platform_cli',
      'run_platform_cli',
      'record_action_checkpoint',
      'message_user',
      'cleanup_submit_duplicates',
      'run_platform_cli',
      'run_platform_cli',
      'run_platform_cli',
      'run_git',
      'manage_submit_worktree',
    ]);
    expect(hostTools[1].args).toMatchObject({ op: 'create', groupId: 'group-1', fileIds: ['f-auth'] });
    expect(hostTools[7].args).toMatchObject({ userConfirmed: true, groupIds: ['group-1'] });
    expect(hostTools[12].args).toMatchObject({ op: 'remove', groupId: 'group-1', deleteLocalBranch: true });

    const done = events.find((e) => e.type === 'customActionCompleted');
    expect(done).toMatchObject({
      type: 'customActionCompleted',
      links: [
        { label: 'Issue #42', url: 'https://github.com/acme/demo/issues/42' },
        { label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' },
        { label: 'PR #102 (v1.9.0 -> test)', url: 'https://github.com/acme/demo/pull/102' },
      ],
    });
  });

  it('reconciles a resumed session on a clean checkout using previousCheckpoint without duplicating resources', async () => {
    let sawCheckpointInPrompt = false;
    const srv = await startMockOpenAiServer((body, callIndex) => {
      if (callIndex === 1) {
        const serialized = JSON.stringify(body);
        sawCheckpointInPrompt = serialized.includes('<previous_session_checkpoint>')
          && serialized.includes('https://github.com/acme/demo/pull/101');
        return {
          toolCalls: [
            { id: 'r1', name: 'manage_submit_worktree', arguments: { op: 'list' } },
            { id: 'r2', name: 'run_platform_cli', arguments: { cli: 'gh', args: ['pr', 'view', '101', '--json', 'state,headRefOid'] } },
          ],
        };
      }
      return {
        text: JSON.stringify({
          summary: 'Reconciled PR #101 from checkpoint and verified live remote state without duplicating Issues or PRs.',
          links: [{ label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' }],
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
        requestId: 'req-phase3-resume',
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
        remoteBranches: ['origin/main', 'origin/v1.9.0'],
        actionLabel: 'Submit to v1.9.0',
        actionPrompt: 'submit -ch to v1.9.0 then merge',
        extraPrompt: '',
        selectedFiles: [],
        unselectedPaths: [],
        localKeepPaths: [],
        locale: 'en',
        commitLanguage: 'zh',
        previousCheckpoint: 'Previous runId=run-crashed status=interrupted\n  - group=group-1 pr=Some("https://github.com/acme/demo/pull/101")',
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
              content: '{"state":"MERGED"}',
            });
          });
        }
      },
    );

    await runner.run();
    expect(sawCheckpointInPrompt).toBe(true);
    expect(seenTools).toEqual(['manage_submit_worktree', 'run_platform_cli']);
    expect(events.find((e) => e.type === 'customActionCompleted')).toBeDefined();
  });
});
