import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { runGitProbe, runPiProbe } from '../../desktop/foundation';

function waitForUi(check: () => boolean, label = 'state'): Promise<void> {
  return new Promise((resolve, reject) => {
    if (check()) { resolve(); return; }
    const timeout = setTimeout(() => {
      observer.disconnect();
      reject(new Error(`Workbench UI did not reach the expected state (${label}): ${document.querySelector('.operation-notice')?.textContent ?? ''}`));
    }, 5000);
    const observer = new MutationObserver(() => {
      if (check()) { clearTimeout(timeout); observer.disconnect(); resolve(); }
    });
    observer.observe(document.body, { childList: true, subtree: true, attributes: true });
  });
}

function setTextarea(el: HTMLTextAreaElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(el, value);
  el.dispatchEvent(new Event('input', { bubbles: true }));
}

function findExtraUntrackedCheckbox(): HTMLInputElement | undefined {
  return document.querySelector<HTMLInputElement>('input[aria-label="Select \u65b0 \u6587\u4ef6.ts"]')
    ?? [...document.querySelectorAll<HTMLInputElement>('.file-row .file-checkbox')].find((el) => {
      const label = el.getAttribute('aria-label') ?? '';
      return !label.includes('part.txt') && !label.includes('pnpm-lock.yaml') && !label.includes('RELEASE_NOTES.md');
    });
}

async function checkDefaultWorkbench() {
  await waitForUi(() => !!document.querySelector('.workbench'));
  const text = document.querySelector('.workbench')!.textContent ?? '';
  if (!text.includes('Open Your Git Repository')
    || !text.includes('Open Local Repository')
    || !text.includes('Configure Model')
    || !text.includes('+ Prompt')
    || text.includes('Write Manually')
    || !!document.querySelector('.prompt-textarea')
    || !!document.querySelector('.action-dropdown')
    || !!document.querySelector('.file-row')) {
    throw new Error('Packaged workbench is not the streamlined zero-model empty state');
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
  document.querySelector<HTMLButtonElement>('button[aria-label="Settings"]')!.click();
  await waitForUi(() => !!document.querySelector('.overlay-heading h1'));
  if (document.querySelector('.overlay-heading h1')!.getBoundingClientRect().left < 84) throw new Error('Settings overlaps native window controls');
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === 'Add First Provider')!.click();
  await waitForUi(() => document.querySelectorAll('.overlay-heading h1').length === 2);
  if (document.querySelectorAll('.overlay-heading h1')[1].getBoundingClientRect().left < 84) throw new Error('Provider form overlaps native window controls');
  document.querySelector<HTMLButtonElement>('button[aria-label="Close Add Provider"]')!.click();
  await waitForUi(() => document.querySelectorAll('.overlay-heading h1').length === 1);
  document.querySelector<HTMLButtonElement>('button[aria-label="Close Settings"]')!.click();
  await waitForUi(() => !document.querySelector('.overlay'));
  const wasPinned = await window.isAlwaysOnTop();
  const [initialOuterPos, initialOuterSize, pinScale] = await Promise.all([
    window.outerPosition(),
    window.outerSize(),
    window.scaleFactor(),
  ]);
  const initialLogicalHeight = initialOuterSize.height / pinScale;
  try {
    if (wasPinned) {
      await window.setAlwaysOnTop(false);
    }
    await waitForUi(
      () => !!document.querySelector('button[aria-label="Collapse to Compact Mode"]:not(:disabled)')
        && !!document.querySelector('button[aria-label="Pin Window"]:not(:disabled)')
        && !!document.querySelector('button[aria-label="Settings"]:not(:disabled)'),
      'header-permanent-buttons-ready',
    );
    // 1. Pin in Full Mode toggles always-on-top only (never auto-collapses):
    document.querySelector<HTMLButtonElement>('button[aria-label="Pin Window"]')!.click();
    await waitForUi(
      () => !document.querySelector('.compact-bar') && !!document.querySelector('button[aria-label="Unpin Window"]:not(:disabled)'),
      'pin-stayed-full',
    );
    const [pinnedInFull, pinnedFullSize] = await Promise.all([window.isAlwaysOnTop(), window.outerSize()]);
    if (!pinnedInFull || Math.abs(pinnedFullSize.height / pinScale - initialLogicalHeight) > 2) {
      throw new Error(`Pin in Full Mode did not keep full window height (pinned=${pinnedInFull}, h=${pinnedFullSize.height / pinScale})`);
    }

    // 2. Collapse button enters ~86px Compact Mode while keeping pinned=true and top-left anchored:
    document.querySelector<HTMLButtonElement>('button[aria-label="Collapse to Compact Mode"]')!.click();
    await waitForUi(
      () => !!document.querySelector('.compact-bar')
        && !!document.querySelector('button[aria-label="Expand Workbench"]:not(:disabled)')
        && !!document.querySelector('button[aria-label="Unpin Window"]:not(:disabled)')
        && !!document.querySelector('button[aria-label="Settings"]:not(:disabled)'),
      'collapse-entered-compact',
    );
    const [compactPinned, compactOuterPos, compactOuterSize] = await Promise.all([
      window.isAlwaysOnTop(),
      window.outerPosition(),
      window.outerSize(),
    ]);
    const compactLogicalHeight = compactOuterSize.height / pinScale;
    if (
      !compactPinned
      || compactLogicalHeight < 80
      || compactLogicalHeight > 112
      || Math.abs(compactOuterPos.x - initialOuterPos.x) > 2
      || Math.abs(compactOuterPos.y - initialOuterPos.y) > 2
    ) {
      throw new Error(
        `Collapse -> Compact did not shrink height with fixed top-left (pinned=${compactPinned}, h=${compactLogicalHeight}, pos=${JSON.stringify(compactOuterPos)}, initialPos=${JSON.stringify(initialOuterPos)})`,
      );
    }

    // 3. Unpin in Compact Mode toggles always-on-top off while staying in Compact Mode:
    document.querySelector<HTMLButtonElement>('button[aria-label="Unpin Window"]')!.click();
    await waitForUi(
      () => !!document.querySelector('.compact-bar') && !!document.querySelector('button[aria-label="Pin Window"]:not(:disabled)'),
      'unpin-stayed-compact',
    );
    const [unpinnedInCompact, unpinnedCompactSize] = await Promise.all([window.isAlwaysOnTop(), window.outerSize()]);
    if (unpinnedInCompact || unpinnedCompactSize.height / pinScale > 112) {
      throw new Error(`Unpin in Compact Mode did not stay compact (pinned=${unpinnedInCompact}, h=${unpinnedCompactSize.height / pinScale})`);
    }

    // 4. Permanent Settings button in Compact Mode enlarges directly to standalone Settings (preserving unpinned, never flashing workbench); closing Settings restores compact geometry:
    let sawWorkbenchInEmptyCompactSettings = false;
    const emptyCompactSettingsObserver = new MutationObserver(() => {
      if (document.querySelector('.workbench-content, .changes-panel, .lower-workspace, .action-footer')) {
        sawWorkbenchInEmptyCompactSettings = true;
      }
    });
    emptyCompactSettingsObserver.observe(document.body, { childList: true, subtree: true });
    try {
      document.querySelector<HTMLButtonElement>('button[aria-label="Settings"]')!.click();
      await waitForUi(
        () => !document.querySelector('.compact-bar')
          && !document.querySelector('.workbench-content')
          && document.querySelector('main')?.getAttribute('data-surface') === 'standaloneSettings'
          && !!document.querySelector('button[aria-label="Close Settings"]:not(:disabled)'),
        'compact-settings-expanded',
      );
      const [settingsPinned, expandedOuterPos, expandedOuterSize] = await Promise.all([
        window.isAlwaysOnTop(),
        window.outerPosition(),
        window.outerSize(),
      ]);
      const expandedLogicalHeight = expandedOuterSize.height / pinScale;
      if (
        settingsPinned
        || Math.abs(expandedLogicalHeight - initialLogicalHeight) > 2
        || Math.abs(expandedOuterPos.x - initialOuterPos.x) > 2
        || Math.abs(expandedOuterPos.y - initialOuterPos.y) > 2
      ) {
        throw new Error(
          `Settings from Compact did not restore full height with fixed top-left (pinned=${settingsPinned}, h=${expandedLogicalHeight}, pos=${JSON.stringify(expandedOuterPos)})`,
        );
      }
      document.querySelector<HTMLButtonElement>('button[aria-label="Close Settings"]')!.click();
      await waitForUi(
        () => !document.querySelector('.overlay')
          && !!document.querySelector('.compact-bar')
          && !!document.querySelector('button[aria-label="Expand Workbench"]:not(:disabled)'),
        'compact-settings-closed-back-to-compact',
      );
      const returnedCompactSize = await window.outerSize();
      if (sawWorkbenchInEmptyCompactSettings) {
        throw new Error('Compact -> Settings -> Compact flashed full workbench content');
      }
      if (returnedCompactSize.height / pinScale > 112) {
        throw new Error(`Closing Settings opened from Compact did not return to Compact Mode (h=${returnedCompactSize.height / pinScale})`);
      }
    } finally {
      emptyCompactSettingsObserver.disconnect();
    }
    // Expand back to Full Mode for the remaining smoke checks:
    document.querySelector<HTMLButtonElement>('button[aria-label="Expand Workbench"]')!.click();
    await waitForUi(
      () => !document.querySelector('.compact-bar') && !!document.querySelector('button[aria-label="Collapse to Compact Mode"]:not(:disabled)'),
      'expand-back-to-full-for-smoke',
    );
  } finally {
    await window.setAlwaysOnTop(wasPinned);
  }
}

async function checkReadOnlyRepository() {
  // The native --foundation-ui-smoke mode selects a real disposable repository;
  // ordinary launches always use the actual macOS directory picker.
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === 'Open Local Repository…')!.click();
  await waitForUi(() => !!document.querySelector('input[aria-label="Select part.txt"]'));
  const partial = document.querySelector('input[aria-label="Select part.txt"]')!.closest('.file-row')!;
  if (!partial.textContent?.includes('Partial staged') || !partial.textContent.includes('+1') || !partial.textContent.includes('-1')) {
    throw new Error('Real Git partial stats did not reach the workbench');
  }
  const extra = findExtraUntrackedCheckbox()!;
  if (!extra) throw new Error('Missing untracked .ts file checkbox in smoke repository');
  if (!document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]')!.checked || !extra.checked) {
    throw new Error('All normal changes including lockfiles and untracked files must start selected by default');
  }
  const selectAllBox = document.querySelector<HTMLInputElement>('.changes-heading input[type="checkbox"]');
  if (!selectAllBox || selectAllBox.getAttribute('aria-label') !== 'Deselect all changes' || !selectAllBox.checked || selectAllBox.indeterminate) {
    throw new Error('Expected checked tri-state select-all checkbox in ChangesPanel when all normal changes start selected');
  }
  selectAllBox.click();
  await waitForUi(() => !selectAllBox.checked && !selectAllBox.indeterminate && !extra.checked && selectAllBox.getAttribute('aria-label') === 'Select all changes', 'select-all-cleared');
  document.querySelector<HTMLInputElement>('input[aria-label="Select part.txt"]')!.click();
  await waitForUi(() => document.querySelector<HTMLInputElement>('input[aria-label="Select part.txt"]')!.checked && selectAllBox.indeterminate, 'reselect-part-txt');
  const promptToggle = [...document.querySelectorAll<HTMLButtonElement>('button')].find((btn) => btn.textContent?.trim() === '+ Prompt');
  if (!promptToggle) throw new Error('Missing + Prompt toggle button');
  promptToggle.click();
  await waitForUi(() => !!document.querySelector('.prompt-textarea'), 'expand-prompt-box');
  const promptInput = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  setTextarea(promptInput, 'read-proof: keep extra instruction');
  promptInput.blur();
  document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.click();
  await waitForUi(() => document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.disabled);
  await waitForUi(() => !document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.disabled);
  if (document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value !== 'read-proof: keep extra instruction' || extra.checked) {
    throw new Error('Refresh replaced prompt or selection');
  }
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'));
  [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((button) => button.textContent?.includes('feat/linked'))!.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/linked') === true);
  if (document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'read-proof: keep extra instruction') {
    throw new Error('Prompt crossed worktrees');
  }
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'));
  [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((button) => button.textContent?.includes('feat/read-proof'))!.click();
  await waitForUi(() => document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'read-proof: keep extra instruction');
  await waitForUi(() => !document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.disabled, 'read-switch-refresh-settled');
  const partRow = document.querySelector('input[aria-label="Select part.txt"]')!.closest<HTMLElement>('.file-row')!;
  partRow.querySelector<HTMLButtonElement>('.file-open')!.click();
  await waitForUi(() => partRow.classList.contains('is-selected'), 'part-row-selected-on-single-click');
  if ((document.querySelector('.operation-notice')?.textContent ?? '').includes('Opened staged diff in IDEA: part.txt')) {
    throw new Error('Single-clicking file row must select row without opening external IDE');
  }
  partRow.querySelector<HTMLButtonElement>('.file-diff-link')!.click();
  await waitForUi(() => (document.querySelector('.operation-notice')?.textContent ?? '').includes('Opened staged diff in IDEA: part.txt'), 'ide-staged-diff-notice');

  // Verify fixed-bottom Local Keep and end-to-end pointer drag into/out of Local Keep in real WKWebView:
  const lockRowBefore = document.querySelector<HTMLElement>('.normal-changes-zone .file-row[data-file-path="pnpm-lock.yaml"]')!;
  const keepHeader = document.querySelector<HTMLButtonElement>('.local-keep-header')!;
  const lockRect = lockRowBefore.getBoundingClientRect();
  const keepRect = keepHeader.getBoundingClientRect();
  lockRowBefore.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 71, clientX: lockRect.left + 24, clientY: lockRect.top + 10 }));
  window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 71, clientX: keepRect.left + 40, clientY: keepRect.top + 12 }));
  window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 71, clientX: keepRect.left + 40, clientY: keepRect.top + 12 }));
  await waitForUi(
    () => document.querySelector('.local-keep-count')?.textContent?.trim() === '1'
      && !document.querySelector('.normal-changes-zone .file-row[data-file-path="pnpm-lock.yaml"]'),
    'wkwebview-drag-into-local-keep',
  );
  await new Promise((resolve) => setTimeout(resolve, 60));
  if (document.querySelector('.local-keep-count')?.textContent?.trim() !== '1') {
    throw new Error('Local Keep count reverted after Rust set_repository_local_keep settled');
  }
  keepHeader.click();
  await waitForUi(() => !!document.querySelector('.local-keep-list .file-row[data-file-path="pnpm-lock.yaml"]'), 'wkwebview-local-keep-expanded');
  const keptLockRow = document.querySelector<HTMLElement>('.local-keep-list .file-row[data-file-path="pnpm-lock.yaml"]')!;
  const normalZoneRect = document.querySelector<HTMLElement>('.changes-list')!.getBoundingClientRect();
  const keptLockRect = keptLockRow.getBoundingClientRect();
  keptLockRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 72, clientX: keptLockRect.left + 24, clientY: keptLockRect.top + 10 }));
  window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 72, clientX: normalZoneRect.left + 40, clientY: normalZoneRect.top + 20 }));
  window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 72, clientX: normalZoneRect.left + 40, clientY: normalZoneRect.top + 20 }));
  await waitForUi(
    () => document.querySelector('.local-keep-count')?.textContent?.trim() === '0'
      && !!document.querySelector('.normal-changes-zone .file-row[data-file-path="pnpm-lock.yaml"]'),
    'wkwebview-drag-out-of-local-keep',
  );
  await new Promise((resolve) => setTimeout(resolve, 60));
  keepHeader.click();
  await waitForUi(() => keepHeader.getAttribute('aria-expanded') === 'false', 'wkwebview-local-keep-collapsed');
  const restoredLockCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]')!;
  if (restoredLockCheckbox.checked) {
    restoredLockCheckbox.click();
    await waitForUi(() => !restoredLockCheckbox.checked, 'wkwebview-restore-lock-unchecked');
  }

  await invoke('verify_repository_read_smoke');
  await invoke('verify_repository_ide_smoke');
}

async function checkBranchOperations() {
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'), 'branch-menu-open');
  const menuText = document.querySelector('.branch-menu')!.textContent ?? '';
  if (menuText.includes('New Branch') || menuText.includes('New Worktree')) {
    throw new Error('Branch menu still renders removed branch/worktree creation items');
  }
  const remoteToggle = [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')]
    .find((button) => button.textContent?.includes('Remote Branches'));
  if (!remoteToggle) throw new Error('Missing Remote Branches collapsible header');
  remoteToggle.click();
  await waitForUi(() => !!document.querySelector('[data-branch-group="remote"]'), 'remote-group-expanded');
  const remoteGroupText = document.querySelector('[data-branch-group="remote"]')!.textContent ?? '';
  if (remoteGroupText.includes('/HEAD')) {
    throw new Error(`Remote branch list leaked symbolic HEAD: ${remoteGroupText}`);
  }
  const remoteCheckoutBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="remote"] button')]
    .find((button) => button.textContent?.includes('origin/feat/remote-checkout'));
  if (!remoteCheckoutBtn) throw new Error('Missing origin/feat/remote-checkout in remote branch list');
  remoteCheckoutBtn.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/remote-checkout') === true, 'remote-branch-checked-out');
  if (document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value !== 'read-proof: keep extra instruction'
    || findExtraUntrackedCheckbox()?.checked) {
    throw new Error('Checking out remote branch lost prompt or selection');
  }

  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'), 'branch-menu-open-local');
  const targetBranchBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')]
    .find((button) => button.textContent?.includes('feat/branch-proof'));
  if (!targetBranchBtn) throw new Error('Missing existing branch feat/branch-proof in branch menu');
  targetBranchBtn.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/branch-proof') === true, 'branch-switched');
  if (document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value !== 'read-proof: keep extra instruction'
    || findExtraUntrackedCheckbox()?.checked) {
    throw new Error('Switching branch lost prompt or selection');
  }

  // Click occupied branch `feat/linked` in branch menu -> jumps to existing linked worktree:
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'), 'branch-menu-reopen');
  [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')]
    .find((button) => button.textContent?.includes('feat/linked'))!.click();
  await waitForUi(() => document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/linked') === true, 'occupied-worktree-jumped');
  if (document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'read-proof: keep extra instruction'
    || document.querySelector('input[aria-label="Select part.txt"]')) {
    throw new Error('Jumping to linked worktree leaked source changes or prompt');
  }
  document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click();
  await waitForUi(() => !!document.querySelector('.branch-menu'), 'worktree-menu-open');
  [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')]
    .find((button) => button.textContent?.includes('feat/branch-proof'))!.click();
  await waitForUi(() => document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'read-proof: keep extra instruction', 'worktree-switched-back');
  await waitForUi(() => !document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.disabled, 'worktree-switch-refresh-settled');
  await invoke('verify_repository_branch_smoke');
}

async function checkPersistenceOperations() {
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);
  findButton('Settings')!.click();
  await waitForUi(() => !!document.querySelector('[data-setting-group="editor"]'), 'persistence-settings-open');
  [...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="editor"] button')].find((item) => item.textContent?.includes('VS Code'))!.click();
  findButton('Close Settings')!.click();
  await waitForUi(() => !document.querySelector('.overlay'), 'persistence-settings-closed');
  const promptInput = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  setTextarea(promptInput, 'persisted-across-restart: extra instruction');
  const partCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="Select part.txt"]')!;
  const lockCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]')!;
  const extraCheckbox = findExtraUntrackedCheckbox()!;
  if (!partCheckbox.checked) partCheckbox.click();
  if (lockCheckbox.checked) lockCheckbox.click();
  if (extraCheckbox.checked) extraCheckbox.click();
  await waitForUi(() => partCheckbox.checked && !lockCheckbox.checked && !extraCheckbox.checked, 'persistence-single-selected');
  await new Promise((resolve) => setTimeout(resolve, 150));
  await invoke('verify_repository_persistence_smoke');
}

async function checkModelAndPreviewPlanOperations() {
  const secret = `smoke-token-${crypto.randomUUID()}`;
  const baseUrl = await invoke<string>('start_model_smoke_server');
  const setInput = (el: HTMLInputElement, value: string) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);

  // Zero-model primary footer button opens ProviderConnectionForm (mode=create) directly:
  const footerPrimary = document.querySelector<HTMLButtonElement>('.action-footer button.primary');
  if (!footerPrimary || !footerPrimary.textContent?.includes('Configure Model')) {
    throw new Error('Zero-model footer primary button did not display Configure Model');
  }
  footerPrimary.click();
  await waitForUi(() => !!document.querySelector('.model-form[data-form-mode="create-provider"]'), 'm3-provider-form-open');
  findButton('Custom Compatible')!.click();
  await waitForUi(() => !!document.querySelector('.model-form [data-field="api-key"]'), 'm3-custom-provider');
  setInput(document.querySelector<HTMLInputElement>('.model-form [data-field="connection-name"]')!, 'Smoke Gateway');
  setInput(document.querySelector<HTMLInputElement>('.model-form [data-field="base-url"]')!, baseUrl);
  setInput(document.querySelector<HTMLInputElement>('.model-form [data-field="api-key"]')!, secret);
  setInput(document.querySelector<HTMLInputElement>('.model-form [data-field="model-id"]')!, 'smoke-coder-v1');
  setInput(document.querySelector<HTMLInputElement>('.model-form [data-field="model-name"]')!, 'Smoke Coder');
  await new Promise((resolve) => setTimeout(resolve, 0));

  findButton('Test Connection')!.click();
  await waitForUi(
    () => {
      const okEl = document.querySelector('.model-form .form-result.success');
      return !!okEl && (okEl.textContent ?? '').includes('smoke-coder-v1');
    },
    'm3-test-conn-ok',
  );
  findButton('Save & Use')!.click();
  await waitForUi(() => !document.querySelector('.overlay') && !!document.querySelector('.model-button'), 'm3-model-saved');

  // Verify provider card key hint, LocalAuthentication-gated reveal on Edit Connection, and adding a second child model in real WKWebView:
  const expectedHint = `smok…${secret.slice(-4)}`;
  findButton('Settings')!.click();
  await waitForUi(
    () => (document.querySelector('.provider-card .model-key-hint')?.textContent ?? '') === expectedHint,
    'm3-settings-key-hint',
  );
  findButton('Edit connection Smoke Gateway')!.click();
  await waitForUi(() => !!findButton('Reveal saved API key'), 'm3-edit-reveal-button');
  await invoke('set_smoke_local_auth', { outcome: 'cancelled' });
  findButton('Reveal saved API key')!.click();
  await waitForUi(
    () => !document.querySelector('.revealed-api-key')
      && (document.querySelector('.model-form')?.textContent ?? '').includes('Authentication cancelled'),
    'm3-reveal-cancelled',
  );
  await invoke('set_smoke_local_auth', { outcome: 'authenticated' });
  findButton('Reveal saved API key')!.click();
  await waitForUi(
    () => document.querySelector('.revealed-api-key')?.textContent === secret
      && document.querySelector<HTMLInputElement>('.model-form input[type="password"]')?.value === '',
    'm3-reveal-authenticated',
  );
  findButton('Hide API key')!.click();
  await waitForUi(() => !document.querySelector('.revealed-api-key'), 'm3-reveal-hidden');
  findButton('Cancel')!.click();
  await waitForUi(() => !document.querySelector('.model-form'), 'm3-edit-closed');

  document.querySelector<HTMLButtonElement>('.provider-card .provider-add-model')!.click();
  await waitForUi(() => !!document.querySelector('.model-form[data-form-mode="add-model"]'), 'm3-add-child-model-open');
  const childModelIdInput = document.querySelector<HTMLInputElement>('.model-form [data-field="model-id"]')!;
  const childModelNameInput = document.querySelector<HTMLInputElement>('.model-form [data-field="model-name"]')!;
  if (document.activeElement !== childModelIdInput) {
    throw new Error('Add child model form did not focus Model ID in WKWebView');
  }
  setInput(childModelIdInput, 'smoke-coder-v2');
  setInput(childModelNameInput, 'Smoke Reviewer');
  await new Promise((resolve) => setTimeout(resolve, 0));
  findButton('Save')!.click();
  await waitForUi(
    () => !document.querySelector('.model-form')
      && document.querySelectorAll('.provider-card').length === 1
      && document.querySelectorAll('.model-row').length === 2,
    'm3-child-model-saved',
  );
  if (document.querySelector('.model-row.is-active strong')?.textContent !== 'Smoke Coder') {
    throw new Error('Adding child model unexpectedly changed activeModelId in WKWebView');
  }
  findButton('Close Settings')!.click();
  await waitForUi(() => !document.querySelector('.overlay'), 'm3-settings-closed');

  // Review selected scope (`part.txt` + `pnpm-lock.yaml`) with extra instructions attached:
  const partCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="Select part.txt"]')!;
  const lockCheckbox = document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]')!;
  if (!partCheckbox.checked) partCheckbox.click();
  if (!lockCheckbox.checked) lockCheckbox.click();
  await waitForUi(() => partCheckbox.checked && lockCheckbox.checked, 'm3-review-selection');
  document.querySelector<HTMLButtonElement>('#review-tab')!.click();
  await waitForUi(() => !!findButton('Review Selected Scope'), 'm3-review-tab');
  findButton('Review Selected Scope')!.click();
  await waitForUi(
    () => !!document.querySelector('.review-finding')
      && (document.querySelector('.review-coverage')?.textContent ?? '').includes('pnpm-lock.yaml'),
    'm3-review-completed',
  );

  // Switch back to Commit Plan, select only `part.txt`, preview plan, edit verbatim, and C-Push:
  document.querySelector<HTMLButtonElement>('#commit-tab')!.click();
  await waitForUi(() => !!document.querySelector('.prompt-textarea'), 'm3-commit-tab');
  if (lockCheckbox.checked) {
    lockCheckbox.click();
    await waitForUi(() => !lockCheckbox.checked && partCheckbox.checked, 'm3-plan-only-part');
  }
  findButton('Preview Plan')!.click();
  await waitForUi(() => !!document.querySelector('.plan-group-textarea'), 'm3-preview-plan-ready');
  if (document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value !== 'persisted-across-restart: extra instruction') {
    throw new Error('Preview Plan unexpectedly overwrote user extra instructions');
  }

  const planTextarea = document.querySelector<HTMLTextAreaElement>('.plan-group-textarea')!;
  setTextarea(planTextarea, 'feat(smoke): commit edited plan verbatim\n\n- commit only staged part');

  const cPushBtn = document.querySelector<HTMLButtonElement>('.action-footer button.primary')!;
  if (!cPushBtn.textContent?.includes('C-Push') || cPushBtn.disabled) {
    throw new Error('C-Push primary button should be enabled when a valid plan exists');
  }
  cPushBtn.click();
  await waitForUi(
    () => !!document.querySelector('.message-done')
      && (document.querySelector('.workbench')?.textContent ?? '').includes('origin/feat/branch-proof')
      && !document.querySelector('.prompt-textarea')
      && !document.querySelector('.plan-group-textarea'),
    'confirmed-plan-cpush-done',
  );

  const remainingPart = document.querySelector<HTMLInputElement>('input[aria-label="Select part.txt"]');
  const remainingExtra = findExtraUntrackedCheckbox();
  const remainingLock = document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]');
  if (!remainingPart || remainingPart.checked || remainingPart.closest('.file-row')?.textContent?.includes('Partial staged')
    || !remainingExtra || remainingExtra.checked || !remainingLock || remainingLock.checked) {
    throw new Error('Remaining unstaged draft or unselected files did not match post-C-Push expectations');
  }

  await invoke('verify_repository_commit_smoke');
  await invoke('verify_model_m3_smoke', { secret });
  await checkCPushM4Operations(secret);
}

async function checkCPushM4Operations(secret: string) {
  const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);
  await invoke('prepare_cpush_m4_smoke');
  document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.click();
  await waitForUi(() => !!document.querySelector('input[aria-label="Select RELEASE_NOTES.md"]') && !document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.disabled, 'm4-release-notes-loaded');

  const partBox = document.querySelector<HTMLInputElement>('input[aria-label="Select part.txt"]')!;
  const docBox = document.querySelector<HTMLInputElement>('input[aria-label="Select RELEASE_NOTES.md"]')!;
  const lockBox = document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]')!;
  if (!partBox.checked) partBox.click();
  if (!docBox.checked) docBox.click();
  if (lockBox.checked) lockBox.click();
  await waitForUi(() => partBox.checked && docBox.checked && !lockBox.checked, 'm4-selection-ready');

  findButton('C-Push')!.click();
  await waitForUi(() => !!document.querySelector('.message-question [data-option-id="split"]'), 'm4-question-arrived');
  document.querySelector<HTMLButtonElement>('.message-question [data-option-id="split"]')!.click();
  await waitForUi(
    () => !!document.querySelector('.message-done')
      && (document.querySelector('.workbench')?.textContent ?? '').includes('origin/feat/branch-proof')
      && document.querySelectorAll('.message-commits > div').length === 2,
    'm4-split-and-pushed',
  );
  await invoke('verify_cpush_m4_smoke', { secret });

  // M5 conflict + Agent merge + dirty-worktree preservation (`R03`, `R06`):
  await invoke('prepare_m5_conflict_smoke');
  document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.click();
  await waitForUi(() => !!document.querySelector('input[aria-label="Select RELEASE_NOTES.md"]') && !document.querySelector<HTMLButtonElement>('button[aria-label="Refresh Changes"]')!.disabled, 'm5-conflict-doc-loaded');
  const m5DocBox = document.querySelector<HTMLInputElement>('input[aria-label="Select RELEASE_NOTES.md"]')!;
  const m5LockBox = document.querySelector<HTMLInputElement>('input[aria-label="Select pnpm-lock.yaml"]')!;
  const m5ExtraBox = findExtraUntrackedCheckbox()!;
  if (!m5DocBox.checked) m5DocBox.click();
  if (m5LockBox.checked) m5LockBox.click();
  if (m5ExtraBox.checked) m5ExtraBox.click();
  await waitForUi(() => m5DocBox.checked && !m5LockBox.checked && !m5ExtraBox.checked, 'm5-conflict-selection-ready');

  findButton('C-Push')!.click();
  await waitForUi(() => !!document.querySelector('.message-conflict [data-option-id="agent-merge"]'), 'm5-conflict-detected');
  document.querySelector<HTMLButtonElement>('.message-conflict [data-option-id="agent-merge"]')!.click();
  await waitForUi(
    () => !!document.querySelector('.message-done')
      && (document.querySelector('.workbench')?.textContent ?? '').includes('origin/feat/branch-proof')
      && document.querySelectorAll('.message-commits > div').length === 1,
    'm5-conflict-merged-and-pushed',
  );
  await invoke('verify_m5_conflict_smoke');
}

// Runs only when the native harness explicitly signals after WebView loading.
export function registerUiSmoke() {
  (window as { __PICKET_UI_SMOKE__?: boolean }).__PICKET_UI_SMOKE__ = true;
  const formatError = (error: unknown) => {
    const labels = [...document.querySelectorAll('button')].map((el) => el.getAttribute('aria-label') || el.textContent?.trim()).filter(Boolean).slice(0, 12).join(', ');
    const base = error instanceof Error ? `${error.message}\n${error.stack ?? ''}` : String(error);
    return `${base} [buttons: ${labels}]`;
  };
  window.addEventListener('picket:foundation-smoke', () => {
    let closing = false;
    const fail = (error: unknown) => invoke('fail_ui_smoke', { message: formatError(error) });
    void (async () => {
      await checkDefaultWorkbench();
      await checkReadOnlyRepository();
      await checkBranchOperations();
      await checkPersistenceOperations();
      await checkModelAndPreviewPlanOperations();
      // Leave both Pin and Compact Mode active before shutdown so Launch 2 verifies cold-process pinned compact window restoration:
      await waitForUi(
        () => !!document.querySelector('button[aria-label="Pin Window"]:not(:disabled)')
          && !!document.querySelector('button[aria-label="Collapse to Compact Mode"]:not(:disabled)'),
        'pre-shutdown-pin-ready',
      );
      document.querySelector<HTMLButtonElement>('button[aria-label="Pin Window"]')!.click();
      await waitForUi(() => !!document.querySelector('button[aria-label="Unpin Window"]:not(:disabled)'), 'pre-shutdown-pinned');
      document.querySelector<HTMLButtonElement>('button[aria-label="Collapse to Compact Mode"]')!.click();
      await waitForUi(() => !!document.querySelector('.compact-bar'), 'pre-shutdown-compact-persisted');
      await new Promise((resolve) => setTimeout(resolve, 150));
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

  window.addEventListener('picket:foundation-restart-smoke', () => {
    const fail = (error: unknown) => invoke('fail_ui_smoke', { message: formatError(error) });
    void (async () => {
      const win = getCurrentWindow();
      const findButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);
      await waitForUi(
        () => !!document.querySelector('.compact-bar')
          && !!document.querySelector('button[aria-label="Expand Workbench"]:not(:disabled)')
          && (document.querySelector('.compact-branch')?.textContent ?? '').includes('feat/branch-proof'),
        'restart-compact-bar-hydrated',
      );
      const [coldPinned, coldSize, coldPos, coldScale] = await Promise.all([
        win.isAlwaysOnTop(),
        win.outerSize(),
        win.outerPosition(),
        win.scaleFactor(),
      ]);
      const coldHeight = coldSize.height / coldScale;
      if (!coldPinned || coldHeight < 80 || coldHeight > 112) {
        throw new Error(`Cold restart did not restore pinned compact window geometry (pinned=${coldPinned}, height=${coldHeight})`);
      }

      // Path 1 real WKWebView verification: Compact -> Standalone Settings -> Compact without mounting/flashing full workbench:
      let sawWorkbenchDuringCompactSettings = false;
      const compactSettingsObserver = new MutationObserver(() => {
        if (document.querySelector('.workbench-content, .changes-panel, .lower-workspace, .action-footer')) {
          sawWorkbenchDuringCompactSettings = true;
        }
      });
      compactSettingsObserver.observe(document.body, { childList: true, subtree: true });
      try {
        findButton('Settings')!.click();
        await waitForUi(
          () => document.querySelector('main')?.getAttribute('data-surface') === 'standaloneSettings'
            && !!document.querySelector('[data-setting-group="editor"]')
            && !!document.querySelector('button[aria-label="Close Settings"]:not(:disabled)')
            && !document.querySelector('.workbench-content'),
          'restart-compact-standalone-settings-open',
        );
        const settingsSize = await win.outerSize();
        if (settingsSize.height / coldScale < 680) {
          throw new Error(`Standalone Settings from Compact Mode did not enlarge window to full height (got ${settingsSize.height / coldScale})`);
        }
        findButton('Close Settings')!.click();
        await waitForUi(
          () => document.querySelector('main')?.getAttribute('data-surface') === 'compact'
            && !!document.querySelector('.compact-bar')
            && !document.querySelector('.overlay'),
          'restart-compact-standalone-settings-closed',
        );
        const [returnedSize, returnedPos] = await Promise.all([win.outerSize(), win.outerPosition()]);
        const returnedHeight = returnedSize.height / coldScale;
        const posDriftX = Math.abs(returnedPos.x - coldPos.x) / coldScale;
        const posDriftY = Math.abs(returnedPos.y - coldPos.y) / coldScale;
        if (sawWorkbenchDuringCompactSettings) {
          throw new Error('Compact -> Settings -> Compact mounted or flashed full workbench content in WKWebView');
        }
        if (returnedHeight < 80 || returnedHeight > 112 || posDriftX > 2 || posDriftY > 2) {
          throw new Error(`Closing standalone Settings did not restore compact geometry (height=${returnedHeight}, drift=${posDriftX},${posDriftY})`);
        }
      } finally {
        compactSettingsObserver.disconnect();
      }

      findButton('Expand Workbench')!.click();
      await waitForUi(() => !document.querySelector('.compact-bar') && document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/branch-proof') === true, 'restart-repo-hydrated');
      const expandedSize = await win.outerSize();
      if (expandedSize.height / coldScale < 680) {
        throw new Error(`Expanding after cold restart did not restore full window height (got ${expandedSize.height / coldScale})`);
      }
      await waitForUi(() => (document.querySelector('.model-button')?.textContent ?? '').includes('Smoke Coder'), 'restart-model-hydrated');
      await waitForUi(() => !!document.querySelector('input[aria-label="Select pnpm-lock.yaml"]'), 'restart-files-hydrated');

      findButton('Settings')!.click();
      await waitForUi(() => !!document.querySelector('[data-setting-group="editor"]'), 'restart-settings-open');
      const editorGrid = document.querySelector('[data-setting-group="editor"]')!;
      const editorGridText = editorGrid.textContent ?? '';
      if (!editorGridText.includes('RustRover')
        || !editorGrid.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')?.textContent?.includes('VS Code')) {
        throw new Error(`Cold restart did not restore saved editor or detected editors: ${editorGridText}`);
      }
      findButton('Close Settings')!.click();
      await waitForUi(() => !document.querySelector('.overlay'), 'restart-settings-closed');

      findButton('C-Push Session History')!.click();
      await waitForUi(() => (document.querySelector('.history-menu')?.textContent ?? '').includes('origin/feat/branch-proof'), 'restart-cpush-history-hydrated');

      await invoke('verify_restart_smoke');
      await invoke('close_smoke_window');
    })().catch((error: unknown) => { void fail(error); });
  }, { once: true });
}
