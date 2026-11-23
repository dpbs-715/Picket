import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { runGitProbe, runPiProbe } from '../../desktop/foundation';

function waitForUi(check: () => boolean, label = 'state'): Promise<void> {
  return new Promise((resolve, reject) => {
    if (check()) { resolve(); return; }
    const timeout = setTimeout(() => { observer.disconnect(); reject(new Error(`Workbench UI did not reach the expected state (${label}): ${document.querySelector('.operation-notice')?.textContent ?? ''}`)); }, 5000);
    const observer = new MutationObserver(() => {
      if (check()) { clearTimeout(timeout); observer.disconnect(); resolve(); }
    });
    observer.observe(document.body, { childList: true, subtree: true, attributes: true });
  });
}
async function checkDefaultWorkbench() {
  await waitForUi(() => !!document.querySelector('.workbench'));
  const text = document.querySelector('.workbench')!.textContent ?? '';
  if (!text.includes('打开本机仓库') || !text.includes('手动编写') || text.includes('my-order-service')) {
    throw new Error('Packaged workbench is not the real empty state');
  }
  const window = getCurrentWindow();
  const [outerSize, scaleFactor, decorated] = await Promise.all([window.outerSize(), window.scaleFactor(), window.isDecorated()]);
  const firstRow = document.querySelector<HTMLElement>('.context-top')!;
  const repositoryButton = document.querySelector<HTMLElement>('.repository-button')!;
  if (!decorated || Math.abs(outerSize.height / scaleFactor - innerHeight) > 1
    || firstRow.getBoundingClientRect().top !== 0 || firstRow.getBoundingClientRect().height !== 38
    || repositoryButton.getBoundingClientRect().left < 78 || !firstRow.hasAttribute('data-tauri-drag-region')) {
    throw new Error('Native titlebar is not overlaid or the repository overlaps window controls');
  }
  document.querySelector<HTMLButtonElement>('button[aria-label="设置"]')!.click();
  await waitForUi(() => !!document.querySelector('.overlay-heading h1'));
  if (document.querySelector('.overlay-heading h1')!.getBoundingClientRect().left < 84) throw new Error('Settings overlaps native window controls');
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '添加第一个模型')!.click();
  await waitForUi(() => document.querySelectorAll('.overlay-heading h1').length === 2);
  if (document.querySelectorAll('.overlay-heading h1')[1].getBoundingClientRect().left < 84) throw new Error('Model form overlaps native window controls');
  document.querySelector<HTMLButtonElement>('button[aria-label="关闭添加模型"]')!.click();
  await waitForUi(() => document.querySelectorAll('.overlay-heading h1').length === 1);
  document.querySelector<HTMLButtonElement>('button[aria-label="关闭设置"]')!.click();
  await waitForUi(() => !document.querySelector('.overlay'));
  const wasPinned = await window.isAlwaysOnTop();
  try {
    const initialLabel = wasPinned ? '取消置顶' : '窗口置顶';
    await waitForUi(() => !!document.querySelector(`button[aria-label="${initialLabel}"]`));
    document.querySelector<HTMLButtonElement>(`button[aria-label="${initialLabel}"]`)!.click();
    await waitForUi(() => !!document.querySelector(`button[aria-label="${wasPinned ? '窗口置顶' : '取消置顶'}"]`));
    if (await window.isAlwaysOnTop() === wasPinned) throw new Error('Window pin did not change');
    document.querySelector<HTMLButtonElement>(`button[aria-label="${wasPinned ? '窗口置顶' : '取消置顶'}"]`)!.click();
    await waitForUi(() => !!document.querySelector(`button[aria-label="${initialLabel}"]`));
    if (await window.isAlwaysOnTop() !== wasPinned) throw new Error('Window pin did not restore');
  } finally { await window.setAlwaysOnTop(wasPinned); }
}

async function checkReadOnlyRepository() {
  // The native --foundation-ui-smoke mode selects a real disposable repository;
  // ordinary launches always use the actual macOS directory picker.
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '打开本机仓库…')!.click();
  await waitForUi(() => !!document.querySelector('input[aria-label="选择 part.txt"]'));
  const partial = document.querySelector('input[aria-label="选择 part.txt"]')!.closest('.file-row')!;
  if (!partial.textContent?.includes('部分暂存') || !partial.textContent.includes('+1') || !partial.textContent.includes('-1')) throw new Error('Real Git partial stats did not reach the workbench');
  const extra = document.querySelector<HTMLInputElement>('input[aria-label="选择 新 文件.ts"]')!;
  extra.click(); await waitForUi(() => !extra.checked);
  if (document.querySelector<HTMLInputElement>('input[aria-label="选择 pnpm-lock.yaml"]')!.checked) throw new Error('Lock file should start unselected');
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '手动编写')!.click();
  await waitForUi(() => !!document.querySelector('textarea'));
  const draft = document.querySelector<HTMLTextAreaElement>('textarea')!;
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(draft, 'read-proof: keep manual draft');
  draft.dispatchEvent(new Event('input', { bubbles: true }));
  document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.click();
  await waitForUi(() => document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.disabled);
  await waitForUi(() => !document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.disabled);
  if (document.querySelector<HTMLTextAreaElement>('textarea')?.value !== 'read-proof: keep manual draft' || extra.checked) throw new Error('Refresh replaced draft or selection');
  document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.worktree-menu'));
  [...document.querySelectorAll<HTMLButtonElement>('.worktree-menu > button')].find((button) => button.textContent?.includes('feat/linked'))!.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/linked') === true);
  if (document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'read-proof: keep manual draft') throw new Error('Draft crossed worktrees');
  document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.worktree-menu'));
  [...document.querySelectorAll<HTMLButtonElement>('.worktree-menu > button')].find((button) => button.textContent?.includes('feat/read-proof'))!.click();
  await waitForUi(() => document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'read-proof: keep manual draft');
  await waitForUi(() => !document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.disabled, 'read-switch-refresh-settled');
  document.querySelector('input[aria-label="选择 part.txt"]')!.closest('.file-row')!.querySelector<HTMLButtonElement>('.file-open')!.click();
  await waitForUi(() => (document.querySelector('.operation-notice')?.textContent ?? '').includes('已在 IDEA 打开已暂存 Diff：part.txt'), 'ide-staged-diff-notice');
  await invoke('verify_repository_read_smoke');
  await invoke('verify_repository_ide_smoke');
}

async function checkBranchOperations() {
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'), 'branch-menu-open');
  const menuText = document.querySelector('.branch-menu')!.textContent ?? '';
  if (menuText.includes('新建分支') || menuText.includes('新建 Worktree')) {
    throw new Error('Branch menu still renders removed branch/worktree creation items');
  }
  const targetBranchBtn = [...document.querySelectorAll<HTMLButtonElement>('.branch-menu > button')]
    .find((button) => button.textContent?.includes('feat/branch-proof'));
  if (!targetBranchBtn) throw new Error('Missing existing branch feat/branch-proof in branch menu');
  targetBranchBtn.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/branch-proof') === true, 'branch-switched');
  if (document.querySelector<HTMLTextAreaElement>('textarea')?.value !== 'read-proof: keep manual draft'
    || document.querySelector<HTMLInputElement>('input[aria-label="选择 新 文件.ts"]')?.checked) {
    throw new Error('Switching branch lost draft or selection');
  }

  // Click occupied branch `feat/linked` in branch menu -> jumps to existing linked worktree:
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'), 'branch-menu-reopen');
  [...document.querySelectorAll<HTMLButtonElement>('.branch-menu > button')]
    .find((button) => button.textContent?.includes('feat/linked'))!.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/linked') === true, 'occupied-worktree-jumped');
  if (document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'read-proof: keep manual draft'
    || document.querySelector('input[aria-label="选择 part.txt"]')) {
    throw new Error('Jumping to linked worktree leaked source changes or draft');
  }
  document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.worktree-menu'), 'worktree-menu-open');
  [...document.querySelectorAll<HTMLButtonElement>('.worktree-menu > button')]
    .find((button) => button.textContent?.includes('feat/branch-proof'))!.click();
  await waitForUi(() => document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'read-proof: keep manual draft', 'worktree-switched-back');
  await waitForUi(() => !document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.disabled, 'worktree-switch-refresh-settled');
  await invoke('verify_repository_branch_smoke');
}

async function checkCommitOperations() {
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label);
  const click = (label: string) => {
    const button = findButton(label);
    if (!button) throw new Error(`Missing commit smoke button: ${label}`);
    button.click();
  };
  await waitForUi(() => {
    const submit = findButton('提交');
    return !!submit && !submit.disabled;
  }, 'submit-button-enabled');
  click('提交');
  await waitForUi(() => !!document.querySelector('.scope-menu'), 'scope-menu-open');
  const scopeText = document.querySelector('.scope-menu')!.textContent ?? '';
  if (!scopeText.includes('feat/branch-proof') || !scopeText.includes('仅已暂存 +1 -1') || scopeText.includes('新 文件.ts') || scopeText.includes('pnpm-lock.yaml')) {
    throw new Error(`Unexpected scope confirmation content: ${scopeText}`);
  }
  await waitForUi(() => {
    const localCommit = findButton('仅本地提交');
    return !!localCommit && !localCommit.disabled;
  }, 'local-commit-enabled');
  click('仅本地提交');
  await waitForUi(() => (document.querySelector('.operation-notice')?.textContent ?? '').includes('已在本地提交'), 'commit-completed-notice');
  if (document.querySelector('textarea') || !document.querySelector('.workbench')?.textContent?.includes('手动编写')) {
    throw new Error('Committed draft did not clear cleanly after local commit');
  }
  const remainingPart = document.querySelector<HTMLInputElement>('input[aria-label="选择 part.txt"]');
  const remainingExtra = document.querySelector<HTMLInputElement>('input[aria-label="选择 新 文件.ts"]');
  const remainingLock = document.querySelector<HTMLInputElement>('input[aria-label="选择 pnpm-lock.yaml"]');
  if (!remainingPart || remainingPart.checked || remainingPart.closest('.file-row')?.textContent?.includes('部分暂存')
    || !remainingExtra || remainingExtra.checked || !remainingLock || remainingLock.checked) {
    throw new Error('Remaining unstaged draft or unselected files did not match post-commit expectations');
  }
  await invoke('verify_repository_commit_smoke');
}

async function checkPersistenceOperations() {
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);
  findButton('设置')!.click();
  await waitForUi(() => !!document.querySelector('.overlay-heading h1'), 'persistence-settings-open');
  [...document.querySelectorAll<HTMLButtonElement>('.editor-grid button')].find((item) => item.textContent?.includes('VS Code'))!.click();
  findButton('关闭设置')!.click();
  await waitForUi(() => !document.querySelector('.overlay'), 'persistence-settings-closed');
  findButton('手动编写')!.click();
  await waitForUi(() => !!document.querySelector('textarea'), 'persistence-textarea');
  const draft = document.querySelector<HTMLTextAreaElement>('textarea')!;
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(draft, 'persisted-across-restart: keep draft');
  draft.dispatchEvent(new Event('input', { bubbles: true }));
  const lockCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="选择 pnpm-lock.yaml"]')!;
  lockCheckbox.click();
  await waitForUi(() => lockCheckbox.checked, 'persistence-lock-selected');
  await new Promise((resolve) => setTimeout(resolve, 120));
  await invoke('verify_repository_persistence_smoke');
}

async function checkModelM3Operations() {
  const secret = 'sk-ui-smoke-keychain-secret-8899';
  const baseUrl = await invoke<string>('start_model_smoke_server');
  const setInput = (el: HTMLInputElement, value: string) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);

  findButton('添加模型')!.click();
  await waitForUi(() => !!document.querySelector('.model-form'), 'm3-model-form-open');
  findButton('自定义兼容')!.click();
  await waitForUi(() => !!document.querySelector('.model-form input[type="password"]'), 'm3-custom-provider');
  const formInputs = [...document.querySelectorAll<HTMLInputElement>('.model-form input')];
  setInput(formInputs[0], 'Smoke Coder');
  setInput(formInputs[1], 'smoke-coder-v1');
  setInput(formInputs[2], baseUrl);
  setInput(formInputs[3], secret);
  await new Promise((resolve) => setTimeout(resolve, 0));

  findButton('测试连接')!.click();
  await waitForUi(() => (document.querySelector('.model-form .form-result')?.textContent ?? '').includes('连接成功'), 'm3-test-conn-ok');
  findButton('保存并使用')!.click();
  await waitForUi(() => !document.querySelector('.overlay') && !!document.querySelector('.model-button'), 'm3-model-saved');

  // Select `part.txt` alongside already-selected `pnpm-lock.yaml`, clear manual draft, and generate:
  const partCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="选择 part.txt"]')!;
  if (!partCheckbox.checked) {
    partCheckbox.click();
    await waitForUi(() => partCheckbox.checked, 'm3-part-selected');
  }
  findButton('清空提交说明')!.click();
  await waitForUi(() => !document.querySelector('textarea'), 'm3-draft-cleared');
  findButton('生成提交说明')!.click();
  await waitForUi(
    () => document.querySelector<HTMLTextAreaElement>('textarea')?.value
      === 'feat(scope): 更新已暂存配置项并同步所选范围\n\n- 提交已暂存的配置键值调整\n- 同步记录所选锁文件范围变更',
    'm3-generate-completed',
  );

  // Switch to Review tab and review selected scope (`part.txt` + `pnpm-lock.yaml`):
  document.querySelector<HTMLButtonElement>('#review-tab')!.click();
  await waitForUi(() => !!findButton('开始审查已选范围'), 'm3-review-tab');
  findButton('开始审查已选范围')!.click();
  await waitForUi(() => !!document.querySelector('.review-finding') && (document.querySelector('.review-coverage')?.textContent ?? '').includes('pnpm-lock.yaml'), 'm3-review-completed');

  await invoke('verify_model_m3_smoke', { secret });
  await checkCPushM4Operations(secret);
}

async function checkCPushM4Operations(secret: string) {
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);
  await invoke('prepare_cpush_m4_smoke');
  document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.click();
  await waitForUi(() => !!document.querySelector('input[aria-label="选择 RELEASE_NOTES.md"]') && !document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.disabled, 'm4-release-notes-loaded');

  const partBox = document.querySelector<HTMLInputElement>('input[aria-label="选择 part.txt"]')!;
  const docBox = document.querySelector<HTMLInputElement>('input[aria-label="选择 RELEASE_NOTES.md"]')!;
  const lockBox = document.querySelector<HTMLInputElement>('input[aria-label="选择 pnpm-lock.yaml"]')!;
  if (!partBox.checked) partBox.click();
  if (!docBox.checked) docBox.click();
  if (lockBox.checked) lockBox.click();
  await waitForUi(() => partBox.checked && docBox.checked && !lockBox.checked, 'm4-selection-ready');

  findButton('C-Push')!.click();
  await waitForUi(() => (document.querySelector('.workbench')?.textContent ?? '').includes('发布说明要一起提交吗？'), 'm4-question-arrived');
  findButton('单独提交并一起推送')!.click();
  await waitForUi(() => (document.querySelector('.workbench')?.textContent ?? '').includes('2 个提交已推送至 origin/feat/branch-proof'), 'm4-split-and-pushed');
  await invoke('verify_cpush_m4_smoke', { secret });

  // M5 conflict + Agent merge + dirty-worktree preservation (`R03`, `R06`):
  await invoke('prepare_m5_conflict_smoke');
  document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.click();
  await waitForUi(() => !!document.querySelector('input[aria-label="选择 RELEASE_NOTES.md"]') && !document.querySelector<HTMLButtonElement>('button[aria-label="刷新变更"]')!.disabled, 'm5-conflict-doc-loaded');
  const m5DocBox = document.querySelector<HTMLInputElement>('input[aria-label="选择 RELEASE_NOTES.md"]')!;
  const m5LockBox = document.querySelector<HTMLInputElement>('input[aria-label="选择 pnpm-lock.yaml"]')!;
  const m5ExtraBox = document.querySelector<HTMLInputElement>('input[aria-label="选择 新 文件.ts"]')!;
  if (!m5DocBox.checked) m5DocBox.click();
  if (m5LockBox.checked) m5LockBox.click();
  if (m5ExtraBox.checked) m5ExtraBox.click();
  await waitForUi(() => m5DocBox.checked && !m5LockBox.checked && !m5ExtraBox.checked, 'm5-conflict-selection-ready');

  findButton('C-Push')!.click();
  await waitForUi(() => (document.querySelector('.workbench')?.textContent ?? '').includes('同步远端改动遇到代码冲突'), 'm5-conflict-detected');
  findButton('让 Agent 合并')!.click();
  await waitForUi(() => (document.querySelector('.workbench')?.textContent ?? '').includes('1 个提交已推送至 origin/feat/branch-proof'), 'm5-conflict-merged-and-pushed');
  await invoke('verify_m5_conflict_smoke');
}

// Runs only when the native harness explicitly signals after WebView loading.
export function registerUiSmoke() {
  window.addEventListener('picket:foundation-smoke', () => {
    let closing = false;
    const fail = (error: unknown) => invoke('fail_ui_smoke', { message: String(error) });
    void (async () => {
      await checkDefaultWorkbench();
      await checkReadOnlyRepository();
      await checkBranchOperations();
      await checkCommitOperations();
      await checkPersistenceOperations();
      await checkModelM3Operations();
      await runPiProbe('ui-complete', () => {});
      await runGitProbe();
      await runPiProbe('ui-close', (event) => {
        if (event.type === 'textDelta' && !closing) {
          closing = true;
          void invoke('close_smoke_window').catch(fail);
        }
      });
    })().catch((error: unknown) => { if (!closing) void fail(error); });
  }, { once: true });
}
