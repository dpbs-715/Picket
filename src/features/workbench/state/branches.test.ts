import { describe, expect, it } from 'vitest';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import { repositoryReply } from '../testing/repository-fixture';
import { emptyWorkbench, targetOf, workbenchReducer } from './workbench';
import type { WorkbenchState } from './workbench';

function initial() {
  let state = workbenchReducer(emptyWorkbench(), { type: 'repositoryReadStarted', key: 'open', requestId: 'open' });
  state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open') });
  state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '人工草稿，切分支后仍保留' });
  return workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'pay' });
}
function started(state: WorkbenchState) { return workbenchReducer(state, { type: 'repositoryBranchStarted', repositoryId: 'orders', requestId: 'branch' }); }
const source = (state: WorkbenchState) => state.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;
function reply(state: WorkbenchState, worktree = false): GitBranchReply {
  const snapshot = repositoryReply('branch', 2).snapshot!;
  if (worktree) {
    snapshot.worktrees.push({ ...snapshot.worktrees[0], worktreeId: 'new-tree', label: 'new', branch: 'feat/new', path: '/test-only/new', files: [] });
    snapshot.activeWorktreeId = 'new-tree';
  } else { snapshot.worktrees[0].branch = 'feat/new'; snapshot.worktrees[0].branchToken = 'new-rust-token'; }
  return { protocolVersion: 1, kind: 'branch', requestId: 'branch', target: targetOf(source(state)), action: worktree ? 'createWorktree' : 'createBranch', outcome: 'completed', snapshot };
}
function finish(state: WorkbenchState, result = reply(state)) { return workbenchReducer(state, { type: 'repositoryBranchFinished', repositoryId: 'orders', reply: result }); }

describe('分支操作与视图、本地输入的合并', () => {
  it('创建分支只采用 Git 快照，保留最新人工草稿和选择并令旧范围失效', () => {
    let state = initial(); const previous = source(state);
    state = started(state);
    state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '请求中继续输入的新草稿' });
    state = finish(state);
    expect(source(state).branch).toBe('feat/new'); expect(source(state).branchToken).toBe('new-rust-token');
    expect(source(state).draft).toMatchObject({ text: '请求中继续输入的新草稿', outdated: true });
    expect(source(state).selectedIds).toEqual(previous.selectedIds);
    expect(source(state).scopeVersion).not.toBe(previous.scopeVersion);
    expect(state.repositoryBranches).toEqual({});
  });
  it('创建 Worktree 后进入真实新工作树，不把原工作树草稿和选择迁移过去', () => {
    let state = started(initial()); const before = source(state);
    state = finish(state, reply(state, true));
    expect(state.activeWorktreeId).toBe('new-tree');
    expect(source(state)).toMatchObject({ branch: before.branch, head: before.head, scopeVersion: before.scopeVersion, draft: before.draft, selectedIds: before.selectedIds, sessionId: before.sessionId });
    expect(state.worktrees.find((tree) => tree.worktreeId === 'new-tree')).toMatchObject({ files: [], selectedIds: [], draft: { text: '', editing: false } });
  });
  it('执行中切换工作树，晚结果只更新原仓库，不切走当前视图', () => {
    let state = started(initial()); const result = reply(state, true);
    state = workbenchReducer(state, { type: 'switch', worktreeId: 'fix-tree' });
    state = finish(state, result);
    expect(state.activeWorktreeId).toBe('fix-tree'); expect(state.worktrees.some((tree) => tree.worktreeId === 'new-tree')).toBe(true);
  });
  it('请求中再次改变勾选，不把旧范围勾回来', () => {
    let state = started(initial()); const result = reply(state);
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'order' });
    state = finish(state, result); expect(source(state).selectedIds).not.toContain('order');
  });
  it('取消或失败保留原分支、草稿与选择，且不伪造新工作树', () => {
    let state = started(initial()); const before = source(state);
    state = finish(state, { ...reply(state, true), outcome: 'cancelled', snapshot: null });
    expect(source(state)).toEqual(before); expect(state.worktrees).toHaveLength(2);
    state = started(state);
    state = workbenchReducer(state, { type: 'repositoryBranchFailed', repositoryId: 'orders', requestId: 'branch' });
    expect(source(state)).toEqual(before); expect(state.repositoryBranches).toEqual({});
  });
  it('拒绝旧请求身份，不把较旧 Git 快照覆盖到新分支状态', () => {
    let state = started(initial());
    expect(finish(state, { ...reply(state), requestId: 'wrong' })).toBe(state);
    const old = reply(state);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'new-read' });
    const newer = repositoryReply('new-read', 3); newer.snapshot!.worktrees[0].branch = 'feat/newer';
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: newer });
    state = finish(state, old); expect(source(state).branch).toBe('feat/newer');
  });
  it('新打开仓库的意图优先于较早的分支请求，较早打开的结果也不能抢回视图', () => {
    let state = started(initial()); const branchResult = reply(state, true);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'later-open' });
    state = finish(state, branchResult); expect(state.activeWorktreeId).toBe('pay-tree');
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('later-open', 3) });
    expect(state.activeWorktreeId).toBe('pay-tree');

    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'earlier-open' });
    state = started(state); const newerBranch = reply(state, true); newerBranch.snapshot!.readSequence = 5;
    state = finish(state, newerBranch); expect(state.activeWorktreeId).toBe('new-tree');
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('earlier-open', 4) });
    expect(state.activeWorktreeId).toBe('new-tree');
  });
});
