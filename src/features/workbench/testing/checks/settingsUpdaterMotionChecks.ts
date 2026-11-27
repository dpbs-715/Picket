import { act } from 'react';
import { version as PACKAGE_VERSION } from '../../../../../package.json';
import type { ModelSaveReply, ProviderRevealKeyReply, ProviderUpdateReply } from '../../../../../shared/model-protocol';
import { PICKET_RELEASES_URL, UPDATE_PROTOCOL_VERSION, type UpdateStateSnapshot } from '../../../../../shared/update-protocol';
import type { WorkbenchSaveRequest } from '../../../../../shared/workbench-persistence-protocol';
import type { WindowController } from '../../../../desktop/window';
import type { ModelConfiguration, ProviderConnection } from '../../../settings/modelProviders';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchBackend, WorkbenchCommand } from '../../commands';
import { fixture } from '../fixtures';
import { assert, button, click, clickElement, createControlledWindow, openSettingsTab, text, tick, type, type BrowserCheckHarness } from './harness';

export async function runSettingsUpdaterMotionChecks(harness: BrowserCheckHarness) {
  const { checks, setInstantMotion, waitUntil, mount } = harness;
  // 18. Provider key hints, Edit Connection reveal (30s / toggle / blur / close auto-hide), key rotation impact, adding child models under a connection, and footer duplicate-name disambiguation:
  const fakeRevealedSecret = 'fake-revealed-gateway-secret-9001';
  let revealOutcome: ProviderRevealKeyReply = {
    protocolVersion: 2,
    connectionId: 'conn-gw',
    outcome: 'cancelled',
    message: 'Authentication cancelled.',
  };
  const tracker = {
    revealCalls: 0,
    testCalls: [] as unknown[],
    failNextTestMessage: null as string | null,
    updateCalls: [] as unknown[],
    saveModelCalls: [] as unknown[],
  };
  const getRevealCalls = () => tracker.revealCalls;
  const persistedPayloads: WorkbenchSaveRequest[] = [];
  let liveConnections: ProviderConnection[] = [
    {
      id: 'conn-gw',
      name: 'Company Gateway',
      provider: 'custom',
      baseUrl: 'https://gw.example.com/v1',
      keyHint: 'fake…9001',
    },
    {
      id: 'conn-ollama',
      name: 'Local Ollama',
      provider: 'ollama',
      baseUrl: 'http://127.0.0.1:11434/v1',
    },
  ];
  let liveModels: ModelConfiguration[] = [
    {
      id: 'm-gw-1',
      connectionId: 'conn-gw',
      name: 'Coder',
      provider: 'custom',
      apiFormat: 'openai-completions',
      modelId: 'coder-alpha',
      baseUrl: 'https://gw.example.com/v1',
      contextWindow: 128_000,
      maxOutputTokens: 16_384,
    },
    {
      id: 'm-ollama',
      connectionId: 'conn-ollama',
      name: 'Coder',
      provider: 'ollama',
      apiFormat: 'openai-completions',
      modelId: 'qwen2.5-coder:14b',
      baseUrl: 'http://127.0.0.1:11434/v1',
      contextWindow: 32_768,
      maxOutputTokens: 4_096,
    },
  ];
  let liveActiveModelId: string | null = 'm-gw-1';

  const credentialState = fixture('typing');
  credentialState.connections = liveConnections.map((item) => ({ ...item }));
  credentialState.models = liveModels.map((item) => ({ ...item }));
  credentialState.activeModelId = liveActiveModelId;

  await mount(credentialState, {
    ...disconnectedBackend,
    async saveWorkbenchState(request) {
      persistedPayloads.push(request);
    },
    async revealProviderApiKey(connectionId) {
      tracker.revealCalls += 1;
      return { ...revealOutcome, connectionId };
    },
    async testModel(input) {
      tracker.testCalls.push(input);
      if (tracker.failNextTestMessage) {
        const msg = tracker.failNextTestMessage;
        tracker.failNextTestMessage = null;
        throw new Error(msg);
      }
      const modelId = 'modelId' in input && input.modelId ? input.modelId : 'coder-alpha';
      return `Connected: ${modelId} (12 ms)`;
    },
    async updateProviderConnection(input): Promise<ProviderUpdateReply> {
      tracker.updateCalls.push(input);
      const nextHint = input.apiKey ? `fake…${input.apiKey.slice(-4)}` : undefined;
      liveConnections = liveConnections.map((conn) =>
        conn.id === input.connectionId
          ? { ...conn, name: input.name, ...(nextHint ? { keyHint: nextHint } : {}) }
          : conn,
      );
      const updated = liveConnections.find((conn) => conn.id === input.connectionId)!;
      return {
        protocolVersion: 2,
        connection: updated,
        connections: liveConnections.map((item) => ({ ...item })),
        models: liveModels.map((item) => ({ ...item })),
        activeModelId: liveActiveModelId,
      };
    },
    async saveModel(input): Promise<ModelSaveReply> {
      tracker.saveModelCalls.push(input);
      const parent = liveConnections.find((item) => item.id === input.connectionId)!;
      const savedModel: ModelConfiguration = {
        id: input.id ?? `m-child-${tracker.saveModelCalls.length}`,
        connectionId: parent.id,
        name: input.name,
        provider: parent.provider,
        apiFormat: input.apiFormat,
        modelId: input.modelId,
        ...(parent.baseUrl ? { baseUrl: parent.baseUrl } : {}),
        contextWindow: input.contextWindow,
        maxOutputTokens: input.maxOutputTokens,
      };
      liveModels = input.id
        ? liveModels.map((item) => (item.id === input.id ? savedModel : item))
        : [...liveModels, savedModel];
      if (!liveActiveModelId) liveActiveModelId = savedModel.id;
      return {
        protocolVersion: 2,
        model: savedModel,
        connections: liveConnections.map((item) => ({ ...item })),
        models: liveModels.map((item) => ({ ...item })),
        activeModelId: liveActiveModelId,
      };
    },
  });

  // Footer disambiguates duplicate model names across connections as `<Connection Name> / <Model Name>`:
  assert(
    (document.querySelector('.model-button')?.textContent ?? '').includes('Company Gateway / Coder'),
    'ActionFooter must disambiguate duplicate model display names across connections',
  );

  // Opening Settings displays `keyHint` on provider cards without calling revealProviderApiKey:
  await click('Settings');
  await openSettingsTab('models');
  const providerCards = [...document.querySelectorAll<HTMLElement>('.provider-card')];
  assert(
    providerCards.length === 2
      && (providerCards[0].querySelector('.model-key-hint')?.textContent ?? '') === 'fake…9001'
      && !providerCards[1].querySelector('.model-key-hint')
      && getRevealCalls() === 0,
    'Settings provider list must show keyHint only for key-bearing connections without invoking reveal',
  );

  // Edit Connection shows immutable provider/URL + keyHint + Eye reveal button; Cancelled auth shows short notice without revealing secret:
  await click('Edit connection Company Gateway');
  assert(
    !!document.querySelector('.model-form[data-form-mode="edit-provider"]')
      && text().includes('https://gw.example.com/v1')
      && !document.querySelector('[data-field="base-url"]')
      && (document.querySelector('.model-form .model-key-hint')?.textContent ?? '') === 'fake…9001'
      && !!document.querySelector('button[aria-label="Reveal saved API key"]'),
    'Edit Connection form must lock baseUrl as read-only identity and display keyHint + Reveal button',
  );
  await click('Reveal saved API key');
  await tick();
  assert(
    getRevealCalls() === 1
      && !document.querySelector('.revealed-api-key')
      && text().includes('Authentication cancelled.')
      && !text().includes(fakeRevealedSecret),
    'Cancelled LocalAuthentication must not display secret and must show short inline notice',
  );

  // Authenticated reveal: verify exact 30,000ms timer registration + 30s auto-hide + password input untouched + zero persistence leak:
  revealOutcome = {
    protocolVersion: 2,
    connectionId: 'conn-gw',
    outcome: 'authenticated',
    apiKey: fakeRevealedSecret,
    keyHint: 'fake…9001',
  };
  const originalSetTimeout = window.setTimeout.bind(window);
  let captured30sCallback: (() => void) | undefined;
  window.setTimeout = ((handler: TimerHandler, timeout?: number, ...args: unknown[]) => {
    if (timeout === 30_000 && typeof handler === 'function') {
      captured30sCallback = () => { (handler as () => void)(); };
    }
    return originalSetTimeout(handler, timeout, ...args);
  }) as typeof window.setTimeout;
  try {
    await click('Reveal saved API key');
    await tick();
    assert(
      getRevealCalls() === 2
        && document.querySelector('.revealed-api-key')?.textContent === fakeRevealedSecret
        && document.querySelector<HTMLInputElement>('input[type="password"]')?.value === ''
        && !!captured30sCallback,
      'Authenticated reveal must render read-only key, leave password input empty, and arm 30s auto-hide timer',
    );
    await act(async () => { captured30sCallback!(); });
    await tick();
    assert(
      !document.querySelector('.revealed-api-key') && !text().includes(fakeRevealedSecret),
      'Revealed API key must auto-hide and clear from memory after 30 seconds',
    );
  } finally {
    window.setTimeout = originalSetTimeout;
  }

  // Second click on Eye button hides immediately without re-invoking backend:
  await click('Reveal saved API key');
  await tick();
  assert(getRevealCalls() === 3 && document.querySelector('.revealed-api-key')?.textContent === fakeRevealedSecret, 'Reveal before toggle check failed');
  await click('Hide API key');
  await tick();
  assert(getRevealCalls() === 3 && !document.querySelector('.revealed-api-key') && !text().includes(fakeRevealedSecret), 'Second click on Eye button must immediately hide revealed key');

  // Window blur hides immediately:
  await click('Reveal saved API key');
  await tick();
  assert(document.querySelector('.revealed-api-key')?.textContent === fakeRevealedSecret, 'Reveal before blur check failed');
  await act(async () => { window.dispatchEvent(new Event('blur')); });
  await tick();
  assert(!document.querySelector('.revealed-api-key') && !text().includes(fakeRevealedSecret), 'Window blur must immediately hide and clear revealed key');

  // Closing form clears revealed key:
  await click('Reveal saved API key');
  await tick();
  assert(document.querySelector('.revealed-api-key')?.textContent === fakeRevealedSecret, 'Reveal before close check failed');
  await click('Cancel');
  await tick();
  assert(!text().includes(fakeRevealedSecret), 'Closing form must clear revealed key');
  assert(!JSON.stringify(persistedPayloads).includes(fakeRevealedSecret), 'Revealed API key must never enter persisted workbench payloads');

  // Add second child model under Company Gateway: Model ID autofocus, Test Connection failure -> Save Anyway, preserves activeModelId:
  const addChildBtn = document.querySelector<HTMLButtonElement>('.provider-card[data-connection-id="conn-gw"] .provider-add-model')!;
  await clickElement(addChildBtn);
  await tick();
  const childModelIdInput = document.querySelector<HTMLInputElement>('.model-form[data-form-mode="add-model"] [data-field="model-id"]')!;
  const childModelNameInput = document.querySelector<HTMLInputElement>('.model-form[data-form-mode="add-model"] [data-field="model-name"]')!;
  assert(
    !!childModelIdInput
      && document.activeElement === childModelIdInput
      && !document.querySelector('.model-form input[type="password"]')
      && !document.querySelector('.model-form [data-field="base-url"]'),
    'Add Model under existing connection must focus Model ID and omit API Key / Base URL fields',
  );
  type(childModelIdInput, 'coder-beta');
  type(childModelNameInput, 'Coder Beta');
  await tick();
  tracker.testCalls = [];
  tracker.failNextTestMessage = 'Model coder-beta temporarily unreachable';
  await click('Test Connection');
  await tick();
  assert(
    tracker.testCalls.length === 1
      && (tracker.testCalls[0] as { kind?: string; connectionId?: string }).kind === 'connectionModel'
      && (tracker.testCalls[0] as { connectionId?: string }).connectionId === 'conn-gw'
      && text().includes('Model coder-beta temporarily unreachable')
      && !!button('Save Anyway'),
    'Failed child model Test Connection must show error and turn submit button into Save Anyway',
  );
  await click('Save Anyway');
  await tick();
  assert(
    tracker.saveModelCalls.length === 1
      && !!document.querySelector('#settings-models-section')
      && !document.querySelector('.model-form')
      && document.querySelectorAll('.provider-card[data-connection-id="conn-gw"] .model-row').length === 2
      && document.querySelector('.model-row.is-active strong')?.textContent === 'Coder',
    'Save Anyway must add child model under Company Gateway, stay in Settings, and keep original activeModelId',
  );

  // Rotating connection API key shows child-count impact notice (`Applies to all 2 models under this connection.`):
  await click('Edit connection Company Gateway');
  await tick();
  const rotateKeyInput = document.querySelector<HTMLInputElement>('.model-form input[type="password"]')!;
  type(rotateKeyInput, 'fake-new-connection-secret-2222');
  await tick();
  assert(
    text().includes('Updates all 2 models under this connection.'),
    'Entering a new key in Edit Connection must display child model impact notice',
  );
  await click('Save Changes');
  await tick();
  assert(
    tracker.updateCalls.length === 1
      && (document.querySelector('.provider-card[data-connection-id="conn-gw"] .model-key-hint')?.textContent ?? '') === 'fake…2222',
    'Saving rotated connection key must update connection keyHint in Settings',
  );
  await click('Close Settings');
  checks.push('Provider key hints, authenticated full-key reveal & 4 auto-hide rules, child model creation, connection key rotation impact, and duplicate-name disambiguation');

  // 19. In-app updater UI: disconnected honesty, indicator dot in Full & Compact without auto-expand, Settings update card, download progress/cancel, and blocker fallback:
  await mount(fixture('empty'));
  await click('Settings');
  assert(!!document.querySelector('#settings-updates-section') && text().includes(`v${PACKAGE_VERSION}`), 'Settings must display About & Updates section with current version');
  await click('Check for Updates');
  await tick();
  assert(text().includes('unavailable in browser preview') && !!button('Download on GitHub'), 'Browser preview Check for Updates must report honest error and offer GitHub fallback');
  await click('Close Settings');

  const updateSub: { receive?: (snapshot: UpdateStateSnapshot) => void } = {};
  let openedReleaseCount = 0;
  let cancelledDownloadCount = 0;
  let currentMockUpdate: UpdateStateSnapshot = {
    protocolVersion: UPDATE_PROTOCOL_VERSION,
    currentVersion: '0.1.0',
    phase: 'idle',
    releasePageUrl: PICKET_RELEASES_URL,
    installBlocker: 'none',
    canInstall: false,
  };
  const compactUpdateState = fixture('compact');
  const updateWin = createControlledWindow({ x: 180, y: 120 }, { width: 400, height: 700 });
  await mount(
    compactUpdateState,
    {
      ...disconnectedBackend,
      async getUpdateStatus() {
        return currentMockUpdate;
      },
      async checkForUpdate() {
        currentMockUpdate = {
          protocolVersion: UPDATE_PROTOCOL_VERSION,
          currentVersion: '0.1.0',
          phase: 'available',
          releasePageUrl: PICKET_RELEASES_URL,
          availableVersion: '0.1.1',
          releaseNotes: 'Faster diff reads and unified ~/.picket storage.',
          installBlocker: 'none',
          canInstall: true,
        };
        return currentMockUpdate;
      },
      async installUpdate() {
        currentMockUpdate = {
          ...currentMockUpdate,
          phase: 'downloading',
          downloadedBytes: 24 * 1024 * 1024,
          totalBytes: 48 * 1024 * 1024,
          canInstall: false,
        };
        return currentMockUpdate;
      },
      async cancelUpdate() {
        cancelledDownloadCount += 1;
        currentMockUpdate = {
          ...currentMockUpdate,
          phase: 'available',
          downloadedBytes: undefined,
          totalBytes: undefined,
          canInstall: true,
        };
        return currentMockUpdate;
      },
      async openReleasePage() {
        openedReleaseCount += 1;
      },
      subscribeUpdateState(receive) {
        updateSub.receive = receive;
        return () => { updateSub.receive = undefined; };
      },
    },
    updateWin.controller,
  );
  await tick();
  assert(!document.querySelector('button[aria-label="Settings"][data-has-update="true"]'), 'Idle update state must not show Settings dot');
  await act(async () => {
    currentMockUpdate = {
      protocolVersion: UPDATE_PROTOCOL_VERSION,
      currentVersion: '0.1.0',
      phase: 'available',
      releasePageUrl: PICKET_RELEASES_URL,
      availableVersion: '0.1.1',
      releaseNotes: 'Faster diff reads and unified ~/.picket storage.',
      installBlocker: 'none',
      canInstall: true,
    };
    updateSub.receive?.(currentMockUpdate);
  });
  await tick();
  assert(
    !!document.querySelector('.compact-bar')
      && !!document.querySelector('.compact-tools button[aria-label="Settings"][data-has-update="true"]'),
    'Available update in Compact Mode must light up Settings dot without auto-expanding the bar',
  );
  await click('Settings');
  await tick();
  assert(
    !document.querySelector('.compact-bar')
      && text().includes('v0.1.1 available')
      && text().includes('Faster diff reads')
      && !!button('Update & Restart')
      && !!button('Download on GitHub'),
    'Opening Settings from Compact Mode must display available update details and install/download actions',
  );
  await click('Update & Restart');
  await tick();
  assert(
    text().includes('50%')
      && !!document.querySelector('[role="progressbar"]')
      && !!button('Cancel'),
    'Clicking Update & Restart must show download progress bar and Cancel button',
  );
  await click('Cancel');
  await tick();
  assert(
    cancelledDownloadCount === 1
      && !!button('Update & Restart'),
    'Cancelling update download must restore available state cleanly',
  );
  await click('Download on GitHub');
  assert(openedReleaseCount === 1, 'Download on GitHub button must invoke openReleasePage');

  // Signing-key-unconfigured blocker hides Update & Restart while keeping Download on GitHub:
  await act(async () => {
    updateSub.receive?.({
      ...currentMockUpdate,
      phase: 'available',
      installBlocker: 'signingKeyUnconfigured',
      canInstall: false,
    });
  });
  await tick();
  assert(
    text().includes('Signing key not configured')
      && ![...document.querySelectorAll('button')].some((b) => b.textContent?.trim() === 'Update & Restart')
      && !!button('Download on GitHub'),
    'Install blocker must hide Update & Restart and explain manual GitHub download',
  );
  await click('Close Settings');
  await tick();
  assert(!!document.querySelector('.compact-bar'), 'Closing Settings opened from Compact Mode after inspecting updates must return to Compact Mode');
  checks.push('In-app updater UI: disconnected boundary, unobtrusive Settings badge dot, progress/cancel, blocker fallback, and compact return');

  // 31. Real DOM motion system: sliding Settings tab indicator, overlay sheet/drill exit animations, popover/collapse/rotating chevrons, dock expand/collapse, and zero-flash compact Settings with animations enabled:
  setInstantMotion(false);
  await mount(fixture('question'));
  await click('Settings');
  assert(
    document.querySelector('.overlay')?.getAttribute('data-overlay-variant') === 'sheet',
    'Settings opened from Full Workbench must use sheet overlay variant',
  );
  const generalTab = document.querySelector<HTMLButtonElement>('[data-settings-tab="general"]')!;
  const modelsTab = document.querySelector<HTMLButtonElement>('[data-settings-tab="models"]')!;
  const indicator = document.querySelector<HTMLElement>('.tab-active-indicator')!;
  assert(
    !!indicator
      && Math.abs(indicator.getBoundingClientRect().width - generalTab.getBoundingClientRect().width) <= 2,
    'Settings segmented tab list should render sliding active indicator aligned with General tab',
  );
  await clickElement(modelsTab);
  await waitUntil(
    () => Math.abs(indicator.getBoundingClientRect().left - modelsTab.getBoundingClientRect().left) <= 2,
    'tab-indicator-slide-to-models',
  );
  await click('Provider');
  assert(
    document.querySelectorAll('.overlay')[1]?.getAttribute('data-overlay-variant') === 'drill',
    'Nested provider form opened over Settings must use drill overlay variant',
  );
  await click('Cancel');
  await waitUntil(() => document.querySelectorAll('.overlay').length === 1, 'drill-overlay-exit');
  await click('Close Settings');
  await waitUntil(() => !document.querySelector('.overlay'), 'sheet-overlay-exit');

  // Branch popover + rotating chevron + smooth Collapse:
  await clickElement(document.querySelector<HTMLButtonElement>('.branch-anchor > button')!);
  assert(
    document.querySelector('.branch-menu')?.getAttribute('data-motion-popover') === 'top',
    'Branch popover must carry top popover motion metadata',
  );
  const remoteSectionToggle = [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')]
    .find((item) => item.textContent?.includes('Remote Branches'))!;
  assert(!remoteSectionToggle.querySelector('.chevron-rotator')?.classList.contains('is-open'), 'Collapsed Remote chevron must not be rotated open');
  await clickElement(remoteSectionToggle);
  const remoteChevronOpen = remoteSectionToggle.querySelector('.chevron-rotator')?.classList.contains('is-open');
  const remoteCollapseEl = document.querySelector('.branch-menu .motion-collapse:has([data-branch-group="remote"])');
  assert(
    remoteChevronOpen && !!remoteCollapseEl,
    `Expanding Remote Branches must rotate chevron and render inside animated Collapse container (chevronOpen=${remoteChevronOpen}, hasCollapse=${Boolean(remoteCollapseEl)}, mode=${remoteCollapseEl?.getAttribute('data-motion-mode')}, visibility=${document.visibilityState}, reduced=${window.matchMedia('(prefers-reduced-motion: reduce)').matches})`,
  );
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await waitUntil(() => !document.querySelector('.branch-menu'), 'branch-popover-exit');

  // MessageDock smooth collapse & expand within lower workspace bounds:
  await click('Collapse Message');
  await waitUntil(() => !document.querySelector('.message-expanded'), 'message-dock-initial-collapse');
  await click('Expand message: Include release notes in this commit?');
  await waitUntil(() => {
    const lowerRect = document.querySelector('.lower-workspace')?.getBoundingClientRect();
    const wrapRect = document.querySelector('.d2-wrap')?.getBoundingClientRect();
    return !!lowerRect && !!wrapRect && Math.abs(lowerRect.top - wrapRect.top) <= 1.5;
  }, 'message-dock-expand-settle');
  await click('Collapse Message');
  await waitUntil(() => !document.querySelector('.message-expanded'), 'message-dock-collapse-exit');

  // Standalone Settings from Compact Mode with real animations enabled must still never flash workbench content:
  const animatedCompactFixture = fixture('typing');
  animatedCompactFixture.windowPrefs = {
    pinned: true,
    compactMode: true,
    pinnedExpanded: false,
    fullSize: { width: 400, height: 700 },
    compactPosition: { x: 180, y: 120 },
  };
  let flashedWorkbenchWithMotion = false;
  const motionCompactObserver = new MutationObserver(() => {
    if (document.querySelector('.workbench-content, .changes-panel, .lower-workspace, .action-footer')) {
      flashedWorkbenchWithMotion = true;
    }
  });
  const motionWindowController: WindowController = {
    isDesktopWindow: true,
    async readSnapshot() {
      return { pinned: true, focused: true, position: { x: 180, y: 120 }, size: { width: 400, height: 86 }, scaleFactor: 2 };
    },
    async setPinned(next) { return next; },
    async applyWindowMode(options) {
      return {
        pinned: options.pinned,
        previousPosition: { x: 180, y: 120 },
        previousSize: options.compactMode ? { width: 400, height: 700 } : { width: 400, height: 86 },
        position: options.targetPosition ?? { x: 180, y: 120 },
        size: options.compactMode
          ? { width: options.compactWidth ?? 400, height: options.compactHeight ?? 86 }
          : options.fullSize,
      };
    },
    async adjustCompactHeight() {},
    async notifyBackgroundAttention() { return false; },
  };
  await mount(animatedCompactFixture, disconnectedBackend, motionWindowController);
  motionCompactObserver.observe(document.body, { childList: true, subtree: true });
  try {
    await click('Settings');
    await waitUntil(
      () => document.querySelector('.overlay')?.getAttribute('data-overlay-variant') === 'standalone'
        && !document.querySelector('.compact-bar'),
      'animated-standalone-settings-open',
    );
    await click('Close Settings');
    await waitUntil(
      () => !!document.querySelector('.compact-bar') && !document.querySelector('.overlay'),
      'animated-standalone-settings-close',
    );
    assert(!flashedWorkbenchWithMotion, 'Compact -> Settings -> Compact flashed workbench content when real animations were active');
  } finally {
    motionCompactObserver.disconnect();
  }
  checks.push('Motion system: sliding tab pill, sheet/drill/standalone overlay transitions, popover & collapse exit animations, rotating chevrons, dock glide, and zero-flash compact Settings');

  // Phase 1 UX: floating semantic Toast (zero layout shift), MessageDock true `done` dismissal, and `message_user` questionnaire:
  setInstantMotion(true);
  await mount(fixture('typing'), {
    ...disconnectedBackend,
    async execute(command) {
      if (command.kind === 'cPush') {
        throw new Error('Authentication failed (401/403). Verify your API key.');
      }
      return disconnectedBackend.execute(command);
    },
  });
  const changesBeforeToast = document.querySelector('.changes-panel')!.getBoundingClientRect();
  await click('C-Push');
  const toastEl = document.querySelector<HTMLElement>('.operation-notice');
  const toastLayer = document.querySelector<HTMLElement>('.operation-notice-layer');
  const changesAfterToast = document.querySelector('.changes-panel')!.getBoundingClientRect();
  assert(
    toastEl?.dataset.noticeTone === 'error'
      && toastLayer && getComputedStyle(toastLayer).position === 'absolute'
      && Math.abs(changesBeforeToast.top - changesAfterToast.top) <= 0.5
      && Math.abs(changesBeforeToast.height - changesAfterToast.height) <= 0.5,
    'Semantic Toast must float above content without shifting changes-panel layout',
  );
  // Header dropdown popovers must stack strictly above the floating Toast and stay clickable in the overlap zone:
  await click(fixture('typing').repositories[0].name);
  const repoMenuEl = document.querySelector<HTMLElement>('.repository-menu');
  const headerEl = document.querySelector<HTMLElement>('.context-header');
  assert(Boolean(repoMenuEl && headerEl), 'Repository dropdown should open while Toast is visible');
  const toastRect = toastEl!.getBoundingClientRect();
  const repoMenuOverlapRect = repoMenuEl!.getBoundingClientRect();
  const overlapLeft = Math.max(toastRect.left, repoMenuOverlapRect.left);
  const overlapRight = Math.min(toastRect.right, repoMenuOverlapRect.right);
  const overlapTop = Math.max(toastRect.top, repoMenuOverlapRect.top);
  const overlapBottom = Math.min(toastRect.bottom, repoMenuOverlapRect.bottom);
  assert(
    Number(getComputedStyle(toastLayer!).zIndex) < Number(getComputedStyle(headerEl!).zIndex),
    `Toast layer z-index (${getComputedStyle(toastLayer!).zIndex}) must stay below header/popover layer (${getComputedStyle(headerEl!).zIndex})`,
  );
  assert(
    overlapRight > overlapLeft + 2 && overlapBottom > overlapTop + 2,
    'Expected repository dropdown and floating Toast bounding boxes to overlap for hit-test verification',
  );
  const overlapHit = document.elementFromPoint((overlapLeft + overlapRight) / 2, (overlapTop + overlapBottom) / 2);
  assert(
    Boolean(overlapHit && repoMenuEl!.contains(overlapHit)),
    `Repository dropdown must receive pointer hits above floating Toast in overlapping region (got ${overlapHit?.className ?? 'null'})`,
  );
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  await click('Dismiss notice');
  assert(!document.querySelector('.operation-notice'), 'Clicking Dismiss notice must clear the floating Toast');

  // Done message dismissal via inline × (collapsed) and header × (expanded) also persists read state via backend:
  const dismissedDoneIds: string[] = [];
  const doneDismissBackend: WorkbenchBackend = {
    ...disconnectedBackend,
    async dismissSessionDone(_target, _runId, messageId) {
      dismissedDoneIds.push(messageId);
    },
  };
  await mount(fixture('terminal-done'), doneDismissBackend);
  assert(!!document.querySelector('.message-collapsed'), 'Collapsed done fixture must show completed summary bar initially');
  await click('Dismiss completed notice');
  assert(!document.querySelector('.message-collapsed') && !document.querySelector('.message-expanded'), 'Clicking inline × on done bar must dismiss the message completely');
  await mount(fixture('done'), doneDismissBackend);
  assert(!!document.querySelector('.message-expanded'), 'Expanded done fixture must open detail card');
  await click('Dismiss completed notice');
  assert(!document.querySelector('.message-collapsed') && !document.querySelector('.message-expanded'), 'Clicking header × on done card must dismiss the message instead of collapsing it back');
  assert(dismissedDoneIds.length === 2 && dismissedDoneIds.every((id) => id === 'done'), `Expected both done dismissals to notify backend persistence: ${JSON.stringify(dismissedDoneIds)}`);

  // Multi-question message_user questionnaire with preview, multi-select, and dontAskAgain:
  const questionnaireCommands: WorkbenchCommand[] = [];
  const qState = fixture('typing');
  qState.worktrees[0].dockExpanded = true;
  qState.worktrees[0].sessionStatus = 'needsDecision';
  qState.worktrees[0].messages = [{
    id: 'msg-ask-q',
    kind: 'question',
    title: 'Confirm release tag action',
    scope: 'tag v0.2.0',
    description: 'Review commands and options before running.',
    questionnaire: {
      questions: [
        {
          id: 'strategy',
          header: 'Tag',
          question: 'How should v0.2.0 be created?',
          multiSelect: false,
          options: [
            {
              id: 'annotated',
              label: 'Annotated tag',
              recommended: true,
              description: 'Creates signed/annotated tag and pushes it.',
              preview: 'git tag -a v0.2.0 -m "Release v0.2.0"\ngit push origin v0.2.0',
            },
            {
              id: 'lightweight',
              label: 'Lightweight tag',
              description: 'Creates lightweight pointer only.',
            },
          ],
        },
        {
          id: 'checks',
          header: 'Checks',
          question: 'Which extra steps should run?',
          multiSelect: true,
          options: [
            { id: 'release', label: 'Draft GitHub release', description: 'Opens a draft release with notes.' },
            { id: 'changelog', label: 'Update changelog', description: 'Prepends v0.2.0 entry.' },
          ],
        },
      ],
      context: {
        scope: 'tag v0.2.0',
        allowDontAskAgain: true,
        dontAskAgainLabel: "Don't ask again for this button",
      },
    },
  }];
  await mount(qState, {
    ...disconnectedBackend,
    async execute(command) {
      questionnaireCommands.push(command);
    },
  });
  assert(document.querySelectorAll('.questionnaire-tab').length === 2, 'Multi-question dock must render one tab per question');
  await click('Preview');
  assert(document.querySelector('.d2-preview-pre')?.textContent?.includes('git tag -a v0.2.0'), 'Clicking Preview must reveal inline preview snippet');
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-opt-btn[data-option-id="annotated"]')!);
  const dontAskCheckbox = document.querySelector<HTMLInputElement>('.dont-ask-toggle input[type="checkbox"]')!;
  await act(async () => { dontAskCheckbox.click(); });
  await clickElement(document.querySelectorAll<HTMLButtonElement>('.questionnaire-tab')[1]);
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-opt-btn[data-option-id="release"]')!);
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-opt-btn[data-option-id="changelog"]')!);
  await click('Submit');
  const submitted = questionnaireCommands.find((cmd) => cmd.kind === 'respond');
  assert(
    submitted?.kind === 'respond'
      && submitted.questionnaireResponse?.dontAskAgain === true
      && submitted.questionnaireResponse.answers.find((item) => item.questionId === 'strategy')?.selectedLabels.includes('Annotated tag')
      && submitted.questionnaireResponse.answers.find((item) => item.questionId === 'checks')?.selectedLabels.length === 2,
    `message_user Submit did not forward structured answers and dontAskAgain: ${JSON.stringify(submitted)}`,
  );
  checks.push('Phase 1 UX: zero-shift semantic floating Toast, MessageDock true done dismissal, and multi-question message_user flow');

  // Model ID preset auto-fill (Kimi 1M/256K presets) without overwriting manual user edits:
  {
    await mount(fixture('empty'));
    await click('Configure Model');
    await click('Custom Compatible');
    const presetModelInput = document.querySelector<HTMLInputElement>('[data-field="model-id"]')!;
    await clickElement(document.querySelector<HTMLButtonElement>('.advanced-toggle')!);
    const readPresetCtx = () => document.querySelector<HTMLInputElement>('input[aria-label="Context Window"]')!;
    const readPresetOut = () => document.querySelector<HTMLInputElement>('input[aria-label="Max Output Tokens"]')!;
    type(presetModelInput, 'kimi-k3.1-pro'); await tick();
    assert(readPresetCtx().value === '1048576' && readPresetOut().value === '131072', `Typing kimi-k3.1-pro should auto-fill 1048576 / 131072 (got ${readPresetCtx().value} / ${readPresetOut().value})`);
    type(presetModelInput, 'deepseek-v4-pro'); await tick();
    assert(readPresetCtx().value === '1048576' && readPresetOut().value === '393216', `Typing deepseek-v4-pro should auto-fill 1048576 / 393216 (got ${readPresetCtx().value} / ${readPresetOut().value})`);
    type(presetModelInput, 'deepseek-reasoner'); await tick();
    assert(readPresetCtx().value === '131072' && readPresetOut().value === '65536', `Typing deepseek-reasoner should auto-fill 131072 / 65536 (got ${readPresetCtx().value} / ${readPresetOut().value})`);
    type(presetModelInput, 'deepseek-chat'); await tick();
    assert(readPresetCtx().value === '131072' && readPresetOut().value === '8192', `Typing deepseek-chat should auto-fill 131072 / 8192 (got ${readPresetCtx().value} / ${readPresetOut().value})`);
    type(presetModelInput, 'kimi-k2.6'); await tick();
    assert(readPresetCtx().value === '262144' && readPresetOut().value === '32768', `Switching to kimi-k2.6 should update untouched defaults to 262144 / 32768 (got ${readPresetCtx().value} / ${readPresetOut().value})`);
    type(readPresetCtx(), '500000'); await tick();
    type(presetModelInput, 'kimi-k2.8'); await tick();
    assert(readPresetCtx().value === '500000' && readPresetOut().value === '131072', `Manual contextWindow edit must not be overwritten while untouched maxOutputTokens updates (got ${readPresetCtx().value} / ${readPresetOut().value})`);
    await click('Cancel');
    checks.push('Model ID preset auto-fill for Kimi & DeepSeek models while preserving manual user overrides');
  }

  // Fetch remote provider models + filter/select + manual fallback (both Add Provider and Add Model):
  {
    const listRequests: Array<Record<string, unknown>> = [];
    let shouldFailList = false;
    const remoteBackend: WorkbenchBackend = {
      ...disconnectedBackend,
      async listProviderModels(input) {
        listRequests.push(input as unknown as Record<string, unknown>);
        if (shouldFailList) {
          throw new Error('Authentication failed (401/403).');
        }
        return [
          { id: 'kimi-k3.1-pro', displayName: 'Kimi K3.1 Pro' },
          { id: 'kimi-k2.6' },
          { id: 'custom-gateway-512k', contextWindow: 512_000, maxOutputTokens: 65_536 },
        ];
      },
    };
    await mount(fixture('empty'), remoteBackend);
    await click('Configure Model');
    await click('Custom Compatible');
    type(document.querySelector<HTMLInputElement>('[data-field="base-url"]')!, 'https://api.moonshot.cn/v1');
    type(document.querySelector<HTMLInputElement>('[data-field="api-key"]')!, 'sk-test-fetch-1234');
    await tick();
    await clickElement(document.querySelector<HTMLButtonElement>('[data-action="fetch-models"]')!);
    await tick();
    const pickerEl = document.querySelector<HTMLElement>('.remote-model-picker')!;
    const nameFieldBeforeClose = document.querySelector<HTMLElement>('[data-field="model-name"]')!.getBoundingClientRect();
    assert(listRequests.length === 1 && listRequests[0].kind === 'newConnection', 'Fetch Models in Add Provider must dispatch newConnection list request');
    assert(document.querySelectorAll('.remote-model-option').length === 3 && getComputedStyle(pickerEl).position === 'absolute', 'Fetched model picker must float as an absolute dropdown with all 3 remote models');
    const modelInput = document.querySelector<HTMLInputElement>('[data-field="model-id"]')!;
    type(modelInput, 'gateway'); await tick();
    assert(document.querySelectorAll('.remote-model-option').length === 1, 'Typing in Model ID must live-filter the fetched model list');
    await clickElement(document.querySelector<HTMLButtonElement>('[data-remote-model="custom-gateway-512k"]')!);
    const nameFieldAfterClose = document.querySelector<HTMLElement>('[data-field="model-name"]')!.getBoundingClientRect();
    assert(Math.abs(nameFieldBeforeClose.top - nameFieldAfterClose.top) <= 0.5, 'Opening/closing floating model dropdown must not shift fields below');
    await clickElement(document.querySelector<HTMLButtonElement>('.advanced-toggle')!);
    assert(
      document.querySelector<HTMLInputElement>('[data-field="model-id"]')!.value === 'custom-gateway-512k'
        && document.querySelector<HTMLInputElement>('input[aria-label="Context Window"]')!.value === '512000'
        && document.querySelector<HTMLInputElement>('input[aria-label="Max Output Tokens"]')!.value === '65536',
      'Selecting a fetched model with API token metadata must populate Model ID and token limits',
    );
    await clickElement(document.querySelector<HTMLButtonElement>('.model-picker-toggle')!);
    await clickElement(document.querySelector<HTMLButtonElement>('[data-remote-model="kimi-k3.1-pro"]')!);
    assert(
      document.querySelector<HTMLInputElement>('[data-field="model-id"]')!.value === 'kimi-k3.1-pro'
        && document.querySelector<HTMLInputElement>('input[aria-label="Context Window"]')!.value === '1048576'
        && document.querySelector<HTMLInputElement>('input[aria-label="Max Output Tokens"]')!.value === '131072',
      'Switching to a fetched Kimi model must apply Kimi preset limits cleanly',
    );
    await click('Cancel');

    // Add Model under existing connection must pass ONLY connectionId (zero API keys) and keep manual entry working on error:
    await mount(fixture('typing'), remoteBackend);
    await click('Settings');
    await openSettingsTab('models');
    await clickElement(document.querySelector<HTMLButtonElement>('.provider-add-model')!);
    shouldFailList = true;
    type(document.querySelector<HTMLInputElement>('[data-field="model-id"]')!, 'manual-typed-model'); await tick();
    await clickElement(document.querySelector<HTMLButtonElement>('[data-action="fetch-models"]')!);
    await tick();
    const lastListReq = listRequests.at(-1)!;
    assert(
      lastListReq.kind === 'existingConnection'
        && typeof lastListReq.connectionId === 'string'
        && !('apiKey' in lastListReq)
        && document.querySelector<HTMLInputElement>('[data-field="model-id"]')!.value === 'manual-typed-model'
        && text().includes('401/403'),
      'Add Model fetch must transmit only connectionId and preserve manual input on fetch error',
    );
    await click('Cancel');
    await click('Close Settings');
    checks.push('Remote provider model list fetching, live filtering, auto-fill, connectionId-only child fetch, and manual fallback');
  }
}
