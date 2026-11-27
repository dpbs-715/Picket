import { describe, expect, it } from 'vitest';
import { repositoryReply } from '../testing/repository-fixture';
import { emptyWorkbench, workbenchReducer } from './workbench';
import type { WorkbenchState } from './workbench';

function open(state = emptyWorkbench(), requestId = 'open-1', sequence = 1) {
  state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId });
  return workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply(requestId, sequence) });
}
function refresh(state: WorkbenchState, change: (reply: ReturnType<typeof repositoryReply>) => void, requestId = 'refresh', sequence = 2) {
  state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId });
  const reply = repositoryReply(requestId, sequence); change(reply);
  return workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply });
}
const active = (state: WorkbenchState) => state.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;

describe('Real repository snapshot merged with local prompt state', () => {
  it('displays Git snapshot on open, seeds repositorySettings, and selects all normal changes including lockfiles by default', () => {
    const state = open();
    expect(state.activeWorktreeId).toBe('pay-tree'); expect(state.repositories[0].name).toBe('real-contract-fixture');
    expect(state.repositorySettings.orders).toEqual({ editor: 'idea', commitLanguage: 'en' });
    expect(active(state).selectedIds).toEqual(['order', 'pay', 'retry', 'release', 'lock']);
    expect(active(state).unselectedIds).toEqual([]);
    expect(active(state).files[0].range).toEqual({ kind: 'partial', added: 28, removed: 4 });
    expect(active(state).prompt.text).toBe(''); expect(active(state).messages).toEqual([]);
  });

  it('migrates prompt typed before opening the first repository', () => {
    const state = open({ ...emptyWorkbench(), emptyPrompt: 'pre-open extra instruction' });
    expect(active(state).prompt.text).toBe('pre-open extra instruction');
  });

  it('preserves existing prompt, repository, and selection when open is cancelled or fails', () => {
    let state = open(); state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'extra instruction' });
    const before = active(state);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'cancel' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: { protocolVersion: 1, requestId: 'cancel', snapshot: null } });
    expect(active(state)).toEqual(before);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'fail' });
    state = workbenchReducer(state, { type: 'repositoryReadFailed', key: 'open', requestId: 'fail' });
    expect(active(state)).toEqual(before);
  });

  it('keeps manually unchecked files unselected without invalidating scope, while auto-selecting newly appearing normal files', () => {
    let state = open();
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'lock' });
    expect(active(state).selectedIds).toEqual(['order', 'pay', 'retry', 'release']);
    expect(active(state).unselectedIds).toEqual(['lock']);

    state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'keep instruction' });
    const scope = active(state).scopeVersion;

    // Manually unchecked lockfile changing on disk does NOT invalidate scope:
    state = refresh(state, (reply) => {
      reply.snapshot!.worktrees[0].files[4].contentVersion = 'changed-lock';
    });
    expect(active(state).prompt.text).toBe('keep instruction');
    expect(active(state).scopeVersion).toBe(scope);
    expect(active(state).selectedIds).not.toContain('lock');

    // Newly appearing normal file IS automatically selected and updates scopeVersion:
    state = refresh(state, (reply) => {
      reply.snapshot!.worktrees[0].files[4].contentVersion = 'changed-lock';
      reply.snapshot!.worktrees[0].files.push({ ...reply.snapshot!.worktrees[0].files[1], id: 'new-file', path: 'new.ts' });
    }, 'refresh-2', 3);
    expect(active(state).selectedIds).toContain('new-file');
    expect(active(state).selectedIds).not.toContain('lock');
    expect(active(state).scopeVersion).not.toBe(scope);
  });

  it('invalidates scope on selected content change while preserving prompt text and selection', () => {
    let state = open();
    state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'manual instruction' });
    const oldScope = active(state).scopeVersion;
    state = refresh(state, (reply) => { reply.snapshot!.worktrees[0].files[0].contentVersion = 'changed-staged-content'; });
    expect(active(state).prompt.text).toBe('manual instruction');
    expect(active(state).scopeVersion).not.toBe(oldScope);
    expect(active(state).selectedIds).toHaveLength(5);
  });

  it('honors latest selection changed while refresh is in flight', () => {
    let state = open(); state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'refresh' });
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'order' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: repositoryReply('refresh', 2) });
    expect(active(state).selectedIds).not.toContain('order');
  });

  it('keeps active view when switching worktrees while open or refresh is in flight', () => {
    let state = open(); state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'open-late' });
    state = workbenchReducer(state, { type: 'switch', worktreeId: 'fix-tree' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open-late', 2) });
    expect(state.activeWorktreeId).toBe('fix-tree');
    state = refresh(state, () => {}, 'refresh', 3); expect(state.activeWorktreeId).toBe('fix-tree');
  });

  it('rejects superseded requestId and older readSequence', () => {
    let state = open();
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'old' });
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'new' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: repositoryReply('old', 99) });
    expect(state.repositories[0].readSequence).toBe(1);
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: repositoryReply('new', 3) });
    state = refresh(state, (reply) => { reply.snapshot!.worktrees[0].branch = 'obsolete'; }, 'late', 2);
    expect(active(state).branch).toBe('feat/pay'); expect(state.repositories[0].readSequence).toBe(3);
  });

  it('preserves prompt on refresh failure and clears readError on subsequent successful retry', () => {
    let state = open(); state = workbenchReducer(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'original instruction' });
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'failed' });
    state = workbenchReducer(state, { type: 'repositoryReadFailed', key: 'orders', requestId: 'failed', worktreeId: 'pay-tree', error: 'Working directory unreadable' });
    expect(active(state).readError).toBe('Working directory unreadable'); expect(active(state).prompt.text).toBe('original instruction');
    state = refresh(state, () => {}); expect(active(state).readError).toBeNull(); expect(active(state).prompt.text).toBe('original instruction');
  });

  it('marks removed worktrees with readError instead of wiping prompt or masquerading as clean', () => {
    let state = open(); state = workbenchReducer(state, { type: 'prompt', worktreeId: 'fix-tree', text: 'other worktree prompt' });
    state = refresh(state, (reply) => { reply.snapshot!.worktrees = reply.snapshot!.worktrees.slice(0, 1); });
    const missing = state.worktrees.find((tree) => tree.worktreeId === 'fix-tree')!;
    expect(missing.readError).toContain('no longer in Git'); expect(missing.prompt.text).toBe('other worktree prompt');
  });

  it('invalidates scope when HEAD or branch changes without fabricating push completion', () => {
    let state = open();
    const scope = active(state).scopeVersion;
    state = refresh(state, (reply) => { reply.snapshot!.worktrees[0].branch = 'another'; reply.snapshot!.worktrees[0].head = 'head-2'; });
    expect(active(state).scopeVersion).not.toBe(scope);
    expect(active(state).messages).toEqual([]);
    expect(active(state).sessionStatus).toBe('idle');
  });
});
