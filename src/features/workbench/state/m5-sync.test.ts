import { describe, expect, it } from 'vitest';
import type { CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import { repositoryReply } from '../testing/repository-fixture';
import { emptyWorkbench, orderedMessages, primaryAction, targetOf, workbenchReducer } from './workbench';

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

function makeM5Session(overrides: Partial<CPushSessionSnapshot> = {}): CPushSessionSnapshot {
  return {
    runId: 'push-run-1',
    repositoryId: 'orders',
    worktreeId: 'pay-tree',
    sessionId: 'sess-m5',
    frozenScopeVersion: 'scope-m5',
    modelId: 'model-1',
    frozenFileIds: ['order'],
    committedFileIds: ['order'],
    skippedFileIds: [],
    sessionStatus: 'pushFailed',
    message: {
      id: 'push-failed-1',
      kind: 'pushFailed',
      title: 'Committed locally, push failed',
      scope: 'origin/feat/pay',
      description: 'Local commit commit-1 is safely preserved on the current branch; Retry Push only pushes existing commits.',
      options: [{ id: 'retry-push', label: 'Retry Push', recommended: true }],
    },
    timeline: [
      { state: 'done', label: 'Local commit commit-1: feat(order): push commit' },
      { state: 'failed', label: 'Push to origin/feat/pay failed' },
    ],
    qaLog: [],
    commits: [{ oid: 'commit-oid-1', shortOid: 'commit-1', title: 'feat(order): push commit', branch: 'feat/pay', fileIds: ['order'] }],
    canRetryRemaining: false,
    ...overrides,
  };
}

describe('M5 remote sync, push-failure preservation, manual merge reconciliation, and recovery', () => {
  it('preserves user prompt during pushFailed retry state and clears prompt only after Retry Push succeeds (R02)', () => {
    let state = openedWorkbench();
    state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'split lockfile separately' });
    const tree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    const frozenTarget = targetOf(tree);

    state = workbenchReducer(state, {
      type: 'cPushStarted',
      worktreeId: 'pay-tree',
      runId: 'push-run-1',
      frozenTarget,
      frozenFileIds: ['order'],
      modelId: 'model-1',
      expandDock: true,
    });

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
        session: makeM5Session({ sessionId: frozenTarget.sessionId, frozenScopeVersion: frozenTarget.scopeVersion }),
      },
    });

    const failedTree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(failedTree.sessionStatus).toBe('pushFailed');
    expect(primaryAction(failedTree, true).label).toBe('Retry Push');
    expect(failedTree.dockExpanded).toBe(true);
    // Prompt MUST remain intact while pushFailed is retryable:
    expect(failedTree.prompt.text).toBe('split lockfile separately');
    expect(failedTree.cPush?.commits).toHaveLength(1);
    expect(orderedMessages(failedTree.messages)[0].title).toBe('Committed locally, push failed');

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
          sessionId: frozenTarget.sessionId,
          frozenScopeVersion: frozenTarget.scopeVersion,
          sessionStatus: 'done',
          message: { id: 'done-push-1', kind: 'done', title: '1 commit pushed to origin/feat/pay', description: 'Completed' },
        }),
      },
    });

    const doneTree = state.worktrees.find((w) => w.worktreeId === 'pay-tree')!;
    expect(doneTree.sessionStatus).toBe('done');
    expect(doneTree.prompt.text).toBe('');
    expect(doneTree.remote).toEqual({ kind: 'tracked', branch: 'origin/feat/pay', ahead: 0, behind: 0, localOnly: false });
  });

  it('prompts to continue push when manual merge resolved locally, or auto-collapses dock when pushed in terminal (R03 / R04)', () => {
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
            title: 'Waiting for manual merge in IDE',
            description: 'Refocusing Picket automatically reconciles conflict and remote state',
            files: [{ id: 'order', label: 'src/service/Order.ts', badge: 'Conflict' }],
          },
        }),
      },
    });
    expect(state.worktrees[0].dockExpanded).toBe(true);
    expect(primaryAction(state.worktrees[0], true).label).toBe('C-Push · Decide');

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
              title: 'Conflict resolved. Continue pushing?',
              description: 'Verified no conflict markers remain and rebase completed',
              options: [{ id: 'continue-push', label: 'Continue Push', recommended: true }],
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
              title: 'Detected merge and push completed in terminal',
              description: 'Remote and local HEAD match',
            },
          }),
        },
      },
    });
    expect(state.worktrees[0].sessionStatus).toBe('done');
    expect(state.worktrees[0].dockExpanded).toBe(false);
  });

  it('hydrates pushFailed/conflict session on cold start with collapsed dock without auto-rerunning writes (W04)', () => {
    const snap = repositoryReply('restore-m5', 1).snapshot!;
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        editor: 'vscode',
        activeWorktreeId: 'pay-tree',
        emptyPrompt: '',
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
