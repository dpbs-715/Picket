import { emptyWorkbench } from '../state/workbench';
import type { Change, Finding, Message, WorkbenchState, Worktree } from '../state/workbench';

export const scenarios = ['empty', 'running', 'question', 'typing', 'done', 'review-during-question', 'push-failed', 'conflict', 'manual', 'resolved', 'terminal-done', 'no-model', 'single', 'clean', 'unselected', 'changed', 'remote-error', 'long-paths'] as const;
export type Scenario = typeof scenarios[number];

export function sampleFiles(): Change[] {
  return [
    { id: 'order', path: 'src/service/Order.ts', status: 'M', range: { kind: 'partial', added: 28, removed: 4 } },
    { id: 'pay', path: 'src/api/pay.ts', status: 'M', range: { kind: 'unstaged', added: 15, removed: 2 } },
    { id: 'retry', path: 'src/utils/retry.ts', status: 'A', range: { kind: 'unstaged', added: 68, removed: 0 } },
    { id: 'release', path: 'RELEASE_NOTES.md', status: 'M', range: { kind: 'unstaged', added: 18, removed: 2 } },
    { id: 'lock', path: 'pnpm-lock.yaml', status: 'M', range: { kind: 'unstaged', added: 840, removed: 9 }, ignoredForReview: true },
  ];
}
export function sampleFindings(): Finding[] {
  return [
    { id: 'finding-pay', fileId: 'pay', path: 'src/api/pay.ts', line: 46, severity: 'high', title: '验签失败日志打印了完整请求体', reason: '请求可能含渠道密钥或敏感凭证，建议仅记录 traceId 与 orderId。' },
    { id: 'finding-order', fileId: 'order', path: 'src/service/Order.ts', line: 82, severity: 'suggestion', title: '幂等锁 TTL 建议抽取为配置常量', reason: '将 300s 硬编码收敛到 PAY_CALLBACK_LOCK_TTL_SEC。' },
  ];
}
export function releaseQuestion(): Message {
  return { id: 'release-question', kind: 'question', title: '发布说明要一起提交吗？', scope: '影响提交 2/2「docs」',
    description: '支付回调修复与重试工具已归入提交 1（沿用 Order.ts 已暂存部分）。RELEASE_NOTES.md 属于文档变更，请确认归属：',
    files: [{ id: 'release', label: 'RELEASE_NOTES.md', badge: '待确认' }, { id: 'order', label: 'Order.ts', badge: '仅已暂存' }],
    options: [{ id: 'merge', label: '合并进提交 1' }, { id: 'split', label: '单独提交并一起推送', recommended: true }, { id: 'skip', label: '暂不提交' }] };
}
export function sampleWorktree(): Worktree {
  return {
    repositoryId: 'orders', worktreeId: 'pay-tree', sessionId: 'pay-session', scopeVersion: 'scope-1',
    label: 'wt/pay-retry', path: '/test-only/order-pay', branch: 'feat/pay',
    remote: { kind: 'tracked', branch: 'upstream/feat/pay', ahead: 2, behind: 1 },
    files: sampleFiles(), selectedIds: ['order', 'pay', 'retry', 'release'],
    draft: { text: 'feat(order): 增加支付回调幂等锁与退避重试\n\n- 沿用 Order.ts 已暂存改动\n- 新增指数退避重试工具并明确渠道验签失败错误码', editing: true, outdated: false, revision: 0 },
    review: { status: 'idle', findings: [], skippedPaths: [], changedPaths: [] },
    sessionStatus: 'idle', messages: [], tab: 'commit', dockExpanded: false, replies: {}, requests: {},
  };
}

export function fixture(scenario: Scenario = 'typing'): WorkbenchState {
  if (scenario === 'empty') return emptyWorkbench();
  const tree = sampleWorktree();
  const state: WorkbenchState = {
    ...emptyWorkbench(),
    repositories: [
      { id: 'orders', name: 'my-order-service', branches: [{ name: 'feat/pay', worktreeId: 'pay-tree' }, { name: 'fix/npe', worktreeId: 'fix-tree' }, { name: 'main', worktreeId: 'main-tree' }, { name: 'feat/invoice' }] },
      { id: 'cli', name: 'picket-cli', branches: [{ name: 'main', worktreeId: 'cli-tree' }] },
    ],
    worktrees: [tree,
      { ...sampleWorktree(), worktreeId: 'fix-tree', sessionId: 'fix-session', label: 'wt/fix-npe', branch: 'fix/npe', path: '/test-only/order-fix', files: [], selectedIds: [], draft: { text: 'fix(refund): 校验订单号非空', editing: true, outdated: false, revision: 0 } },
      { ...sampleWorktree(), worktreeId: 'main-tree', sessionId: 'main-session', label: 'main (主目录)', branch: 'main', path: '/test-only/orders', files: [], selectedIds: [] },
      { ...sampleWorktree(), repositoryId: 'cli', worktreeId: 'cli-tree', sessionId: 'cli-session', label: 'main', branch: 'main', path: '/test-only/cli', remote: { kind: 'missing', reason: '尚未配置跟踪远端分支' } },
    ],
    activeWorktreeId: tree.worktreeId,
    models: [{ id: 'test-claude', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }, { id: 'test-gpt', name: 'GPT-4.1', provider: 'openai', modelId: 'gpt-4.1' }], activeModelId: 'test-claude',
  };
  const commits = [{ hash: 'a19e7c2', title: 'feat(order): 增加支付回调幂等锁与退避重试' }, { hash: 'b84d120', title: 'docs: 更新发布说明' }];
  const conflictFiles = [{ id: 'order', label: 'Order.ts', badge: '冲突' }];
  switch (scenario) {
    case 'running': tree.sessionStatus = 'running'; tree.dockExpanded = true; tree.messages = [{ id: 'progress', kind: 'progress', title: '正在分析并拆分已选改动', description: '固定本次范围；未选锁文件与未暂存草稿保留在原处。', scope: '4 个文件 · 仅已暂存 +28 -4', progress: { completed: 1, total: 4, label: '读取范围 → 规划 → 提交 → 推送' } }]; break;
    case 'question': case 'review-during-question': tree.sessionStatus = 'needsDecision'; tree.messages = [releaseQuestion()]; tree.dockExpanded = scenario === 'question';
      if (scenario === 'review-during-question') {
        tree.review = { status: 'done', findings: sampleFindings(), skippedPaths: ['pnpm-lock.yaml'], changedPaths: [], scopeFileIds: ['order', 'pay', 'retry', 'release'] };
        tree.messages.push({ id: 'review-notice', kind: 'review', title: '审查完成：1 项高危、1 项建议', description: '已更新审查意见。' });
      } break;
    case 'done': case 'terminal-done': tree.sessionStatus = 'done'; tree.dockExpanded = scenario !== 'terminal-done'; tree.messages = [{ id: 'done', kind: 'done', title: '2 个提交已推送至 upstream/feat/pay', description: '所选范围已完成。未选锁文件与 Order.ts 未暂存草稿仍保留。', commits }]; break;
    case 'push-failed': tree.sessionStatus = 'pushFailed'; tree.dockExpanded = true; tree.messages = [{ id: 'push-failed', kind: 'pushFailed', title: '已在本地提交，推送失败', description: '无法连接远端。已有提交保留；重试只推送，不重新创建提交。', scope: 'upstream/feat/pay', commits, options: [{ id: 'retry-push', label: '重试推送', recommended: true }] }]; break;
    case 'conflict': tree.sessionStatus = 'conflict'; tree.files[0] = { ...tree.files[0], status: 'U' }; tree.dockExpanded = true; tree.messages = [{ id: 'conflict', kind: 'conflict', title: '同步远端时遇到代码冲突', description: '变基已暂停。只有你授权后 Agent 才能修改下列冲突文件。', scope: '变基 · 1 个冲突文件', files: conflictFiles, options: [{ id: 'agent-merge', label: '让 Agent 合并', recommended: true }, { id: 'manual-merge', label: '我手动合并' }] }]; break;
    case 'manual': tree.sessionStatus = 'waitingManual'; tree.dockExpanded = true; tree.messages = [{ id: 'manual', kind: 'waitingManual', title: '等待你在 IDE 中合并', description: '当前写操作已暂停，请保留暂存与未暂存内容。返回后需核对真实 Git 状态。', files: conflictFiles }]; break;
    case 'resolved': tree.sessionStatus = 'needsDecision'; tree.dockExpanded = true; tree.messages = [{ id: 'resolved', kind: 'resolved', title: '冲突已解决，是否继续推送？', description: '已有提交保留，尚未确认远端已收到。', commits, options: [{ id: 'continue-push', label: '继续推送', recommended: true }] }]; break;
    case 'no-model': state.models = []; state.activeModelId = null; tree.draft = { text: '', editing: false, outdated: false, revision: 0 }; break;
    case 'single': state.activeWorktreeId = 'cli-tree'; break;
    case 'clean': tree.files = []; tree.selectedIds = []; break;
    case 'unselected': tree.selectedIds = []; break;
    case 'changed': tree.draft.outdated = true; tree.review = { status: 'done', findings: sampleFindings(), skippedPaths: ['pnpm-lock.yaml'], changedPaths: ['src/service/Order.ts'], scopeFileIds: ['order', 'pay'] }; break;
    case 'remote-error': tree.remote = { kind: 'error', reason: '远端状态读取失败，无法确认领先 / 落后数量' }; break;
    case 'long-paths': tree.files = [...tree.files, ...Array.from({ length: 30 }, (_, index): Change => ({ id: `long-${index}`, path: `src/包含中文和空格的长目录/多级目录/极长文件名-with-a-very-long-name-${index}.tsx`, previousPath: `src/old-name-${index}.tsx`, status: 'R', range: { kind: 'staged', added: 120, removed: 80 } }))]; break;
  }
  return state;
}
