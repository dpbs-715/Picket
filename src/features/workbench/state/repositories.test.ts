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

describe('真实仓库快照与本地输入合并', () => {
  it('打开后展示 Git 数据，默认排除锁文件但允许后续主动选择', () => {
    const state = open();
    expect(state.activeWorktreeId).toBe('pay-tree'); expect(state.repositories[0].name).toBe('real-contract-fixture');
    expect(active(state).selectedIds).toEqual(['order', 'pay', 'retry', 'release']);
    expect(active(state).files[0].range).toEqual({ kind: 'partial', added: 28, removed: 4 });
    expect(active(state).draft.text).toBe(''); expect(active(state).messages).toEqual([]);
  });
  it('首次打开保留无仓库时已经输入的说明', () => {
    const state = open({ ...emptyWorkbench(), emptyDraft: '先写的说明', emptyEditing: true });
    expect(active(state).draft.text).toBe('先写的说明');
  });
  it('取消或打开失败不丢已有草稿、仓库或选择', () => {
    let state = open(); state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '草稿' });
    const before = active(state);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'cancel' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: { protocolVersion: 1, requestId: 'cancel', snapshot: null } });
    expect(active(state)).toEqual(before);
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'fail' });
    state = workbenchReducer(state, { type: 'repositoryReadFailed', key: 'open', requestId: 'fail' });
    expect(active(state)).toEqual(before);
  });
  it('只刷新未选文件不令当前范围或草稿失效，新文件不自动扩大选择', () => {
    let state = open(); state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '保持说明' });
    const scope = active(state).scopeVersion;
    state = refresh(state, (reply) => {
      reply.snapshot!.worktrees[0].files[4].contentVersion = 'changed-lock';
      reply.snapshot!.worktrees[0].files.push({ ...reply.snapshot!.worktrees[0].files[1], id: 'new-file', path: 'new.ts' });
    });
    expect(active(state).draft).toMatchObject({ text: '保持说明', outdated: false });
    expect(active(state).scopeVersion).toBe(scope); expect(active(state).selectedIds).not.toContain('new-file');
  });
  it('相关内容变化令范围失效，但保留文字、选区所属输入状态与选择', () => {
    let state = open(); state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '人工说明' });
    const oldScope = active(state).scopeVersion;
    state = refresh(state, (reply) => { reply.snapshot!.worktrees[0].files[0].contentVersion = 'changed-staged-content'; });
    expect(active(state).draft).toMatchObject({ text: '人工说明', editing: true, outdated: true });
    expect(active(state).scopeVersion).not.toBe(oldScope); expect(active(state).selectedIds).toHaveLength(4);
  });
  it('读取中修改选择以最新选择为准，不用旧快照勾回文件', () => {
    let state = open(); state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'refresh' });
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'order' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: repositoryReply('refresh', 2) });
    expect(active(state).selectedIds).not.toContain('order');
  });
  it('打开或刷新中切换视图，晚结果不会切回原工作树', () => {
    let state = open(); state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'open-late' });
    state = workbenchReducer(state, { type: 'switch', worktreeId: 'fix-tree' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: repositoryReply('open-late', 2) });
    expect(state.activeWorktreeId).toBe('fix-tree');
    state = refresh(state, () => {}, 'refresh', 3); expect(state.activeWorktreeId).toBe('fix-tree');
  });
  it('拒绝被替代的 requestId 与更旧的原生读取序号', () => {
    let state = open();
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'old' });
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'new' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: repositoryReply('old', 99) });
    expect(state.repositories[0].readSequence).toBe(1);
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'orders', reply: repositoryReply('new', 3) });
    state = refresh(state, (reply) => { reply.snapshot!.worktrees[0].branch = 'obsolete'; }, 'late', 2);
    expect(active(state).branch).toBe('feat/pay'); expect(state.repositories[0].readSequence).toBe(3);
  });
  it('刷新失败保留输入，成功重试会清除状态不可用标记', () => {
    let state = open(); state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '原草稿' });
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'orders', requestId: 'failed' });
    state = workbenchReducer(state, { type: 'repositoryReadFailed', key: 'orders', requestId: 'failed', worktreeId: 'pay-tree', error: '工作目录不可读' });
    expect(active(state).readError).toBe('工作目录不可读'); expect(active(state).draft.text).toBe('原草稿');
    state = refresh(state, () => {}); expect(active(state).readError).toBeNull(); expect(active(state).draft.text).toBe('原草稿');
  });
  it('缺失工作树不抹掉草稿或冒充干净工作区', () => {
    let state = open(); state = workbenchReducer(state, { type: 'draft', worktreeId: 'fix-tree', text: '其他工作树草稿' });
    state = refresh(state, (reply) => { reply.snapshot!.worktrees = reply.snapshot!.worktrees.slice(0, 1); });
    const missing = state.worktrees.find((tree) => tree.worktreeId === 'fix-tree')!;
    expect(missing.readError).toContain('不在 Git'); expect(missing.draft.text).toBe('其他工作树草稿');
  });
  it('HEAD / 分支变化使旧范围失效且不伪造推送成功', () => {
    let state = open(); const scope = active(state).scopeVersion;
    state = workbenchReducer(state, { type: 'draft', worktreeId: 'pay-tree', text: '人工草稿' });
    state = refresh(state, (reply) => { reply.snapshot!.worktrees[0].branch = 'another'; reply.snapshot!.worktrees[0].head = 'head-2'; });
    expect(active(state).scopeVersion).not.toBe(scope); expect(active(state).draft.outdated).toBe(true);
    expect(active(state).messages).toEqual([]); expect(active(state).sessionStatus).toBe('idle');
  });
});
