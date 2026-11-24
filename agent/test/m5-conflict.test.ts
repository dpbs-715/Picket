import { describe, expect, it } from 'vitest';
import type { ResolveConflictCommand } from '../../shared/pi-protocol';
import { getSidecarMessages } from '../src/i18n';
import { sanitizeProviderError } from '../src/providers';
import { buildResolveConflictPrompts, hasConflictMarkers, parseConflictResolution } from '../src/tasks';

function sampleConflictCommand(overrides: Partial<ResolveConflictCommand> = {}): ResolveConflictCommand {
  return {
    type: 'resolveConflict',
    requestId: 'req-conflict-1',
    context: {
      repositoryId: 'repo-1',
      worktreeId: 'wt-1',
      sessionId: 'sess-1',
      scopeVersion: 'scope-v1',
    },
    model: {
      provider: 'openai',
      modelId: 'gpt-4.1',
      baseUrl: '',
      apiKey: 'sk-test-only',
    },
    branch: 'feat/pay',
    upstream: 'origin/feat/pay',
    userInstruction: 'Preserve local idempotency lock and keep upstream traceId logging',
    commits: ['a3f91c2 feat(order): add callback idempotency lock'],
    files: [
      {
        fileId: 'f-order',
        path: 'src/service/Order.ts',
        baseContent: 'export function handlePay() {\n  return ok();\n}\n',
        upstreamContent: 'export function handlePay(traceId: string) {\n  log(traceId);\n  return ok();\n}\n',
        localContent: 'export function handlePay() {\n  acquireLock();\n  return ok();\n}\n',
        conflictedContent: [
          '<<<<<<< HEAD',
          'export function handlePay(traceId: string) {',
          '  log(traceId);',
          '=======',
          'export function handlePay() {',
          '  acquireLock();',
          '>>>>>>> a3f91c2',
          '  return ok();',
          '}',
          '',
        ].join('\n'),
      },
    ],
    ...overrides,
  };
}

describe('M5 Pi Core conflict resolution parser and safety guards', () => {
  it('parses fenced / <think>-prefixed JSON conflict resolution and localizes default merge summaries', () => {
    const cmd = sampleConflictCommand();
    const raw = [
      '<think>Combine traceId parameter and acquireLock call.</think>',
      '```json',
      JSON.stringify({
        summary: 'Preserved local idempotency lock and merged upstream traceId parameter',
        resolvedFiles: [
          {
            fileId: 'f-order',
            path: 'src/service/Order.ts',
            content: 'export function handlePay(traceId: string) {\n  log(traceId);\n  acquireLock();\n  return ok();\n}\n',
          },
        ],
      }),
      '```',
    ].join('\n');

    const res = parseConflictResolution(raw, cmd);
    expect(res.summary).toContain('idempotency lock');
    expect(res.resolvedFiles).toEqual([
      {
        fileId: 'f-order',
        path: 'src/service/Order.ts',
        content: 'export function handlePay(traceId: string) {\n  log(traceId);\n  acquireLock();\n  return ok();\n}\n',
      },
    ]);

    const rawWithoutSummary = JSON.stringify({
      summary: '',
      resolvedFiles: [
        {
          fileId: 'f-order',
          path: 'src/service/Order.ts',
          content: 'export function handlePay(traceId: string) {\n  log(traceId);\n  acquireLock();\n  return ok();\n}\n',
        },
      ],
    });
    expect(parseConflictResolution(rawWithoutSummary, cmd).summary).toBe('Merged 1 conflicted file(s)');
    expect(parseConflictResolution(rawWithoutSummary, sampleConflictCommand({ locale: 'zh' })).summary).toBe(
      getSidecarMessages('zh').defaultConflictMergeSummary(1),
    );
  });

  it('rejects unauthorized file modifications, missing files, empty output, and leftover conflict markers in en and zh', () => {
    const cmdEn = sampleConflictCommand();
    const cmdZh = sampleConflictCommand({ locale: 'zh' });
    expect(hasConflictMarkers('normal code\n=======\nother\n')).toBe(true);
    expect(hasConflictMarkers('const sep = "======";\n')).toBe(false);

    // 1. Unauthorized file outside conflict list (and verify sanitizeProviderError does not misclassify "unauthorized" as 401):
    const unauthorizedJson = JSON.stringify({
      summary: 'bad',
      resolvedFiles: [
        { fileId: 'f-other', path: 'src/api/pay.ts', content: 'export const x = 1;\n' },
      ],
    });
    let caughtEn: unknown;
    try {
      parseConflictResolution(unauthorizedJson, cmdEn);
    } catch (err) {
      caughtEn = err;
    }
    expect(caughtEn).toBeInstanceOf(Error);
    expect((caughtEn as Error).message).toContain('Blocked unauthorized modification attempt');
    expect(sanitizeProviderError(caughtEn, cmdEn.model, 'en')).not.toContain('401/403');
    expect(() => parseConflictResolution(unauthorizedJson, cmdZh)).toThrow(
      getSidecarMessages('zh').unauthorizedConflictFile('src/api/pay.ts'),
    );

    // 2. Leftover conflict markers:
    const leftoverMarkersJson = JSON.stringify({
      summary: 'bad',
      resolvedFiles: [
        {
          fileId: 'f-order',
          path: 'src/service/Order.ts',
          content: '<<<<<<< HEAD\nfoo\n=======\nbar\n>>>>>>> commit\n',
        },
      ],
    });
    expect(() => parseConflictResolution(leftoverMarkersJson, cmdEn)).toThrow(/conflict markers/);
    expect(() => parseConflictResolution(leftoverMarkersJson, cmdZh)).toThrow(/\u51b2\u7a81\u6807\u8bb0/);

    // 3. Empty content wiping non-empty file:
    const emptyMergedJson = JSON.stringify({
      summary: 'bad',
      resolvedFiles: [
        { fileId: 'f-order', path: 'src/service/Order.ts', content: '   \n' },
      ],
    });
    expect(() => parseConflictResolution(emptyMergedJson, cmdEn)).toThrow(/is empty; merge aborted/);
    expect(() => parseConflictResolution(emptyMergedJson, cmdZh)).toThrow(/\u7a7a\u5185\u5bb9/);
  });

  it('switches conflict summary language directive between English and Chinese in prompts', () => {
    expect(buildResolveConflictPrompts(sampleConflictCommand()).systemPrompt).toContain(
      'Write `summary` in concise English.',
    );
    expect(buildResolveConflictPrompts(sampleConflictCommand({ locale: 'zh' })).systemPrompt).toContain(
      'Write `summary` in concise Chinese.',
    );
  });
});
