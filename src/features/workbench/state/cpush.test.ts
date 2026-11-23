import { describe, expect, it } from 'vitest';
import type { CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import { repositoryReply } from '../testing/repository-fixture';
import { sampleFindings } from '../testing/fixtures';
import { emptyWorkbench, orderedMessages, targetOf, workbenchReducer } from './workbench';

function openedWorkbench() {
  let state = workbenchReducer(emptyWorkbench(), { type: 'repositoryReadStarted', key: 'open', requestId: 'open-1' });
  state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open-1') });
  return workbenchReducer(state, {
    type: 'modelSaved',
    model: { id: 'model-1', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' },
  });
}

function makeCPushSession(overrides: Partial<CPushSessionSnapshot> = {}): CPushSessionSnapshot {
  return {
    runId: 'run-1',
    repositoryId: 'orders',
    worktreeId: 'pay-tree',
    sessionId: 'sess-frozen',
    frozenScopeVersion: 'scope-frozen',
    modelId: 'model-1',
    frozenFileIds: ['order', 'pay', 'retry', 'release'],
    committedFileIds: [],
    skippedFileIds: [],
    sessionStatus: 'needsDecision',
    message: {
      id: 'q-run-1-1',
      kind: 'question',
      title: '发布说明要一起提交吗？',
      scope: '影响提交 2/2「docs」',
      description: '请确认 RELEASE_NOTES.md 归属方式：',
      options: [
        { id: 'merge', label: '合并进提交 1' },
        { id: 'split', label: '单独提交并一起推送', recommended: true },
        { id: 'skip', label: '暂不提交' },
      ],
    },
    timeline: [
      { state: 'done', label: '已固定 4 个已选文件范围' },
      { state: 'active', label: '等待决定：发布说明要一起提交吗？' },
    ],
    qaLog: [],
    commits: [],
    canRetryRemaining: false,
    ...overrides,
  };
}

describe('M4 C-Push 会话状态、范围冻结与恢复隔离', () => {
  it('运行中修改勾选或切换工作树不改变已冻结的会话目标与文件范围（W03）', () => {
    let state = openedWorkbench();
    const tree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    const frozenTarget = targetOf(tree);

    state = workbenchReducer(state, {
      type: 'cPushStarted',
      worktreeId: 'pay-tree',
      runId: 'run-1',
      frozenTarget,
      frozenFileIds: ['order', 'pay', 'retry', 'release'],
      modelId: 'model-1',
      expandDock: false,
    });

    // User ticks `lock` and switches worktrees mid-run:
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'lock' });
    state = workbenchReducer(state, { type: 'switch', worktreeId: 'fix-tree' });

    const payAfterSelect = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(payAfterSelect.selectedIds).toContain('lock');
    expect(payAfterSelect.scopeVersion).not.toBe(frozenTarget.scopeVersion);
    // Frozen C-Push target & authorized fileIds remain strictly immutable:
    expect(payAfterSelect.cPush?.frozenTarget).toEqual(frozenTarget);
    expect(payAfterSelect.cPush?.frozenFileIds).toEqual(['order', 'pay', 'retry', 'release']);

    // Question arrives while viewing `fix-tree`: updates ONLY `pay-tree`, never auto-expands dock!
    state = workbenchReducer(state, {
      type: 'event',
      event: {
        type: 'cPushProgress',
        progress: {
          protocolVersion: 1,
          session: makeCPushSession({ sessionId: frozenTarget.sessionId, frozenScopeVersion: frozenTarget.scopeVersion }),
        },
      },
    });
    expect(state.activeWorktreeId).toBe('fix-tree');
    const payWaiting = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(payWaiting.sessionStatus).toBe('needsDecision');
    expect(payWaiting.dockExpanded).toBe(false);
    expect(orderedMessages(payWaiting.messages)[0].title).toBe('发布说明要一起提交吗？');
  });

  it('运行期间手动输入的新草稿不会被 C-Push 完成回执清空，次级审查通知继续保留（C03 / R01）', () => {
    let state = openedWorkbench();
    const tree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    const frozenTarget = targetOf(tree);

    state = workbenchReducer(state, {
      type: 'cPushStarted',
      worktreeId: 'pay-tree',
      runId: 'run-1',
      frozenTarget,
      frozenFileIds: ['order', 'retry'],
      modelId: 'model-1',
      expandDock: false,
    });
    // Parallel review notification arrives:
    state = workbenchReducer(state, {
      type: 'event',
      event: {
        type: 'message',
        target: frozenTarget,
        message: { id: 'review-notice', kind: 'review', title: '审查完成：1 项建议', description: '已更新' },
      },
    });
    // User types new notes for a future commit while C-Push is running:
    state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '下一笔提交的人工备注' });

    const snapAfter = repositoryReply('after-cpush', 2).snapshot!;
    snapAfter.worktrees[0].files = snapAfter.worktrees[0].files.filter((f) => f.id !== 'retry');
    state = workbenchReducer(state, {
      type: 'cPushSettled',
      reply: {
        protocolVersion: 1,
        kind: 'cPush',
        requestId: 'run-1',
        repositorySnapshot: snapAfter,
        session: makeCPushSession({
          sessionId: frozenTarget.sessionId,
          frozenScopeVersion: frozenTarget.scopeVersion,
          sessionStatus: 'done',
          committedFileIds: ['order', 'retry'],
          commits: [{ oid: 'c1', shortOid: 'c1', title: 'feat(order): 拆分提交 1', branch: 'feat/pay', fileIds: ['order', 'retry'] }],
          message: { id: 'done-run-1', kind: 'done', title: '已完成 1 个本地拆分提交', description: '已完成' },
        }),
      },
    });

    const settled = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(settled.sessionStatus).toBe('done');
    expect(settled.draft.text).toBe('下一笔提交的人工备注');
    expect(settled.messages.map((m) => m.kind).sort()).toEqual(['done', 'review']);
    expect(sampleFindings()).toHaveLength(2);
  });

  it('冷启动恢复已持久化的 C-Push 中断会话时保持收起并对齐会话 ID', () => {
    const snap = repositoryReply('restore', 1).snapshot!;
    const interrupted = makeCPushSession({
      sessionStatus: 'needsDecision',
      canRetryRemaining: true,
      commits: [{ oid: 'oid-1', shortOid: 'oid-1', title: 'feat: step 1', branch: 'feat/pay', fileIds: ['order'] }],
      message: {
        id: 'interrupted-run-1',
        kind: 'error',
        title: '上次会话已完成 1 个提交后中断',
        description: '请确认是否继续剩余步骤',
        options: [{ id: 'retry-remaining', label: '确认继续剩余提交', recommended: true }],
      },
    });
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 1,
        editor: 'idea',
        activeWorktreeId: 'pay-tree',
        emptyDraft: '',
        emptyEditing: false,
        worktrees: [],
        snapshots: [snap],
        cPushSessions: [interrupted],
        warning: null,
      },
    });
    const pay = hydrated.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(pay.sessionId).toBe('sess-frozen');
    expect(pay.sessionStatus).toBe('needsDecision');
    expect(pay.dockExpanded).toBe(false);
    expect(pay.cPush?.canRetryRemaining).toBe(true);
    expect(pay.cPush?.commits).toHaveLength(1);
  });
});
