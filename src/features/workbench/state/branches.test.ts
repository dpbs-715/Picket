import { describe, expect, it } from 'vitest';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import { repositoryReply } from '../testing/repository-fixture';
import { emptyWorkbench, targetOf, workbenchReducer } from './workbench';
import type { WorkbenchState } from './workbench';

function initial() {
  let state = workbenchReducer(emptyWorkbench(), { type: 'repositoryReadStarted', key: 'open', requestId: 'open' });
  state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open') });
  state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'extra instruction preserved across branch switch' });
  return workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'pay' });
}
function started(state: WorkbenchState) { return workbenchReducer(state, { type: 'repositoryBranchStarted', repositoryId: 'orders', requestId: 'branch' }); }
const source = (state: WorkbenchState) => state.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;
function reply(state: WorkbenchState, jumpToOtherWorktree = false): GitBranchReply {
  const snapshot = repositoryReply('branch', 2).snapshot!;
  if (jumpToOtherWorktree) {
    snapshot.activeWorktreeId = 'fix-tree';
  } else {
    snapshot.worktrees[0].branch = 'feat/existing';
    snapshot.worktrees[0].branchToken = 'new-rust-token';
  }
  return { protocolVersion: 1, kind: 'branch', requestId: 'branch', target: targetOf(source(state)), action: 'switchBranch', outcome: 'completed', snapshot };
}
function finish(state: WorkbenchState, result = reply(state)) { return workbenchReducer(state, { type: 'repositoryBranchFinished', repositoryId: 'orders', reply: result }); }

describe('Branch switching merged with local prompts and view state', () => {
  it('applies Git snapshot on switch, preserves latest prompt and selection, and updates scopeVersion', () => {
    let state = initial();
    const previous = source(state);
    state = started(state);
    state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'newer prompt typed while branch switch is in flight' });
    state = finish(state);
    expect(source(state).branch).toBe('feat/existing');
    expect(source(state).branchToken).toBe('new-rust-token');
    expect(source(state).prompt.text).toBe('newer prompt typed while branch switch is in flight');
    expect(source(state).selectedIds).toEqual(previous.selectedIds);
    expect(source(state).scopeVersion).not.toBe(previous.scopeVersion);
    expect(state.repositoryBranches).toEqual({});
  });

  it('jumps to occupied worktree without migrating source worktree prompt or selection', () => {
    let state = started(initial()); const before = source(state);
    state = finish(state, reply(state, true));
    expect(state.activeWorktreeId).toBe('fix-tree');
    expect(source(state)).toMatchObject({ branch: before.branch, head: before.head, scopeVersion: before.scopeVersion, prompt: before.prompt, selectedIds: before.selectedIds, sessionId: before.sessionId });
  });

  it('updates only source repository when worktree is switched mid-flight without stealing active view', () => {
    let state = started(initial()); const result = reply(state);
    state = workbenchReducer(state, { type: 'switch', worktreeId: 'fix-tree' });
    state = finish(state, result);
    expect(state.activeWorktreeId).toBe('fix-tree'); expect(source(state).branch).toBe('feat/existing');
  });

  it('keeps latest selection changed while branch switch is in flight', () => {
    let state = started(initial()); const result = reply(state);
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'order' });
    state = finish(state, result); expect(source(state).selectedIds).not.toContain('order');
  });

  it('preserves original branch, prompt, and selection on failure', () => {
    let state = started(initial()); const before = source(state);
    state = workbenchReducer(state, { type: 'repositoryBranchFailed', repositoryId: 'orders', requestId: 'branch' });
    expect(source(state)).toEqual(before); expect(state.repositoryBranches).toEqual({});
  });

  it('rejects stale requestId and older readSequence snapshots', () => {
    let state = started(initial());
    expect(finish(state, { ...reply(state), requestId: 'wrong' })).toBe(state);
    const old = reply(state);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'new-read' });
    const newer = repositoryReply('new-read', 3); newer.snapshot!.worktrees[0].branch = 'feat/newer';
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: newer });
    state = finish(state, old); expect(source(state).branch).toBe('feat/newer');
  });

  it('honors newer openRepository navigation revision over earlier branch switch', () => {
    let state = started(initial()); const branchResult = reply(state, true);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'later-open' });
    state = finish(state, branchResult); expect(state.activeWorktreeId).toBe('pay-tree');
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('later-open', 3) });
    expect(state.activeWorktreeId).toBe('pay-tree');

    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'earlier-open' });
    state = started(state); const newerBranch = reply(state, true); newerBranch.snapshot!.readSequence = 5;
    state = finish(state, newerBranch); expect(state.activeWorktreeId).toBe('fix-tree');
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('earlier-open', 4) });
    expect(state.activeWorktreeId).toBe('fix-tree');
  });
});
