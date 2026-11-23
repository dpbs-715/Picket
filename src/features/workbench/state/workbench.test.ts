import { describe, expect, it } from 'vitest';
import { fixture, releaseQuestion, sampleFindings } from '../testing/fixtures';
import { emptyWorkbench, groupFiles, orderedMessages, primaryAction, scopeSummary, selectedFiles, targetOf, workbenchReducer } from './workbench';
import type { WorkbenchAction, WorkbenchState } from './workbench';

function apply(state: WorkbenchState, action: WorkbenchAction) { return workbenchReducer(state, action); }

describe('工作台选择与上下文', () => {
  it('真实默认状态没有示例仓库、模型、会话或凭据', () => {
    expect(emptyWorkbench()).toMatchObject({ repositories: [], worktrees: [], models: [], activeWorktreeId: null, activeModelId: null });
    expect(primaryAction(undefined, false)).toEqual({ label: '提交', resume: false, disabled: true });
  });
  it('勾选只改变选择，不改文件元数据；锁文件仍可选择', () => {
    const state = fixture();
    const beforeFiles = structuredClone(state.worktrees[0].files);
    const next = apply(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'lock' });
    expect(next.worktrees[0].files).toEqual(beforeFiles);
    expect(state.worktrees[0].selectedIds).not.toContain('lock');
    expect(next.worktrees[0].selectedIds).toContain('lock');
    expect(next.worktrees[1]).toEqual(state.worktrees[1]);
  });
  it('部分暂存的列表与确认范围只使用 +28 -4，不混入未暂存统计', () => {
    const tree = fixture().worktrees[0];
    expect(scopeSummary(tree)).toBe('已选 4/5 项 · 含部分暂存');
    expect(selectedFiles(tree)[0].range).toEqual({ kind: 'partial', added: 28, removed: 4 });
    expect(scopeSummary({ ...tree, selectedIds: [] })).toBe('未选择变更');
    expect(groupFiles(tree.files).find((group) => group.directory === '(根目录)')?.files.map((file) => file.id)).toEqual(['release', 'lock']);
  });
  it('工作树切换保留各自草稿、选择、Tab 和消息回复', () => {
    let state = fixture();
    state = apply(state, { type: 'draft', worktreeId: 'pay-tree', text: '自己的说明' });
    state = apply(state, { type: 'reply', worktreeId: 'pay-tree', messageId: 'question', text: '文档另提交' });
    state = apply(state, { type: 'tab', worktreeId: 'pay-tree', tab: 'review' });
    state = apply(state, { type: 'switch', worktreeId: 'fix-tree' });
    expect(state.worktrees[1].draft.text).toBe('fix(refund): 校验订单号非空');
    state = apply(state, { type: 'switch', worktreeId: 'pay-tree' });
    expect(state.worktrees[0]).toMatchObject({ draft: { text: '自己的说明' }, tab: 'review', replies: { question: '文档另提交' } });
    expect(state.worktrees[0].selectedIds).toHaveLength(4);
  });
  it('零模型、空范围与可恢复会话的主按钮正确', () => {
    expect(primaryAction(fixture('no-model').worktrees[0], false).label).toBe('提交');
    expect(primaryAction(fixture('unselected').worktrees[0], true).disabled).toBe(true);
    const failed = fixture('push-failed').worktrees[0];
    expect(primaryAction({ ...failed, selectedIds: [] }, false)).toEqual({ label: '重试推送', resume: true, disabled: false });
    expect(primaryAction(fixture('conflict').worktrees[0], true).label).toBe('处理冲突');
  });
  it('删除当前模型后回退剩余模型，删到零时无空下拉', () => {
    const state = fixture();
    const remaining = apply(state, { type: 'modelDeleted', modelId: 'test-claude' });
    expect(remaining.activeModelId).toBe('test-gpt');
    expect(apply(remaining, { type: 'modelDeleted', modelId: 'test-gpt' }).activeModelId).toBeNull();
  });
});

describe('后台事件与草稿保全', () => {
  it('新问题不展开、不换 Tab、不改写输入；审查完成不能替换问题', () => {
    const state = fixture('typing');
    const tree = state.worktrees[0];
    let next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(tree), message: releaseQuestion(), sessionStatus: 'needsDecision' } });
    next = apply(next, { type: 'event', event: { type: 'message', target: targetOf(tree), message: { id: 'review', kind: 'review', title: '审查完成', description: '1 项高危' } } });
    expect(next.worktrees[0].draft).toEqual(tree.draft);
    expect(next.worktrees[0].tab).toBe('commit');
    expect(next.worktrees[0].dockExpanded).toBe(false);
    expect(orderedMessages(next.worktrees[0].messages)[0].kind).toBe('question');
    expect(orderedMessages(next.worktrees[0].messages)).toHaveLength(2);
  });
  it.each(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const)('拒绝 %s 不匹配的晚响应', (field) => {
    const state = fixture();
    const target = { ...targetOf(state.worktrees[0]), [field]: 'obsolete' };
    const next = apply(state, { type: 'event', event: { type: 'message', target, message: releaseQuestion() } });
    expect(next).toEqual(state);
  });
  it('另一工作树的有效消息只更新原目标', () => {
    const state = fixture();
    const next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(state.worktrees[1]), message: releaseQuestion() } });
    expect(next.worktrees[0]).toEqual(state.worktrees[0]);
    expect(next.worktrees[1].messages).toHaveLength(1);
  });
  it('人工改写后，生成的晚结果不覆盖文字，也不保留卡住的忙碌状态', () => {
    let state = fixture();
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'generate', requestId: 'generate-1' });
    state = apply(state, { type: 'draft', worktreeId: 'pay-tree', text: '正在写的新内容' });
    state = apply(state, { type: 'event', event: { type: 'generated', target, requestId: 'generate-1', text: '旧生成内容' } });
    expect(state.worktrees[0].draft.text).toBe('正在写的新内容');
    expect(state.worktrees[0].requests.generate).toBeUndefined();
  });
  it('新请求、清空和改变选择均令旧生成结果失效', () => {
    let state = fixture();
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'generate', requestId: 'old' });
    state = apply(state, { type: 'request', target, kind: 'generate', requestId: 'new' });
    state = apply(state, { type: 'event', event: { type: 'generated', target, requestId: 'old', text: '不可覆盖' } });
    expect(state.worktrees[0].requests.generate?.id).toBe('new');
    state = apply(state, { type: 'clear', worktreeId: 'pay-tree' });
    state = apply(state, { type: 'event', event: { type: 'generated', target, requestId: 'new', text: '不可回填' } });
    expect(state.worktrees[0].draft.text).toBe('');
    state = apply(state, { type: 'request', target, kind: 'generate', requestId: 'third' });
    state = apply(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'order' });
    state = apply(state, { type: 'event', event: { type: 'generated', target, requestId: 'third', text: '旧选择' } });
    expect(state.worktrees[0].draft.text).toBe('');
  });
  it('相关文件变化保留草稿并标过期，无关未选变化不标过期', () => {
    const state = fixture('changed');
    state.worktrees[0].draft.outdated = false;
    const tree = state.worktrees[0];
    const unrelated = apply(state, { type: 'event', event: { type: 'files', target: targetOf(tree), nextScopeVersion: tree.scopeVersion, files: tree.files, changedPaths: ['pnpm-lock.yaml'] } });
    expect(unrelated.worktrees[0].draft.outdated).toBe(false);
    const related = apply(state, { type: 'event', event: { type: 'files', target: targetOf(tree), nextScopeVersion: 'scope-2', files: tree.files, changedPaths: ['src/api/pay.ts'] } });
    expect(related.worktrees[0].draft).toMatchObject({ text: tree.draft.text, outdated: true });
    expect(related.worktrees[0].review.changedPaths).toContain('src/api/pay.ts');
  });
  it('复审只替换受影响文件，保留其他仍有效意见', () => {
    let state = fixture('changed');
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'review', requestId: 'review-order', fileIds: ['order'] });
    state = apply(state, { type: 'event', event: { type: 'reviewed', target, requestId: 'review-order', findings: [], skippedPaths: [] } });
    expect(state.worktrees[0].review.findings).toEqual([sampleFindings()[0]]);
    expect(state.worktrees[0].review.changedPaths).toEqual([]);
    expect(state.worktrees[0].review.skippedPaths).toEqual(['pnpm-lock.yaml']);
  });
  it('重新审查失败保留先前的零发现结果，不退回未审查', () => {
    let state = fixture();
    state.worktrees[0].review = { status: 'done', findings: [], skippedPaths: ['pnpm-lock.yaml'], changedPaths: [], scopeFileIds: ['order', 'pay'] };
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'review', requestId: 'failed-review' });
    state = apply(state, { type: 'requestFailed', target, kind: 'review', requestId: 'failed-review' });
    expect(state.worktrees[0].review.status).toBe('done');
    expect(state.worktrees[0].review.skippedPaths).toEqual(['pnpm-lock.yaml']);
  });
  it('桌面层确认终端已推送后，收起并移除旧提问，保留次级审查通知', () => {
    const state = fixture('review-during-question');
    state.worktrees[0].dockExpanded = true;
    const next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(state.worktrees[0]), sessionStatus: 'done',
      message: { id: 'terminal-pushed', kind: 'done', title: '远端已核对', description: '原提问已结束' } } });
    expect(next.worktrees[0].messages.map((message) => message.kind)).toEqual(['review', 'done']);
    expect(orderedMessages(next.worktrees[0].messages)[0].kind).toBe('done');
    expect(next.worktrees[0].dockExpanded).toBe(false);
    expect(next.worktrees[0].sessionStatus).toBe('done');
  });
  it('重复消息按身份替换，不重复堆积', () => {
    let state = fixture('question');
    const event = { type: 'message' as const, target: targetOf(state.worktrees[0]), message: releaseQuestion() };
    state = apply(apply(state, { type: 'event', event }), { type: 'event', event });
    expect(state.worktrees[0].messages).toHaveLength(1);
  });
});
