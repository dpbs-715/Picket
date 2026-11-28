import { act, createElement } from 'react';
import type { TrayMainWindowAction, TrayStateSnapshot } from '../../../../../shared/tray-protocol';
import type { WorkbenchSaveRequest } from '../../../../../shared/workbench-persistence-protocol';
import type { WindowController } from '../../../../desktop/window';
import { getMessages } from '../../../../i18n/messages';
import '../../../splash/splash.css';
import { WhaleSplashStage } from '../../../splash/WhaleSplashStage';
import { TrayPopover } from '../../../tray/TrayPopover';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchCommand } from '../../commands';
import { fixture } from '../fixtures';
import { assert, click, clickElement, createControlledWindow, text, tick, type, type BrowserCheckHarness } from './harness';

export async function runTrayThemeTransitionsChecks(harness: BrowserCheckHarness) {
  const { root, checks, zh, zhLanguageLabel, setInstantMotion, waitUntil, mount } = harness;
  // Menu bar tray popover: read-only cards, urgency sort, clean-workspace auto-folding, zero search box, and clean actions:
  const trayActions: string[] = [];
  const traySnapshot: TrayStateSnapshot = {
    protocolVersion: 1,
    locale: 'zh',
    theme: 'system',
    hasUpdate: true,
    refreshing: false,
    repositories: [
      { repositoryId: 'r-clean-1', worktreeId: 'w-clean-1', name: 'docs-site', branch: 'main', detached: false, ahead: 0, behind: 0, hasUpstream: true, changeCount: 0, localKeepCount: 0, worktreeCount: 1, active: false, sessionStatus: 'idle' },
      { repositoryId: 'r-dirty', worktreeId: 'w-dirty', name: 'Picket', branch: 'feat/tray', detached: false, ahead: 2, behind: 0, hasUpstream: true, changeCount: 4, localKeepCount: 0, worktreeCount: 1, active: true, sessionStatus: 'idle' },
      { repositoryId: 'r-clean-2', worktreeId: 'w-clean-2', name: 'design-kit', branch: 'main', detached: false, ahead: 0, behind: 0, hasUpstream: true, changeCount: 0, localKeepCount: 0, worktreeCount: 1, active: false, sessionStatus: 'idle' },
      { repositoryId: 'r-urgent', worktreeId: 'w-urgent', name: 'backend-api', branch: 'feat/pay', detached: false, ahead: 0, behind: 1, hasUpstream: true, changeCount: 2, localKeepCount: 0, worktreeCount: 1, active: false, sessionStatus: 'needsDecision', statusDetail: '敏感文件确认' },
      { repositoryId: 'r-clean-3', worktreeId: 'w-clean-3', name: 'mobile-app', branch: 'main', detached: false, ahead: 0, behind: 0, hasUpstream: true, changeCount: 0, localKeepCount: 0, worktreeCount: 1, active: false, sessionStatus: 'idle' },
      { repositoryId: 'r-clean-4', worktreeId: 'w-clean-4', name: 'infra-ops', branch: 'main', detached: false, ahead: 0, behind: 0, hasUpstream: true, changeCount: 0, localKeepCount: 0, worktreeCount: 1, active: false, sessionStatus: 'idle' },
    ],
  };
  await act(async () => {
    root.render(createElement(TrayPopover, {
      snapshot: traySnapshot,
      onRefresh: () => { trayActions.push('refresh'); },
      onOpenSettings: () => { trayActions.push('settings'); },
      onOpenMainWindow: (action?: TrayMainWindowAction) => { trayActions.push(`open:${JSON.stringify(action)}`); },
      onQuit: () => { trayActions.push('quit'); },
      onDismiss: () => { trayActions.push('dismiss'); },
    }));
  });
  await tick();
  assert(!document.querySelector('input[type="search"], input[type="text"]'), 'Tray popover must not render a search box');
  assert(!!document.querySelector('.tray-update-dot'), 'Tray popover did not display update indicator dot on Settings');
  const visibleCardsBeforeFold = [...document.querySelectorAll<HTMLButtonElement>('[data-tray-repo]')].map((el) => el.dataset.trayRepo);
  assert(
    JSON.stringify(visibleCardsBeforeFold) === JSON.stringify(['r-urgent', 'r-dirty']),
    `Tray popover did not prioritize urgent/dirty repos and auto-fold clean repos: ${JSON.stringify(visibleCardsBeforeFold)}`,
  );
  const foldBtn = document.querySelector<HTMLButtonElement>('[data-tray-clean-toggle]');
  assert(foldBtn && foldBtn.textContent?.includes('4'), 'Missing folded clean workspaces toggle for 4 clean repos');
  await clickElement(foldBtn);
  const visibleCardsAfterExpand = [...document.querySelectorAll<HTMLButtonElement>('[data-tray-repo]')].map((el) => el.dataset.trayRepo);
  assert(visibleCardsAfterExpand.length === 6, `Expanding clean fold did not reveal all 6 repos (${visibleCardsAfterExpand.length})`);
  await clickElement(document.querySelector<HTMLButtonElement>('[data-tray-repo="r-urgent"]')!);
  await clickElement(document.querySelector<HTMLButtonElement>('[data-tray-open-main]')!);
  await clickElement(document.querySelector<HTMLButtonElement>('[data-tray-quit]')!);
  await act(async () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  });
  assert(
    trayActions.includes('open:{"kind":"focusWorktree","repositoryId":"r-urgent","worktreeId":"w-urgent","openDock":true}')
      && trayActions.includes('open:{"kind":"show"}')
      && trayActions.includes('quit')
      && trayActions.includes('dismiss'),
    `Tray popover actions mismatch: ${JSON.stringify(trayActions)}`,
  );
  checks.push('Menu bar tray popover: read-only priority cards, auto-folded clean group (>5 repos), zero search input, update dot, and Open Main / Quit / Escape actions');

  // Custom Action in-place follow-up steering in MessageDock (done & failed cards forward followUpPrompt + resumeFromRunId):
  {
    const followUpCommands: WorkbenchCommand[] = [];
    const followUpState = fixture('typing');
    const repoId = followUpState.worktrees[0].repositoryId;
    followUpState.repositorySettings[repoId] = {
      editor: 'idea',
      commitLanguage: 'en',
      customActions: [{
        id: 'act-release',
        label: 'Patch Release',
        prompt: 'Bump patch version, verify, commit, tag, and push.',
      }],
      activeActionId: 'act-release',
    };
    followUpState.worktrees[0].sessionStatus = 'done';
    followUpState.worktrees[0].dockExpanded = true;
    followUpState.worktrees[0].activeRun = {
      profile: 'customAction',
      runId: 'run-prev-42',
      actionId: 'act-release',
      actionLabel: 'Patch Release',
      frozenTarget: {
        repositoryId: followUpState.worktrees[0].repositoryId,
        worktreeId: followUpState.worktrees[0].worktreeId,
        sessionId: followUpState.worktrees[0].sessionId,
        scopeVersion: followUpState.worktrees[0].scopeVersion,
      },
      frozenFileIds: [],
      promptRevisionAtStart: 0,
      modelId: 'test-claude',
      timeline: [{ state: 'done', label: 'Edited package.json' }],
      qaLog: [],
      commits: [],
      links: [],
      canRetryRemaining: false,
    };
    followUpState.worktrees[0].messages = [{
      id: 'done-action-run-prev-42',
      kind: 'done',
      title: 'Patch Release completed',
      description: 'Updated version manifests; ready for next step.',
    }];
    await mount(followUpState, {
      ...disconnectedBackend,
      async execute(command) {
        followUpCommands.push(command);
        if (command.kind === 'startAction' && command.profile === 'customAction' && command.action) {
          return {
            protocolVersion: 2,
            kind: 'action',
            requestId: command.requestId,
            session: {
              runId: command.requestId,
              repositoryId: command.target.repositoryId,
              worktreeId: command.target.worktreeId,
              sessionId: command.target.sessionId,
              actionId: command.action.id,
              actionLabel: command.action.label,
              profile: 'customAction',
              frozenScopeVersion: command.target.scopeVersion,
              modelId: command.modelId,
              frozenFileIds: command.fileIds,
              committedFileIds: [],
              skippedFileIds: [],
              status: 'done',
              summary: 'Committed and pushed follow-up changes.',
              message: {
                id: `done-action-${command.requestId}`,
                kind: 'done',
                title: `${command.action.label} completed`,
                description: 'Committed and pushed follow-up changes.',
              },
              timeline: [{ state: 'done', label: 'Committed follow-up' }],
              qaLog: [],
              commits: [],
              links: [],
              canRetryRemaining: false,
            },
          };
        }
        return undefined;
      },
    });
    const followUpInput = document.querySelector<HTMLInputElement>('.message-expanded .d2-composer input')!;
    assert(
      Boolean(followUpInput) && followUpInput.placeholder.includes('Continue or adjust'),
      'Settled Custom Action card in MessageDock must render follow-up steering input',
    );
    type(followUpInput, 'Create CHANGELOG.md and push v0.2.2');
    await tick();
    await click('Send Reply');
    await tick();
    const sent = followUpCommands.find((cmd) => cmd.kind === 'startAction' && cmd.profile === 'customAction');
    assert(
      sent?.kind === 'startAction'
        && sent.followUpPrompt === 'Create CHANGELOG.md and push v0.2.2'
        && sent.resumeFromRunId === 'run-prev-42'
        && sent.action?.id === 'act-release',
      `Follow-up submission did not forward followUpPrompt and resumeFromRunId: ${JSON.stringify(sent)}`,
    );
    assert(text().includes('Committed and pushed follow-up changes.'), 'Follow-up completion message did not render in MessageDock');
    await click('Dismiss completed notice');
    assert(!document.querySelector('.message-expanded') && !document.querySelector('.message-collapsed'), 'Clicking header × must dismiss settled follow-up action card');
    checks.push('Custom Action in-place follow-up steering in MessageDock forwards followUpPrompt & resumeFromRunId and dismisses cleanly');
  }

  // Pinned MessageDock progress bar & independent auto-scrolling command timeline:
  {
    const progressState = fixture('typing');
    const repoId = progressState.worktrees[0].repositoryId;
    progressState.repositorySettings[repoId] = {
      editor: 'idea',
      commitLanguage: 'en',
      customActions: [{ id: 'act-merge', label: 'Merge test', prompt: 'Merge into test branch' }],
      activeActionId: 'act-merge',
    };
    progressState.worktrees[0].sessionStatus = 'running';
    progressState.worktrees[0].dockExpanded = true;
    progressState.worktrees[0].activeRun = {
      profile: 'customAction',
      runId: 'run-progress-scroll',
      actionId: 'act-merge',
      actionLabel: 'Merge test',
      frozenTarget: {
        repositoryId: progressState.worktrees[0].repositoryId,
        worktreeId: progressState.worktrees[0].worktreeId,
        sessionId: progressState.worktrees[0].sessionId,
        scopeVersion: progressState.worktrees[0].scopeVersion,
      },
      frozenFileIds: [],
      promptRevisionAtStart: 0,
      modelId: 'test-claude',
      timeline: [
        ...Array.from({ length: 17 }, (_, idx) => ({ state: 'done' as const, label: `Run command step ${idx + 1}` })),
        { state: 'active' as const, label: 'Run glab label' },
      ],
      qaLog: [],
      commits: [],
      links: [],
      canRetryRemaining: false,
    };
    progressState.worktrees[0].messages = [{
      id: 'progress-run-scroll',
      kind: 'progress',
      title: 'Merge test',
      description: 'Run glab label',
      progress: { completed: 60, total: 100, label: 'Run glab label' },
    }];
    await mount(progressState);
    const headerEl = document.querySelector<HTMLElement>('.message-expanded .d2-header')!;
    const pinnedProgress = document.querySelector<HTMLElement>('.message-expanded .d2-progress-pinned')!;
    const scrollArea = document.querySelector<HTMLElement>('.message-expanded .d2-scroll')!;
    const fallbackDesc = document.querySelector('.message-expanded .d2-fallback-desc');
    assert(
      Boolean(pinnedProgress)
        && !scrollArea.contains(pinnedProgress)
        && !fallbackDesc
        && (pinnedProgress.textContent ?? '').includes('Run glab label')
        && (pinnedProgress.textContent ?? '').includes('17/18')
        && pinnedProgress.getBoundingClientRect().top >= headerEl.getBoundingClientRect().bottom - 1
        && scrollArea.getBoundingClientRect().top >= pinnedProgress.getBoundingClientRect().bottom - 1,
      'Progress bar must stay pinned directly beneath MessageDock header without duplicating identical description text',
    );
    const pinnedTopBefore = pinnedProgress.getBoundingClientRect().top;
    assert(
      scrollArea.scrollHeight > scrollArea.clientHeight && scrollArea.scrollTop > 0,
      `Long running timeline must overflow and auto-scroll to latest step (scrollHeight=${scrollArea.scrollHeight}, clientHeight=${scrollArea.clientHeight}, scrollTop=${scrollArea.scrollTop})`,
    );
    await act(async () => {
      scrollArea.scrollTop = 0;
      scrollArea.dispatchEvent(new Event('scroll'));
    });
    assert(
      Math.abs(pinnedProgress.getBoundingClientRect().top - pinnedTopBefore) < 0.5,
      'Scrolling timeline body must keep pinned progress header fixed in place',
    );
    checks.push('MessageDock pins progress strip beneath header, deduplicates matching description, and auto-scrolls command timeline independently');
  }

  // Appearance theme selector (System / Dark / Light): live CSS tokens, native window sync, persistence, i18n labels, and TrayPopover sync:
  {
    const themeCalls: string[] = [];
    const savedThemePayloads: WorkbenchSaveRequest[] = [];
    const themeWinMock = createControlledWindow();
    const themeController: WindowController = {
      ...themeWinMock.controller,
      async setTheme(nextTheme) {
        themeCalls.push(nextTheme);
        if (nextTheme === 'system') return 'dark';
        return nextTheme;
      },
    };
    await mount(
      fixture('typing'),
      {
        ...disconnectedBackend,
        async saveWorkbenchState(req) {
          savedThemePayloads.push(req);
        },
      },
      themeController,
    );
    await tick();

    await click('Settings');
    const currentDocTheme = () => document.documentElement.dataset.theme;
    const themeGroup = () => document.querySelector<HTMLElement>('[data-setting-group="app-theme"]')!;
    assert(
      Boolean(themeGroup())
        && themeGroup().querySelector<HTMLButtonElement>('[data-theme-option="system"]')?.getAttribute('aria-pressed') === 'true'
        && (themeGroup().textContent ?? '').includes('System')
        && (themeGroup().textContent ?? '').includes('Dark')
        && (themeGroup().textContent ?? '').includes('Light'),
      `Settings General tab must render Theme segmented control defaulting to System (got: ${themeGroup()?.textContent})`,
    );

    // Switch to Light mode -> light canvas & dark obsidian primary button + persisted theme: 'light':
    await clickElement(themeGroup().querySelector<HTMLButtonElement>('[data-theme-option="light"]')!);
    await tick();
    const workbenchEl = document.querySelector<HTMLElement>('.workbench')!;
    const primaryBtnEl = document.querySelector<HTMLButtonElement>('.action-footer button.primary')!;
    await waitUntil(
      () => getComputedStyle(primaryBtnEl).backgroundColor === 'rgb(24, 24, 27)',
      () => `light-primary-bg=${getComputedStyle(primaryBtnEl).backgroundColor}`,
    );
    assert(
      currentDocTheme() === 'light'
        && document.documentElement.style.colorScheme === 'light'
        && getComputedStyle(workbenchEl).backgroundColor === 'rgb(247, 248, 250)'
        && getComputedStyle(primaryBtnEl).backgroundColor === 'rgb(24, 24, 27)'
        && themeCalls.includes('light')
        && savedThemePayloads.some((req) => req.theme === 'light'),
      `Light theme must apply light canvas tokens, obsidian primary CTA, native setTheme, and persistence (dataTheme=${currentDocTheme()}, bg=${getComputedStyle(workbenchEl).backgroundColor}, primaryBg=${getComputedStyle(primaryBtnEl).backgroundColor})`,
    );

    // Switch UI language to Chinese and verify localized theme labels (外观主题 / 跟随系统 / 深色 / 浅色):
    const zhAppLangBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="app-language"] button')]
      .find((b) => b.textContent?.trim() === zhLanguageLabel)!;
    await clickElement(zhAppLangBtn);
    await tick();
    assert(
      (themeGroup().textContent ?? '').includes(zh.settings.themeOptions.system)
        && (themeGroup().textContent ?? '').includes(zh.settings.themeOptions.dark)
        && (themeGroup().textContent ?? '').includes(zh.settings.themeOptions.light),
      `Chinese UI must localize theme options (got: ${themeGroup().textContent})`,
    );

    // Switch to Dark mode -> dark canvas & light primary button:
    await clickElement(themeGroup().querySelector<HTMLButtonElement>('[data-theme-option="dark"]')!);
    await tick();
    await waitUntil(
      () => getComputedStyle(primaryBtnEl).backgroundColor === 'rgb(244, 244, 245)',
      () => `dark-primary-bg=${getComputedStyle(primaryBtnEl).backgroundColor}`,
    );
    assert(
      currentDocTheme() === 'dark'
        && getComputedStyle(workbenchEl).backgroundColor === 'rgb(11, 13, 16)'
        && getComputedStyle(primaryBtnEl).backgroundColor === 'rgb(244, 244, 245)'
        && themeCalls.includes('dark'),
      `Dark theme must apply dark canvas tokens and bright primary CTA (bg=${getComputedStyle(workbenchEl).backgroundColor})`,
    );

    // Switch back to System -> invokes setTheme('system') and resolves cleanly:
    await clickElement(themeGroup().querySelector<HTMLButtonElement>('[data-theme-option="system"]')!);
    await tick();
    assert(
      themeGroup().querySelector<HTMLButtonElement>('[data-theme-option="system"]')?.getAttribute('aria-pressed') === 'true'
        && themeCalls.at(-1) === 'system'
        && currentDocTheme() === 'dark',
      'Switching back to System theme must invoke native setTheme("system") and apply resolved appearance',
    );

    // Verify TrayPopover applies theme snapshot cleanly:
    await act(async () => {
      root.render(
        createElement(TrayPopover, {
          snapshot: { protocolVersion: 1, locale: 'en', theme: 'light', hasUpdate: false, refreshing: false, repositories: [] },
          onRefresh: () => {},
          onOpenSettings: () => {},
          onOpenMainWindow: () => {},
          onQuit: () => {},
          onDismiss: () => {},
        }),
      );
    });
    assert(currentDocTheme() === 'light', 'TrayPopover must apply light theme when snapshot.theme is light');
    await act(async () => {
      root.render(
        createElement(TrayPopover, {
          snapshot: { protocolVersion: 1, locale: 'en', theme: 'dark', hasUpdate: false, refreshing: false, repositories: [] },
          onRefresh: () => {},
          onOpenSettings: () => {},
          onOpenMainWindow: () => {},
          onQuit: () => {},
          onDismiss: () => {},
        }),
      );
    });
    assert(currentDocTheme() === 'dark', 'TrayPopover must apply dark theme when snapshot.theme is dark');

    // Verify startup WhaleSplashStage adapts SVG gradient stops and CSS tokens across dark & light themes:
    const splashHost = document.createElement('div');
    document.body.append(splashHost);
    const splashStage = new WhaleSplashStage({
      dx: 0,
      dy: 0,
      distance: 0,
      arcHeight: 76,
      leapDurationMs: 440,
      compactMode: false,
      workbenchAlreadyReady: true,
      theme: 'dark',
    });
    splashHost.append(splashStage.element);
    try {
      const splashDomTheme = () => splashStage.element.dataset.theme;
      const darkStop0 = splashStage.element.querySelector('#picket-whale-fill stop')?.getAttribute('stop-color');
      const darkEyeFill = getComputedStyle(splashStage.element.querySelector('.whale-eye')!).fill;
      const darkDropletFill = getComputedStyle(splashStage.element.querySelector('.whale-droplet')!).fill;
      assert(
        splashDomTheme() === 'dark'
          && splashStage.getTheme() === 'dark'
          && darkStop0 === '#FFFFFF'
          && darkEyeFill === 'rgb(30, 41, 59)'
          && darkDropletFill === 'rgb(99, 102, 241)',
        `Dark splash whale must use Pearl Whale palette (stop0=${darkStop0}, eye=${darkEyeFill}, droplet=${darkDropletFill})`,
      );

      splashStage.updatePlan({
        dx: 0,
        dy: 0,
        distance: 0,
        arcHeight: 76,
        leapDurationMs: 440,
        compactMode: false,
        workbenchAlreadyReady: true,
        theme: 'light',
      });
      const lightStop0 = splashStage.element.querySelector('#picket-whale-fill stop')?.getAttribute('stop-color');
      const lightEyeFill = getComputedStyle(splashStage.element.querySelector('.whale-eye')!).fill;
      const lightDropletFill = getComputedStyle(splashStage.element.querySelector('.whale-droplet')!).fill;
      assert(
        splashDomTheme() === 'light'
          && splashStage.getTheme() === 'light'
          && lightStop0 === '#5B7DB1'
          && lightEyeFill === 'rgb(248, 250, 252)'
          && lightDropletFill === 'rgb(96, 165, 250)',
        `Light splash whale must use Slate Marine palette (stop0=${lightStop0}, eye=${lightEyeFill}, droplet=${lightDropletFill})`,
      );
    } finally {
      splashStage.dispose();
      splashHost.remove();
    }
    checks.push('Appearance theme setting (System / Dark / Light): live workbench, tray & startup whale splash tokens, native window sync, persistence, and EN/ZH labels');
  }

  // Regression: Compact -> Full workbench switch with persisted numeric dock height must not flash MessageDock to bottom on first grab-handle click:
  {
    const en = getMessages('en');
    const rootEl = document.getElementById('root')!;
    const prevRootHeight = rootEl.style.height;
    const savedDockHeight = 220;
    const createAnimatedViewportWindow = () => {
      const base = createControlledWindow();
      const controller: WindowController = {
        ...base.controller,
        async applyWindowMode(opts) {
          // Simulate native macOS window height shrinking/expanding while applyWindowMode runs:
          rootEl.style.height = opts.compactMode ? `${opts.compactHeight ?? 86}px` : `${opts.fullSize.height}px`;
          await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
          return base.controller.applyWindowMode(opts);
        },
      };
      return controller;
    };

    try {
      // Scenario A: Normal (expanded dock at 220px) -> Collapse to Compact (window shrinks to 86px) -> Expand back to Normal -> first pointerdown on grab handle:
      window.localStorage.setItem('picket.dockHeight', String(savedDockHeight));
      const fullWithDock = fixture('question');
      fullWithDock.worktrees[0].dockExpanded = true;
      await mount(fullWithDock, disconnectedBackend, createAnimatedViewportWindow());
      await tick();

      const lowerBeforeCollapse = document.querySelector<HTMLElement>('.lower-workspace')!.getBoundingClientRect().height;
      assert(
        Math.abs(lowerBeforeCollapse - savedDockHeight) <= 3,
        `Initial expanded dock must honor saved numeric height ${savedDockHeight}px (got ${lowerBeforeCollapse})`,
      );

      await click(en.header.collapseWindow);
      await waitUntil(
        () => Boolean(document.querySelector('.compact-bar')),
        'did not enter compact mode',
      );

      await click('Expand Workbench');
      await waitUntil(
        () => Boolean(document.querySelector('.d2-grab-zone')),
        'did not return to full workbench with expanded dock',
      );
      await tick();

      const grabAfterReturn = document.querySelector<HTMLElement>('.d2-grab-zone')!;
      const lowerAfterReturn = document.querySelector<HTMLElement>('.lower-workspace')!;
      const changesAfterReturn = document.querySelector<HTMLElement>('.changes-panel')!;
      const grabTopBeforeClick = grabAfterReturn.getBoundingClientRect().top;
      const lowerHeightBeforeClick = lowerAfterReturn.getBoundingClientRect().height;
      assert(
        changesAfterReturn.getAttribute('data-layout-mode') === 'peek'
          && Math.abs(lowerHeightBeforeClick - savedDockHeight) <= 3,
        `Returning from compact mode must immediately restore saved ${savedDockHeight}px dock height without shrinking-frame poisoning (mode=${changesAfterReturn.getAttribute('data-layout-mode')}, height=${lowerHeightBeforeClick})`,
      );

      // Bare first pointerdown on grab handle must NOT flash or jump dock position:
      await act(async () => {
        grabAfterReturn.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 91, clientX: 180, clientY: grabTopBeforeClick + 6 }));
      });
      const grabTopOnFirstClick = grabAfterReturn.getBoundingClientRect().top;
      const lowerHeightOnFirstClick = lowerAfterReturn.getBoundingClientRect().height;
      assert(
        Math.abs(grabTopOnFirstClick - grabTopBeforeClick) <= 1
          && Math.abs(lowerHeightOnFirstClick - lowerHeightBeforeClick) <= 1,
        `First pointerdown on grab handle after compact->full must not jump (top ${grabTopBeforeClick} -> ${grabTopOnFirstClick}, height ${lowerHeightBeforeClick} -> ${lowerHeightOnFirstClick})`,
      );

      // Drag upward by 90px smoothly grows dock from saved height:
      await act(async () => {
        window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 91, clientX: 180, clientY: grabTopBeforeClick + 6 - 90 }));
        window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 91, clientX: 180, clientY: grabTopBeforeClick + 6 - 90 }));
      });
      const lowerHeightAfterDrag = lowerAfterReturn.getBoundingClientRect().height;
      assert(
        Math.abs(lowerHeightAfterDrag - (savedDockHeight + 90)) <= 4,
        `Dragging grab handle upward after compact->full must track pointer smoothly (expected ~${savedDockHeight + 90}, got ${lowerHeightAfterDrag})`,
      );

      // Scenario B: Cold-start directly in Compact Mode with saved 220px dock height -> Expand to Normal -> first pointerdown on grab handle:
      window.localStorage.setItem('picket.dockHeight', String(savedDockHeight));
      rootEl.style.height = '86px';
      const coldCompactWithDock = fixture('question');
      coldCompactWithDock.windowPrefs = { ...coldCompactWithDock.windowPrefs, compactMode: true };
      coldCompactWithDock.worktrees[0].dockExpanded = true;
      await mount(coldCompactWithDock, disconnectedBackend, createAnimatedViewportWindow());
      await tick();

      await click('Expand Workbench');
      await waitUntil(
        () => Boolean(document.querySelector('.d2-grab-zone')),
        'cold-compact expand did not mount expanded dock',
      );
      await tick();

      const grabAfterColdExpand = document.querySelector<HTMLElement>('.d2-grab-zone')!;
      const lowerAfterColdExpand = document.querySelector<HTMLElement>('.lower-workspace')!;
      const coldTopBefore = grabAfterColdExpand.getBoundingClientRect().top;
      const coldHeightBefore = lowerAfterColdExpand.getBoundingClientRect().height;
      assert(
        Math.abs(coldHeightBefore - savedDockHeight) <= 3,
        `Cold-start compact->full expand must measure freshly mounted workbench immediately (expected ~${savedDockHeight}, got ${coldHeightBefore})`,
      );
      await act(async () => {
        grabAfterColdExpand.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 92, clientX: 180, clientY: coldTopBefore + 6 }));
        window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 92, clientX: 180, clientY: coldTopBefore + 6 }));
      });
      const coldTopAfter = grabAfterColdExpand.getBoundingClientRect().top;
      assert(
        Math.abs(coldTopAfter - coldTopBefore) <= 1,
        `First click on grab handle after cold compact->full must not flash to bottom (top ${coldTopBefore} -> ${coldTopAfter})`,
      );
      checks.push('Compact->Full switch re-measures dock split cleanly without shrink poisoning or first-click grab-handle flash');
    } finally {
      rootEl.style.height = prevRootHeight;
      window.localStorage.removeItem('picket.dockHeight');
    }
  }

  // Regression: Window mode switches (Compact <-> Full, Compact <-> Settings) must lock surface height during native resize and never flash opacity:0:
  {
    const en = getMessages('en');
    const rootEl = document.getElementById('root')!;
    const prevRootHeight = rootEl.style.height;
    setInstantMotion(false);
    try {
      const compactState = fixture('compact');
      const winMock = createControlledWindow();
      rootEl.style.height = '86px';
      await mount(compactState, disconnectedBackend, winMock.controller);
      await tick();

      // 1) Compact -> Full: WorkbenchContent mounts BEFORE resize completes at opacity:1 and full 700px locked height even while viewport is still 86px:
      const releaseExpand = winMock.pauseNextApply();
      await click('Expand Workbench');
      await tick();
      const midExpandMain = document.querySelector<HTMLElement>('main')!;
      const midExpandContent = document.querySelector<HTMLElement>('.workbench-content')!;
      const midExpandOpacity = Number(getComputedStyle(midExpandContent).opacity);
      const midExpandHeight = Math.round(midExpandContent.getBoundingClientRect().height);
      assert(
        midExpandMain.getAttribute('data-surface') === 'workbench'
          && midExpandMain.getAttribute('data-window-transition') === 'resizing'
          && midExpandMain.getAttribute('data-window-transition-kind') === 'expanding'
          && !document.querySelector('.compact-bar')
          && midExpandOpacity === 1
          && Math.abs(midExpandHeight - 700) <= 2,
        `Compact->Full must pre-mount full-height workbench at opacity:1 before window finishes expanding (opacity=${midExpandOpacity}, height=${midExpandHeight})`,
      );
      rootEl.style.height = '700px';
      await act(async () => { releaseExpand(); });
      await tick();
      assert(
        document.querySelector('main')?.getAttribute('data-window-transition-kind') === 'idle',
        'Compact->Full did not settle back to idle after resize',
      );

      // 2) Full -> Compact: WorkbenchContent stays height-locked at 700px while viewport shrinks, then swaps directly to CompactBar at opacity:1 (zero blank flash):
      const releaseCollapse = winMock.pauseNextApply();
      await click(en.header.collapseWindow);
      await tick();
      assert(
        document.querySelector('main')?.getAttribute('data-window-transition-kind') === 'collapsing',
        'Full->Compact did not enter collapsing transition state',
      );
      // Simulate mid-shrink viewport height (220px): frozen workbench-content must keep 700px height without reflowing/crushing ActionFooter:
      rootEl.style.height = '220px';
      await tick();
      const midCollapseContent = document.querySelector<HTMLElement>('.workbench-content')!;
      const midCollapseHeight = Math.round(midCollapseContent.getBoundingClientRect().height);
      assert(
        Math.abs(midCollapseHeight - 700) <= 2,
        `Full->Compact must lock workbench-content height during shrink (expected ~700px, got ${midCollapseHeight}px)`,
      );
      rootEl.style.height = '86px';
      await act(async () => { releaseCollapse(); });
      await tick();
      assert(
        Boolean(document.querySelector('.compact-surface'))
          && document.querySelector('main')?.getAttribute('data-window-transition-kind') === 'idle',
        'Full->Compact did not settle into compact surface',
      );
      const compactOpacity = Number(getComputedStyle(document.querySelector<HTMLElement>('.compact-surface')!).opacity);
      assert(
        compactOpacity === 1,
        `Compact surface must mount immediately at opacity:1 without blank flash (got ${compactOpacity})`,
      );

      // 3) Compact -> Settings -> Compact: standalone Settings mounts at opacity:1 and stays height-locked at 700px across both expand and collapse:
      const releaseOpenSettings = winMock.pauseNextApply();
      await click('Settings');
      await tick();
      assert(
        document.querySelector('main')?.getAttribute('data-surface') === 'standaloneSettings',
        'Compact->Settings did not mount standalone settings',
      );
      const midOpenOverlay = document.querySelector<HTMLElement>('.overlay')!;
      const midOpenOverlayOpacity = Number(getComputedStyle(midOpenOverlay).opacity);
      const midOpenOverlayHeight = Math.round(midOpenOverlay.getBoundingClientRect().height);
      assert(
        midOpenOverlayOpacity === 1 && Math.abs(midOpenOverlayHeight - 700) <= 2,
        `Compact->Settings must mount standalone overlay at opacity:1 and full 700px height (opacity=${midOpenOverlayOpacity}, height=${midOpenOverlayHeight})`,
      );
      rootEl.style.height = '700px';
      await act(async () => { releaseOpenSettings(); });
      await tick();
      assert(
        document.querySelector('main')?.getAttribute('data-window-transition-kind') === 'idle',
        'Compact->Settings did not settle to idle',
      );

      const releaseCloseSettings = winMock.pauseNextApply();
      await click('Close Settings');
      await tick();
      assert(
        document.querySelector('main')?.getAttribute('data-window-transition-kind') === 'collapsing',
        'Settings->Compact did not enter collapsing transition state',
      );
      rootEl.style.height = '240px';
      await tick();
      const midCloseOverlayHeight = Math.round(document.querySelector<HTMLElement>('.overlay')!.getBoundingClientRect().height);
      assert(
        Math.abs(midCloseOverlayHeight - 700) <= 2,
        `Settings->Compact must lock standalone overlay height during shrink (expected ~700px, got ${midCloseOverlayHeight}px)`,
      );
      rootEl.style.height = '86px';
      await act(async () => { releaseCloseSettings(); });
      await tick();
      assert(
        Boolean(document.querySelector('.compact-surface'))
          && document.querySelector('main')?.getAttribute('data-window-transition-kind') === 'idle',
        'Settings->Compact did not return to compact surface',
      );
      const compactOpacityAfterSettings = Number(getComputedStyle(document.querySelector<HTMLElement>('.compact-surface')!).opacity);
      assert(
        compactOpacityAfterSettings === 1,
        `Returning from Settings to Compact must mount compact surface at opacity:1 (got ${compactOpacityAfterSettings})`,
      );
      checks.push('Window mode transitions (Compact <-> Full, Compact <-> Settings) lock surface height during native resize with zero opacity:0 flashes');
    } finally {
      setInstantMotion(true);
      rootEl.style.height = prevRootHeight;
    }
  }
}
