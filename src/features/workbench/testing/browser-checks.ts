import { act, createElement } from 'react';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitCommitReply } from '../../../../shared/git-commit-protocol';
import type { GitOpenFileReply } from '../../../../shared/ide-protocol';
import type { GenerateCommitReply, ReviewScopeReply } from '../../../../shared/model-protocol';
import type { WorkbenchRestoreReply, WorkbenchSaveRequest } from '../../../../shared/workbench-persistence-protocol';
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
  assert(!document.querySelector('.changes-heading') && !document.querySelector('[aria-label="切换目录树视图"]') && !document.querySelector('[aria-label="刷新变更"]'), '空项目不应提前渲染变更工具栏');
  assert(document.querySelector<HTMLButtonElement>('.repository-open-button')?.classList.contains('primary'), '打开本机仓库应作为空项目唯一主入口');
  assert(!document.querySelector('.commit-empty button.primary'), '空项目下半区不应与打开仓库抢夺主按钮视觉层级');
  assert(!document.querySelector('.commit-empty .help'), '提交说明空态不应展示冗余描述段落');
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
  await mount(fixture('clean'));
  assert(text().includes('工作区干净') && !!document.querySelector('[aria-label="刷新变更"]') && !document.querySelector('[aria-label="切换目录树视图"]') && !document.querySelector('.changes-heading input[type="checkbox"]'), '干净工作区应保留刷新并隐藏全选框与目录树切换');
  checks.push('真实默认空态、干净工作区层级、手动输入、共用模型入口与取消返回');

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
  assert(!document.querySelector('.changes-heading h2') && !document.querySelector('.changes-select-all'), '顶栏不应保留冗余变更标题或文字全选按钮');
  const checkbox = document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]')!;
  const topCheckbox = document.querySelector<HTMLInputElement>('.changes-heading input[type="checkbox"]')!;
  assert(checkbox.checked, '样例选择错误');
  assert(
    topCheckbox && topCheckbox.getAttribute('aria-label') === '全选变更' && !topCheckbox.checked && topCheckbox.indeterminate
      && Math.abs(topCheckbox.getBoundingClientRect().left - checkbox.getBoundingClientRect().left) < 0.5,
    '顶栏三态全选框应与文件复选框左列对齐且初始呈半选状态',
  );
  await act(async () => { topCheckbox.click(); });
  assert(
    topCheckbox.checked && !topCheckbox.indeterminate && topCheckbox.getAttribute('aria-label') === '取消全选变更'
      && document.querySelector<HTMLInputElement>('[aria-label="选择 pnpm-lock.yaml"]')!.checked,
    '点击半选顶栏全选框应选中包含锁文件在内的全部变更',
  );
  await act(async () => { topCheckbox.click(); });
  assert(!topCheckbox.checked && !topCheckbox.indeterminate && topCheckbox.getAttribute('aria-label') === '全选变更' && !checkbox.checked && text().includes('未选择变更'), '再次点击全选框应清空全部勾选');
  await click('切换目录树视图');
  assert(text().includes('(根目录)'), '目录分组缺少根目录');
  await act(async () => {
    document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="选择 RELEASE_NOTES.md"]')!.click();
  });
  const rootDirCheckbox = document.querySelector<HTMLInputElement>('[aria-label="选择目录 (根目录)"]')!;
  const groupedCheckbox = document.querySelector<HTMLInputElement>('.file-row.grouped .file-checkbox')!;
  assert(
    rootDirCheckbox && !rootDirCheckbox.checked && rootDirCheckbox.indeterminate
      && Math.abs(rootDirCheckbox.getBoundingClientRect().left - topCheckbox.getBoundingClientRect().left) < 0.5
      && groupedCheckbox.getBoundingClientRect().left > rootDirCheckbox.getBoundingClientRect().right,
    '目录分组复选框应与顶栏全选框同列对齐、半选准确且子文件缩进清晰',
  );
  await act(async () => { rootDirCheckbox.click(); });
  assert(
    rootDirCheckbox.checked && !rootDirCheckbox.indeterminate && rootDirCheckbox.getAttribute('aria-label') === '取消选择目录 (根目录)'
      && document.querySelector<HTMLInputElement>('[aria-label="选择 pnpm-lock.yaml"]')!.checked
      && document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.checked
      && !document.querySelector('[role="alert"]'),
    '点击半选目录复选框应全选该目录文件且不触发外部 IDE 命令',
  );
  await act(async () => { rootDirCheckbox.click(); });
  assert(
    !rootDirCheckbox.checked && !rootDirCheckbox.indeterminate
      && !document.querySelector<HTMLInputElement>('[aria-label="选择 RELEASE_NOTES.md"]')!.checked
      && !document.querySelector<HTMLInputElement>('[aria-label="选择 pnpm-lock.yaml"]')!.checked
      && document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.checked,
    '取消全选某目录不应清除其他目录已选文件',
  );
  await click('切换列表视图');
  assert(!document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]')!.checked && document.querySelector('.file-stats')?.textContent?.includes('+28'), '部分暂存统计错误');
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
  single.repositories[1].remoteBranches!.push({
    name: 'origin/103-shan-dao-bug-xiu-gai-very-long-branch-name',
    remote: 'origin',
    localName: '103-shan-dao-bug-xiu-gai-very-long-branch-name',
  });
  await mount(single);
  const outerBranchLabel = document.querySelector<HTMLElement>('.branch-anchor > button .truncate')!;
  assert(!document.querySelector('.worktree-anchor') && outerBranchLabel.scrollWidth <= outerBranchLabel.clientWidth, '外部头部应只展示当前分支入口且短分支名不得被截断出省略号');
  await click('main');
  const groupToggles = [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')];
  assert(
    groupToggles.length === 2
      && groupToggles[0].textContent?.includes('本地分支') && groupToggles[0].getAttribute('aria-expanded') === 'true'
      && groupToggles[1].textContent?.includes('远程分支') && groupToggles[1].getAttribute('aria-expanded') === 'false'
      && document.querySelector('.branch-menu .branch-tag')?.textContent === '保护分支'
      && !text().includes('新建分支')
      && !text().includes('新建 Worktree'),
    '单目录分支菜单应展示 IDEA 风格的本地分支 / 远程分支双折叠结构与保护分支短标签',
  );
  await act(async () => { groupToggles[1].click(); });
  const branchMenuRect = document.querySelector('.branch-menu')!.getBoundingClientRect();
  const remoteItems = [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="remote"] .branch-item')];
  assert(
    groupToggles[1].getAttribute('aria-expanded') === 'true'
      && (document.querySelector('[data-branch-group="remote"]')?.textContent ?? '').includes('origin/release/v1')
      && branchMenuRect.left >= 0 && branchMenuRect.right <= innerWidth
      && remoteItems.every((item) => item.getBoundingClientRect().height <= 32),
    '展开远程分支折叠组应展示远程分支、不越界且右侧标签不纵向折行撑高条目',
  );
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await click('提交');
  assert(!button('仅本地提交').disabled && !button('提交并推送').disabled && document.querySelector('.scope-menu .branch-tag')?.textContent === '保护分支', '保护分支应仅展示紧凑标签并允许直接提交');
  checks.push('外部单一分支入口、IDEA 风格本地/远程双折叠菜单与保护分支直接提交');

  await mount(fixture('no-model'));
  await click('设置'); await click('添加第一个模型');
  const formatSelect = document.querySelector<HTMLSelectElement>('select[aria-label="接口格式"]')!;
  const ctxInput = document.querySelector<HTMLInputElement>('input[aria-label="上下文长度"]')!;
  const maxOutInput = document.querySelector<HTMLInputElement>('input[aria-label="最大输出 Token"]')!;
  assert(formatSelect && formatSelect.value === 'anthropic-messages', 'Anthropic 默认接口格式应为 anthropic-messages');
  assert(ctxInput.value === '200000' && maxOutInput.value === '16384', 'Anthropic 默认上下文与最大输出 Token 应从共享默认配置加载');
  const keyHelpTip = document.querySelector<HTMLElement>('[aria-label="API Key 安全存储说明"]')!;
  assert(
    !!keyHelpTip && getComputedStyle(keyHelpTip.querySelector('.help-tip-bubble')!).visibility === 'hidden'
      && !document.querySelector('.model-form > p.help'),
    '模型表单应移除底部常驻说明并将必要规则收敛为 ? 悬浮提示',
  );
  await click('自定义兼容');
  assert((formatSelect.value as string) === 'openai-completions', '自定义兼容默认接口格式应为 openai-completions');
  assert((ctxInput.value as string) === '128000' && (maxOutInput.value as string) === '16384', '自定义兼容应加载对应默认 Token 配置');
  await act(async () => {
    formatSelect.value = 'openai-responses';
    formatSelect.dispatchEvent(new Event('change', { bubbles: true }));
  });
  assert((formatSelect.value as string) === 'openai-responses', '接口格式下拉框应支持自由切换协议');
  assert((ctxInput.value as string) === '200000' && (maxOutInput.value as string) === '32768', '未手动改动时切换接口格式应联动更新默认 Token 配置');
  await click('Ollama（本地）');
  assert(!document.querySelector('input[type="password"]'), 'Ollama 不应要求 Key');
  assert(document.querySelector('input[value="http://127.0.0.1:11434/v1"]'), 'Ollama 地址默认值错误');
  await click('测试连接');
  assert(text().includes('未发送连接请求'), '不能伪造模型连接成功');
  await click('保存并使用');
  assert(text().includes('没有保存模型或密钥'), '不能伪造模型保存成功');
  await click('Anthropic');
  assert(document.querySelector<HTMLInputElement>('input[type="password"]')?.value === '', '切换提供商残留 Key');
  checks.push('提供商字段、接口格式下拉框、真实零模型边界、未接入连接不伪报成功');

  const modelCalls: WorkbenchCommand[] = [];
  let editedPayload: unknown;
  await mount(fixture('typing'), {
    ...disconnectedBackend,
    async execute(command) { modelCalls.push(command); },
    async saveModel(input) {
      editedPayload = input;
      return {
        id: input.id ?? 'new-id',
        name: input.name,
        provider: input.provider,
        apiFormat: input.apiFormat,
        modelId: input.modelId,
        contextWindow: input.contextWindow,
        maxOutputTokens: input.maxOutputTokens,
        ...(input.baseUrl ? { baseUrl: input.baseUrl } : {}),
      };
    },
  });
  await click('设置');
  const activeSelect = document.querySelector<HTMLButtonElement>('.model-row.is-active .model-select')!;
  assert(activeSelect && getComputedStyle(activeSelect).backgroundColor === 'rgba(0, 0, 0, 0)', '选中模型背景应由整张卡片承载而非局部内嵌块');
  await click('编辑模型 GPT-4.1');
  assert(text().includes('编辑模型') && document.querySelector<HTMLInputElement>('input[type="password"]')?.value === '', '打开编辑模型不应回填明文密钥');
  const editModelIdInput = [...document.querySelectorAll<HTMLInputElement>('.model-form input')].find((el) => el.value === 'gpt-4.1')!;
  type(editModelIdInput, 'gpt-4.1-mini'); await tick();
  const editCtxInput = document.querySelector<HTMLInputElement>('input[aria-label="上下文长度"]')!;
  const editMaxOutInput = document.querySelector<HTMLInputElement>('input[aria-label="最大输出 Token"]')!;
  type(editCtxInput, '256000');
  type(editMaxOutInput, '24576'); await tick();
  const editFormatSelect = document.querySelector<HTMLSelectElement>('select[aria-label="接口格式"]')!;
  await act(async () => {
    editFormatSelect.value = 'openai-completions';
    editFormatSelect.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await click('保存修改');
  assert(
    !!editedPayload && (editedPayload as { id?: string; apiKey: string; apiFormat?: string }).id === 'test-gpt'
      && (editedPayload as { apiKey: string }).apiKey === ''
      && (editedPayload as { apiFormat?: string }).apiFormat === 'openai-completions'
      && (editedPayload as { contextWindow?: number }).contextWindow === 256_000
      && (editedPayload as { maxOutputTokens?: number }).maxOutputTokens === 24_576
      && !!document.querySelector('[aria-label="关闭设置"]')
      && text().includes('gpt-4.1-mini')
      && text().includes('256K/24K'),
    '编辑模型留空密钥应就地保存更新并回到设置列表，同时保留自定义上下文长度与最大输出 Token',
  );
  await click('删除模型 Claude Sonnet 4');
  assert(text().includes('GPT-4.1') && !text().includes('Claude Sonnet 4'), '删除当前模型没有回退');
  assert(modelCalls.length === 1 && modelCalls[0].kind === 'deleteModel', '删除或编辑误触发模型选择');
  await click('关闭设置');
  assert(button('GPT-4.1'), '设置与底栏没有共享选中模型');
  await click('设置'); await click('删除模型 GPT-4.1');
  assert(!!document.querySelector('.model-empty') && ![...document.querySelectorAll('.settings-content button')].some((item) => item.textContent?.trim() === '添加模型'), '零模型空态不应重复展示顶部添加按钮');
  await click('关闭设置');
  assert(!document.querySelector('.model-button') && button('添加模型'), '删到零后仍保留空模型下拉');
  checks.push('两处模型状态共享、卡片式模型列表、删除不触发选择与删到零回到无模型（脚本化边界）');

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
  assert(text().includes('工作树状态不可用') && !document.querySelector('.changes-heading input[type="checkbox"]') && button('提交').disabled && textarea.value === '打开前先写好的说明', '读取失败未隐藏顶栏全选框、未保留草稿或仍允许使用旧状态');
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
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')].find((btn) => btn.textContent?.includes('工作树'))!.click();
  });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="worktree"] button')].find((button) => button.textContent?.includes('fix/npe'))!.click();
  });
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
  let rejectNonGit = false;
  await mount(fixture('empty'), { ...reads, async execute(command) {
    if (command.kind === 'openRepository' && rejectNonGit) {
      throw new Error('无法打开“plain-folder”：该目录未受 Git 管理（未找到 .git），请先执行 git init 或选择其他 Git 仓库');
    }
    return reads.execute(command);
  } });
  await click('打开本机仓库…');
  rejectNonGit = true;
  await click('real-contract-fixture');
  await click('打开本机仓库…');
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  assert(
    (document.querySelector('.operation-notice')?.textContent ?? '').includes('未受 Git 管理')
      && text().includes('real-contract-fixture'),
    '切换非 Git 目录时窗口聚焦自动刷新不应冲掉错误提示',
  );
  checks.push('打开目录取消、切换非 Git 目录报错保留与窗口聚焦不冲掉提示（协议夹具）');
  let branchCallCount: number = 0;
  const branchResponse: { finish?: (reply: GitBranchReply) => void; command?: Extract<WorkbenchCommand, { kind: 'switchBranch' | 'checkoutRemoteBranch' }> } = {};
  const branches: WorkbenchBackend = { ...reads, execute(command) {
    if (command.kind !== 'switchBranch' && command.kind !== 'checkoutRemoteBranch') return reads.execute(command);
    branchCallCount++; branchResponse.command = command;
    return new Promise<GitBranchReply>((resolve) => { branchResponse.finish = resolve; });
  } };
  await mount(fixture('empty'), branches); await click('打开本机仓库…'); await click('手动编写');
  textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  type(textarea, '分支操作时的人工草稿'); await tick();
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click(); document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  assert(!text().includes('新建分支') && !text().includes('新建 Worktree'), '分支菜单不应包含新建分支或新建 Worktree 入口');
  await act(async () => {
    const targetBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((btn) => btn.textContent?.includes('feat/invoice'))!;
    targetBtn.click(); targetBtn.click();
  });
  assert(branchCallCount === 1 && branchResponse.command?.kind === 'switchBranch' && branchResponse.command.branch === 'feat/invoice' && branchResponse.command.branchToken, '切换分支重复请求或没有携带 Rust 版本');
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/pay'), 'Git 未返回就伪造分支切换成功');
  type(textarea, '等待 Git 时继续写的新草稿'); await tick();
  const branchSnapshot = repositoryReply(branchResponse.command!.requestId, ++readSequence).snapshot!;
  branchSnapshot.worktrees[0].branch = 'feat/invoice'; branchSnapshot.worktrees[0].branchToken = 'updated-branch-token';
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: branchResponse.command!.requestId, target: branchResponse.command!.target, action: 'switchBranch', outcome: 'completed', snapshot: branchSnapshot }); });
  assert(!document.querySelector('.branch-menu') && document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/invoice'), '成功后未采用 Git 分支或未关闭菜单');
  assert(textarea.value === '等待 Git 时继续写的新草稿' && !document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.checked, '分支切换回执覆盖了最新草稿或选择');
  // Expanding remote branches and checking out a remote branch without a local counterpart:
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')].find((btn) => btn.textContent?.includes('远程分支'))!.click();
  });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="remote"] button')].find((btn) => btn.textContent?.includes('mirror/feat/reconcile'))!.click();
  });
  const remoteCmd = branchResponse.command as Extract<WorkbenchCommand, { kind: 'switchBranch' | 'checkoutRemoteBranch' }> | undefined;
  assert((branchCallCount as number) === 2 && remoteCmd?.kind === 'checkoutRemoteBranch' && remoteCmd.branch === 'mirror/feat/reconcile', '点击远程分支未发起受控检出请求');
  const remoteCheckoutSnapshot = repositoryReply(remoteCmd.requestId, ++readSequence).snapshot!;
  remoteCheckoutSnapshot.worktrees[0].branch = 'feat/reconcile';
  remoteCheckoutSnapshot.worktrees[0].branchToken = 'remote-checkout-token';
  remoteCheckoutSnapshot.repository.branches.push({ name: 'feat/reconcile', worktreeId: 'pay-tree' });
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: remoteCmd.requestId, target: remoteCmd.target, action: 'checkoutRemoteBranch', outcome: 'completed', snapshot: remoteCheckoutSnapshot }); });
  assert(!document.querySelector('.branch-menu') && document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/reconcile'), '远程分支检出后未更新为本地跟踪分支');
  // Clicking a branch occupied by another worktree jumps directly to that worktree:
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((btn) => btn.textContent?.includes('fix/npe'))!.click();
  });
  assert((branchCallCount as number) === 2 && document.querySelector('.branch-anchor > button')?.textContent?.includes('fix/npe'), '点击已占用分支未跳转到对应工作树');
  checks.push('切换已有本地分支、远程分支检出本地跟踪分支、草稿选择保全与已占用工作树跳转（协议夹具）');

  branchCallCount = 0;
  await mount(fixture('empty'), branches); await click('打开本机仓库…');
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((btn) => btn.textContent?.includes('feat/invoice'))!.click();
  });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((btn) => btn.textContent?.includes('fix/npe'))!.click();
  });
  const lateBranchSnapshot = repositoryReply(branchResponse.command!.requestId, ++readSequence).snapshot!;
  lateBranchSnapshot.worktrees[0].branch = 'feat/invoice';
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: branchResponse.command!.requestId, target: branchResponse.command!.target, action: 'switchBranch', outcome: 'completed', snapshot: lateBranchSnapshot }); });
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('fix/npe'), '分支切换晚回执切走了用户当前视图');
  checks.push('切换分支期间切换工作树视图，晚回执只更新原工作树而不抢回界面（协议夹具）');

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

  const openCalls: Extract<WorkbenchCommand, { kind: 'openFile' }>[] = [];
  let failOpen = false;
  const ideState = fixture('review-during-question');
  await mount(ideState, {
    ...disconnectedBackend,
    async execute(command) {
      if (command.kind !== 'openFile') return disconnectedBackend.execute(command);
      openCalls.push(command);
      if (failOpen) throw new Error('未找到 Cursor 可执行入口（cursor）。当前草稿与选择已保留。');
      const reply: GitOpenFileReply = {
        protocolVersion: 1, kind: 'openFile', requestId: command.requestId, target: command.target,
        editor: command.editor, filePath: command.line ? 'src/api/pay.ts' : 'src/service/Order.ts',
        action: command.line ? 'goto' : 'stagedDiff', ...(command.line ? { line: command.line } : {}),
      };
      return reply;
    },
  });
  await act(async () => {
    document.querySelector('input[aria-label="选择 src/service/Order.ts"]')!.closest('.file-row')!.querySelector<HTMLButtonElement>('.file-open')!.click();
  });
  assert(openCalls.length === 1 && openCalls[0].editor === 'idea' && !openCalls[0].line && text().includes('已在 IDEA 打开已暂存 Diff：Order.ts'), '文件行点击未触发已暂存 Diff');
  await act(async () => {
    document.querySelector<HTMLButtonElement>('#review-tab')!.click();
  });
  await act(async () => {
    document.querySelector<HTMLButtonElement>('.review-finding')!.click();
  });
  assert(openCalls.at(1)?.line === 46 && text().includes('已在 IDEA 定位到 pay.ts:46'), '审查意见点击未定位行号');
  await click('设置'); await click('Cursor'); await click('关闭设置');
  failOpen = true;
  await act(async () => {
    document.querySelector('input[aria-label="选择 src/service/Order.ts"]')!.closest('.file-row')!.querySelector<HTMLButtonElement>('.file-open')!.click();
  });
  assert(openCalls.at(2)?.editor === 'cursor' && text().includes('未找到 Cursor 可执行入口'), '切换编辑器或 CLI 缺失报错未生效');
  checks.push('外部编辑器文件已暂存 Diff、审查行号定位、编辑器切换与 CLI 缺失提示（协议夹具）');

  const savedRequests: WorkbenchSaveRequest[] = [];
  const restoredSnapshot = repositoryReply('restore-1', 1).snapshot!;
  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 1, editor: 'vscode', activeWorktreeId: 'pay-tree', emptyDraft: '', emptyEditing: false,
            snapshots: [restoredSnapshot], warning: null,
            worktrees: [{
              repositoryId: 'orders', worktreeId: 'pay-tree', draftText: '重启后恢复的说明', draftEditing: true,
              draftOutdated: false, selectedIds: ['order'], scopeVersion: 'stale-before-restart', tab: 'commit',
            }],
          };
        },
        async saveWorkbenchState(request) { savedRequests.push(request); },
      },
    }));
  });
  await tick();
  textarea = document.querySelector<HTMLTextAreaElement>('textarea')!;
  assert(textarea.value === '重启后恢复的说明' && text().includes('变更已更新，已保留当前草稿'), '跨重启恢复未还原草稿或未提示外部变化');
  assert(document.querySelector<HTMLInputElement>('[aria-label="选择 src/service/Order.ts"]')!.checked && !document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.checked, '跨重启恢复未还原保存的文件勾选');
  assert(savedRequests.length === 0, '未修改状态下不应在启动水合时重复落盘');
  await click('设置');
  assert(document.querySelector<HTMLButtonElement>('.editor-grid button[aria-pressed="true"]')?.textContent?.includes('VS Code'), '跨重启未还原首选编辑器');
  await click('WebStorm'); await click('关闭设置');
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click(); });
  assert(savedRequests.length >= 2 && savedRequests.at(-1)?.editor === 'webstorm' && savedRequests.at(-1)?.worktrees[0].selectedIds.includes('pay'), '修改编辑器与文件勾选未触发持久化保存');
  checks.push('跨重启恢复编辑器、草稿、勾选、离线变化提示与变更自动保存（协议夹具）');

  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 1, editor: 'cursor', installedEditors: ['idea', 'rustrover', 'webstorm'],
            activeWorktreeId: null, emptyDraft: '', emptyEditing: false, snapshots: [], worktrees: [], warning: null,
          };
        },
      },
    }));
  });
  await tick();
  await click('设置');
  const gridText = document.querySelector('.editor-grid')?.textContent ?? '';
  assert(
    gridText.includes('IntelliJ IDEA') && gridText.includes('RustRover') && gridText.includes('WebStorm')
      && !gridText.includes('VS Code') && !gridText.includes('Cursor')
      && document.querySelector<HTMLButtonElement>('.editor-grid button[aria-pressed="true"]')?.textContent?.includes('IntelliJ IDEA'),
    '本机已安装编辑器过滤未隐藏未安装项或未自动回退已卸载的首选项',
  );
  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 1, editor: 'idea', installedEditors: [],
            activeWorktreeId: null, emptyDraft: '', emptyEditing: false, snapshots: [], worktrees: [], warning: null,
          };
        },
      },
    }));
  });
  await tick();
  await click('设置');
  assert(!document.querySelector('.editor-grid') && text().includes('未检测到已安装的受支持编辑器'), '零已安装编辑器时未展示明确空态');
  checks.push('本机已安装 IDE 动态识别过滤、未安装项隐藏与零编辑器空态（协议夹具）');

  const cancelledIds: string[] = [];
  let finishGen: ((reply: GenerateCommitReply) => void) | undefined;
  const m3State = fixture('typing');
  m3State.worktrees[0].draft = { text: '', editing: false, outdated: false, revision: 0 };
  m3State.worktrees[0].files = m3State.worktrees[0].files.map((file) => ({ ...file, commitToken: `tok-${file.id}` }));
  await mount(m3State, {
    ...disconnectedBackend,
    async testModel(input) {
      throw new Error(`上游鉴权错误：Bearer ${input.apiKey} 无效`);
    },
    async cancelRequest(requestId) { cancelledIds.push(requestId); },
    execute(command) {
      if (command.kind === 'generate') {
        assert(command.selections?.length === command.fileIds.length && command.selections[0].commitToken === 'tok-order', '生成说明未传递每文件 commitToken');
        return new Promise<GenerateCommitReply>((resolve) => { finishGen = resolve; });
      }
      if (command.kind === 'review') {
        return new Promise<ReviewScopeReply>(() => {});
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('设置'); await click('添加模型');
  const keyField = document.querySelector<HTMLInputElement>('.model-form input[type="password"]')!;
  type(keyField, 'sk-form-secret-5566'); await tick();
  await click('测试连接');
  assert(text().includes('[REDACTED]') && !text().includes('sk-form-secret-5566'), '模型表单报错未脱敏 API Key');
  await click('取消'); await click('关闭设置');

  await click('生成提交说明');
  assert(text().includes('正在生成提交说明') && !!document.querySelector('textarea'), '空态点击生成说明未进入可编辑与可取消状态');
  await click('取消');
  assert(cancelledIds.length === 1 && !text().includes('正在生成提交说明'), '取消生成未触发后端取消或未清理忙碌态');

  await click('生成提交说明');
  const activeGenTarget = targetOf(m3State.worktrees[0]);
  await act(async () => {
    finishGen!({
      protocolVersion: 1, kind: 'generate', requestId: 'ignored', target: activeGenTarget,
      modelId: 'test-claude', text: 'feat(pay): 自动生成幂等锁说明', skippedPaths: [],
    });
  });
  // Request ID mismatch in reply must not overwrite; now run clean generation:
  assert(!document.querySelector<HTMLTextAreaElement>('textarea')?.value, 'requestId 不匹配的生成回执不应写入草稿');

  type ModelTaskCmd = Extract<WorkbenchCommand, { fileIds: string[]; modelId: string }>;
  let capturedGenCmd: ModelTaskCmd | undefined;
  let capturedRevCmd: ModelTaskCmd | undefined;
  await mount(m3State, {
    ...disconnectedBackend,
    async cancelRequest(requestId) { cancelledIds.push(requestId); },
    async execute(command) {
      if (command.kind === 'generate') {
        capturedGenCmd = command;
        return {
          protocolVersion: 1, kind: 'generate', requestId: command.requestId, target: command.target,
          modelId: command.modelId, text: 'feat(order): 增加支付回调幂等锁与退避重试\n\n- 沿用 Order.ts 已暂存改动\n- 补充指数退避重试与验签错误返回', skippedPaths: [],
        };
      }
      if (command.kind === 'review') {
        capturedRevCmd = command;
        return new Promise<ReviewScopeReply>(() => {});
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('生成提交说明');
  assert(
    capturedGenCmd?.modelId === 'test-claude'
      && document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'feat(order): 增加支付回调幂等锁与退避重试\n\n- 沿用 Order.ts 已暂存改动\n- 补充指数退避重试与验签错误返回',
    '生成提交说明未完整填入标题与正文要点',
  );
  await act(async () => { document.querySelector<HTMLButtonElement>('#review-tab')!.click(); });
  await click('开始审查已选范围');
  // While review is running, manual scope confirmation dropdown must NOT be blocked:
  assert(!document.querySelector<HTMLButtonElement>('button[aria-label="确认提交范围或手动提交"]')!.disabled, '只读审查运行中不应阻塞手动提交入口');
  await click('取消审查');
  assert(cancelledIds.includes(capturedRevCmd!.requestId) && !!document.querySelector('.review-empty'), '取消审查未恢复待审查状态');
  checks.push('M3 生成说明、范围凭证传递、表单密钥脱敏、取消生成/审查与不阻塞手动路径（协议夹具）');

  const lockOnlyState = fixture('typing');
  lockOnlyState.worktrees[0].selectedIds = ['lock'];
  lockOnlyState.worktrees[0].files = lockOnlyState.worktrees[0].files.map((file) => ({ ...file, commitToken: `tok-${file.id}` }));
  await mount(lockOnlyState, {
    ...disconnectedBackend,
    async execute(command) {
      if (command.kind === 'review') {
        return {
          protocolVersion: 1, kind: 'review', requestId: command.requestId, target: command.target,
          modelId: command.modelId, findings: [], skippedPaths: ['pnpm-lock.yaml（锁文件默认忽略审查）'], reviewedFileIds: [],
        };
      }
      return disconnectedBackend.execute(command);
    },
  });
  await act(async () => { document.querySelector<HTMLButtonElement>('#review-tab')!.click(); });
  await click('开始审查已选范围');
  assert(text().includes('所选范围均未纳入审查') && !text().includes('审查通过') && text().includes('未覆盖') && text().includes('pnpm-lock.yaml（锁文件默认忽略审查）'), '仅勾选锁文件时不应伪称审查通过');
  checks.push('M3 锁文件过滤覆盖说明与禁止伪称审查通过（协议夹具）');

  const cPushSub: { receive?: (event: WorkbenchEvent) => void } = {};
  const cPushCmds: WorkbenchCommand[] = [];
  const cPushState = fixture('typing');
  cPushState.worktrees[0].files = cPushState.worktrees[0].files.map((file) => ({ ...file, commitToken: `tok-${file.id}` }));
  let resolveInitialCPush: ((reply: Awaited<ReturnType<WorkbenchBackend['execute']>>) => void) | undefined;
  let step2Paused = true;
  await mount(cPushState, {
    ...disconnectedBackend,
    subscribe(receive) { cPushSub.receive = receive; return () => { cPushSub.receive = undefined; }; },
    async execute(command) {
      cPushCmds.push(command);
      if (command.kind === 'cPush') {
        return new Promise((resolve) => { resolveInitialCPush = resolve; });
      }
      if (command.kind === 'respond') {
        if (step2Paused) {
          step2Paused = false;
          return {
            protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
            session: {
              runId: command.runId!, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
              sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: 'test-claude',
              frozenFileIds: ['order', 'pay', 'retry', 'release'], committedFileIds: ['order', 'pay', 'retry'], skippedFileIds: [],
              sessionStatus: 'needsDecision', canRetryRemaining: true,
              timeline: [
                { state: 'done', label: '提交 1/2：a3f91c2 feat(order): 增加幂等锁' },
                { state: 'failed', label: '提交 2/2 暂停：docs: 更新发布说明' },
              ],
              qaLog: [{ questionId: command.messageId, question: '发布说明要一起提交吗？', answer: '单独提交并一起推送' }],
              commits: [{ oid: 'a3f91c2000', shortOid: 'a3f91c2', title: 'feat(order): 增加幂等锁', branch: 'feat/pay', fileIds: ['order', 'pay', 'retry'] }],
              message: {
                id: 'err-step-2', kind: 'error', title: '已完成 1/2 个提交，提交 2 暂停', scope: '提交 2/2',
                description: '提交钩子暂时拒绝。已完成提交保留，修复后可重试剩余步骤。',
                commits: [{ hash: 'a3f91c2', title: 'feat(order): 增加幂等锁' }],
                options: [{ id: 'retry-remaining', label: '重试剩余提交', recommended: true }],
              },
            },
          };
        }
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
          session: {
            runId: command.runId!, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: 'test-claude',
            frozenFileIds: ['order', 'pay', 'retry', 'release'], committedFileIds: ['order', 'pay', 'retry', 'release'], skippedFileIds: [],
            sessionStatus: 'done', canRetryRemaining: false,
            timeline: [
              { state: 'done', label: '提交 1/2：a3f91c2 feat(order): 增加幂等锁' },
              { state: 'done', label: '提交 2/2：b84d120 docs: 更新发布说明' },
            ],
            qaLog: [{ questionId: 'q-m4', question: '发布说明要一起提交吗？', answer: '单独提交并一起推送' }],
            commits: [
              { oid: 'a3f91c2000', shortOid: 'a3f91c2', title: 'feat(order): 增加幂等锁', branch: 'feat/pay', fileIds: ['order', 'pay', 'retry'] },
              { oid: 'b84d120000', shortOid: 'b84d120', title: 'docs: 更新发布说明', branch: 'feat/pay', fileIds: ['release'] },
            ],
            message: {
              id: 'done-m4', kind: 'done', title: '已完成 2 个本地拆分提交', scope: 'feat/pay',
              description: '未选锁文件与未暂存草稿保留在原处。',
              commits: [{ hash: 'a3f91c2', title: 'feat(order): 增加幂等锁' }, { hash: 'b84d120', title: 'docs: 更新发布说明' }],
            },
          },
        };
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('C-Push');
  assert(cPushCmds.length === 1 && cPushCmds[0].kind === 'cPush' && cPushCmds[0].selections?.length === 4, 'C-Push 未附带精确文件范围凭证');
  const startedCmd = cPushCmds[0];
  // Collapse dock, focus textarea, and toggle `pnpm-lock.yaml` mid-run (`C03` + `W03`):
  await click('收起消息');
  const m4Editor = document.querySelector<HTMLTextAreaElement>('textarea')!;
  m4Editor.focus();
  type(m4Editor, '后续提交草稿，运行中不被冲掉'); await tick();
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="选择 pnpm-lock.yaml"]')!.click(); });
  await act(async () => {
    resolveInitialCPush!({
      protocolVersion: 1, kind: 'cPush', requestId: startedCmd.requestId,
      session: {
        runId: startedCmd.requestId, repositoryId: startedCmd.target.repositoryId, worktreeId: startedCmd.target.worktreeId,
        sessionId: startedCmd.target.sessionId, frozenScopeVersion: startedCmd.target.scopeVersion, modelId: 'test-claude',
        frozenFileIds: ['order', 'pay', 'retry', 'release'], committedFileIds: [], skippedFileIds: [],
        sessionStatus: 'needsDecision', canRetryRemaining: false,
        timeline: [{ state: 'done', label: '已固定 4 个已选文件范围' }, { state: 'active', label: '等待决定：发布说明要一起提交吗？' }],
        qaLog: [], commits: [],
        message: { ...releaseQuestion(), id: 'q-m4' },
      },
    });
  });
  assert(document.activeElement === m4Editor && !document.querySelector('.message-expanded') && m4Editor.value === '后续提交草稿，运行中不被冲掉', 'C-Push 后台提问抢走焦点或自动展开');
  assert(button('C-Push · 待决定'), '待决定状态未同步到底栏主按钮');
  // Clicking primary button resumes existing session instead of starting a second C-Push:
  await click('C-Push · 待决定');
  assert(cPushCmds.length === 1 && !!document.querySelector('.message-expanded'), '点击恢复按钮不应重复启动第二个任务');
  await click('单独提交并一起推送');
  const firstRespond = cPushCmds[1];
  assert(firstRespond?.kind === 'respond' && firstRespond.runId === startedCmd.requestId && firstRespond.target.scopeVersion === startedCmd.target.scopeVersion, '运行中勾选改变了当前 C-Push 冻结目标');
  assert(text().includes('已完成 1/2 个提交，提交 2 暂停') && !!document.querySelector('[aria-label="会话执行步骤"]'), '部分提交暂停态或执行步骤未展示');
  await click('重试剩余提交');
  assert(text().includes('已完成 2 个本地拆分提交') && text().includes('b84d120') && !!document.querySelector('[aria-label="会话问答记录"]'), '重试剩余步骤未完成或缺少问答记录');
  assert(m4Editor.value === '后续提交草稿，运行中不被冲掉', 'C-Push 完成覆盖了运行期间新写的人工草稿');
  checks.push('M4 C-Push 范围冻结、后台提问不抢焦点、分步暂停重试与问答记录（协议夹具）');

  const m5Sub: { receive?: (event: WorkbenchEvent) => void } = {};
  const m5Cmds: WorkbenchCommand[] = [];
  let reconcileCalls = 0;
  let pullcreatesConflict = false;
  await mount(fixture('empty'), {
    ...reads,
    subscribe(receive) { m5Sub.receive = receive; return () => { m5Sub.receive = undefined; }; },
    async reconcileSession(target) {
      reconcileCalls++;
      const pushedSnap = repositoryReply(`reconcile-${reconcileCalls}`, ++readSequence).snapshot!;
      pushedSnap.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 0, behind: 0, localOnly: false };
      m5Sub.receive?.({
        type: 'cPushProgress',
        progress: {
          protocolVersion: 1,
          repositorySnapshot: pushedSnap,
          session: {
            runId: 'conflict-run-1', repositoryId: target.repositoryId, worktreeId: target.worktreeId,
            sessionId: target.sessionId, frozenScopeVersion: target.scopeVersion, modelId: '',
            frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'done', autoCollapse: true, canRetryRemaining: false,
            timeline: [{ state: 'done', label: '检测到终端已完成合并并推送至 mirror/feat/pay' }],
            qaLog: [], commits: [],
            message: { id: 'reconciled-done', kind: 'done', title: '检测到终端已完成合并并推送', description: '本地与远端已一致。' },
          },
        },
      });
    },
    async execute(command) {
      m5Cmds.push(command);
      if (command.kind === 'pull') {
        const pullSnap = repositoryReply(command.requestId, ++readSequence).snapshot!;
        if (pullcreatesConflict) {
          return {
            protocolVersion: 1, kind: 'cPush', requestId: command.requestId, repositorySnapshot: pullSnap,
            session: {
              runId: 'conflict-run-1', repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
              sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: '',
              frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
              sessionStatus: 'conflict', canRetryRemaining: false,
              timeline: [{ state: 'failed', label: '变基遇到冲突：src/service/Order.ts' }],
              qaLog: [], commits: [],
              message: {
                id: 'conflict-msg-1', kind: 'conflict', title: '同步远端改动遇到代码冲突', scope: 'mirror/feat/pay',
                description: '请选择合并方式；未授权前 Agent 不会修改冲突文件。',
                files: [{ id: 'order', label: 'src/service/Order.ts', badge: '冲突' }],
                options: [
                  { id: 'manual-merge', label: '我手动合并', recommended: true },
                  { id: 'abort-rebase', label: '中止变基' },
                ],
              },
            },
          };
        }
        pullSnap.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 2, behind: 0, localOnly: false };
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId, repositorySnapshot: pullSnap,
          session: {
            runId: command.requestId, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: '',
            frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'idle', message: null, canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
          },
        };
      }
      if (command.kind === 'commit' && command.push) {
        const localCommittedSnap = repositoryReply(command.requestId, ++readSequence).snapshot!;
        localCommittedSnap.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 3, behind: 0, localOnly: true };
        localCommittedSnap.worktrees[0].files = localCommittedSnap.worktrees[0].files.filter((f) => f.id !== 'retry');
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId, repositorySnapshot: localCommittedSnap,
          session: {
            runId: 'manual-push-run', repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: '',
            frozenFileIds: command.fileIds, committedFileIds: command.fileIds, skippedFileIds: [],
            sessionStatus: 'pushFailed', canRetryRemaining: false,
            timeline: [
              { state: 'done', label: '本地提交 c9012ab：feat(order): 提交并推送' },
              { state: 'failed', label: '推送至 mirror/feat/pay 失败' },
            ],
            qaLog: [],
            commits: [{ oid: 'c9012ab000', shortOid: 'c9012ab', title: 'feat(order): 提交并推送', branch: 'feat/pay', fileIds: command.fileIds }],
            message: {
              id: 'push-err-1', kind: 'pushFailed', title: '已在本地提交，推送失败', scope: 'mirror/feat/pay',
              description: '本地提交已保留，重连后点击重试推送仅推送已有提交。',
              commits: [{ hash: 'c9012ab', title: 'feat(order): 提交并推送' }],
              options: [{ id: 'retry-push', label: '重试推送', recommended: true }],
            },
          },
        };
      }
      if (command.kind === 'respond') {
        if (command.optionId === 'retry-push') {
          const pushedSnap = repositoryReply(command.requestId, ++readSequence).snapshot!;
          pushedSnap.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 0, behind: 0, localOnly: false };
          return {
            protocolVersion: 1, kind: 'cPush', requestId: command.requestId, repositorySnapshot: pushedSnap,
            session: {
              runId: command.runId!, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
              sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: '',
              frozenFileIds: ['order', 'retry'], committedFileIds: ['order', 'retry'], skippedFileIds: [],
              sessionStatus: 'done', canRetryRemaining: false,
              timeline: [{ state: 'done', label: '1 个提交已推送至 mirror/feat/pay' }],
              qaLog: [],
              commits: [{ oid: 'c9012ab000', shortOid: 'c9012ab', title: 'feat(order): 提交并推送', branch: 'feat/pay', fileIds: ['order', 'retry'] }],
              message: { id: 'push-done-1', kind: 'done', title: '1 个提交已推送至 mirror/feat/pay', description: '已完成远端推送。' },
            },
          };
        }
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
          session: {
            runId: command.runId!, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: '',
            frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'waitingManual', canRetryRemaining: false,
            timeline: [{ state: 'active', label: '等待你在外部 IDE 完成手动合并…' }],
            qaLog: [], commits: [],
            message: {
              id: 'waiting-manual-1', kind: 'waitingManual', title: '等待你在 IDE 完成手动合并',
              description: '解决冲突后切回 Picket 将自动核对冲突与远端状态。',
              files: [{ id: 'order', label: 'src/service/Order.ts', badge: '冲突' }],
            },
          },
        };
      }
      return reads.execute(command);
    },
  });
  await click('打开本机仓库…');
  await click('远端详情');
  assert(text().includes('尚未联网检查远端'), '离线跟踪状态应提示尚未联网检查');
  await click('拉取并变基');
  assert(text().includes('已完成拉取并变基') && text().includes('↓ 0'), '拉取并变基完成后未更新远端落后计数');
  await click('远端详情');
  assert(!text().includes('尚未联网检查远端'), '真实远端同步后不应再提示仅本地引用');
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });

  await act(async () => {
    document.querySelector<HTMLInputElement>('[aria-label="选择 src/api/pay.ts"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="选择 RELEASE_NOTES.md"]')!.click();
  });
  await click('手动编写');
  type(document.querySelector<HTMLTextAreaElement>('textarea')!, 'feat(order): 提交并推送'); await tick();
  await click('提交');
  await click('提交并推送');
  assert(text().includes('已在本地提交，推送失败') && text().includes('c9012ab') && button('重试推送'), '推送失败未展示本地提交保留提示或底栏重试入口');
  const commitCmdCountBeforeRetry = m5Cmds.filter((c) => c.kind === 'commit').length;
  await click('收起消息');
  await click('重试推送');
  assert(!!document.querySelector('.message-expanded') && m5Cmds.filter((c) => c.kind === 'commit').length === commitCmdCountBeforeRetry, '点击底栏重试推送恢复按钮未展开失败面板');
  await click('重试推送');
  assert(m5Cmds.filter((c) => c.kind === 'commit').length === commitCmdCountBeforeRetry && text().includes('1 个提交已推送至 mirror/feat/pay') && text().includes('↑ 0'), '重试推送重复执行了提交或未同步远端领先计数');

  pullcreatesConflict = true;
  await click('收起消息');
  await click('远端详情');
  await click('拉取并变基');
  assert(text().includes('同步远端改动遇到代码冲突') && !!document.querySelector('.message-expanded'), '拉取遇到冲突未展开冲突卡片');
  await click('我手动合并');
  assert(text().includes('等待你在 IDE 完成手动合并') && button('C-Push · 待决定'), '切换手动合并未进入等待状态');
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  assert(reconcileCalls >= 1 && !document.querySelector('.message-expanded') && text().includes('检测到终端已完成合并并推送'), '切回窗口未触发对账或终端已推送后未自动收起面板');
  checks.push('M5 远端拉取变基、手动提交推送失败仅重试推送、冲突转手动合并与切回自动对账收起（协议夹具）');
  return { environment: 'Chrome headless · 开发样例，不是真实 Git / 模型验收', viewport: { width: innerWidth, height: innerHeight }, checks };
}
