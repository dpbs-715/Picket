import { describe, expect, it } from 'vitest';
import type { CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import { repositoryReply } from '../testing/repository-fixture';
import { emptyWorkbench, orderedMessages, primaryAction, targetOf, workbenchReducer } from './workbench';

function openedWorkbench() {
  let state = workbenchReducer(emptyWorkbench(), { type: 'repositoryReadStarted', key: 'open', requestId: 'open-1' });
  state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open-1') });
  return state;
}

function makeM5Session(overrides: Partial<CPushSessionSnapshot> = {}): CPushSessionSnapshot {
  return {
    runId: 'push-run-1',
    repositoryId: 'orders',
    worktreeId: 'pay-tree',
    sessionId: 'sess-m5',
    frozenScopeVersion: 'scope-m5',
    modelId: '',
    frozenFileIds: ['order'],
    committedFileIds: ['order'],
    skippedFileIds: [],
    sessionStatus: 'pushFailed',
    message: {
      id: 'push-failed-1',
      kind: 'pushFailed',
      title: '已在本地提交，推送失败',
      scope: 'origin/feat/pay',
      description: '本地提交 commit-1 已安全保留在当前分支；重连后点击「重试推送」仅推送已有提交。',
      options: [{ id: 'retry-push', label: '重试推送', recommended: true }],
    },
    timeline: [
      { state: 'done', label: '本地提交 commit-1：feat(order): manual push' },
      { state: 'failed', label: '推送至 origin/feat/pay 失败' },
    ],
    qaLog: [],
    commits: [{ oid: 'commit-oid-1', shortOid: 'commit-1', title: 'feat(order): manual push', branch: 'feat/pay', fileIds: ['order'] }],
    canRetryRemaining: false,
    ...overrides,
  };
}

describe('M5 远端同步、推送失败保留、手动合并对账与恢复', () => {
  it('手动提交并推送失败时记录本地提交并进入 pushFailed，重试推送不重复提交（R02 / A01）', () => {
    let state = openedWorkbench();
    state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: 'feat(order): manual push' });
    state = workbenchReducer(state, { type: 'repositoryCommitStarted', repositoryId: 'orders', worktreeId: 'pay-tree', requestId: 'push-run-1', fileIds: ['order'] });

    const snapAfterCommit = repositoryReply('after-local-commit', 2).snapshot!;
    snapAfterCommit.worktrees[0].files = snapAfterCommit.worktrees[0].files.filter((f) => f.id !== 'order');
    snapAfterCommit.worktrees[0].remote = { kind: 'tracked', branch: 'origin/feat/pay', ahead: 1, behind: 0, localOnly: true };

    state = workbenchReducer(state, {
      type: 'cPushSettled',
      expandDock: true,
      reply: {
        protocolVersion: 1,
        kind: 'cPush',
        requestId: 'push-run-1',
        repositorySnapshot: snapAfterCommit,
        session: makeM5Session(),
      },
    });

    const failedTree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(failedTree.sessionStatus).toBe('pushFailed');
    expect(primaryAction(failedTree, false).label).toBe('重试推送');
    expect(failedTree.dockExpanded).toBe(true);
    expect(failedTree.draft.text).toBe('');
    expect(failedTree.cPush?.commits).toHaveLength(1);
    expect(orderedMessages(failedTree.messages)[0].title).toBe('已在本地提交，推送失败');

    const snapAfterPush = repositoryReply('after-push', 3).snapshot!;
    snapAfterPush.worktrees[0].files = snapAfterCommit.worktrees[0].files;
    snapAfterPush.worktrees[0].remote = { kind: 'tracked', branch: 'origin/feat/pay', ahead: 0, behind: 0, localOnly: false };

    state = workbenchReducer(state, {
      type: 'cPushSettled',
      reply: {
        protocolVersion: 1,
        kind: 'cPush',
        requestId: 'retry-push-1',
        repositorySnapshot: snapAfterPush,
        session: makeM5Session({
          sessionStatus: 'done',
          message: { id: 'done-push-1', kind: 'done', title: '1 个提交已推送至 origin/feat/pay', description: '已完成' },
        }),
      },
    });

    const doneTree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(doneTree.sessionStatus).toBe('done');
    expect(doneTree.remote).toEqual({ kind: 'tracked', branch: 'origin/feat/pay', ahead: 0, behind: 0, localOnly: false });
  });

  it('冲突转手动合并后，对账检测到未推送则询问继续推送，检测到终端已推送则自动收起消息面板（R03 / R04）', () => {
    let state = openedWorkbench();
    const tree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    const target = targetOf(tree);

    state = workbenchReducer(state, {
      type: 'cPushSettled',
      expandDock: true,
      reply: {
        protocolVersion: 1,
        kind: 'cPush',
        requestId: 'conflict-1',
        session: makeM5Session({
          sessionId: target.sessionId,
          frozenScopeVersion: target.scopeVersion,
          sessionStatus: 'waitingManual',
          message: {
            id: 'manual-wait-1',
            kind: 'waitingManual',
            title: '等待你在 IDE 完成手动合并',
            description: '切回窗口将自动核对冲突与远端状态',
            files: [{ id: 'order', label: 'src/service/Order.ts', badge: '冲突' }],
          },
        }),
      },
    });
    expect(state.worktrees[0].dockExpanded).toBe(true);
    expect(primaryAction(state.worktrees[0], false).label).toBe('C-Push · 待决定');

    // Case A: Reconcile detects conflict resolved locally, NOT pushed yet -> keeps dock open to ask:
    state = workbenchReducer(state, {
      type: 'event',
      event: {
        type: 'cPushProgress',
        progress: {
          protocolVersion: 1,
          session: makeM5Session({
            sessionId: target.sessionId,
            frozenScopeVersion: target.scopeVersion,
            sessionStatus: 'needsDecision',
            message: {
              id: 'resolved-ask-1',
              kind: 'resolved',
              title: '冲突已解决，是否继续推送？',
              description: '已核对工作区无冲突标记且变基已完成',
              options: [{ id: 'continue-push', label: '继续推送', recommended: true }],
            },
          }),
        },
      },
    });
    expect(state.worktrees[0].sessionStatus).toBe('needsDecision');
    expect(state.worktrees[0].dockExpanded).toBe(true);

    // Case B: User already pushed from terminal -> reconcile sends `autoCollapse: true` and collapses dock!
    state = workbenchReducer(state, {
      type: 'event',
      event: {
        type: 'cPushProgress',
        progress: {
          protocolVersion: 1,
          session: makeM5Session({
            sessionId: target.sessionId,
            frozenScopeVersion: target.scopeVersion,
            sessionStatus: 'done',
            autoCollapse: true,
            message: {
              id: 'terminal-pushed-1',
              kind: 'done',
              title: '检测到终端已完成合并并推送',
              description: '远端与本地 HEAD 已一致',
            },
          }),
        },
      },
    });
    expect(state.worktrees[0].sessionStatus).toBe('done');
    expect(state.worktrees[0].dockExpanded).toBe(false);
  });

  it('冷启动恢复 pushFailed / conflict 会话时保持面板收起且不自动重跑写入（W04）', () => {
    const snap = repositoryReply('restore-m5', 1).snapshot!;
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 1,
        editor: 'vscode',
        activeWorktreeId: 'pay-tree',
        emptyDraft: '',
        emptyEditing: false,
        worktrees: [],
        snapshots: [snap],
        cPushSessions: [makeM5Session({ sessionStatus: 'conflict' })],
        warning: null,
      },
    });
    const pay = hydrated.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(pay.sessionStatus).toBe('conflict');
    expect(pay.dockExpanded).toBe(false);
    expect(pay.cPush?.commits).toHaveLength(1);
  });
});
