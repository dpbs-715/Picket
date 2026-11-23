import { describe, expect, it } from 'vitest';
import type { ResolveConflictCommand } from '../../shared/pi-protocol';
import { hasConflictMarkers, parseConflictResolution } from '../src/tasks';

function sampleConflictCommand(): ResolveConflictCommand {
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
    userInstruction: '保留本地幂等锁并合入远端 traceId 日志',
    commits: ['a3f91c2 feat(order): 增加回调幂等锁'],
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
  };
}

describe('M5 Pi Core conflict resolution parser and safety guards', () => {
  it('parses fenced / <think>-prefixed JSON conflict resolution for authorized files', () => {
    const cmd = sampleConflictCommand();
    const raw = [
      '<think>Combine traceId parameter and acquireLock call.</think>',
      '```json',
      JSON.stringify({
        summary: '已保留本地原子锁并合入远端 traceId 日志参数',
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
    expect(res.summary).toContain('原子锁');
    expect(res.resolvedFiles).toEqual([
      {
        fileId: 'f-order',
        path: 'src/service/Order.ts',
        content: 'export function handlePay(traceId: string) {\n  log(traceId);\n  acquireLock();\n  return ok();\n}\n',
      },
    ]);
  });

  it('rejects unauthorized file modifications, missing files, empty output, and leftover conflict markers', () => {
    const cmd = sampleConflictCommand();
    expect(hasConflictMarkers('normal code\n=======\nother\n')).toBe(true);
    expect(hasConflictMarkers('const sep = "======";\n')).toBe(false);

    // 1. Unauthorized file outside conflict list:
    expect(() =>
      parseConflictResolution(
        JSON.stringify({
          summary: 'bad',
          resolvedFiles: [
            { fileId: 'f-other', path: 'src/api/pay.ts', content: 'export const x = 1;\n' },
          ],
        }),
        cmd,
      ),
    ).toThrow(/未授权/);

    // 2. Leftover conflict markers:
    expect(() =>
      parseConflictResolution(
        JSON.stringify({
          summary: 'bad',
          resolvedFiles: [
            {
              fileId: 'f-order',
              path: 'src/service/Order.ts',
              content: '<<<<<<< HEAD\nfoo\n=======\nbar\n>>>>>>> commit\n',
            },
          ],
        }),
        cmd,
      ),
    ).toThrow(/冲突标记/);

    // 3. Empty content wiping non-empty file:
    expect(() =>
      parseConflictResolution(
        JSON.stringify({
          summary: 'bad',
          resolvedFiles: [
            { fileId: 'f-order', path: 'src/service/Order.ts', content: '   \n' },
          ],
        }),
        cmd,
      ),
    ).toThrow(/空内容/);
  });
});
