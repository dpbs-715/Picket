import { describe, expect, it } from 'vitest';
import {
  extractMessageUserCard,
  formatMessageUserResponseForModel,
  hasMessageQuestions,
  validateMessageUserRequest,
} from '../../shared/message-user-protocol';
import type { ProbeEvent } from '../../shared/pi-protocol';
import { sanitizeProviderError } from '../src/providers';
import {
  SkillRegistry,
  getBundledSkillsManifest,
  normalizeUpstreamSkillMarkdown,
} from '../src/skills';
import { ToolBridge, createMessageUserTool } from '../src/tools';

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

  it('removes disabled built-in skills from registry without falling back to bundled copies', async () => {
    const registry = new SkillRegistry({
      sourceRepo: 'https://github.com/dpbs-715/skills',
      sourceCommit: 'deadbeef',
      skills: [
        { name: 'commit', description: 'Synced commit', files: ['SKILL.md'] },
      ],
      disabledSkills: ['dcr', 'submit'],
    });

    expect(registry.hasSkill('dcr')).toBe(false);
    expect(registry.hasSkill('submit')).toBe(false);
    expect(registry.hasSkill('commit')).toBe(true);
    expect(registry.listSkills().map((s) => s.name)).not.toContain('dcr');
    expect(registry.formatIndex()).not.toContain('`dcr`');
    await expect(registry.readDoc('dcr', 'SKILL.md')).rejects.toThrow(/Unknown skill/);
  });
});

describe('message_user validation, rich blocks, formatting, and tool wrapper', () => {
  it('validates question constraints, non-blocking cards, and sanitizes unsafe paths/links', () => {
    // Rejects completely empty message_user calls:
    expect(validateMessageUserRequest({})).toMatchObject({ ok: false, code: 'missing_title' });

    // Rejects reserved labels:
    expect(validateMessageUserRequest({
      questions: [{
        header: 'Scope',
        question: 'How should we group these changes?',
        options: [
          { label: 'Single PR', description: 'Combine into one PR' },
          { label: 'Other', description: 'Custom choice' },
        ],
      }],
    })).toMatchObject({ ok: false, code: 'reserved_option_label' });

    // Rejects preview with multiSelect:
    expect(validateMessageUserRequest({
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

    // Rejects unsafe path escapes and .git paths in findings and files blocks:
    expect(validateMessageUserRequest({
      title: 'Unsafe finding path',
      blocks: [{
        type: 'findings',
        items: [{
          severity: 'low',
          title: 'Escape attempt',
          detail: 'Should reject ../ path.',
          path: '../outside/secret.env',
          line: 1,
        }],
      }],
    })).toMatchObject({ ok: false, code: 'invalid_finding_path' });

    expect(validateMessageUserRequest({
      title: 'Unsafe file chip path',
      blocks: [{
        type: 'files',
        items: [{ path: '.git/config', badge: 'blocked' }],
      }],
    })).toMatchObject({ ok: false, code: 'invalid_file_path' });

    // Validates non-blocking rich card with all 7 blocks and strips unsafe links:
    const richCardRes = validateMessageUserRequest({
      tone: 'warning',
      title: 'Code review finished',
      subtitle: '2 findings across 3 files',
      blocks: [
        { type: 'markdown', content: 'Review completed on `feat/pay`.' },
        { type: 'callout', tone: 'warning', title: 'Action suggested', content: 'Address P1 before merging.' },
        {
          type: 'facts',
          items: [
            { label: 'Scope', value: '3 files', mono: true },
            { label: 'High risk', value: '1', tone: 'danger' },
          ],
        },
        {
          type: 'findings',
          items: [
            {
              severity: 'high',
              tag: 'P1',
              title: 'Plaintext secret logged',
              detail: 'Redact payload before logging.',
              path: 'src/api/pay.ts',
              line: 46,
            },
            {
              severity: 'low',
              title: 'General observation',
              detail: 'Optional finding without file path.',
            },
          ],
          skipped: [{ path: 'pnpm-lock.yaml', reason: 'Lockfile' }],
        },
        {
          type: 'checklist',
          items: [{ status: 'pass', label: 'TypeScript check' }],
        },
        {
          type: 'code',
          title: 'tsc output',
          content: 'No errors',
          collapsed: false,
        },
        {
          type: 'files',
          items: [
            { path: 'src/api/pay.ts', line: 46, badge: 'M' },
          ],
        },
      ],
      context: {
        links: [
          { label: 'PR #42', url: 'https://github.com/acme/demo/pull/42' },
          { label: 'XSS', url: 'javascript:alert(1)' },
        ],
      },
    });
    expect(richCardRes.ok).toBe(true);
    if (!richCardRes.ok) return;
    expect(hasMessageQuestions(richCardRes.request)).toBe(false);
    const card = extractMessageUserCard(richCardRes.request);
    expect(card?.blocks).toHaveLength(7);
    const findingsBlock = card?.blocks?.[3];
    expect(findingsBlock?.type === 'findings' && findingsBlock.items[0].path).toBe('src/api/pay.ts');
    expect(findingsBlock?.type === 'findings' && findingsBlock.items[1].path).toBeUndefined();
    const filesBlock = card?.blocks?.[6];
    expect(filesBlock?.type === 'files' && filesBlock.items).toHaveLength(1);
    expect(richCardRes.request.context?.links).toEqual([
      { label: 'PR #42', url: 'https://github.com/acme/demo/pull/42' },
    ]);

    // Validates blocking Q&A + response formatting:
    const valid = validateMessageUserRequest({
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
    expect(hasMessageQuestions(valid.request)).toBe(true);

    const answeredText = formatMessageUserResponseForModel(valid.request, {
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

    const cancelledText = formatMessageUserResponseForModel(valid.request, {
      cancelled: true,
      answers: [],
      unanswered: [0],
    });
    expect(cancelledText).toBe('User declined to answer questions.');
  });

  it('bridges message_user tool execution for both non-blocking display cards and blocking Q&A', async () => {
    const events: ProbeEvent[] = [];
    const bridge = new ToolBridge(
      {
        requestId: 'req-msg',
        context: { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' },
      },
      (event) => {
        events.push(event);
        if (event.type === 'toolCall') {
          const args = event.args as { questions?: unknown[] };
          if (args.questions?.length) {
            bridge.deliverToolResult({
              type: 'toolResult',
              requestId: 'req-msg',
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
          } else {
            bridge.deliverToolResult({
              type: 'toolResult',
              requestId: 'req-msg',
              callId: event.callId,
              ok: true,
              content: 'Message displayed to user in Picket MessageDock.',
            });
          }
        }
      },
    );

    const tool = createMessageUserTool(bridge);

    // 1. Non-blocking display call:
    const displayResult = await tool.execute('call-display', {
      tone: 'success',
      title: 'Checks passed',
      blocks: [{ type: 'checklist', items: [{ status: 'pass', label: 'cargo test' }] }],
    });
    expect(displayResult.isError).toBeFalsy();
    expect((displayResult.content[0] as { text: string }).text).toContain('Message displayed');

    // 2. Blocking question call:
    const result = await tool.execute('call-1', {
      title: 'Install GitHub CLI?',
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
    expect(events.filter((e) => e.type === 'toolCall' && e.toolName === 'message_user')).toHaveLength(2);
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
