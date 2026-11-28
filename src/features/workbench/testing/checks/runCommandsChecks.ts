import { act, createElement } from 'react';
import type { RunEnvEntry, RunProcessSnapshot } from '../../../../../shared/run-protocol';
import type { TrayStateSnapshot } from '../../../../../shared/tray-protocol';
import { TrayPopover } from '../../../tray/TrayPopover';
import { disconnectedBackend, type WorkbenchBackend } from '../../commands';
import { fixture } from '../fixtures';
import { assert, button, clickElement, createControlledWindow, text, tick, type, type BrowserCheckHarness } from './harness';

export async function runRunCommandsChecks(harness: BrowserCheckHarness) {
  const { root, checks, mount } = harness;

  let liveProcesses: RunProcessSnapshot[] = [];
  let notifyRunState: ((next: RunProcessSnapshot[]) => void) | undefined;
  const openedLogs: string[] = [];
  const openedUrls: string[] = [];
  const startedCommands: string[] = [];
  const stoppedProcesses: string[] = [];

  const runBackend: WorkbenchBackend = {
    ...disconnectedBackend,
    async listRunProcesses() {
      return liveProcesses;
    },
    subscribeRunState(receive) {
      notifyRunState = receive;
      return () => { notifyRunState = undefined; };
    },
    async discoverRunCommands(request, onProgress) {
      onProgress?.({
        protocolVersion: 1,
        requestId: request.requestId,
        status: 'running',
        statusLabel: 'Reading package.json...',
      });
      return {
        protocolVersion: 1,
        kind: 'discoverRunCommands',
        requestId: request.requestId,
        status: 'done',
        summary: 'Detected 2 runnable commands.',
        commands: [
          {
            label: 'Desktop Dev',
            command: 'pnpm desktop dev --no-watch',
            description: 'Launch desktop dev environment.',
            category: 'dev',
          },
          {
            label: 'Web Dev',
            command: 'pnpm dev',
            cwd: 'web',
            description: 'Launch frontend dev server.',
            category: 'dev',
          },
        ],
      };
    },
    async startRunCommand(request) {
      startedCommands.push(request.command.command);
      const snap: RunProcessSnapshot = {
        processId: `${request.target.worktreeId}::${request.command.id}`,
        generation: 1,
        repositoryId: request.target.repositoryId,
        repositoryName: 'orders-service',
        worktreeId: request.target.worktreeId,
        worktreeLabel: 'orders-service',
        commandId: request.command.id,
        label: request.command.label,
        command: request.command.command,
        cwd: request.command.cwd ?? '.',
        status: 'running',
        pid: 4242,
        startedAtMs: Date.now(),
        detectedUrls: ['http://localhost:1420/'],
        logLineCount: 3,
        lastLogLine: 'Local: http://localhost:1420/ (ready in 140ms)',
      };
      liveProcesses = [
        snap,
        ...liveProcesses.filter((p) => p.processId !== snap.processId),
      ];
      notifyRunState?.(liveProcesses);
      return snap;
    },
    async stopRunCommand(request) {
      stoppedProcesses.push(request.processId);
      const prev = liveProcesses.find((p) => p.processId === request.processId)!;
      const stopped: RunProcessSnapshot = {
        ...prev,
        status: 'exited',
        pid: undefined,
        exitCode: 0,
        endedAtMs: Date.now(),
      };
      liveProcesses = liveProcesses.map((p) => (p.processId === request.processId ? stopped : p));
      notifyRunState?.(liveProcesses);
      return stopped;
    },
    async openRunLogsWindow(request) {
      openedLogs.push(request?.processId ?? '');
    },
    async openRunLocalUrl(request) {
      openedUrls.push(request.url);
    },
  };

  // 1. Full mode: Git | Run segmented view switch -> empty panel -> AI scan modal (with proper overlay-footer & no hint prose) -> start -> port chip -> live log preview -> stop:
  await mount(fixture('typing'), runBackend);
  const runTab = document.querySelector<HTMLButtonElement>('[data-main-view="run"]');
  const gitTab = document.querySelector<HTMLButtonElement>('[data-main-view="git"]');
  assert(!!runTab && !!gitTab, 'Header Row 2 must render Git | Run segmented view tabs when a repository worktree is open');
  await clickElement(runTab);
  assert(
    !!document.querySelector('.run-services-panel') && text().includes('No run commands yet'),
    'Switching to Run view on an empty repository should show clean 0-command empty panel',
  );

  await clickElement(button('Scan with AI'));
  await tick();
  assert(
    !!document.querySelector('.overlay .overlay-footer .form-actions')
      && !document.querySelector('.run-discover-hint')
      && text().includes('Detected 2 runnable commands.'),
    'AI discovery modal must use padded .overlay-footer without redundant hint text and list scanned commands',
  );
  await clickElement(button('Add Selected (2)'));
  await tick();

  assert(
    text().includes('Desktop Dev') && text().includes('Web Dev'),
    'Added run commands should render as service cards in RunServicesPanel',
  );
  await clickElement(document.querySelector<HTMLButtonElement>('.run-start-icon')!);
  await tick();
  assert(
    startedCommands.includes('pnpm desktop dev --no-watch')
      && runTab.classList.contains('has-running')
      && text().includes(':1420')
      && text().includes('ready in 140ms'),
    'Starting a service must update Run tab running indicator, show clickable port chip, and render live last-line output preview',
  );

  // Clicking port chip opens validated local URL; clicking Logs pill opens pop-out logs window; starting a second service exposes Stop All:
  await clickElement(document.querySelector<HTMLButtonElement>('.run-port-chip')!);
  assert(openedUrls.includes('http://localhost:1420/'), 'Clicking detected port chip must invoke openRunLocalUrl');
  await clickElement(document.querySelector<HTMLButtonElement>('.run-log-pill')!);
  assert(openedLogs.length === 1 && openedLogs[0].length > 0, 'Clicking Logs pill must invoke openRunLogsWindow with processId');

  // Start second service so 2 services run simultaneously -> Stop All appears and stops both:
  await clickElement(document.querySelector<HTMLButtonElement>('.run-start-icon')!);
  await tick();
  const stopAllBtn = button('Stop All');
  assert(!!stopAllBtn, 'Toolbar must reveal Stop All when 2+ services are running');
  await clickElement(stopAllBtn);
  await tick();
  assert(stoppedProcesses.length === 2, `Stop All must stop both running services (got ${stoppedProcesses.length})`);
  const telemetryStrip = document.querySelector<HTMLElement>('.run-telemetry-strip');
  const telemetryOutput = telemetryStrip?.querySelector<HTMLButtonElement>('.run-telemetry-output');
  const telemetryActions = telemetryStrip?.querySelector<HTMLElement>('.run-telemetry-actions');
  const telemetryStyle = telemetryOutput ? getComputedStyle(telemetryOutput) : null;
  const stripRect = telemetryStrip?.getBoundingClientRect();
  const outputRect = telemetryOutput?.getBoundingClientRect();
  const outputChildRect = telemetryOutput?.firstElementChild?.getBoundingClientRect();
  const actionsRect = telemetryActions?.getBoundingClientRect();
  assert(
    !!telemetryStyle
      && !!stripRect
      && !!outputRect
      && !!outputChildRect
      && !!actionsRect
      && telemetryStyle.justifyContent !== 'center'
      && telemetryStyle.textAlign === 'left'
      && outputRect.width >= 150
      && outputChildRect.left - outputRect.left <= 2
      && stripRect.right - actionsRect.right <= 12,
    `Telemetry strip must left-align status/log output while keeping actions pinned right (justifyContent=${telemetryStyle?.justifyContent}, inset=${outputRect && outputChildRect ? Math.round(outputChildRect.left - outputRect.left) : 'n/a'})`,
  );

  // Edit command modal supports picking a custom built-in language icon (e.g. Java / JVM) and saves it onto the card:
  await clickElement(document.querySelector<HTMLButtonElement>('[aria-label="Edit"]')!);
  await tick();
  await clickElement(button('Java / JVM'));
  await clickElement(button('Save Command'));
  await tick();
  assert(
    !!document.querySelector('.run-service-card .run-icon-well.tone-java'),
    'Manual icon selection in RunCommandModal must persist and render the selected built-in SVG icon',
  );

  // Starting state must keep Stop enabled so startup can be cancelled before spawn completes:
  let releasePendingStart: ((snap: RunProcessSnapshot) => void) | undefined;
  let startingCancelRequested = false;
  const pendingStartBackend: WorkbenchBackend = {
    ...runBackend,
    async startRunCommand(request) {
      const pid = `${request.target.worktreeId}::${request.command.id}`;
      const startingSnap: RunProcessSnapshot = {
        processId: pid,
        generation: 1,
        repositoryId: request.target.repositoryId,
        repositoryName: 'orders-service',
        worktreeId: request.target.worktreeId,
        worktreeLabel: 'orders-service',
        commandId: request.command.id,
        label: request.command.label,
        command: request.command.command,
        cwd: request.command.cwd ?? '.',
        status: 'starting',
        startedAtMs: Date.now(),
        detectedUrls: [],
        logLineCount: 0,
      };
      liveProcesses = [startingSnap, ...liveProcesses.filter((p) => p.processId !== pid)];
      notifyRunState?.(liveProcesses);
      return await new Promise<RunProcessSnapshot>((resolve) => {
        releasePendingStart = resolve;
      });
    },
    async stopRunCommand(request) {
      startingCancelRequested = true;
      const prev = liveProcesses.find((p) => p.processId === request.processId)!;
      const cancelled: RunProcessSnapshot = {
        ...prev,
        status: 'exited',
        endedAtMs: Date.now(),
      };
      liveProcesses = liveProcesses.map((p) => (p.processId === request.processId ? cancelled : p));
      notifyRunState?.(liveProcesses);
      releasePendingStart?.(cancelled);
      return cancelled;
    },
  };
  const startingState = fixture('typing');
  const startRepoId = startingState.worktrees[0].repositoryId;
  startingState.repositorySettings[startRepoId] = {
    editor: 'idea',
    commitLanguage: 'en',
    runCommands: [{ id: 'run-slow-start', label: 'Slow Start API', command: 'sleep 60' }],
  };
  await mount(startingState, pendingStartBackend);
  await clickElement(document.querySelector<HTMLButtonElement>('[data-main-view="run"]')!);
  // Click Start without awaiting the blocked IPC; emit puts the card into `starting` immediately:
  await act(async () => {
    document.querySelector<HTMLButtonElement>('.run-start-icon')!.click();
  });
  await tick();
  const stopWhileStartingBtn = document.querySelector<HTMLButtonElement>('.run-stop-icon');
  assert(
    !!stopWhileStartingBtn && !stopWhileStartingBtn.disabled && text().includes('Starting'),
    'Stop button must remain enabled while process status is starting so users can cancel startup',
  );
  await clickElement(stopWhileStartingBtn!);
  await tick();
  assert(
    startingCancelRequested && !text().includes('Started Slow Start API.'),
    'Cancelling during starting must invoke stopRunCommand and must not show a bogus Started notice',
  );

  // Switch back to Git view cleanly:
  const gitTabAfterCancel = document.querySelector<HTMLButtonElement>('[data-main-view="git"]')!;
  await clickElement(gitTabAfterCancel);
  assert(!!document.querySelector('.changes-panel'), 'Switching back to Git tab must restore ChangesPanel');

  // 2. Multi-module Spring Cloud workflow: grouped AI scan -> filter -> Shared Env -> Running Now pinning & collapsible groups:
  let multiSharedEnvSeen: RunEnvEntry[] | undefined;
  let multiLiveProcesses: RunProcessSnapshot[] = [];
  let notifyMultiState: ((next: RunProcessSnapshot[]) => void) | undefined;
  const multiBackend: WorkbenchBackend = {
    ...disconnectedBackend,
    async listRunProcesses() {
      return multiLiveProcesses;
    },
    subscribeRunState(receive) {
      notifyMultiState = receive;
      return () => { notifyMultiState = undefined; };
    },
    async discoverRunCommands(request) {
      return {
        protocolVersion: 1,
        kind: 'discoverRunCommands',
        requestId: request.requestId,
        status: 'done',
        summary: 'Detected root Maven install and 6 Spring Boot services.',
        commands: [
          {
            label: 'scm-service [install]',
            command: './mvnw install -DskipTests',
            group: 'Maven',
            icon: 'java',
            description: 'Install root workspace dependencies skipping tests.',
            category: 'build',
          },
          ...['Aggregation', 'Alarm', 'Business', 'Contract', 'Data', 'Fee'].map((name) => ({
            label: `${name}ServerApplication`,
            command: `./mvnw -pl scm-modules/scm-${name.toLowerCase()}-starter spring-boot:run`,
            group: 'Spring Boot',
            icon: 'spring',
            description: `Start ${name} microservice.`,
            category: 'dev',
          })),
        ],
      };
    },
    async startRunCommand(request) {
      multiSharedEnvSeen = request.sharedEnv;
      const snap: RunProcessSnapshot = {
        processId: `${request.target.worktreeId}::${request.command.id}`,
        generation: 1,
        repositoryId: request.target.repositoryId,
        repositoryName: 'scm-service',
        worktreeId: request.target.worktreeId,
        worktreeLabel: 'scm-service',
        commandId: request.command.id,
        label: request.command.label,
        command: request.command.command,
        cwd: request.command.cwd ?? '.',
        status: 'running',
        pid: 5150,
        startedAtMs: Date.now(),
        detectedUrls: ['http://localhost:8082/'],
        logLineCount: 2,
        lastLogLine: 'Started BusinessServerApplication in 4.2s',
      };
      multiLiveProcesses = [snap, ...multiLiveProcesses.filter((p) => p.processId !== snap.processId)];
      notifyMultiState?.(multiLiveProcesses);
      return snap;
    },
  };

  await mount(fixture('typing'), multiBackend);
  await clickElement(document.querySelector<HTMLButtonElement>('[data-main-view="run"]')!);
  await clickElement(button('Scan with AI'));
  await tick();
  const firstDiscoverCopy = document.querySelector<HTMLButtonElement>('.run-discover-row-copy');
  assert(
    !!document.querySelector('[data-discover-group="Maven"]')
      && !!document.querySelector('[data-discover-group="Spring Boot"]')
      && !!document.querySelector('[data-run-discover-filter]')
      && document.querySelectorAll('.run-discover-row input[type="text"]').length === 0
      && !!firstDiscoverCopy
      && getComputedStyle(firstDiscoverCopy).alignItems === 'stretch'
      && getComputedStyle(firstDiscoverCopy).textAlign === 'left',
    'Multi-module AI scan modal must group candidates cleanly, left-align row copy, and avoid permanent text inputs',
  );
  // Collapsing and expanding a group inside DiscoverRunCommandsModal works smoothly:
  await clickElement(document.querySelector<HTMLButtonElement>('[data-discover-group-toggle="Spring Boot"]')!);
  await tick();
  assert(
    document.querySelector('[data-discover-group="Spring Boot"] .run-discover-group-list') === null,
    'Clicking a group toggle in DiscoverRunCommandsModal must collapse its candidate rows',
  );
  await clickElement(document.querySelector<HTMLButtonElement>('[data-discover-group-toggle="Spring Boot"]')!);
  await tick();
  assert(
    !!document.querySelector('[data-discover-group="Spring Boot"] .run-discover-group-list'),
    'Clicking a collapsed group toggle in DiscoverRunCommandsModal must expand its candidate rows',
  );
  await clickElement(button('Add Selected (7)'));
  await tick();
  const firstRunGroupHeader = document.querySelector<HTMLButtonElement>('[data-run-group-toggle="Maven"]');
  const headerStyle = firstRunGroupHeader ? getComputedStyle(firstRunGroupHeader) : null;
  const headerRect = firstRunGroupHeader?.getBoundingClientRect();
  const headerFirstChildRect = firstRunGroupHeader?.firstElementChild?.getBoundingClientRect();
  assert(
    !!document.querySelector('[data-run-filter-input]')
      && !!document.querySelector('[data-run-group="Maven"]')
      && !!document.querySelector('[data-run-group="Spring Boot"]')
      && !!headerStyle
      && !!headerRect
      && !!headerFirstChildRect
      && headerStyle.justifyContent !== 'center'
      && headerStyle.textAlign === 'left'
      && headerRect.width >= 200
      && headerFirstChildRect.left - headerRect.left <= 16
      && document.querySelectorAll('.run-service-card.is-compact').length === 7,
    `Multi-module RunServicesPanel must show quick filter, left-aligned collapsible groups, and compact rows for >= 6 commands (justifyContent=${headerStyle?.justifyContent}, textAlign=${headerStyle?.textAlign}, inset=${headerRect && headerFirstChildRect ? Math.round(headerFirstChildRect.left - headerRect.left) : 'n/a'})`,
  );

  // Configure repository Shared Env with preset chips and verify it propagates on Start:
  await clickElement(document.querySelector<HTMLButtonElement>('[data-run-shared-env-btn]')!);
  await tick();
  await clickElement(button('+ SPRING_PROFILES_ACTIVE=dev'));
  await clickElement(button('Save Shared Env'));
  await tick();
  assert(
    document.querySelector('.run-toolbar-env-count')?.textContent?.trim() === '1',
    'Toolbar Shared Env button must show count badge after saving shared env',
  );

  // Filter down to BusinessServerApplication, start it, clear filter, and verify it pins into Running Now:
  const filterInput = document.querySelector<HTMLInputElement>('[data-run-filter-input]')!;
  type(filterInput, 'Business');
  await tick();
  assert(
    document.querySelectorAll('.run-service-card').length === 1 && text().includes('BusinessServerApplication'),
    'Quick filter must narrow visible services by query',
  );
  await clickElement(document.querySelector<HTMLButtonElement>('.run-start-icon')!);
  await tick();
  type(filterInput, '');
  await tick();
  assert(
    multiSharedEnvSeen?.some((e) => e.key === 'SPRING_PROFILES_ACTIVE' && e.value === 'dev')
      && !!document.querySelector('[data-run-running-section]')
      && !!document.querySelector('[data-run-running-section] .run-service-card.is-running')
      && text().includes('1 running'),
    'Starting a service in a multi-module repo must forward sharedEnv, pin active card into Running Now, and show group running badge',
  );

  // Collapsing Spring Boot group hides its idle compact rows cleanly:
  await clickElement(document.querySelector<HTMLButtonElement>('[data-run-group-toggle="Spring Boot"]')!);
  await tick();
  assert(
    document.querySelector('[data-run-group="Spring Boot"] .run-group-body') === null,
    'Clicking a group header must collapse its idle services',
  );

  // 3. Compact mode: borderless IDEA quick-run controls in Row 2 open CompactRunDrawer & support 1-click start:
  const compactState = fixture('compact');
  const repoId = compactState.worktrees[0].repositoryId;
  compactState.repositorySettings[repoId] = {
    editor: 'idea',
    commitLanguage: 'en',
    runCommands: [{ id: 'run-c1', label: 'Desktop Dev', command: 'pnpm desktop dev --no-watch' }],
  };
  const winMock = createControlledWindow();
  await mount(compactState, runBackend, winMock.controller);
  const compactRunTrigger = document.querySelector<HTMLButtonElement>('[data-compact-run-trigger]');
  assert(!!compactRunTrigger, 'Compact mode must expose IDEA quick-run picker trigger');
  await clickElement(compactRunTrigger);
  assert(!!document.querySelector('.compact-run-drawer') && text().includes('Desktop Dev'), 'Compact Run drawer should render repository run commands');

  // 3. Menu Bar Tray Popover: Running Services section with direct Stop & Open Logs actions:
  const trayEvents: string[] = [];
  const traySnap: TrayStateSnapshot = {
    protocolVersion: 1,
    locale: 'zh',
    theme: 'dark',
    hasUpdate: false,
    refreshing: false,
    repositories: [],
    runningServices: [
      {
        processId: 'wt-1::run-1',
        repositoryId: 'repo-1',
        repositoryName: 'Picket',
        worktreeId: 'wt-1',
        commandId: 'run-1',
        label: 'Desktop Dev',
        status: 'running',
      },
    ],
  };
  await act(async () => {
    root.render(createElement(TrayPopover, {
      snapshot: traySnap,
      onRefresh: () => {},
      onOpenSettings: () => {},
      onOpenMainWindow: () => {},
      onStopRunService: (pid) => { trayEvents.push(`stop:${pid}`); },
      onOpenRunLogs: (pid) => { trayEvents.push(`logs:${pid}`); },
      onQuit: () => {},
      onDismiss: () => {},
    }));
  });
  await tick();
  const trayText = document.body.textContent ?? '';
  assert(!!document.querySelector('[data-tray-running-services]') && trayText.includes('Desktop Dev'), 'Tray popover should display running services section');
  await clickElement(document.querySelector<HTMLButtonElement>('.tray-stop-btn')!);
  await clickElement(document.querySelector<HTMLButtonElement>('.tray-running-info')!);
  assert(
    trayEvents.includes('stop:wt-1::run-1') && trayEvents.includes('logs:wt-1::run-1'),
    `Tray popover running service controls failed: ${JSON.stringify(trayEvents)}`,
  );

  // 4. Independent MessageDock over Run view (A3) & deleting a running Run command with confirmation (A4) & Discover error retry:
  {
    const a3State = fixture('question');
    const a3RepoId = a3State.worktrees[0].repositoryId;
    a3State.repositorySettings[a3RepoId] = {
      editor: 'idea',
      commitLanguage: 'en',
      runCommands: [{ id: 'run-a4', label: 'Running API', command: 'node server.js' }],
    };
    let a4StoppedIds: string[] = [];
    let discoverFailOnce = true;
    const a4RunningSnap: RunProcessSnapshot = {
      processId: `${a3State.worktrees[0].worktreeId}::run-a4`,
      generation: 1,
      repositoryId: a3RepoId,
      repositoryName: 'my-order-service',
      worktreeId: a3State.worktrees[0].worktreeId,
      worktreeLabel: 'wt/pay-retry',
      commandId: 'run-a4',
      label: 'Running API',
      command: 'node server.js',
      cwd: '.',
      status: 'running',
      pid: 9999,
      startedAtMs: Date.now(),
      detectedUrls: [],
      logLineCount: 1,
      lastLogLine: 'Listening on :3000',
    };
    let a4Processes: RunProcessSnapshot[] = [a4RunningSnap];
    let a4Notify: ((next: RunProcessSnapshot[]) => void) | undefined;
    const a34Backend: WorkbenchBackend = {
      ...disconnectedBackend,
      async listRunProcesses() {
        return a4Processes;
      },
      subscribeRunState(receive) {
        a4Notify = receive;
        return () => { a4Notify = undefined; };
      },
      async stopRunCommand(request) {
        a4StoppedIds.push(request.processId);
        const stopped: RunProcessSnapshot = {
          ...a4RunningSnap,
          status: 'exited',
          pid: undefined,
          exitCode: 0,
          endedAtMs: Date.now(),
        };
        a4Processes = [stopped];
        a4Notify?.(a4Processes);
        return stopped;
      },
      async discoverRunCommands(request) {
        if (discoverFailOnce) {
          discoverFailOnce = false;
          throw new Error('Temporary scan failure');
        }
        return {
          protocolVersion: 1,
          kind: 'discoverRunCommands',
          requestId: request.requestId,
          status: 'done',
          summary: 'Recovered on retry.',
          commands: [],
        };
      },
    };
    await mount(a3State, a34Backend);
    await tick();
    // Switch to Run tab while MessageDock is expanded -> dock MUST stay expanded over RunServicesPanel:
    await clickElement(document.querySelector<HTMLButtonElement>('[data-main-view="run"]')!);
    await tick();
    const runPanelWithDock = document.querySelector<HTMLElement>('.run-services-panel');
    assert(
      !!runPanelWithDock
        && !!document.querySelector('.lower-workspace.is-dock-expanded')
        && !!document.querySelector('.message-expanded')
        && runPanelWithDock.dataset.layoutMode !== 'idle',
      'Switching to Run view must keep MessageDock expanded and share space with RunServicesPanel',
    );
    // Collapsing & re-expanding MessageDock while on Run view must stay on Run view:
    await clickElement(document.querySelector<HTMLButtonElement>('.message-expanded [aria-label="Collapse Message"]')!);
    await tick();
    assert(
      !!document.querySelector('.run-services-panel') && !!document.querySelector('.message-collapsed'),
      'Collapsing MessageDock on Run view must keep Run view active',
    );
    await clickElement(document.querySelector<HTMLButtonElement>('.message-collapsed')!);
    await tick();
    assert(
      !!document.querySelector('.run-services-panel') && !!document.querySelector('.message-expanded'),
      'Expanding MessageDock on Run view must stay on Run view instead of jumping back to Git',
    );
    await clickElement(document.querySelector<HTMLButtonElement>('.message-expanded [aria-label="Collapse Message"]')!);
    await tick();

    // First-scan failure in DiscoverRunCommandsModal must expose Scan Again retry button:
    await clickElement(button('AI Scan'));
    await tick();
    assert(
      text().includes('Temporary scan failure') && !!button('Scan Again'),
      'First-scan error in DiscoverRunCommandsModal must display error and Scan Again retry button',
    );
    await clickElement(button('Scan Again'));
    await tick();
    assert(text().includes('Recovered on retry.'), 'Clicking Scan Again after initial error must retry discovery');
    await clickElement(button('Cancel'));
    await tick();

    // Deleting a running command shows inline confirmation, stops all active instances, then deletes config:
    await clickElement(document.querySelector<HTMLButtonElement>('[aria-label="Stop"]')!);
    await tick();
    // Re-activate process in backend state & edit command via modal:
    a4StoppedIds = [];
    a4Processes = [a4RunningSnap];
    await act(async () => { a4Notify?.(a4Processes); });
    await tick();
    // Temporarily open Edit via stopped state, then emit running before clicking Delete to exercise running-delete guard:
    await act(async () => { a4Notify?.([{ ...a4RunningSnap, status: 'exited' }]); });
    await tick();
    await clickElement(document.querySelector<HTMLButtonElement>('[aria-label="Edit"]')!);
    await tick();
    await act(async () => { a4Notify?.([a4RunningSnap]); });
    await tick();
    await clickElement(button('Delete'));
    await tick();
    assert(
      !!document.querySelector('[data-run-delete-confirm]'),
      'Deleting a running Run command must show inline confirmation prompt',
    );
    await clickElement(document.querySelector<HTMLButtonElement>('[data-run-delete-confirm-btn]')!);
    await tick();
    assert(
      a4StoppedIds.includes(a4RunningSnap.processId) && !document.querySelector('[data-run-card="run-a4"]'),
      'Confirming Stop & Delete must stop the running process and remove the command config',
    );
  }

  checks.push('Local Run commands: Git|Run dual view switch, multi-module grouping/filter/Running Now/Shared Env, independent MessageDock over Run, running-delete confirmation, Discover error retry, Compact IDEA quick-run, and Tray controls');
}
