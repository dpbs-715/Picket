import { act, createElement } from 'react';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitCommitReply } from '../../../../shared/git-commit-protocol';
import type { Root } from 'react-dom/client';
import { Workbench } from '../Workbench';
import { disconnectedBackend } from '../commands';
import type { WorkbenchBackend, WorkbenchCommand } from '../commands';
import { targetOf } from '../state/workbench';
import type { WorkbenchEvent, WorkbenchState } from '../state/workbench';
import { fixture, releaseQuestion } from './fixtures';
import { repositoryReply } from './repository-fixture';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const tick = () => act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 0)); });
function assert(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.getAttribute('aria-label') === label || button.textContent?.trim() === label);
  assert(found, `找不到按钮：${label}`); return found;
}
async function click(label: string) {
  const target = button(label);
  await act(async () => { target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); });
  await act(async () => { button(label).click(); });
}
function type(element: HTMLInputElement | HTMLTextAreaElement, text: string) {
  const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, text);
    element.dispatchEvent(new Event('input', { bubbles: true }));
  });
}
const text = () => document.querySelector('.workbench')!.textContent ?? '';

export async function verifyWorkbench(root: Root) {
  const checks: string[] = [];
  let key = 0;
  async function mount(initialState: WorkbenchState, backend: WorkbenchBackend = disconnectedBackend) {
    await act(async () => { root.render(createElement(Workbench, { key: ++key, initialState, backend })); });
  }

  await mount(fixture('empty'));
  assert(text().includes('打开你的 Git 仓库') && !text().includes('my-order-service'), '默认状态混入示例');
  assert(button('提交').disabled, '无仓库不应提交');
  await click('打开本机仓库…');
  assert(text().includes('此操作尚未接入'), `未接入的打开操作应明确说明：${text()}`);
  await click('手动编写');
  let textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  assert(document.activeElement === textarea, '手动编写应聚焦输入');
  type(textarea, '尚未选择仓库时写的草稿'); await tick();
  await click('设置'); await click('添加第一个模型');
  assert(document.querySelector<HTMLInputElement>('input[type="password"]')?.value === '', 'Key 应为空且遮蔽');
  await click('取消');
  assert(document.querySelector('[aria-label="关闭设置"]'), '取消添加后应回到设置');
  await click('关闭设置');
  assert(textarea.value === '尚未选择仓库时写的草稿', '打开设置不能丢失草稿');
  checks.push('真实默认空态、手动输入、共用模型入口与取消返回');

  const subscription: { receive?: (event: WorkbenchEvent) => void } = {};
  const typingState = fixture('typing');
  await mount(typingState, { ...disconnectedBackend, subscribe(receive) { subscription.receive = receive; return () => { subscription.receive = undefined; }; } });
  textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  textarea.focus(); type(textarea, '持续输入，不许被问题覆盖'); await tick();
  textarea.setSelectionRange(3, 7);
  await act(async () => { subscription.receive?.({ type: 'message', target: targetOf(typingState.worktrees[0]), message: releaseQuestion(), sessionStatus: 'needsDecision' }); });
  assert(document.activeElement === textarea, '后台提问抢走焦点');
  assert(textarea.value === '持续输入，不许被问题覆盖' && textarea.selectionStart === 3 && textarea.selectionEnd === 7, '后台提问改变文字或光标范围');
  assert(!document.querySelector('.message-expanded'), '后台提问不应自动展开');
  await act(async () => { subscription.receive?.({ type: 'message', target: targetOf(typingState.worktrees[0]), message: { id: 'review', kind: 'review', title: '审查已完成', description: '次级动态' } }); });
  assert(text().includes('+1'), '问题到达后的审查应是次级动态');
  assert(document.activeElement === textarea, '次级动态抢走焦点');
  await click('展开消息：发布说明要一起提交吗？');
  const lower = document.querySelector('.lower-workspace')!.getBoundingClientRect();
  const expanded = document.querySelector('.message-expanded')!.getBoundingClientRect();
  assert(Math.abs(lower.top - expanded.top) <= 1 && Math.abs(lower.bottom - expanded.bottom) <= 1, '消息覆盖超出下半工作区');
  assert(document.querySelector('.context-header')!.getBoundingClientRect().bottom <= expanded.top, '消息覆盖头部');
  assert(document.querySelector('.action-footer')!.getBoundingClientRect().top >= expanded.bottom, '消息覆盖底栏');
  const reply = document.querySelector<HTMLInputElement>('[aria-label="回复当前事项"]')!;
  type(reply, '文档单独提交'); await tick();
  await click('收起消息'); await click('展开消息：发布说明要一起提交吗？');
  assert(document.querySelector<HTMLInputElement>('[aria-label="回复当前事项"]')!.value === '文档单独提交', '收起丢失自由回复');
  await click('单独提交并一起推送');
  assert(text().includes('发布说明要一起提交吗？'), '失败不能伪造问题已解决');
  await click('查看');
  assert(button('审查意见').getAttribute('aria-selected') === 'true', '次级动态查看不跳审查 Tab');
  checks.push('输入焦点、光标范围、文字保全、消息优先级与局部覆盖');

  await mount(fixture('changed'));
  const checkbox = document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]')!;
  assert(checkbox.checked, '样例选择错误');
  await act(async () => { checkbox.click(); });
  assert(!checkbox.checked && !document.querySelector('[role="alert"]'), '勾选不能触发 IDE 或 Git 命令');
  await click('切换目录树视图');
  assert(text().includes('(根目录)'), '目录分组缺少根目录');
  await click('切换列表视图');
  assert(document.querySelector('.file-stats')?.textContent?.includes('+28'), '部分暂存统计错误');
  await click('C-Push 会话记录');
  const historyEntry = [...document.querySelectorAll<HTMLButtonElement>('.history-entry')].find((entry) => entry.textContent?.includes('fix/npe'))!;
  await act(async () => { historyEntry.click(); });
  assert(document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'fix(refund): 校验订单号非空', '切换工作树串草稿');
  await click('C-Push 会话记录');
  await act(async () => { document.querySelectorAll<HTMLButtonElement>('.history-entry')[0].click(); });
  assert(!document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]')!.checked, '工作树选择未保留');
  checks.push('勾选与文件动作分离、部分暂存统计、目录分组、工作树隔离');

  const calls: WorkbenchCommand[] = [];
  let finish: (() => void) | undefined;
  await mount(fixture('typing'), { ...disconnectedBackend, execute(command) { calls.push(command); return new Promise<void>((resolve) => { finish = resolve; }); } });
  await act(async () => { button('C-Push').click(); button('C-Push').click(); });
  assert(calls.length === 1 && calls[0].kind === 'cPush', '一键流程重复触发或增加必经确认页');
  assert(!document.querySelector('.scope-menu'), 'C-Push 不应打开手动确认页');
  if (calls[0].kind === 'cPush') assert(!calls[0].fileIds.includes('lock') && calls[0].target.worktreeId === 'pay-tree', '请求扩大范围或串目标');
  await act(async () => { finish?.(); });
  checks.push('C-Push 直接请求、请求身份、精确选择与重复点击保护（脚本化边界）');

  const manual = fixture('no-model'); manual.worktrees[0].draft = { text: '手写说明', editing: true, outdated: false, revision: 0 };
  const manualSubscription: { receive?: (event: WorkbenchEvent) => void } = {};
  await mount(manual, { ...disconnectedBackend, subscribe(receive) { manualSubscription.receive = receive; return () => { manualSubscription.receive = undefined; }; } });
  await click('提交');
  assert(text().includes('仅已暂存 +28 -4'), '确认范围没有标明仅已暂存');
  await click('仅本地提交');
  assert(text().includes('提交尚未执行') && document.querySelector<HTMLTextAreaElement>('textarea')?.value === '手写说明', '手动提交失败未保留草稿');
  await act(async () => { manualSubscription.receive?.({ type: 'files', target: targetOf(manual.worktrees[0]), nextScopeVersion: 'external-change', files: manual.worktrees[0].files, changedPaths: ['src/api/pay.ts'] }); });
  assert(text().includes('范围已变化'), '确认期间改变选择未拦截');
  assert(button('仅本地提交').disabled, '失效范围仍可写入');
  checks.push('无模型手动范围、失效范围拦截与失败草稿保留');

  const single = fixture('single'); single.models = []; single.activeModelId = null;
  await mount(single);
  assert(!document.querySelector('.worktree-anchor'), '单目录仓库应隐藏工作树选择器');
  await click('main');
  assert(text().includes('新建 Worktree'), '单目录仓库缺少新建 Worktree 入口');
  await click('新建 Worktree');
  assert(document.querySelector('[aria-label="Worktree 新分支名"]'), 'Worktree 内联表单不可达');
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await click('提交');
  assert(button('仅本地提交').disabled && text().includes('保护分支'), '保护分支应禁止直接写入');
  checks.push('单目录 Worktree 可达与保护分支拦截');

  await mount(fixture('no-model'));
  await click('设置'); await click('添加第一个模型');
  await click('Ollama（本地）');
  assert(!document.querySelector('input[type="password"]'), 'Ollama 不应要求 Key');
  assert(document.querySelector('input[value="http://127.0.0.1:11434/v1"]'), 'Ollama 地址默认值错误');
  await click('测试连接');
  assert(text().includes('未发送连接请求'), '不能伪造模型连接成功');
  await click('保存并使用');
  assert(text().includes('没有保存模型或密钥'), '不能伪造模型保存成功');
  await click('Anthropic');
  assert(document.querySelector<HTMLInputElement>('input[type="password"]')?.value === '', '切换提供商残留 Key');
  checks.push('提供商字段、真实零模型边界、未接入连接不伪报成功');

  const modelCalls: WorkbenchCommand[] = [];
  await mount(fixture('typing'), { ...disconnectedBackend, async execute(command) { modelCalls.push(command); } });
  await click('设置'); await click('删除模型 Claude Sonnet 4');
  assert(text().includes('GPT-4.1') && !text().includes('Claude Sonnet 4'), '删除当前模型没有回退');
  assert(modelCalls.length === 1 && modelCalls[0].kind === 'deleteModel', '删除误触发模型选择');
  await click('关闭设置');
  assert(button('GPT-4.1'), '设置与底栏没有共享选中模型');
  await click('设置'); await click('删除模型 GPT-4.1'); await click('关闭设置');
  assert(!document.querySelector('.model-button') && button('添加模型'), '删到零后仍保留空模型下拉');
  checks.push('两处模型状态共享、删除不触发选择、删到零回到无模型（脚本化边界）');

  await mount(fixture('no-model'), { ...disconnectedBackend, async saveModel(input) {
    assert(input.apiKey === '' && input.provider === 'ollama', '隐藏的 Key 不应进入本地模型保存请求');
    return { id: 'fixture-saved', name: input.name, provider: input.provider, modelId: input.modelId, baseUrl: input.baseUrl };
  } });
  await click('设置'); await click('添加第一个模型'); await click('Ollama（本地）'); await click('保存并使用');
  assert(!document.querySelector('.overlay') && button('Qwen 2.5 Coder'), '保存成功后应关闭两层表单并共享当前模型');
  checks.push('添加表单保存回执、自动选中与返回工作台（脚本化边界）');

  const connections: { resolve: (description: string) => void }[] = [];
  await mount(fixture('no-model'), { ...disconnectedBackend, testModel() { return new Promise((resolve) => connections.push({ resolve })); } });
  await click('设置'); await click('添加第一个模型'); await click('Ollama（本地）'); await click('测试连接');
  const modelInput = [...document.querySelectorAll<HTMLInputElement>('.model-form input')].find((input) => input.value === 'qwen2.5-coder:14b')!;
  type(modelInput, 'changed-model'); await tick(); await click('测试连接');
  await act(async () => { connections[0].resolve('旧连接测试结果'); });
  assert(!text().includes('旧连接测试结果') && text().includes('测试中…'), '旧连接结果污染新字段或结束新请求');
  await act(async () => { connections[1].resolve('新字段的测试结果（测试夹具）'); });
  assert(text().includes('新字段的测试结果（测试夹具）'), '新连接结果未正确展示');
  type(modelInput, 'changed-again'); await tick();
  assert(!text().includes('新字段的测试结果（测试夹具）'), '修改连接字段未令旧测试失效');
  checks.push('连接字段变化和晚测试结果隔离（脚本化边界）');

  await mount(fixture('long-paths'));
  const workbench = document.querySelector<HTMLElement>('.workbench')!;
  assert(workbench.scrollWidth <= workbench.clientWidth, '长路径造成工作台横向溢出');
  assert(document.querySelector('.changes-list')!.scrollHeight > document.querySelector('.changes-list')!.clientHeight, '多文件未独立滚动');
  for (const height of [600, 700]) {
    document.getElementById('root')!.style.height = `${height}px`; await tick();
    assert(document.querySelector('.lower-workspace')!.getBoundingClientRect().height > 150, '窄窗下半区不可用');
  }
  checks.push('长路径、多文件独立滚动、600 / 700 高度布局');

  document.documentElement.classList.add('native-titlebar');
  const compact = fixture('typing');
  compact.repositories[0].name = 'a-repository-with-a-very-long-name-that-must-truncate';
  compact.models = []; compact.activeModelId = null;
  await mount(compact);
  const firstRow = document.querySelector<HTMLElement>('.context-top')!;
  const repositoryButton = document.querySelector<HTMLButtonElement>('.repository-button')!;
  assert(firstRow.getBoundingClientRect().height === 38, '合并标题栏不应增加头部行高');
  assert(repositoryButton.getBoundingClientRect().left >= 78, '仓库入口侵入原生窗口按钮区域');
  assert(repositoryButton.scrollWidth <= repositoryButton.clientWidth, '长仓库名挤掉头部工具');
  assert(document.querySelector('.header-tools')!.getBoundingClientRect().right <= innerWidth, '头部工具超出窄窗');
  assert(firstRow.hasAttribute('data-tauri-drag-region') && !repositoryButton.hasAttribute('data-tauri-drag-region'), '拖动区不能覆盖仓库按钮');
  await click(compact.repositories[0].name);
  const repositoryMenu = document.querySelector('.repository-menu')!.getBoundingClientRect();
  assert(repositoryMenu.left >= 0 && repositoryMenu.right <= innerWidth, '合并标题栏的仓库菜单超出窗口');
  await click('设置');
  assert(document.querySelector('.overlay-heading h1')!.getBoundingClientRect().left >= 84, '设置标题遮住原生窗口按钮');
  await click('添加第一个模型');
  assert(document.querySelectorAll('.overlay-heading h1')[1].getBoundingClientRect().left >= 84, '添加模型标题遮住原生窗口按钮');
  assert(!document.querySelector('.overlay-heading button[data-tauri-drag-region]'), '覆盖界面的关闭按钮不能作为拖动区');
  document.documentElement.classList.remove('native-titlebar');
  checks.push('合并标题栏的原生按钮留白、长仓库名、浮层范围与交互按钮边界（布局样例）');

  let readSequence = 0, refreshCount = 0, failRefresh = false;
  const reads: WorkbenchBackend = { ...disconnectedBackend, repositoryReadsAvailable: true, async execute(command) {
    if (command.kind !== 'openRepository' && command.kind !== 'refresh') return disconnectedBackend.execute(command);
    if (command.kind === 'refresh') { refreshCount++; if (failRefresh) throw new Error('工作目录暂时不可读'); }
    const reply = repositoryReply(command.requestId, ++readSequence);
    if (command.kind === 'refresh') { reply.target = command.target; reply.snapshot!.activeWorktreeId = command.target.worktreeId; }
    return reply;
  } };
  await mount(fixture('empty'), reads);
  await click('手动编写');
  textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  type(textarea, '打开前先写好的说明'); await tick();
  await click('打开本机仓库…');
  assert(text().includes('real-contract-fixture') && textarea.value === '打开前先写好的说明', '读取仓库后未显示快照或丢失初始说明');
  assert(!document.querySelector<HTMLInputElement>('[aria-label="选择 pnpm-lock.yaml"]')!.checked, '真实快照的锁文件默认选择错误');
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click(); });
  textarea.focus(); textarea.setSelectionRange(1, 4);
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  assert(refreshCount === 1 && document.activeElement === textarea && textarea.selectionStart === 1 && textarea.selectionEnd === 4, '回到窗口刷新未发生，或抢走输入焦点 / 选区');
  assert(textarea.value === '打开前先写好的说明' && !document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.checked, '刷新覆盖草稿或勾选');
  checks.push('打开仓库回执、首次草稿迁移、锁文件选择与窗口聚焦刷新（协议夹具）');
  failRefresh = true; await click('刷新变更');
  assert(text().includes('工作树状态不可用') && button('提交').disabled && textarea.value === '打开前先写好的说明', '读取失败未保留草稿或仍允许使用旧状态');
  failRefresh = false; await click('重试读取');
  assert(!text().includes('工作树状态不可用') && document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]'), '重新读取未恢复文件区');
  checks.push('读取错误不冒充干净工作区，保留草稿并支持重试（协议夹具）');

  const delayed: { finish?: (reply: ReturnType<typeof repositoryReply>) => void; requestId?: string; target?: ReturnType<typeof targetOf> } = {};
  await mount(fixture('empty'), { ...reads, execute(command) {
    if (command.kind === 'refresh') return new Promise((resolve) => { delayed.finish = resolve; delayed.requestId = command.requestId; delayed.target = command.target; });
    return reads.execute(command);
  } });
  await click('打开本机仓库…');
  await click('刷新变更');
  await act(async () => { document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click(); });
  await act(async () => { [...document.querySelectorAll<HTMLButtonElement>('.worktree-menu > button')].find((button) => button.textContent?.includes('fix/npe'))!.click(); });
  assert(document.querySelector('.branch-anchor')!.textContent?.includes('fix/npe'), '工作树切换没有生效');
  assert(delayed.requestId && delayed.target && delayed.finish, '没有开始受控刷新');
  const lateReply = repositoryReply(delayed.requestId, ++readSequence); lateReply.target = delayed.target;
  await act(async () => { delayed.finish!(lateReply); });
  assert(document.querySelector('.branch-anchor')!.textContent?.includes('fix/npe'), '晚返回的刷新切走了当前工作树');
  checks.push('切换工作树期间的晚刷新只更新原目标，不迁移当前视图（协议夹具）');

  await mount(fixture('empty'), { ...reads, async execute(command) {
    if (command.kind === 'openRepository') return { protocolVersion: 1, requestId: command.requestId, snapshot: null };
    return reads.execute(command);
  } });
  await click('手动编写'); type(document.querySelector<HTMLTextAreaElement>('textarea')!, '取消后仍保留'); await tick();
  await click('打开本机仓库…');
  assert(document.querySelector<HTMLTextAreaElement>('textarea')?.value === '取消后仍保留' && text().includes('打开你的 Git 仓库'), '取消目录选择丢失输入或伪造打开成功');
  checks.push('打开目录取消保持原输入与空态（协议夹具）');
  let branchCallCount = 0;
  const branchResponse: { finish?: (reply: GitBranchReply) => void; command?: Extract<WorkbenchCommand, { branch: string }> } = {};
  const branches: WorkbenchBackend = { ...reads, execute(command) {
    if (command.kind !== 'createBranch' && command.kind !== 'createWorktree' && command.kind !== 'switchBranch') return reads.execute(command);
    branchCallCount++; branchResponse.command = command;
    return new Promise<GitBranchReply>((resolve) => { branchResponse.finish = resolve; });
  } };
  await mount(fixture('empty'), branches); await click('打开本机仓库…'); await click('手动编写');
  textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  type(textarea, '分支操作时的人工草稿'); await tick();
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click(); document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await click('新建分支（带走当前改动）');
  type(document.querySelector<HTMLInputElement>('[aria-label="新分支名"]')!, 'feat/new'); await tick();
  await act(async () => { const form = document.querySelector<HTMLFormElement>('.branch-form')!; form.requestSubmit(); form.requestSubmit(); });
  assert(branchCallCount === 1 && branchResponse.command?.branchToken, '创建分支重复请求或没有携带 Rust 版本');
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/pay'), 'Git 未返回就伪造分支成功');
  type(textarea, '等待 Git 时继续写的新草稿'); await tick();
  const branchSnapshot = repositoryReply(branchResponse.command!.requestId, ++readSequence).snapshot!;
  branchSnapshot.worktrees[0].branch = 'feat/new'; branchSnapshot.worktrees[0].branchToken = 'updated-branch-token';
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: branchResponse.command!.requestId, target: branchResponse.command!.target, action: 'createBranch', outcome: 'completed', snapshot: branchSnapshot }); });
  assert(!document.querySelector('.branch-menu') && document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/new'), '成功后未采用 Git 分支或未关闭菜单');
  assert(textarea.value === '等待 Git 时继续写的新草稿' && !document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.checked, '分支回执覆盖了最新草稿或选择');
  checks.push('分支创建只采用 Git 回执、重复点击保护、请求中继续输入与选择保全（协议夹具）');

  await mount(fixture('empty'), { ...reads, async execute(command) {
    if (command.kind === 'createWorktree') return { protocolVersion: 1, kind: 'branch', requestId: command.requestId, target: command.target, action: command.kind, outcome: 'cancelled', snapshot: null };
    return reads.execute(command);
  } });
  await click('打开本机仓库…');
  await act(async () => { document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click(); });
  await click('新建 Worktree'); type(document.querySelector<HTMLInputElement>('[aria-label="Worktree 新分支名"]')!, 'feat/cancelled'); await tick(); await click('创建');
  assert(text().includes('已取消创建 Worktree') && document.querySelector<HTMLInputElement>('[aria-label="Worktree 新分支名"]')?.value === 'feat/cancelled', '取消目录选择丢失分支名或伪造完成');
  assert(document.querySelectorAll('.worktree-menu > button').length === 2, '取消后伪造了新工作树');
  checks.push('Worktree 取消目录选择后保留表单、原分支和工作树列表（协议夹具）');

  await mount(fixture('empty'), branches); await click('打开本机仓库…');
  await act(async () => { document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click(); });
  await click('新建 Worktree'); type(document.querySelector<HTMLInputElement>('[aria-label="Worktree 新分支名"]')!, 'feat/later'); await tick(); await click('创建');
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await act(async () => { document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click(); });
  await act(async () => { [...document.querySelectorAll<HTMLButtonElement>('.worktree-menu > button')].find((button) => button.textContent?.includes('fix/npe'))!.click(); });
  const worktreeSnapshot = repositoryReply(branchResponse.command!.requestId, ++readSequence).snapshot!;
  worktreeSnapshot.worktrees.push({ ...worktreeSnapshot.worktrees[0], worktreeId: 'new-tree', branch: 'feat/later', label: 'new', files: [] }); worktreeSnapshot.activeWorktreeId = 'new-tree';
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: branchResponse.command!.requestId, target: branchResponse.command!.target, action: 'createWorktree', outcome: 'completed', snapshot: worktreeSnapshot }); });
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('fix/npe'), 'Worktree 晚结果切走了用户当前视图');
  checks.push('创建 Worktree 中切换视图，晚回执只更新原仓库而不抢回界面（协议夹具）');

  let commitCalls = 0, failCommitWithMutation = false;
  const commitPending: { finish?: (reply: GitCommitReply) => void; command?: Extract<WorkbenchCommand, { kind: 'commit' }> } = {};
  const commits: WorkbenchBackend = {
    ...reads,
    async execute(command) {
      if (command.kind === 'refresh') {
        const reply = repositoryReply(command.requestId, ++readSequence);
        reply.target = command.target; reply.snapshot!.activeWorktreeId = command.target.worktreeId;
        if (failCommitWithMutation) reply.snapshot!.worktrees[0].files[0].contentVersion = 'hook-mutated';
        return reply;
      }
      if (command.kind !== 'commit') return reads.execute(command);
      if (command.push) throw new Error('远端推送将在 M5 接入；本次未执行提交或推送，草稿与选择已保留。');
      commitCalls++; commitPending.command = command;
      if (failCommitWithMutation) throw new Error('Git pre-commit 钩子修改了仓库内容，不能继续沿用旧范围；未创建提交。');
      return new Promise<GitCommitReply>((resolve) => { commitPending.finish = resolve; });
    },
  };
  await mount(fixture('empty'), commits);
  await click('打开本机仓库…');
  await act(async () => {
    document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="选择 RELEASE_NOTES.md"]')!.click();
  });
  await click('手动编写');
  textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  type(textarea, 'feat(order): 仅本地提交已暂存与新文件'); await tick();
  await click('提交');
  assert(text().includes('确认本次提交范围') && text().includes('仅已暂存 +28 -4'), '范围确认未显示部分暂存统计');
  await click('提交并推送');
  assert(text().includes('M5') && textarea.value === 'feat(order): 仅本地提交已暂存与新文件', 'M5 推送拒绝丢失草稿或伪造推送');
  failCommitWithMutation = true;
  await click('仅本地提交');
  assert(text().includes('钩子修改了仓库内容') && text().includes('范围已变化') && button('仅本地提交').disabled, '钩子改内容后未自动刷新并拦截旧范围');
  assert(textarea.value === 'feat(order): 仅本地提交已暂存与新文件' && text().includes('已保留当前草稿'), '钩子改内容丢失人工草稿');
  failCommitWithMutation = false; commitCalls = 0;
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await click('提交');
  await act(async () => { button('仅本地提交').click(); button('仅本地提交').click(); });
  assert(commitCalls === 1 && commitPending.command?.selections?.length === 2 && commitPending.command.selections[0].commitToken === 'commit-order', '本地提交重复触发或未携带每文件 commitToken');
  const postCommitSnapshot = repositoryReply(commitPending.command!.requestId, ++readSequence).snapshot!;
  postCommitSnapshot.worktrees[0].head = 'a9c41ef000';
  postCommitSnapshot.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 3, behind: 1, localOnly: true };
  postCommitSnapshot.worktrees[0].files = postCommitSnapshot.worktrees[0].files
    .filter((file) => file.id !== 'retry')
    .map((file) => file.id === 'order' ? { ...file, gitStatus: '.M', range: { kind: 'unstaged', added: 14, removed: 4 }, contentVersion: 'order-draft', commitToken: 'commit-order-draft' } : file);
  await act(async () => {
    commitPending.finish!({
      protocolVersion: 1, kind: 'commit', requestId: commitPending.command!.requestId, target: commitPending.command!.target,
      snapshot: postCommitSnapshot,
      commit: { oid: 'a9c41ef000', shortOid: 'a9c41ef', title: 'feat(order): 仅本地提交已暂存与新文件', branch: 'feat/pay', parent: 'head-1' },
    });
  });
  assert(!document.querySelector('.scope-menu') && text().includes('已在本地提交 a9c41ef') && text().includes('↑ 3'), '本地提交完成后未更新提示或领先数');
  assert(!document.querySelector('textarea') && text().includes('手动编写'), '成功消费的提交草稿未回到空态');
  assert(!document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]')!.checked, '部分暂存剩余草稿不应自动保持勾选');
  assert(!document.querySelector('[aria-label="选择 src/utils/retry.ts"]') && !!document.querySelector('[aria-label="选择 pnpm-lock.yaml"]'), '已提交文件未移除或未选文件丢失');
  checks.push('无模型精确范围本地提交、commitToken 传递、钩子改内容重核与部分暂存剩余草稿未选（协议夹具）');

  const detached = fixture('no-model');
  detached.worktrees[0].detached = true; detached.worktrees[0].branch = '(detached)';
  detached.worktrees[0].draft = { text: '分离 HEAD 草稿', editing: true, outdated: false, revision: 0 };
  await mount(detached);
  await click('提交');
  assert(button('仅本地提交').disabled && text().includes('分离 HEAD'), '分离 HEAD 不应允许直接提交');
  checks.push('分离 HEAD 提交拦截与草稿保留');
  return { environment: 'Chrome headless · 开发样例，不是真实 Git / 模型验收', viewport: { width: innerWidth, height: innerHeight }, checks };
}
