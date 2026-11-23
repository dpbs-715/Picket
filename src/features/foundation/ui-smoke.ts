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
  await invoke('verify_repository_read_smoke');
}

async function checkBranchOperations() {
  function click(label: string) {
    const button = [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent?.trim() === label);
    if (!button) throw new Error(`Missing branch smoke button: ${label}`);
    button.click();
  }
  function fill(label: string, branch: string) {
    const input = document.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, branch);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  }
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'));
  click('新建分支（带走当前改动）');
  await waitForUi(() => !!document.querySelector('input[aria-label="新分支名"]'));
  fill('新分支名', 'feat/branch-proof');
  await new Promise((resolve) => setTimeout(resolve, 0));
  click('切出');
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/branch-proof') === true);
  if (document.querySelector<HTMLTextAreaElement>('textarea')?.value !== 'read-proof: keep manual draft'
    || document.querySelector<HTMLInputElement>('input[aria-label="选择 新 文件.ts"]')?.checked) throw new Error('Creating branch lost draft or selection');
  document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.worktree-menu'));
  click('新建 Worktree');
  await waitForUi(() => !!document.querySelector('input[aria-label="Worktree 新分支名"]'));
  fill('Worktree 新分支名', 'feat/worktree-proof');
  await new Promise((resolve) => setTimeout(resolve, 0));
  click('创建');
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/worktree-proof') === true);
  if (document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'read-proof: keep manual draft'
    || document.querySelector('input[aria-label="选择 part.txt"]')) throw new Error('Creating worktree moved source changes or draft');
  document.querySelector<HTMLButtonElement>('.worktree-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.worktree-menu'));
  [...document.querySelectorAll<HTMLButtonElement>('.worktree-menu > button')].find((button) => button.textContent?.includes('feat/branch-proof'))!.click();
  await waitForUi(() => document.querySelector<HTMLTextAreaElement>('textarea')?.value === 'read-proof: keep manual draft');
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
