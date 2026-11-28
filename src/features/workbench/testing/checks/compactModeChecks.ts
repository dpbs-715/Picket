import { act } from 'react';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchCommand } from '../../commands';
import { targetOf } from '../../state/workbench';
import type { WorkbenchEvent } from '../../state/workbench';
import { fixture, releaseQuestion } from '../fixtures';
import { assert, button, click, clickElement, createControlledWindow, text, tick, type BrowserCheckHarness } from './harness';

export async function runCompactModeChecks(harness: BrowserCheckHarness) {
  const { checks, setInstantMotion, mount } = harness;
  // 28. Pinned Compact Mode: browser-preview honesty, 2-row ~86px layout, project & file drawers with top-left anchored height adjustment, ▸ jump targets, non-auto-expand, and background notifications:
  await mount(fixture('typing'));
  await click('Pin Window');
  assert(
    !document.querySelector('.compact-bar')
      && text().includes('unavailable in browser preview'),
    'Browser preview without native window must report explicit unsupported error instead of faking Pin/resize',
  );
  await mount(fixture('compact'));
  assert(!!document.querySelector('.compact-bar') && !document.querySelector('.prompt-textarea'), 'Compact scenario must render 2-row bar without prompt textarea');
  await click('Expand Workbench');
  assert(
    !!document.querySelector('.compact-bar')
      && text().includes('unavailable in browser preview'),
    'Clicking Expand in browser preview without native window must report unsupported resize instead of pretending OS window resized',
  );


  // Native titlebar clearance & long repo/branch truncation in 400x86 2-row CompactBar:
  document.documentElement.classList.add('native-titlebar');
  try {
    const longCompact = fixture('compact');
    longCompact.repositories[0].name = 'extremely-long-enterprise-monorepo-service-name-that-must-truncate';
    longCompact.worktrees[0].branch = 'main';
    const winMock1 = createControlledWindow();
    await mount(longCompact, disconnectedBackend, winMock1.controller);
    const compactTop = document.querySelector<HTMLElement>('.compact-top')!;
    const compactRepoBtn = document.querySelector<HTMLButtonElement>('.compact-repo-button')!;
    const compactBranchBtn = document.querySelector<HTMLButtonElement>('.compact-branch-button')!;
    const compactRemoteBtn = document.querySelector<HTMLButtonElement>('.compact-remote-button')!;
    const compactDragSpacer = document.querySelector<HTMLElement>('.compact-drag-spacer')!;
    const expandBtn = button('Expand Workbench');
    const unpinBtn = button('Unpin Window');
    const compactSettingsBtn = button('Settings');
    const compactToolLabels = [...document.querySelectorAll<HTMLButtonElement>('.compact-tools .icon-button')].map(
      (item) => item.getAttribute('aria-label'),
    );
    assert(
      compactTop.getBoundingClientRect().height === 38
        && compactTop.hasAttribute('data-tauri-drag-region')
        && !compactRepoBtn.hasAttribute('data-tauri-drag-region')
        && !compactBranchBtn.hasAttribute('data-tauri-drag-region')
        && !compactRemoteBtn.hasAttribute('data-tauri-drag-region')
        && compactDragSpacer.hasAttribute('data-tauri-drag-region')
        && !expandBtn.hasAttribute('data-tauri-drag-region')
        && !unpinBtn.hasAttribute('data-tauri-drag-region')
        && !compactSettingsBtn.hasAttribute('data-tauri-drag-region')
        && compactRepoBtn.getBoundingClientRect().left >= 78
        && document.querySelector<HTMLElement>('.compact-repo')!.getBoundingClientRect().left >= 84
        && compactSettingsBtn.getBoundingClientRect().right <= 400
        && expandBtn.getBoundingClientRect().width === 26
        && unpinBtn.getBoundingClientRect().width === 26
        && compactSettingsBtn.getBoundingClientRect().width === 26
        && JSON.stringify(compactToolLabels) === JSON.stringify(['Expand Workbench', 'Unpin Window', 'Settings'])
        && !document.querySelector('[aria-label="C-Push Session History"]')
        && !document.querySelector('.prompt-textarea')
        && !document.querySelector('.compact-top .branch-tag')
        && compactRemoteBtn.textContent?.replace(/\s+/g, ' ').trim() === '\u21912 \u21931',
      'Compact Row 1 must clear native traffic lights (84px), show permanent Expand/Pin/Settings in order without overflow, hide History, and omit outer Protected badge',
    );
    const barHeight = document.querySelector<HTMLElement>('.compact-bar')!.getBoundingClientRect().height;
    assert(barHeight >= 82 && barHeight <= 94, `Expected collapsed 2-row compact bar height ~86px, got ${barHeight}`);
  } finally {
    document.documentElement.classList.remove('native-titlebar');
  }

  // Mode transitions, project/branch/remote drawers, inline file checklist drawer + tri-state Select All, non-auto-expand, and background notifications:
  const compactSub: { receive?: (event: WorkbenchEvent) => void } = {};
  const compactCommands: WorkbenchCommand[] = [];
  const winMock2 = createControlledWindow({ x: 210, y: 145 }, { width: 400, height: 700 });
  const modeState = fixture('typing');
  modeState.worktrees[0].branchToken = 'branch-token-compact';
  await mount(
    modeState,
    {
      ...disconnectedBackend,
      async execute(command) {
        compactCommands.push(command);
        if (command.kind === 'runRecovery' && command.action.kind === 'pullRebase') {
          return undefined;
        }
        return disconnectedBackend.execute(command);
      },
      subscribe(receive) {
        compactSub.receive = receive;
        return () => { compactSub.receive = undefined; };
      },
    },
    winMock2.controller,
  );
  await tick();
  const fullToolLabels = [...document.querySelectorAll<HTMLButtonElement>('.header-tools .icon-button')].map(
    (item) => item.getAttribute('aria-label'),
  );
  assert(
    JSON.stringify(fullToolLabels) === JSON.stringify(['C-Push Session History', 'Collapse to Compact Mode', 'Pin Window', 'Settings']),
    `Full mode header must always show History, Collapse, Pin, and Settings in order (got ${JSON.stringify(fullToolLabels)})`,
  );

  // Clicking Pin only toggles always-on-top while staying in Full Mode (height & position unchanged):
  await click('Pin Window');
  await tick();
  assert(
    !document.querySelector('.compact-bar')
      && winMock2.getPinned() === true
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145
      && winMock2.getSize().height === 700,
    'Clicking Pin in Full Mode must only pin the window without auto-collapsing into Compact Mode',
  );

  // Clicking Collapse enters 2-row ~86px Compact Mode while keeping pinned=true and top-left (210, 145) unchanged:
  await click('Collapse to Compact Mode');
  await tick();
  assert(
    !!document.querySelector('.compact-bar')
      && !document.querySelector('.prompt-textarea')
      && winMock2.getPinned() === true
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145
      && winMock2.getSize().height >= 82
      && winMock2.getSize().height <= 94,
    `Clicking Collapse must enter ~86px 2-row Compact Mode with top-left coordinates unchanged (got size=${JSON.stringify(winMock2.getSize())})`,
  );
  // Short repo & branch buttons must hug tightly on the left instead of stretching across Row 1:
  const repoRect = document.querySelector<HTMLElement>('.compact-repo-button')!.getBoundingClientRect();
  const branchRect = document.querySelector<HTMLElement>('.compact-branch-button')!.getBoundingClientRect();
  assert(
    branchRect.left - repoRect.right <= 18,
    `Expected compact branch button to sit adjacent to repo button, got gap=${branchRect.left - repoRect.right}`,
  );
  assert(text().includes('Selected 4/5'), `Expected Selected 4/5 trigger in Row 2, got: ${text()}`);

  // 1. Project Switcher Drawer in Row 1: expands height temporarily, switches repository, and auto-collapses back to ~86px:
  await clickElement(document.querySelector<HTMLButtonElement>('.compact-repo-button')!);
  await tick();
  assert(
    !!document.querySelector('[aria-label="Opened repositories"]')
      && text().includes('picket-cli')
      && text().includes('Open Local Repository\u2026')
      && winMock2.getSize().height > 120
      && winMock2.getSize().height <= 280
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145,
    `Opening Project Drawer in Compact Mode must grow window downward without shifting top-left (size=${JSON.stringify(winMock2.getSize())})`,
  );
  const cliProjectBtn = [...document.querySelectorAll<HTMLButtonElement>('.compact-project-item')]
    .find((item) => item.textContent?.includes('picket-cli'))!;
  await clickElement(cliProjectBtn);
  await tick();
  assert(
    !document.querySelector('.compact-drawer')
      && (document.querySelector('.compact-repo')?.textContent ?? '').includes('picket-cli')
      && winMock2.getSize().height <= 94,
    'Selecting a project in Compact Project Drawer must switch repository and auto-collapse back to ~86px',
  );
  // Switch back to my-order-service:
  await clickElement(document.querySelector<HTMLButtonElement>('.compact-repo-button')!);
  await tick();
  const ordersProjectBtn = [...document.querySelectorAll<HTMLButtonElement>('.compact-project-item')]
    .find((item) => item.textContent?.includes('my-order-service'))!;
  await clickElement(ordersProjectBtn);
  await tick();
  assert((document.querySelector('.compact-repo')?.textContent ?? '').includes('my-order-service'), 'Did not switch back to my-order-service');

  // 1b. Branches & Worktrees Drawer in Row 1: shows Protected inside drawer, switches branch/worktree, and auto-collapses back to ~86px:
  await clickElement(document.querySelector<HTMLButtonElement>('.compact-branch-button')!);
  await tick();
  assert(
    !!document.querySelector('.compact-branches-drawer')
      && document.querySelector('.compact-branches-drawer .branch-tag')?.textContent === 'Protected'
      && winMock2.getSize().height > 140
      && winMock2.getSize().height <= 280
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145,
    `Opening Branches Drawer in Compact Mode must show Protected badge inside Local Branches and anchor top-left (size=${JSON.stringify(winMock2.getSize())})`,
  );
  const compactRemoteGroupToggle = [...document.querySelectorAll<HTMLButtonElement>('.compact-branches-drawer .branch-group-toggle')]
    .find((item) => item.textContent?.includes('Remote Branches'))!;
  await clickElement(compactRemoteGroupToggle);
  await tick();
  assert(
    (document.querySelector('.compact-branches-drawer [data-branch-group="remote"]')?.textContent ?? '').includes('upstream/feat/reconcile'),
    'Compact Branches Drawer must expand Remote Branches list',
  );
  const fixBranchItem = [...document.querySelectorAll<HTMLButtonElement>('.compact-branches-drawer [data-branch-group="local"] .branch-item')]
    .find((item) => item.textContent?.includes('fix/npe'))!;
  await clickElement(fixBranchItem);
  await tick();
  assert(
    !document.querySelector('.compact-drawer')
      && (document.querySelector('.compact-branch')?.textContent ?? '') === 'fix/npe'
      && winMock2.getSize().height <= 94,
    'Selecting a branch/worktree in Compact Branches Drawer must switch context and auto-collapse back to ~86px',
  );
  // Switch back to feat/pay via Compact Branches Drawer:
  await clickElement(document.querySelector<HTMLButtonElement>('.compact-branch-button')!);
  await tick();
  const payBranchItem = [...document.querySelectorAll<HTMLButtonElement>('.compact-branches-drawer [data-branch-group="local"] .branch-item')]
    .find((item) => item.textContent?.includes('feat/pay'))!;
  await clickElement(payBranchItem);
  await tick();
  assert((document.querySelector('.compact-branch')?.textContent ?? '') === 'feat/pay', 'Did not switch back to feat/pay');

  // 1c. Remote Details Drawer in Row 1: shows tracked remote + Pull & Rebase and auto-collapses on Pull:
  await clickElement(document.querySelector<HTMLButtonElement>('.compact-remote-button')!);
  await tick();
  assert(
    !!document.querySelector('.compact-remote-drawer')
      && text().includes('Tracked Remote Branch')
      && text().includes('upstream/feat/pay')
      && text().includes('2 ahead \u00b7 1 behind')
      && winMock2.getSize().height > 120
      && winMock2.getSize().height <= 280
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145,
    `Opening Remote Drawer in Compact Mode must show tracked remote details and Pull & Rebase (size=${JSON.stringify(winMock2.getSize())})`,
  );
  await click('Pull & Rebase');
  await tick();
  assert(
    !document.querySelector('.compact-drawer')
      && compactCommands.some((command) => command.kind === 'runRecovery' && command.action.kind === 'pullRebase' && command.target.worktreeId === 'pay-tree')
      && winMock2.getSize().height <= 94,
    'Clicking Pull & Rebase in Compact Remote Drawer must trigger pull command and auto-collapse back to ~86px',
  );

  // 2. Tri-state Select All + Files Checklist Drawer in Row 2:
  const compactSelectAll = () => document.querySelector<HTMLInputElement>('.compact-selection-group > .file-checkbox')!;
  assert(compactSelectAll().indeterminate && !compactSelectAll().checked, 'Compact Row 2 Select All must reflect indeterminate 4/5 state');
  await clickElement(document.querySelector<HTMLButtonElement>('.compact-files-trigger')!);
  await tick();
  assert(
    !!document.querySelector('.compact-files-drawer')
      && winMock2.getSize().height > 160
      && winMock2.getSize().height <= 280
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145,
    `Opening Files Drawer in Compact Mode must grow height up to ~240px while keeping top-left anchored (size=${JSON.stringify(winMock2.getSize())})`,
  );
  const compactLockBox = document.querySelector<HTMLInputElement>('.compact-files-drawer input[aria-label="Select pnpm-lock.yaml"]')!;
  assert(!compactLockBox.checked, 'Unchecked lockfile in fixture should appear unchecked in Compact Files Drawer');
  await act(async () => { compactLockBox.click(); });
  await tick();
  assert(
    compactSelectAll().checked && !compactSelectAll().indeterminate && text().includes('Selected 5/5'),
    'Checking pnpm-lock.yaml inside Compact Files Drawer must update selection to 5/5',
  );
  // Toggle Row 2 Select All to clear -> disables C-Push, then click again to select all 5/5:
  await act(async () => { compactSelectAll().click(); });
  await tick();
  assert(
    !compactSelectAll().checked
      && text().includes('Selected 0/5')
      && document.querySelector<HTMLButtonElement>('.compact-bottom button.primary')?.disabled === true,
    'Deselecting all in Compact Mode must show Selected 0/5 and disable C-Push',
  );
  await act(async () => { compactSelectAll().click(); });
  await tick();
  assert(compactSelectAll().checked && text().includes('Selected 5/5'), 'Re-checking Select All in Compact Mode must select all normal files');
  // Press Escape to close Files Drawer -> shrinks back to ~86px:
  await act(async () => { window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await tick();
  assert(!document.querySelector('.compact-files-drawer') && winMock2.getSize().height <= 94, 'Escape must close Compact Files Drawer and shrink back to ~86px');

  // Incoming question / pushFailed / conflict in Compact Mode NEVER auto-expands window, updates Row 2 status, and triggers background notification:
  const activeTarget = targetOf({
    ...modeState.worktrees[0],
    selectedIds: ['order', 'pay', 'retry', 'release', 'lock'],
    scopeVersion: 'scope-1#order,pay,retry,release,lock',
  });
  await act(async () => {
    compactSub.receive?.({
      type: 'message',
      target: activeTarget,
      sessionStatus: 'needsDecision',
      message: releaseQuestion(),
    });
  });
  await tick();
  assert(
    !!document.querySelector('.compact-bar')
      && text().includes('Needs decision \u25b8')
      && winMock2.notifications.length === 1
      && winMock2.notifications[0].title.includes('Decision Needed'),
    'Incoming question in Compact Mode must update Row 2 status & notify without auto-expanding',
  );

  // Plain Expand button preserves pinned=true, top-left (210, 145), and prompt text:
  await click('Expand Workbench');
  await tick();
  const fullPromptAfterExpand = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  assert(
    !document.querySelector('.compact-bar')
      && winMock2.getPinned() === true
      && winMock2.getPos().x === 210
      && winMock2.getPos().y === 145
      && winMock2.getSize().height === 700
      && !!document.querySelector('[aria-label="Collapse to Compact Mode"]')
      && fullPromptAfterExpand.value.includes('split release notes'),
    'Expanding from Compact Mode must preserve pinned state, top-left position, and worktree prompt text',
  );

  // Unpin in Full Mode only unpins and stays in Full Mode; Collapse while unpinned enters unpinned Compact Mode:
  await click('Unpin Window');
  await tick();
  assert(!winMock2.getPinned() && !document.querySelector('.compact-bar'), 'Unpin in full mode should stay in full mode');
  await click('Collapse to Compact Mode');
  await tick();
  assert(
    !!document.querySelector('.compact-bar')
      && !winMock2.getPinned()
      && winMock2.getSize().height <= 94,
    'Collapse while unpinned must enter unpinned Compact Mode without forcing pin',
  );

  // Pin & Unpin while in Compact Mode only toggle pin without exiting Compact Mode:
  await click('Pin Window');
  await tick();
  assert(!!document.querySelector('.compact-bar') && winMock2.getPinned(), 'Pin in Compact Mode must stay in Compact Mode');
  await click('Unpin Window');
  await tick();
  assert(!!document.querySelector('.compact-bar') && !winMock2.getPinned(), 'Unpin in Compact Mode must stay in Compact Mode');

  // Path 1 (Compact -> Settings -> Compact): standalone Settings expands without EVER mounting .workbench-content / .changes-panel / .lower-workspace / .action-footer, and restores exact pre-settings compact size & position even if Settings was dragged:
  await act(async () => {
    winMock2.dragTo(260, 640);
    winMock2.resizeTo(460, 88);
  });
  await tick();
  const observedWorkbenchFlashes: string[] = [];
  const recordFlashIfPresent = (stage: string) => {
    if (document.querySelector('.workbench-content, .changes-panel, .lower-workspace, .action-footer')) {
      observedWorkbenchFlashes.push(stage);
    }
  };
  const flashObserver = new MutationObserver(() => recordFlashIfPresent('mutation'));
  flashObserver.observe(document.body, { childList: true, subtree: true });
  try {
    const releaseOpenSettings = winMock2.pauseNextApply();
    await click('Settings');
    await tick();
    // Mid-open (native resize still in flight): Settings is already mounted standalone, Close Settings is disabled until resize finishes, NO full workbench in DOM!
    recordFlashIfPresent('mid-open');
    assert(
      document.querySelector('main')?.getAttribute('data-surface') === 'standaloneSettings'
        && document.querySelector('main')?.getAttribute('data-window-transition') === 'resizing'
        && !document.querySelector('.compact-bar')
        && !document.querySelector('.workbench-content')
        && !!document.querySelector('button[aria-label="Close Settings"]:disabled'),
      'Opening Settings from Compact Mode must render standalone Settings immediately (with close guarded during resize) without mounting full workbench',
    );
    await act(async () => { releaseOpenSettings(); });
    await tick();
    recordFlashIfPresent('opened');
    assert(
      !document.querySelector('.compact-bar')
        && !document.querySelector('.workbench-content')
        && document.querySelector('main')?.getAttribute('data-window-transition') === 'idle'
        && !winMock2.getPinned()
        && winMock2.getSize().height === 700
        && !!document.querySelector('button[aria-label="Close Settings"]:not(:disabled)'),
      'Clicking Settings in Compact Mode must expand to Full Size and show standalone Settings while preserving pin state',
    );

    // Drag the Settings window elsewhere; closing must still restore the exact pre-settings compact position (260, 640) and size (460x88):
    await act(async () => {
      winMock2.dragTo(620, 90);
    });
    await tick();

    const releaseCloseSettings = winMock2.pauseNextApply();
    await click('Close Settings');
    await tick();
    // Mid-close (native shrink still in flight): standalone Settings stays mounted, NO full workbench flash!
    recordFlashIfPresent('mid-close');
    assert(
      document.querySelector('main')?.getAttribute('data-surface') === 'standaloneSettings'
        && !!document.querySelector('[aria-label="Close Settings"]')
        && !document.querySelector('.workbench-content')
        && !document.querySelector('.compact-bar'),
      'Closing Settings opened from Compact Mode must keep Settings visible during resize and never flash full workbench',
    );
    await act(async () => { releaseCloseSettings(); });
    await tick();
    recordFlashIfPresent('closed');
    assert(
      observedWorkbenchFlashes.length === 0
        && document.querySelector('main')?.getAttribute('data-surface') === 'compact'
        && !document.querySelector('.overlay')
        && !!document.querySelector('.compact-bar')
        && !winMock2.getPinned()
        && winMock2.getPos().x === 260
        && winMock2.getPos().y === 640
        && winMock2.getSize().width === 460
        && winMock2.getSize().height <= 94,
      `Closing Settings opened from Compact Mode must restore pre-settings compact size/position with zero workbench flashes (flashes=${JSON.stringify(observedWorkbenchFlashes)}, pos=${JSON.stringify(winMock2.getPos())}, size=${JSON.stringify(winMock2.getSize())})`,
    );
  } finally {
    flashObserver.disconnect();
  }

  // Path 3 + Path 2 (Compact -> Full Workbench -> Settings -> Full Workbench -> Compact):
  // Expanding to Full Workbench preserves repo, branch, selected files, and typed prompt;
  // Opening/Closing Settings from Full Workbench keeps current window size & position completely untouched:
  await click('Expand Workbench');
  await tick();
  await act(async () => {
    winMock2.dragTo(310, 120);
    winMock2.resizeTo(480, 620);
  });
  await tick();
  const applyCountBeforeFullSettings = winMock2.history.length;
  assert(
    document.querySelector('main')?.getAttribute('data-surface') === 'workbench'
      && (document.querySelector('.repository-button')?.textContent ?? '').includes('my-order-service')
      && (document.querySelector('.branch-anchor > button')?.textContent ?? '').includes('feat/pay')
      && document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value.includes('split release notes'),
    'Compact -> Full Workbench must preserve active repository, branch, and typed prompt',
  );
  await click('Settings');
  await tick();
  assert(
    winMock2.history.length === applyCountBeforeFullSettings
      && document.querySelector('main')?.getAttribute('data-surface') === 'workbench'
      && !!document.querySelector('.workbench-content')
      && !!document.querySelector('[aria-label="Close Settings"]')
      && winMock2.getPos().x === 310
      && winMock2.getPos().y === 120
      && winMock2.getSize().width === 480
      && winMock2.getSize().height === 620,
    'Opening Settings from Full Workbench must keep window size/position unchanged and overlay on workbench',
  );
  await click('Close Settings');
  await tick();
  assert(
    winMock2.history.length === applyCountBeforeFullSettings
      && !document.querySelector('.overlay')
      && !!document.querySelector('.workbench-content')
      && winMock2.getPos().x === 310
      && winMock2.getPos().y === 120
      && winMock2.getSize().width === 480
      && winMock2.getSize().height === 620
      && document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value.includes('split release notes'),
    'Closing Settings opened from Full Workbench must return directly to workbench with unchanged size, position, and prompt',
  );
  await click('Collapse to Compact Mode');
  await tick();
  assert(
    !!document.querySelector('.compact-bar')
      && (document.querySelector('.compact-repo')?.textContent ?? '').includes('my-order-service')
      && (document.querySelector('.compact-branch')?.textContent ?? '') === 'feat/pay',
    'Collapsing back to Compact Mode must preserve active repository and branch',
  );

  // Row 2 click-to-expand jump targets:
  // (a) Click "Push failed ▸ Retry" -> expands with MessageDock open
  const jumpWin = createControlledWindow();
  const pushFailedCompact = fixture('push-failed');
  pushFailedCompact.worktrees[0].dockExpanded = false;
  pushFailedCompact.windowPrefs = { ...pushFailedCompact.windowPrefs, pinned: true, compactMode: true };
  await mount(pushFailedCompact, disconnectedBackend, jumpWin.controller);
  assert(text().includes('Push failed \u25b8 Retry'), 'Compact Row 2 must display Push failed ▸ Retry');
  await clickElement(document.querySelector<HTMLButtonElement>('[data-expand-target="dock"]')!);
  await tick();
  assert(!document.querySelector('.compact-bar') && !!document.querySelector('.message-expanded'), 'Clicking Push failed ▸ must expand and open MessageDock');

  // (b) Zero-model Compact Mode -> primary button shows "Configure Model", clicking expands & opens AddModelForm:
  const zeroModelCompact = fixture('no-model');
  zeroModelCompact.windowPrefs = { ...zeroModelCompact.windowPrefs, pinned: true, compactMode: true };
  await mount(zeroModelCompact, disconnectedBackend, jumpWin.controller);
  const compactPrimaryBtn = document.querySelector<HTMLButtonElement>('.compact-bottom button.primary')!;
  assert(compactPrimaryBtn.textContent?.includes('Configure Model') && text().includes('No model'), 'Zero-model Compact Mode must show No model badge and Configure Model button');
  await clickElement(compactPrimaryBtn);
  await tick();
  assert(!document.querySelector('.compact-bar') && !!document.querySelector('.model-form'), 'Clicking Configure Model in Compact Mode must expand and open AddModelForm');

  // (c) Blocking states in Compact Mode (Detached HEAD & Read Error) disable C-Push and explain reason in Row 2:
  const detachedCompact = fixture('compact');
  detachedCompact.worktrees[0].detached = true;
  await mount(detachedCompact, disconnectedBackend, jumpWin.controller);
  assert(
    text().includes('Detached HEAD \u25b8')
      && document.querySelector<HTMLButtonElement>('.compact-bottom button.primary')?.disabled === true,
    'Detached HEAD in Compact Mode must explain reason in Row 2 and disable C-Push',
  );

  // (d) Compact Mode notice & drawer height invariants WITH real animations enabled (no clipped Row 2):
  setInstantMotion(false);
  try {
    const noticeWin = createControlledWindow({ x: 180, y: 120 }, { width: 400, height: 86 });
    const noticeCompact = fixture('compact');
    noticeCompact.worktrees[0].branchToken = 'branch-token-notice';
    noticeCompact.repositories[0].branches = [
      ...noticeCompact.repositories[0].branches,
      ...Array.from({ length: 12 }, (_, idx) => ({ name: `feat/branch-${idx + 1}` })),
    ];
    await mount(
      noticeCompact,
      {
        ...disconnectedBackend,
        async execute(command) {
          if (command.kind === 'runRecovery' && command.action.kind === 'pullRebase') {
            return {
              protocolVersion: 2,
              kind: 'action',
              requestId: command.requestId,
              session: {
                runId: 'pull-compact-1',
                repositoryId: command.target.repositoryId,
                worktreeId: command.target.worktreeId,
                sessionId: command.target.sessionId,
                actionId: 'builtin:pull-rebase',
                actionLabel: 'Pull & Rebase',
                profile: 'pullRebase',
                frozenScopeVersion: command.target.scopeVersion,
                modelId: command.modelId ?? '',
                frozenFileIds: [],
                committedFileIds: [],
                skippedFileIds: [],
                status: 'idle',
                summary: '',
                message: null,
                timeline: [],
                qaLog: [],
                commits: [],
                links: [],
                canRetryRemaining: false,
                autoCollapse: true,
              },
            };
          }
          if (command.kind === 'switchBranch') {
            throw new Error('Cannot switch branch while uncommitted changes conflict with target branch.');
          }
          return disconnectedBackend.execute(command);
        },
      },
      noticeWin.controller,
    );
    await tick();

    // 1. Trigger Pull & Rebase completion notice with real animations -> window MUST grow immediately so Row 2 is never clipped:
    await clickElement(document.querySelector<HTMLButtonElement>('.compact-remote-button')!);
    await tick();
    await click('Pull & Rebase');
    await tick();
    const compactBarEl = () => document.querySelector<HTMLElement>('.compact-bar')!;
    const compactBottomEl = () => document.querySelector<HTMLElement>('.compact-bottom')!;
    assert(
      Boolean(document.querySelector('.compact-notice'))
        && noticeWin.getSize().height >= Math.ceil(compactBarEl().scrollHeight)
        && noticeWin.getSize().height > 105
        && compactBottomEl().getBoundingClientRect().bottom <= noticeWin.getSize().height + 1,
      `Compact notice with real animations must immediately expand window to fit Row 2 (winH=${noticeWin.getSize().height}, scrollH=${compactBarEl().scrollHeight}, bottom=${compactBottomEl().getBoundingClientRect().bottom})`,
    );

    // 2. Opening Settings while a notice is visible and closing Settings after dismissal must restore ~86px (not poisoned ~117px):
    await click('Settings');
    await tick();
    await click('Close Settings');
    await tick();
    await click('Dismiss notice');
    await tick();
    assert(
      !document.querySelector('.compact-notice')
        && noticeWin.getSize().height <= 94,
      `Dismissing compact notice with real animations must immediately shrink window back to ~86px (winH=${noticeWin.getSize().height})`,
    );

    // 3. Failed branch switch keeps full Branches Drawer open AND shows error notice simultaneously -> total height must stay <= 280px with Row 2 unclipped:
    await clickElement(document.querySelector<HTMLButtonElement>('.compact-branch-button')!);
    await tick();
    const failingBranchBtn = [...document.querySelectorAll<HTMLButtonElement>('.compact-branches-drawer [data-branch-group="local"] .branch-item')]
      .find((item) => item.textContent?.includes('feat/branch-1'))!;
    await clickElement(failingBranchBtn);
    await tick();
    assert(
      Boolean(document.querySelector('.compact-branches-drawer'))
        && Boolean(document.querySelector('.compact-notice'))
        && compactBarEl().classList.contains('has-notice')
        && compactBarEl().scrollHeight <= 280
        && noticeWin.getSize().height >= Math.ceil(compactBarEl().scrollHeight)
        && compactBottomEl().getBoundingClientRect().bottom <= noticeWin.getSize().height + 1,
      `Compact notice + open full-height drawer must stay <= 280px without clipping Row 2 (winH=${noticeWin.getSize().height}, scrollH=${compactBarEl().scrollHeight}, bottom=${compactBottomEl().getBoundingClientRect().bottom})`,
    );

    // 4. Long error message inside CompactRemoteDrawer must scroll inside max-height without overflowing into Row 2:
    const longRemoteErr = fixture('compact');
    longRemoteErr.worktrees[0].remote = {
      kind: 'error',
      reason: 'fatal: unable to access remote repository: '.repeat(24),
    };
    const remoteErrWin = createControlledWindow({ x: 180, y: 120 }, { width: 400, height: 86 });
    await mount(longRemoteErr, disconnectedBackend, remoteErrWin.controller);
    await tick();
    await clickElement(document.querySelector<HTMLButtonElement>('.compact-remote-button')!);
    await tick();
    const remoteDrawerEl = document.querySelector<HTMLElement>('.compact-remote-drawer')!;
    const remoteBarEl = document.querySelector<HTMLElement>('.compact-bar')!;
    const remoteBottomEl = document.querySelector<HTMLElement>('.compact-bottom')!;
    assert(
      getComputedStyle(remoteDrawerEl).overflowY === 'auto'
        && remoteBarEl.scrollHeight <= 280
        && remoteErrWin.getSize().height >= Math.ceil(remoteBarEl.scrollHeight)
        && remoteBottomEl.getBoundingClientRect().bottom <= remoteErrWin.getSize().height + 1,
      `CompactRemoteDrawer with long error must scroll cleanly without pushing Row 2 off-screen (overflowY=${getComputedStyle(remoteDrawerEl).overflowY}, scrollH=${remoteBarEl.scrollHeight}, winH=${remoteErrWin.getSize().height})`,
    );
  } finally {
    setInstantMotion(true);
  }
  checks.push('Pinned Compact Mode: browser-preview boundary, 2-row ~86px layout, project & file drawers, ▸ jump targets, non-auto-expand, animated notice height sync, drawer+notice <=280px cap, and background notifications');
}
