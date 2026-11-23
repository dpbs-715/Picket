import { describe, expect, it } from 'vitest';
import type { PlanCPushCommand } from '../../shared/pi-protocol';
import { parseCPushPlan } from '../src/tasks';

function basePlanCommand(qaCount = 0): PlanCPushCommand {
  return {
    type: 'planCPush',
    requestId: 'plan-1',
    context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'v1' },
    model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: '', apiKey: 'sk-placeholder' },
    branch: 'feat/pay',
    recentCommits: ['feat(order): 初始化订单模块'],
    draft: '',
    files: [
      { fileId: 'f-order', path: 'src/Order.ts', status: 'M', rangeKind: 'partial', added: 10, removed: 2, patch: '+lock()' },
      { fileId: 'f-retry', path: 'src/retry.ts', status: 'A', rangeKind: 'unstaged', added: 25, removed: 0, patch: '+retry()' },
      { fileId: 'f-doc', path: 'RELEASE_NOTES.md', status: 'M', rangeKind: 'unstaged', added: 6, removed: 0, patch: '+# Notes' },
    ],
    unselectedPaths: ['pnpm-lock.yaml'],
    qaHistory: qaCount > 0
      ? [{ questionId: 'q1', question: '发布说明要一起提交吗？', answer: '暂不提交', optionId: 'skip' }]
      : [],
  };
}

describe('M4 Pi Core C-Push plan parser and scope invariants', () => {
  it('parses disjoint split groups and preserves two-part commit messages', () => {
    const raw = [
      '<think>检查文件关联性</think>',
      '```json',
      JSON.stringify({
        kind: 'ready',
        summary: '拆分为业务功能与文档两笔提交',
        groups: [
          {
            id: 'g1',
            fileIds: ['f-order', 'f-retry'],
            message: 'feat(order): 增加幂等锁与重试工具\n\n- 沿用 Order.ts 已暂存范围更新抢锁逻辑\n- 新增退避重试工具',
          },
          {
            id: 'g2',
            fileIds: ['f-doc'],
            message: 'docs: 更新发布说明\n\n- 补充幂等锁上线说明',
          },
        ],
        skippedFileIds: [],
      }),
      '```',
    ].join('\n');
    const plan = parseCPushPlan(raw, basePlanCommand(0));
    expect(plan).toEqual({
      kind: 'ready',
      summary: '拆分为业务功能与文档两笔提交',
      groups: [
        {
          id: 'g1',
          fileIds: ['f-order', 'f-retry'],
          message: 'feat(order): 增加幂等锁与重试工具\n\n- 沿用 Order.ts 已暂存范围更新抢锁逻辑\n- 新增退避重试工具',
        },
        {
          id: 'g2',
          fileIds: ['f-doc'],
          message: 'docs: 更新发布说明\n\n- 补充幂等锁上线说明',
        },
      ],
      skippedFileIds: [],
    });
  });

  it('parses disambiguation questions and filters out hallucinated affected files', () => {
    const raw = JSON.stringify({
      kind: 'question',
      title: '发布说明要一起提交吗？',
      scope: '影响提交 2/2「docs」',
      description: '请确认 RELEASE_NOTES.md 的归属方式：',
      affectedFileIds: ['f-doc', 'pnpm-lock.yaml'],
      options: [
        { id: 'merge', label: '合并进提交 1' },
        { id: 'split', label: '单独提交并一起推送', recommended: true },
        { id: 'skip', label: '暂不提交' },
      ],
    });
    const plan = parseCPushPlan(raw, basePlanCommand(0));
    expect(plan).toMatchObject({
      kind: 'question',
      title: '发布说明要一起提交吗？',
      affectedFileIds: ['f-doc'],
    });
  });

  it('rejects unauthorized files, duplicate assignments, missing selected files, and unconfirmed skips', () => {
    // Unauthorized file `pnpm-lock.yaml`:
    expect(() => parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'bad',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry', 'f-doc', 'pnpm-lock.yaml'], message: 'feat: bad\n\n- x' }],
      skippedFileIds: [],
    }), basePlanCommand(0))).toThrow('未授权文件');

    // Unconfirmed skip when qaHistory is empty:
    expect(() => parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'unasked skip',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry'], message: 'feat: ok\n\n- x' }],
      skippedFileIds: ['f-doc'],
    }), basePlanCommand(0))).toThrow('不能擅自跳过');

    // Confirmed skip when qaHistory is non-empty succeeds:
    expect(parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'confirmed skip',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry'], message: 'feat: ok\n\n- x' }],
      skippedFileIds: ['f-doc'],
    }), basePlanCommand(1))).toMatchObject({
      kind: 'ready',
      skippedFileIds: ['f-doc'],
    });

    // Missing selected file `f-doc`:
    expect(() => parseCPushPlan(JSON.stringify({
      kind: 'ready',
      summary: 'incomplete',
      groups: [{ id: 'g1', fileIds: ['f-order', 'f-retry'], message: 'feat: partial\n\n- x' }],
      skippedFileIds: [],
    }), basePlanCommand(0))).toThrow('未被纳入');
  });
});
