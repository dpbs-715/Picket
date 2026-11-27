import { describe, expect, it } from 'vitest';
import type { CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import { repositoryReply } from '../testing/repository-fixture';
import {
  emptyWorkbench,
  orderedMessages,
  targetOf,
  workbenchReducer,
} from './workbench';

function openedWorkbench() {
  let state = workbenchReducer(emptyWorkbench(), { type: 'repositoryReadStarted', key: 'open', requestId: 'open-1' });
  state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open-1') });
  return workbenchReducer(state, {
    type: 'catalogSynced',
    connections: [{ id: 'conn-1', name: 'Anthropic', provider: 'anthropic' }],
    models: [{ id: 'model-1', connectionId: 'conn-1', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }],
    activeModelId: 'model-1',
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
      title: 'Include release notes in this commit?',
      scope: 'Affects commit 2/2 "docs"',
      description: 'Confirm how to group RELEASE_NOTES.md:',
      options: [
        { id: 'merge', label: 'Merge into commit 1' },
        { id: 'split', label: 'Commit separately & push together', recommended: true },
        { id: 'skip', label: 'Skip for now' },
      ],
    },
    timeline: [
      { state: 'done', label: 'Froze scope for 4 selected files' },
      { state: 'active', label: 'Waiting for decision: Include release notes in this commit?' },
    ],
    qaLog: [],
    commits: [],
    canRetryRemaining: false,
    ...overrides,
  };
}

describe('M4 C-Push session state, scope freezing, and recovery isolation', () => {
  it('keeps frozen session target and fileIds immutable when selection or worktree changes mid-run (W03)', () => {
    let state = openedWorkbench();
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'lock' });
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
    expect(payAfterSelect.activeRun?.kind).toBe('cpush');
    if (payAfterSelect.activeRun?.kind === 'cpush') {
      expect(payAfterSelect.activeRun.frozenTarget).toEqual(frozenTarget);
      expect(payAfterSelect.activeRun.frozenFileIds).toEqual(['order', 'pay', 'retry', 'release']);
    }

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
    expect(orderedMessages(payWaiting.messages)[0].title).toBe('Include release notes in this commit?');
  });

  it('clears unchanged prompt after C-Push succeeds and preserves mid-run prompt edits', () => {
    // Case A: Prompt untouched during C-Push -> cleared when push succeeds (`sessionStatus: 'done'`):
    let stateA = openedWorkbench();
    stateA = workbenchReducer(stateA, { type: 'prompt', worktreeId: 'pay-tree', text: 'split lockfile separately' });
    const treeA = stateA.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    const frozenTargetA = targetOf(treeA);
    stateA = workbenchReducer(stateA, {
      type: 'cPushStarted',
      worktreeId: 'pay-tree',
      runId: 'run-a',
      frozenTarget: frozenTargetA,
      frozenFileIds: ['order', 'retry'],
      modelId: 'model-1',
      expandDock: false,
    });
    const snapDoneA = repositoryReply('done-a', 2).snapshot!;
    stateA = workbenchReducer(stateA, {
      type: 'cPushSettled',
      reply: {
        protocolVersion: 1,
        kind: 'cPush',
        requestId: 'run-a',
        repositorySnapshot: snapDoneA,
        session: makeCPushSession({
          runId: 'run-a',
          sessionId: frozenTargetA.sessionId,
          frozenScopeVersion: frozenTargetA.scopeVersion,
          sessionStatus: 'done',
          committedFileIds: ['order', 'retry'],
          commits: [{ oid: 'c1', shortOid: 'c1', title: 'feat(order): split commit 1', branch: 'feat/pay', fileIds: ['order', 'retry'] }],
          message: { id: 'done-run-a', kind: 'done', title: '1 commit pushed to origin/feat/pay', description: 'Done' },
        }),
      },
    });
    expect(stateA.worktrees.find((w) => w.worktreeId === 'pay-tree')!.prompt.text).toBe('');

    // Case B: User edits prompt mid-run -> preserved on completion!
    let state = openedWorkbench();
    state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'initial extra instruction' });
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
    state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'extra instruction for next C-Push' });

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
          commits: [{ oid: 'c1', shortOid: 'c1', title: 'feat(order): split commit 1', branch: 'feat/pay', fileIds: ['order', 'retry'] }],
          message: { id: 'done-run-1', kind: 'done', title: 'Completed 1 local split commit', description: 'Done' },
        }),
      },
    });

    const settled = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(settled.sessionStatus).toBe('done');
    expect(settled.prompt.text).toBe('extra instruction for next C-Push');
    expect(settled.messages.map((m) => m.kind)).toEqual(['done']);
  });

  it('hydrates interrupted C-Push session on cold start with collapsed dock and aligned sessionId', () => {
    const snap = repositoryReply('restore', 1).snapshot!;
    const interrupted = makeCPushSession({
      sessionStatus: 'needsDecision',
      canRetryRemaining: true,
      commits: [{ oid: 'oid-1', shortOid: 'oid-1', title: 'feat: step 1', branch: 'feat/pay', fileIds: ['order'] }],
      message: {
        id: 'interrupted-run-1',
        kind: 'error',
        title: 'Previous session interrupted after 1 commit',
        description: 'Confirm whether to continue remaining steps',
        options: [{ id: 'retry-remaining', label: 'Continue remaining commits', recommended: true }],
      },
    });
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        editor: 'idea',
        activeWorktreeId: 'pay-tree',
        emptyPrompt: '',
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
    expect(pay.activeRun?.kind).toBe('cpush');
    if (pay.activeRun?.kind === 'cpush') {
      expect(pay.activeRun.canRetryRemaining).toBe(true);
      expect(pay.activeRun.commits).toHaveLength(1);
    }
  });
});
