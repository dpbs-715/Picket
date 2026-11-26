import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { afterEach, describe, expect, it } from 'vitest';
import {
  formatAskUserQuestionResponseForModel,
  validateAskUserQuestionRequest,
} from '../../shared/ask-user-protocol';
import type { ProbeEvent, ReviewScopeCommand } from '../../shared/pi-protocol';
import { sanitizeProviderError } from '../src/providers';
import {
  SkillRegistry,
  getBundledSkillsManifest,
  normalizeUpstreamSkillMarkdown,
} from '../src/skills';
import { createTaskRunner, parseReviewFindings } from '../src/tasks';
import { ToolBridge, createAskUserQuestionTool } from '../src/tools';

describe('Dual-layer Git skill registry & upstream normalization', () => {
  it('bundles all 8 Git skills with references and normalizes {{REPO_ROOT}} links', async () => {
    const manifest = getBundledSkillsManifest();
    const names = manifest.skills.map((s) => s.name).sort();
    expect(names).toEqual([
      'commit',
      'cpush',
      'create-issues',
      'create-pr',
      'dcr',
      'push',
      'review-pr',
      'submit',
    ]);

    const registry = new SkillRegistry();
    const cpush = await registry.readDoc('cpush', 'SKILL.md');
    expect(cpush.source).toBe('bundled');
    expect(cpush.content).toContain('skill://commit/SKILL.md');
    expect(cpush.content).toContain('skill://push/SKILL.md');
    expect(cpush.content).not.toContain('{{REPO_ROOT}}');

    const mergeChain = await registry.readDoc('submit', 'references/merge-chain.md');
    expect(mergeChain.source).toBe('bundled');
    expect(mergeChain.content).toContain('skill://review-pr/SKILL.md');

    const approvalRestore = await registry.readDoc('review-pr', 'references/approval-restoration.md');
    expect(approvalRestore.content).toContain('glab mr approve');

    await expect(registry.readDoc('dcr', '../commit/SKILL.md')).rejects.toThrow(/stay inside/);
    await expect(registry.readDoc('nonexistent', 'SKILL.md')).rejects.toThrow(/Unknown skill/);
  });

  it('prefers runtime override layer over bundled skills while falling back cleanly', async () => {
    const registry = new SkillRegistry(
      {
        sourceRepo: 'https://github.com/dpbs-715/skills',
        sourceCommit: 'deadbeef',
        skills: [
          { name: 'dcr', description: 'Hot-synced DCR', files: ['SKILL.md'] },
        ],
      },
      async (skill, path) => {
        if (skill === 'dcr' && path === 'SKILL.md') {
          return '---\nname: dcr\ndescription: Hot-synced DCR\n---\n\n# Hot DCR\nSee {{REPO_ROOT}}/skills/review-pr/SKILL.md';
        }
        return null;
      },
    );

    const dcr = await registry.readDoc('dcr', 'SKILL.md');
    expect(dcr.source).toBe('override');
    expect(dcr.content).toContain('# Hot DCR');
    expect(dcr.content).toContain('skill://review-pr/SKILL.md');

    const commit = await registry.readDoc('commit', 'SKILL.md');
    expect(commit.source).toBe('bundled');

    expect(normalizeUpstreamSkillMarkdown('See {{REPO_ROOT}}/skills/dcr/SKILL.md')).toBe('See skill://dcr/SKILL.md');
  });
});

describe('ask_user_question validation, formatting, and tool wrapper', () => {
  it('validates constraints and rejects reserved labels or preview+multiSelect', () => {
    expect(validateAskUserQuestionRequest({
      questions: [{
        header: 'Scope',
        question: 'How should we group these changes?',
        options: [
          { label: 'Single PR', description: 'Combine into one PR' },
          { label: 'Other', description: 'Custom choice' },
        ],
      }],
    })).toMatchObject({ ok: false, code: 'reserved_option_label' });

    expect(validateAskUserQuestionRequest({
      questions: [{
        header: 'Labels',
        question: 'Pick labels',
        multiSelect: true,
        options: [
          { label: 'bug', description: 'Bug fix', preview: 'preview text' },
          { label: 'feat', description: 'Feature' },
        ],
      }],
    })).toMatchObject({ ok: false, code: 'preview_with_multiselect' });

    const valid = validateAskUserQuestionRequest({
      questions: [
        {
          header: 'Grouping',
          question: 'Group payment and docs separately?',
          options: [
            { id: 'split', label: 'Split into 2 PRs', description: 'Cleaner review', recommended: true, preview: 'PR 1: pay\nPR 2: docs' },
            { id: 'single', label: 'Single PR', description: 'Ship together' },
          ],
        },
      ],
      context: {
        scope: '2 feature areas',
        allowDontAskAgain: true,
      },
    });
    expect(valid.ok).toBe(true);
    if (!valid.ok) return;

    const answeredText = formatAskUserQuestionResponseForModel(valid.request, {
      cancelled: false,
      answers: [{
        questionIndex: 0,
        questionId: 'q-1',
        header: 'Grouping',
        selectedOptionIds: ['split'],
        selectedLabels: ['Split into 2 PRs'],
      }],
      unanswered: [],
      dontAskAgain: true,
    });
    expect(answeredText).toContain('[Grouping] Split into 2 PRs (id=split)');
    expect(answeredText).toContain('do not ask again');

    const cancelledText = formatAskUserQuestionResponseForModel(valid.request, {
      cancelled: true,
      answers: [],
      unanswered: [0],
    });
    expect(cancelledText).toBe('User declined to answer questions.');
  });

  it('bridges ask_user_question tool execution over ToolBridge', async () => {
    const events: ProbeEvent[] = [];
    const bridge = new ToolBridge(
      {
        requestId: 'req-ask',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          bridge.deliverToolResult({
            type: 'toolResult',
            requestId: 'req-ask',
            callId: event.callId,
            ok: true,
            content: '{}',
            details: {
              cancelled: false,
              answers: [{
                questionIndex: 0,
                questionId: 'q-1',
                header: 'CLI Install',
                selectedOptionIds: ['brew-install'],
                selectedLabels: ['Install gh with Homebrew'],
              }],
              unanswered: [],
            },
          });
        }
      },
    );

    const tool = createAskUserQuestionTool(bridge);
    const result = await tool.execute('call-1', {
      questions: [{
        header: 'CLI Install',
        question: 'GitHub CLI (`gh`) is missing. Install via Homebrew?',
        options: [
          { id: 'brew-install', label: 'Install gh with Homebrew', description: 'Runs `brew install gh`', recommended: true },
          { id: 'draft-only', label: 'Draft only', description: 'Generate content without remote writes' },
        ],
      }],
    });
    expect(result.isError).toBeFalsy();
    expect(result.content[0]).toMatchObject({ type: 'text' });
    expect((result.content[0] as { text: string }).text).toContain('Install gh with Homebrew (id=brew-install)');
    expect(events.some((e) => e.type === 'toolCall' && e.toolName === 'ask_user_question')).toBe(true);
  });
});

describe('DCR review with real Pi Core multi-turn tool loop', () => {
  const servers: ReturnType<typeof createServer>[] = [];

  afterEach(async () => {
    while (servers.length > 0) {
      const server = servers.pop();
      await new Promise<void>((resolve) => server?.close(() => resolve()));
    }
  });

  it('maps P0/P1 to high and P2 to suggestion and runs read_repo_file tool roundtrip', async () => {
    const parsed = parseReviewFindings(
      JSON.stringify({
        findings: [
          { fileId: 'f1', path: 'src/auth.ts', line: 2, severity: 'P1', title: 'Missing expiry check', reason: 'Expired tokens pass validation.' },
          { fileId: 'f1', path: 'src/auth.ts', line: 5, severity: 'P2', title: 'Missing unit test', reason: 'Add regression test for expired token.' },
        ],
      }),
      [{ fileId: 'f1', path: 'src/auth.ts', status: 'M', rangeKind: 'unstaged', added: 2, removed: 1, patch: '+return true;' }],
    );
    expect(parsed[0].severity).toBe('high');
    expect(parsed[1].severity).toBe('suggestion');

    let turn = 0;
    const receivedBodies: string[] = [];
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      const chunks: Buffer[] = [];
      req.on('data', (chunk: Buffer) => chunks.push(chunk));
      req.on('end', () => {
        const body = Buffer.concat(chunks).toString('utf8');
        receivedBodies.push(body);
        turn += 1;
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          Connection: 'close',
        });

        if (turn === 1) {
          // Turn 1: model invokes read_repo_file tool via Anthropic Messages SSE
          const toolArgs = JSON.stringify({ path: 'src/caller.ts', startLine: 1, endLine: 20 });
          res.write(`event: message_start\ndata: ${JSON.stringify({
            type: 'message_start',
            message: { id: 'msg_1', type: 'message', role: 'assistant', model: 'claude-test', content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 1 } },
          })}\n\n`);
          res.write(`event: content_block_start\ndata: ${JSON.stringify({
            type: 'content_block_start',
            index: 0,
            content_block: { type: 'tool_use', id: 'toolu_1', name: 'read_repo_file', input: {} },
          })}\n\n`);
          res.write(`event: content_block_delta\ndata: ${JSON.stringify({
            type: 'content_block_delta',
            index: 0,
            delta: { type: 'input_json_delta', partial_json: toolArgs },
          })}\n\n`);
          res.write(`event: content_block_stop\ndata: ${JSON.stringify({ type: 'content_block_stop', index: 0 })}\n\n`);
          res.write(`event: message_delta\ndata: ${JSON.stringify({
            type: 'message_delta',
            delta: { stop_reason: 'tool_use', stop_sequence: null },
            usage: { output_tokens: 15 },
          })}\n\n`);
          res.write(`event: message_stop\ndata: ${JSON.stringify({ type: 'message_stop' })}\n\n`);
          res.end();
        } else {
          // Turn 2: model emits final structured JSON after reading caller context
          const finalJson = JSON.stringify({
            findings: [{
              fileId: 'f1',
              path: 'src/auth.ts',
              line: 2,
              severity: 'P1',
              title: 'Caller expects strict expiry rejection',
              reason: 'src/caller.ts passes expired tokens expecting false; returning true bypasses auth.',
            }],
          });
          res.write(`event: message_start\ndata: ${JSON.stringify({
            type: 'message_start',
            message: { id: 'msg_2', type: 'message', role: 'assistant', model: 'claude-test', content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 20, output_tokens: 1 } },
          })}\n\n`);
          res.write(`event: content_block_start\ndata: ${JSON.stringify({
            type: 'content_block_start',
            index: 0,
            content_block: { type: 'text', text: '' },
          })}\n\n`);
          res.write(`event: content_block_delta\ndata: ${JSON.stringify({
            type: 'content_block_delta',
            index: 0,
            delta: { type: 'text_delta', text: finalJson },
          })}\n\n`);
          res.write(`event: content_block_stop\ndata: ${JSON.stringify({ type: 'content_block_stop', index: 0 })}\n\n`);
          res.write(`event: message_delta\ndata: ${JSON.stringify({
            type: 'message_delta',
            delta: { stop_reason: 'end_turn', stop_sequence: null },
            usage: { output_tokens: 25 },
          })}\n\n`);
          res.write(`event: message_stop\ndata: ${JSON.stringify({ type: 'message_stop' })}\n\n`);
          res.end();
        }
      });
    });
    servers.push(server);
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('Expected TCP address');

    const command: ReviewScopeCommand = {
      type: 'reviewScope',
      requestId: 'req-dcr-loop',
      context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
      model: {
        provider: 'anthropic',
        apiFormat: 'anthropic-messages',
        modelId: 'claude-test',
        baseUrl: `http://127.0.0.1:${address.port}`,
        apiKey: 'sk-ant-test-key',
      },
      branch: 'feat/auth',
      prompt: 'Check caller assumptions',
      files: [{
        fileId: 'f1',
        path: 'src/auth.ts',
        status: 'M',
        rangeKind: 'unstaged',
        added: 1,
        removed: 1,
        patch: '@@ -1,3 +1,3 @@\n-  return isValid(token);\n+  return true;',
      }],
      skippedPaths: [],
      locale: 'en',
      enableTools: true,
    };

    const emitted: ProbeEvent[] = [];
    let runner!: ReturnType<typeof createTaskRunner>;
    runner = createTaskRunner(command, (event) => {
      emitted.push(event);
      if (event.type === 'toolCall') {
        expect(event.toolName).toBe('read_repo_file');
        expect(event.args).toEqual({ path: 'src/caller.ts', startLine: 1, endLine: 20 });
        runner.deliverToolResult?.({
          type: 'toolResult',
          requestId: command.requestId,
          callId: event.callId,
          ok: true,
          content: 'File: src/caller.ts\n1: import { verifyToken } from "./auth";\n2: if (!verifyToken(expired)) deny();',
        });
      }
    });

    await runner.run();

    expect(turn).toBe(2);
    expect(receivedBodies[0]).toContain('<dcr_skill_spec>');
    expect(receivedBodies[0]).toContain('read_scope_diff');
    expect(receivedBodies[0]).toContain('read_repo_file');
    expect(receivedBodies[1]).toContain('verifyToken(expired)');
    const completed = emitted.find((e) => e.type === 'reviewCompleted');
    expect(completed?.type).toBe('reviewCompleted');
    if (completed?.type === 'reviewCompleted') {
      expect(completed.findings).toHaveLength(1);
      expect(completed.findings[0]).toMatchObject({
        fileId: 'f1',
        severity: 'high',
        title: 'Caller expects strict expiry rejection',
      });
    }
  });

  it('surfaces localized actionable guidance when a model does not support tool calling', () => {
    const spec = {
      provider: 'ollama' as const,
      apiFormat: 'openai-completions' as const,
      modelId: 'tiny-local',
      baseUrl: 'http://127.0.0.1:11434/v1',
      apiKey: '',
    };
    const enMsg = sanitizeProviderError(new Error('400 model tiny-local does not support tools'), spec, 'en');
    const zhMsg = sanitizeProviderError(new Error('400 model tiny-local does not support tools'), spec, 'zh');
    expect(enMsg).toContain('does not support tool calling');
    expect(zhMsg).toContain('不支持工具调用');
  });
});
