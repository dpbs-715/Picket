import { describe, expect, it } from 'vitest';
import type { GitCommitReply } from '../../../../shared/git-commit-protocol';
import { sampleFindings } from '../testing/fixtures';
import { repositoryReply } from '../testing/repository-fixture';
import { emptyWorkbench, targetOf, workbenchReducer } from './workbench';
import type { WorkbenchState } from './workbench';

function initial() {
  let state = workbenchReducer(emptyWorkbench(), { type: 'repositoryReadStarted', key: 'open', requestId: 'open' });
  state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open') });
  state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: 'feat(order): 仅提交部分暂存与新增文件' });
  state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'pay' });
  return workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'release' });
}

const active = (state: WorkbenchState) => state.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;

function started(state: WorkbenchState, requestId = 'commit-1') {
  return workbenchReducer(state, {
    type: 'repositoryCommitStarted',
    repositoryId: 'orders',
    worktreeId: 'pay-tree',
    requestId,
    fileIds: ['order', 'retry'],
  });
}

function commitReply(state: WorkbenchState, requestId = 'commit-1', sequence = 2): GitCommitReply {
  const snapshot = repositoryReply(requestId, sequence).snapshot!;
  snapshot.worktrees[0].head = 'head-2';
  snapshot.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 3, behind: 1, localOnly: true };
  snapshot.worktrees[0].files = snapshot.worktrees[0].files
    .filter((file) => file.id !== 'retry')
    .map((file) => file.id === 'order'
      ? { ...file, gitStatus: '.M', range: { kind: 'unstaged', added: 14, removed: 4 }, contentVersion: 'content-order-unstaged', commitToken: 'commit-order-unstaged' }
      : file);
  return {
    protocolVersion: 1,
    kind: 'commit',
    requestId,
    target: targetOf(active(state)),
    snapshot,
    commit: { oid: 'head-2', shortOid: 'head-2', title: 'feat(order): 仅提交部分暂存与新增文件', branch: 'feat/pay', parent: 'head-1' },
  };
}

describe('受控本地提交与工作树状态合并', () => {
  it('成功后清空已消费草稿，部分暂存剩余草稿转为未选未暂存，不伪造已推送会话状态', () => {
    let state = started(initial());
    state = workbenchReducer(state, { type: 'repositoryCommitFinished', repositoryId: 'orders', reply: commitReply(state) });
    const tree = active(state);
    expect(tree.head).toBe('head-2');
    expect(tree.draft).toMatchObject({ text: '', editing: false, outdated: false });
    expect(tree.selectedIds).toEqual([]);
    expect(tree.files.find((file) => file.id === 'order')?.range).toEqual({ kind: 'unstaged', added: 14, removed: 4 });
    expect(tree.files.some((file) => file.id === 'retry')).toBe(false);
    expect(tree.sessionStatus).toBe('idle');
    expect(state.repositoryCommits).toEqual({});
  });

  it('提交等待期间继续输入的新草稿或新勾选不会被成功回执抹掉', () => {
    let state = started(initial());
    state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '下一笔提交的新说明' });
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'pay' });
    state = workbenchReducer(state, { type: 'repositoryCommitFinished', repositoryId: 'orders', reply: commitReply(state) });
    expect(active(state).draft).toMatchObject({ text: '下一笔提交的新说明', editing: true, outdated: true });
    expect(active(state).selectedIds).toEqual(['pay']);
  });

  it('提交期间切换工作树，晚到的成功回执只更新原工作树而不抢回视图', () => {
    let state = initial();
    state = workbenchReducer(state, { type: 'draft', worktreeId: 'fix-tree', text: '另一工作树草稿' });
    state = started(state);
    const reply = commitReply(state);
    state = workbenchReducer(state, { type: 'switch', worktreeId: 'fix-tree' });
    state = workbenchReducer(state, { type: 'repositoryCommitFinished', repositoryId: 'orders', reply });
    expect(state.activeWorktreeId).toBe('fix-tree');
    expect(state.worktrees.find((tree) => tree.worktreeId === 'fix-tree')?.draft.text).toBe('另一工作树草稿');
    expect(active(state).head).toBe('head-2');
  });

  it('已提交文件的旧审查发现被清理，仍留在工作区的未提交审查结果保留', () => {
    let state = initial();
    state = workbenchReducer(state, { type: 'request', target: targetOf(active(state)), kind: 'review', requestId: 'rev-1', fileIds: ['order', 'pay'] });
    state = workbenchReducer(state, { type: 'event', event: { type: 'reviewed', target: targetOf(active(state)), requestId: 'rev-1', findings: sampleFindings(), skippedPaths: [] } });
    state = started(state);
    state = workbenchReducer(state, { type: 'repositoryCommitFinished', repositoryId: 'orders', reply: commitReply(state) });
    expect(active(state).review.findings.map((finding) => finding.fileId)).toEqual(['pay']);
    expect(active(state).review.scopeFileIds).toEqual(['pay']);
  });

  it('失败或错误请求身份不抹掉草稿与选择，也不会应用较旧快照', () => {
    let state = started(initial());
    const before = active(state);
    expect(workbenchReducer(state, { type: 'repositoryCommitFinished', repositoryId: 'orders', reply: { ...commitReply(state), requestId: 'wrong' } })).toBe(state);
    state = workbenchReducer(state, { type: 'repositoryCommitFailed', repositoryId: 'orders', requestId: 'commit-1' });
    expect(active(state)).toEqual(before);
    expect(state.repositoryCommits).toEqual({});
  });
});
