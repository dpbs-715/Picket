import { act, createElement } from 'react';
import { computeCustomActionPromptHash } from '../../../../../shared/custom-action-protocol';
import type { WorkbenchRestoreReply } from '../../../../../shared/workbench-persistence-protocol';
import { Workbench } from '../../Workbench';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchBackend, WorkbenchCommand } from '../../commands';
import type { WorkbenchEvent } from '../../state/workbench';
import { fixture } from '../fixtures';
import { repositorySnapshot } from '../repository-fixture';
import { assert, click, clickElement, openSettingsTab, text, tick, type, type BrowserCheckHarness } from './harness';

export async function runCustomActionsChecks(harness: BrowserCheckHarness) {
  const { root, checks, nextKey, createReadsBackend, setInstantMotion, waitUntil, mount } = harness;
  const reads = createReadsBackend();
  // Phase 2: Repository-bound Custom Actions, Split Button switcher (Full & Compact 400x86), Create with Pi, Sync-to-all, and Tier-2 ask_user_question roundtrip:
  {
    const phase2Sub: { receive?: (event: WorkbenchEvent) => void } = {};
    const phase2Cmds: WorkbenchCommand[] = [];
    const phase2QuestionResponses: Array<{ runId: string; callId: string; dontAskAgain?: boolean }> = [];
    let resolvePendingGen: (() => void) | null = null;
    let resolvePendingTier2: (() => void) | null = null;
    let holdTier2Completion = false;
    const phase2State = fixture('typing');
    phase2State.worktrees[0].selectedIds = [];
    phase2State.worktrees[0].files = [];

    const phase2Backend: WorkbenchBackend = {
      ...disconnectedBackend,
      subscribe(receive) {
        phase2Sub.receive = receive;
        return () => { phase2Sub.receive = undefined; };
      },
      async generateCustomAction(request, onProgress) {
        onProgress?.({
          protocolVersion: 1,
          runId: request.requestId,
          status: 'needsDecision',
          statusLabel: 'Confirm tag format',
          pendingCallId: 'gen-tc-1',
          questionnaire: {
            questions: [{
              id: 'q-tag',
              header: 'Tag Format',
              question: 'Which tag prefix should this button use?',
              options: [
                { id: 'v-prefix', label: 'v0.x.y', description: 'Standard semver v-prefix', recommended: true },
                { id: 'plain', label: '0.x.y', description: 'Plain numeric version' },
              ],
            }],
          },
        });
        await new Promise<void>((resolve) => {
          if (phase2QuestionResponses.some((r) => r.callId === 'gen-tc-1')) {
            resolve();
          } else {
            resolvePendingGen = resolve;
          }
        });
        return {
          protocolVersion: 1,
          kind: 'generateCustomAction',
          requestId: request.requestId,
          status: 'done',
          draft: {
            label: 'Release Tag',
            prompt: 'git tag -a v0.2.0 -m "Release v0.2.0" && git push origin v0.2.0',
            summary: 'Creates and pushes annotated semver tag v0.2.0.',
          },
        };
      },
      async respondActionQuestion(request) {
        phase2QuestionResponses.push({
          runId: request.runId,
          callId: request.callId,
          dontAskAgain: request.response.dontAskAgain,
        });
        if (request.callId === 'gen-tc-1') {
          resolvePendingGen?.();
          resolvePendingGen = null;
        }
        if (request.callId === 'confirm-tc-1') {
          // Intentionally let the test assert the post-IPC loading state before releasing backend completion.
          if (!holdTier2Completion) {
            resolvePendingTier2?.();
            resolvePendingTier2 = null;
          }
        }
        return true;
      },
      async execute(command) {
        phase2Cmds.push(command);
        if (command.kind === 'startAction' && command.profile === 'customAction' && command.action) {
          phase2Sub.receive?.({
            type: 'actionProgress',
            progress: {
              protocolVersion: 2,
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
                status: 'needsDecision',
                pendingCallId: 'confirm-tc-1',
                summary: '',
                message: {
                  id: 'ask-tier2-1',
                  kind: 'question',
                  title: 'Confirm Write',
                  description: 'Confirm before running custom write command',
                  questionnaire: {
                    questions: [{
                      id: 'confirm-write',
                      header: 'Confirm Write',
                      question: 'Run custom Git command?',
                      options: [
                        {
                          id: 'approve-write',
                          label: 'Run command',
                          description: 'Execute in current repository',
                          recommended: true,
                          preview: 'git tag -f v0.2.0',
                        },
                        { id: 'cancel-write', label: 'Cancel', description: 'Skip command' },
                      ],
                    }],
                    context: { allowDontAskAgain: true },
                  },
                },
                timeline: [{ state: 'active', label: 'Running git tag' }],
                qaLog: [],
                commits: [],
                links: [],
                canRetryRemaining: false,
              },
            },
          });
          await new Promise<void>((resolve) => {
            if (phase2QuestionResponses.some((r) => r.callId === 'confirm-tc-1')) {
              resolve();
            } else {
              resolvePendingTier2 = resolve;
            }
          });
          const approvedDontAsk = phase2QuestionResponses.some(
            (r) => r.callId === 'confirm-tc-1' && r.dontAskAgain === true,
          );
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
              summary: 'Tagged v0.2.0 and pushed to origin.',
              message: {
                id: 'done-action-1',
                kind: 'done',
                title: 'Release Tag completed',
                description: 'Tagged v0.2.0 and pushed to origin.',
                links: [{ label: 'Release v0.2.0', url: 'https://github.com/acme/demo/releases/tag/v0.2.0' }],
              },
              timeline: [{ state: 'done', label: 'Pushed v0.2.0' }],
              qaLog: [],
              commits: [],
              links: [{ label: 'Release v0.2.0', url: 'https://github.com/acme/demo/releases/tag/v0.2.0' }],
              canRetryRemaining: false,
            },
            ...(approvedDontAsk
              ? {
                  trustedActionUpdate: {
                    repositoryId: command.target.repositoryId,
                    actionId: command.action.id,
                    trustedPromptHash: computeCustomActionPromptHash(command.action.prompt),
                  },
                }
              : {}),
          };
        }
        return disconnectedBackend.execute(command);
      },
    };

    await mount(phase2State, phase2Backend);
    // C-Push must stay disabled on clean tree, while Split Button toggle is available:
    assert(!document.querySelector('.action-dropdown'), 'Split Button must never reuse legacy .action-dropdown selector');
    const splitToggle = document.querySelector<HTMLButtonElement>('.action-split-toggle')!;
    assert(Boolean(splitToggle), 'Full mode footer must render Split Button switcher toggle');
    await clickElement(splitToggle);
    const switcherMenu = document.querySelector<HTMLElement>('.action-switcher-menu');
    assert(Boolean(switcherMenu), 'Clicking Split Button toggle must open action switcher menu');
    const toggleRect = splitToggle.getBoundingClientRect();
    const menuRect = switcherMenu!.getBoundingClientRect();
    assert(
      menuRect.bottom <= toggleRect.top + 1
        && menuRect.top >= 0
        && menuRect.left >= 0
        && menuRect.right <= innerWidth + 1,
      `Action switcher menu must open upward within viewport bounds (menu=${JSON.stringify({ top: menuRect.top, bottom: menuRect.bottom, left: menuRect.left, right: menuRect.right })}, toggleTop=${toggleRect.top}, viewport=${innerWidth}x${innerHeight})`,
    );
    await click('Create with Picket…');
    assert(Boolean(document.querySelector('.create-pi-pane')), 'Create with Pi menu entry must open Create modal in Pi mode');
    const createDesc = document.querySelector<HTMLTextAreaElement>('.create-pi-pane textarea')!;
    type(createDesc, 'Create a semver release tag and push it'); await tick();
    await click('Generate Draft');
    await waitUntil(
      () => Boolean(document.querySelector('.create-pi-pane .option-card')),
      'create-pi-inline-question',
    );
    await clickElement(document.querySelector<HTMLButtonElement>('.create-pi-pane .option-card')!);
    await waitUntil(
      () => document.querySelector<HTMLInputElement>('.custom-action-form input')?.value === 'Release Tag',
      'create-pi-draft-populated',
    );
    assert(
      document.querySelector<HTMLInputElement>('.custom-action-form input')?.value === 'Release Tag'
        && document.querySelector<HTMLTextAreaElement>('.custom-action-form textarea')?.value.includes('git tag -a v0.2.0'),
      'Create with Pi draft did not populate the editable button form',
    );
    await clickElement(document.querySelector<HTMLInputElement>('[data-sync-select-all]')!);
    assert(
      document.querySelector<HTMLButtonElement>('.custom-action-form button[type="submit"]')?.textContent?.trim() === 'Save & Sync (1)',
      'Selecting all sync targets after Create with Pi draft must update Save button to Save & Sync (1)',
    );
    await click('Save & Sync (1)');
    assert(text().includes('added to 1'), 'Save & Sync (1) did not copy custom action to the second registered repo');

    // Active button is now "Release Tag", ActionContextStrip shows compact 1-line pill that expands into full preview drawer, and main button is clickable on clean tree!
    const mainBtn = document.querySelector<HTMLButtonElement>('.action-split-main')!;
    const contextPill = document.querySelector<HTMLButtonElement>('.action-context-pill')!;
    assert(
      mainBtn.textContent?.includes('Release Tag')
        && !mainBtn.disabled
        && Boolean(contextPill)
        && contextPill.getAttribute('aria-expanded') === 'false'
        && contextPill.textContent?.includes('Release Tag'),
      'Saved custom action must become clickable on clean tree and display compact action pill in ActionContextStrip',
    );
    await clickElement(contextPill);
    assert(
      contextPill.getAttribute('aria-expanded') === 'true'
        && Boolean(document.querySelector('.action-context-drawer .custom-action-prompt-preview')?.textContent?.includes('git tag -a v0.2.0')),
      'Clicking ActionContextStrip pill must expand custom action prompt drawer',
    );
    await clickElement(contextPill);
    await clickElement(mainBtn);
    await tick();
    assert(Boolean(document.querySelector('.message-expanded')), 'Tier-2 write confirmation should open in MessageDock');
    const approveBtn = document.querySelector<HTMLButtonElement>('.d2-opt-btn[data-option-id="approve-write"]')!;
    const cancelOptBtn = document.querySelector<HTMLButtonElement>('.d2-opt-btn[data-option-id="cancel-write"]')!;
    const primaryShell = approveBtn.closest<HTMLElement>('.d2-opt-shell.is-primary');
    assert(
      Boolean(primaryShell)
        && Boolean(primaryShell?.querySelector('.option-recommended-badge'))
        && getComputedStyle(primaryShell!).backgroundColor !== 'rgb(244, 244, 245)',
      'Recommended single-select choice must use refined tinted primary surface (not stark white slab) with Recommended badge',
    );
    holdTier2Completion = true;
    await clickElement(approveBtn);
    await tick();
    assert(
      approveBtn.disabled
        && cancelOptBtn.disabled
        && approveBtn.getAttribute('aria-busy') === 'true'
        && Boolean(primaryShell?.classList.contains('is-loading'))
        && Boolean(approveBtn.querySelector('.d2-opt-spinner .icon.spinning')),
      'Clicking a decision option must immediately lock all choices and keep a spinning loader until backend transitions',
    );
    // Repeated clicks while awaiting backend transition must be completely ignored:
    await clickElement(approveBtn);
    await clickElement(cancelOptBtn);
    assert(
      phase2QuestionResponses.filter((r) => r.callId === 'confirm-tc-1').length === 1,
      `Duplicate clicks during loading window must not send multiple responses: ${JSON.stringify(phase2QuestionResponses)}`,
    );
    holdTier2Completion = false;
    await act(async () => {
      resolvePendingTier2?.();
      resolvePendingTier2 = null;
    });
    await waitUntil(
      () => text().includes('Release Tag completed') && Boolean(document.querySelector('.d2-link-pill')),
      () => `phase2-custom-action-done (responses=${JSON.stringify(phase2QuestionResponses)}, dockText=${document.querySelector('.message-expanded, .message-collapsed')?.textContent ?? '<none>'})`,
    );
    assert(
      phase2QuestionResponses.some((r) => r.callId === 'confirm-tc-1'),
      `Expected Tier-2 approval response, got ${JSON.stringify(phase2QuestionResponses)}`,
    );
    await click('Dismiss completed notice');

    // Custom action context pill stays clean without legacy per-button Auto-approve badge and keeps drawer actions single-line:
    {
      const actionPill = document.querySelector<HTMLButtonElement>('.action-context-pill')!;
      assert(!actionPill.querySelector('.action-meta-badge.is-amber'), 'Action pill must not show legacy per-button Auto-approve badge');
      await clickElement(actionPill);
      const drawerRect = document.querySelector('.action-context-drawer')!.getBoundingClientRect();
      const actionBtns = [...document.querySelectorAll<HTMLButtonElement>('.action-context-drawer .custom-action-preview-actions button')];
      assert(
        actionBtns.length === 2
          && actionBtns.every((btn) => {
            const r = btn.getBoundingClientRect();
            return r.height <= 28 && r.right <= drawerRect.right + 1;
          }),
        'Custom action drawer header must keep Edit / C-Push buttons single-line within drawer bounds',
      );
      await clickElement(actionPill);
    }

    // Compact Mode (400 x 86) Split Button & top-left-anchored actions drawer:
    const compactActionsState = fixture('typing');
    compactActionsState.windowPrefs.compactMode = true;
    compactActionsState.repositorySettings.orders = {
      editor: 'idea',
      commitLanguage: 'en',
      customActions: [{ id: 'act-1', label: 'Release Tag', prompt: 'git tag v0.2.0' }],
      activeActionId: 'act-1',
    };
    await mount(compactActionsState, phase2Backend);
    const compactToggle = document.querySelector<HTMLButtonElement>('.compact-bar .action-split-toggle')!;
    assert(Boolean(compactToggle) && document.querySelector('.compact-bar .action-split-main')?.textContent?.includes('Release Tag'), 'Compact bar must render Split Button with active custom action');
    await clickElement(compactToggle);
    assert(Boolean(document.querySelector('.compact-actions-drawer')), 'Compact Split Button toggle must expand actions drawer');
    await clickElement(document.querySelector<HTMLButtonElement>('.compact-action-item')!);
    assert(!document.querySelector('.compact-actions-drawer') && document.querySelector('.compact-bar .action-split-main')?.textContent?.includes('C-Push'), 'Selecting C-Push in compact actions drawer must switch main button and collapse drawer');
    // Phase 2 + Pinned Space-switch resilience: verify Create with Pi, ProviderConnectionForm, MessageDock questionnaire, and + Prompt Collapse never unmount or lose state across visibilitychange (visible -> hidden -> visible) with real animations enabled:
    {
      const cancelledGenRuns: string[] = [];
      let resolveSpaceGen: (() => void) | null = null;
      const spaceSwitchState = fixture('typing');
      spaceSwitchState.windowPrefs.pinned = true;
      spaceSwitchState.worktrees[0].dockExpanded = true;
      spaceSwitchState.worktrees[0].sessionStatus = 'needsDecision';
      spaceSwitchState.worktrees[0].messages = [{
        id: 'msg-space-q',
        kind: 'question',
        title: 'Multi-step Space-switch check',
        description: 'Confirm state survives macOS Space switches.',
        questionnaire: {
          questions: [{
            id: 'q-multi',
            header: 'Options',
            question: 'Pick options to keep across Space switch',
            multiSelect: true,
            options: [
              { id: 'opt-a', label: 'Option A', description: 'First option', recommended: true },
              { id: 'opt-b', label: 'Option B', description: 'Second option' },
            ],
          }],
          context: { allowDontAskAgain: true },
        },
      }];
      const spaceSwitchBackend: WorkbenchBackend = {
        ...disconnectedBackend,
        async generateCustomAction(request, onProgress) {
          onProgress?.({
            protocolVersion: 1,
            runId: request.requestId,
            status: 'needsDecision',
            statusLabel: 'Confirm branch target',
            pendingCallId: 'space-gen-tc-1',
            questionnaire: {
              questions: [{
                id: 'q-target',
                header: 'Target',
                question: 'Which branch should this action target?',
                options: [
                  { id: 'test-branch', label: 'test', description: 'Push to test branch', recommended: true },
                  { id: 'staging-branch', label: 'staging', description: 'Push to staging branch' },
                ],
              }],
            },
          });
          await new Promise<void>((resolve) => { resolveSpaceGen = resolve; });
          return {
            protocolVersion: 1,
            kind: 'generateCustomAction',
            requestId: request.requestId,
            status: 'done',
            draft: {
              label: 'Deploy Test',
              prompt: 'submit -ch to test',
              summary: 'Submits changes to test branch.',
            },
          };
        },
        async respondActionQuestion(request) {
          if (request.callId === 'space-gen-tc-1') {
            resolveSpaceGen?.();
            resolveSpaceGen = null;
          }
          return true;
        },
        async cancelAction(requestId) {
          cancelledGenRuns.push(requestId);
        },
      };

      const simulateSpaceSwitch = async () => {
        const prevDesc = Object.getOwnPropertyDescriptor(document, 'visibilityState');
        try {
          Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
          await act(async () => { document.dispatchEvent(new Event('visibilitychange')); });
          await tick();
          Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
          await act(async () => { document.dispatchEvent(new Event('visibilitychange')); });
          await tick();
        } finally {
          if (prevDesc) Object.defineProperty(document, 'visibilityState', prevDesc);
          else delete (document as { visibilityState?: unknown }).visibilityState;
        }
      };

      setInstantMotion(false);
      try {
        await mount(spaceSwitchState, spaceSwitchBackend);

        // 1) + Prompt textarea inside <Collapse> and MessageDock questionnaire selections survive Space switch:
        const promptBefore = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
        assert(Boolean(promptBefore), 'Expected + Prompt textarea inside Collapse');
        await clickElement(document.querySelector<HTMLButtonElement>('.message-expanded .d2-opt-btn[data-option-id="opt-a"]')!);
        const dockCustomInput = document.querySelector<HTMLInputElement>('.message-expanded .d2-composer input')!;
        type(dockCustomInput, 'keep this note');
        const dockDontAsk = document.querySelector<HTMLInputElement>('.message-expanded .dont-ask-toggle input[type="checkbox"]')!;
        await act(async () => { dockDontAsk.click(); });
        await simulateSpaceSwitch();
        assert(
          document.querySelector<HTMLTextAreaElement>('.prompt-textarea') === promptBefore
            && document.querySelector('.message-expanded .d2-opt-btn[data-option-id="opt-a"]')?.classList.contains('is-selected')
            && document.querySelector<HTMLInputElement>('.message-expanded .d2-composer input')?.value === 'keep this note'
            && document.querySelector<HTMLInputElement>('.message-expanded .dont-ask-toggle input[type="checkbox"]')?.checked === true,
          'Space switch (visibilitychange) must not unmount Collapse prompt or wipe MessageDock questionnaire state',
        );

        // 2) Create with Pi session (running + inline ask_user_question) survives Space switch in Pinned mode:
        const spaceSplitState = fixture('typing');
        spaceSplitState.windowPrefs.pinned = true;
        await mount(spaceSplitState, spaceSwitchBackend);
        await clickElement(document.querySelector<HTMLButtonElement>('.action-split-toggle')!);
        await click('Create with Picket…');
        const createInput = document.querySelector<HTMLTextAreaElement>('.create-pi-pane textarea')!;
        type(createInput, 'Submit my changes to test branch');
        await tick();
        await click('Generate Draft');
        await waitUntil(() => Boolean(document.querySelector('.create-pi-pane .create-pi-question')), 'space-switch-create-pi-question');
        const questionNodeBefore = document.querySelector('.create-pi-pane .create-pi-question');
        await simulateSpaceSwitch();
        assert(
          cancelledGenRuns.length === 0
            && document.querySelector('.create-pi-pane .create-pi-question') === questionNodeBefore
            && document.querySelector<HTMLTextAreaElement>('.create-pi-pane textarea')?.value === 'Submit my changes to test branch',
          `Create with Pi session vanished or remounted across Space switch (cancelled=${JSON.stringify(cancelledGenRuns)})`,
        );
        await clickElement(document.querySelector<HTMLButtonElement>('.create-pi-pane .option-card[data-option-id="test-branch"]')!);
        await waitUntil(
          () => document.querySelector<HTMLInputElement>('.custom-action-form input')?.value === 'Deploy Test',
          'space-switch-create-pi-draft-completed',
        );
        await click('Cancel');
        await waitUntil(() => !document.querySelector('.overlay'), 'space-switch-create-modal-close');

        // 3) ProviderConnectionForm unsaved fields & Advanced Collapse survive Space switch, and occluded close works instantly:
        await click('Settings');
        await openSettingsTab('models');
        await click('Provider');
        await click('Custom Compatible');
        const urlField = document.querySelector<HTMLInputElement>('[data-field="base-url"]')!;
        const keyField = document.querySelector<HTMLInputElement>('[data-field="api-key"]')!;
        assert(Boolean(urlField) && Boolean(keyField), 'Expected Custom Compatible provider form fields');
        type(urlField, 'https://api.example.com/v1');
        type(keyField, 'sk-space-persist-test');
        await clickElement(document.querySelector<HTMLButtonElement>('.advanced-toggle')!);
        const advancedNodeBefore = document.querySelector('.model-form .motion-collapse');
        await simulateSpaceSwitch();
        assert(
          document.querySelector<HTMLInputElement>('[data-field="base-url"]')?.value === 'https://api.example.com/v1'
            && document.querySelector<HTMLInputElement>('[data-field="api-key"]')?.value === 'sk-space-persist-test'
            && document.querySelector('.model-form .motion-collapse') === advancedNodeBefore,
          'Provider form fields or Advanced Collapse reset across Space switch',
        );
        // Occluded WebView (visibilityState === 'hidden') must still close Overlay cleanly without stalling on rAF:
        const prevDesc = Object.getOwnPropertyDescriptor(document, 'visibilityState');
        try {
          Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
          await act(async () => { document.dispatchEvent(new Event('visibilitychange')); });
          await click('Cancel');
          assert(document.querySelectorAll('.overlay').length === 1, 'Overlay must unmount immediately when closed while document is hidden');
        } finally {
          if (prevDesc) Object.defineProperty(document, 'visibilityState', prevDesc);
          else delete (document as { visibilityState?: unknown }).visibilityState;
          await act(async () => { document.dispatchEvent(new Event('visibilitychange')); });
        }
        await click('Close Settings');
        await waitUntil(() => !document.querySelector('.overlay'), 'space-switch-settings-close');
      } finally {
        setInstantMotion(true);
      }
      checks.push('Pinned Space-switch resilience: visibilitychange preserves Create with Pi session, Provider form & Collapse state, MessageDock questionnaire selections, + Prompt textarea, and instant occluded close');
    }

    checks.push('Phase 2 Custom Actions: Split Button switcher (Full & Compact), Create with Pi draft, Sync to All Repos, clean-tree clickability, CommitPanel preview, and Tier-2 ask_user_question');

    // Phase 3: Cold-restart restoration of interrupted submit session with preserved links + dismissal persistence:
    {
      const dismissedActions: Array<{ runId?: string; messageId: string }> = [];
      const restoredSnap = repositorySnapshot(1);
      await act(async () => {
        root.render(createElement(Workbench, {
          key: nextKey(),
          backend: {
            ...reads,
            async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
              return {
                protocolVersion: 2,
                locale: 'en',
                editor: 'idea',
                defaultCommitLanguage: 'en',
                activeWorktreeId: 'pay-tree',
                emptyPrompt: '',
                snapshots: [restoredSnap],
                worktrees: [],
                actionSessions: [{
                  runId: 'run-submit-restore-1',
                  repositoryId: 'orders',
                  worktreeId: 'pay-tree',
                  sessionId: 'sess-submit-1',
                  actionId: 'act-submit',
                  actionLabel: 'Submit -> v1.9.0 -> test',
                  profile: 'customAction',
                  frozenScopeVersion: 'sv-1',
                  modelId: 'test-claude',
                  frozenFileIds: [],
                  committedFileIds: [],
                  skippedFileIds: [],
                  status: 'interrupted',
                  summary: 'Interrupted before final promotion; created PRs and isolated worktrees were preserved.',
                  message: {
                    id: 'interrupted-action-run-submit-restore-1',
                    kind: 'error',
                    title: 'Interrupted: Submit -> v1.9.0 -> test',
                    description: 'Interrupted before final promotion; created PRs and isolated worktrees were preserved.',
                    links: [
                      { label: 'Issue #42', url: 'https://github.com/acme/demo/issues/42' },
                      { label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' },
                    ],
                  },
                  timeline: [
                    { state: 'done', label: 'Created isolated submit worktree' },
                    { state: 'failed', label: 'Running gh pr merge' },
                  ],
                  qaLog: [],
                  commits: [{
                    oid: '1111111111111111111111111111111111111111',
                    shortOid: '1111111',
                    title: 'fix(auth): validate expired tokens',
                    branch: 'fix/issue-42-auth',
                    fileIds: [],
                  }],
                  links: [
                    { label: 'Issue #42', url: 'https://github.com/acme/demo/issues/42' },
                    { label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' },
                  ],
                  canRetryRemaining: false,
                }],
                warning: null,
              };
            },
            async dismissSessionDone(_target, runId, messageId) {
              dismissedActions.push({ runId, messageId });
            },
          },
        }));
      });
      await tick();
      const restoredPill = document.querySelector<HTMLButtonElement>('.message-collapsed');
      assert(
        Boolean(restoredPill) && (restoredPill?.textContent ?? '').includes('Interrupted: Submit -> v1.9.0 -> test'),
        `Cold-restored interrupted submit session did not surface collapsed dock chip (pill=${restoredPill?.textContent})`,
      );
      await clickElement(restoredPill!);
      const expandedText = document.querySelector('.message-expanded')?.textContent ?? '';
      assert(
        expandedText.includes('Issue #42') && expandedText.includes('PR #101'),
        `Expanded restored submit session did not preserve Issue/PR links (text=${expandedText})`,
      );
      const okBtn = [...document.querySelectorAll<HTMLButtonElement>('.message-expanded button')].find((b) =>
        /dismiss|got it|close|acknowledge/i.test(b.getAttribute('aria-label') ?? b.textContent ?? ''),
      );
      assert(Boolean(okBtn), 'Expected dismiss button on restored interrupted custom action card');
      await clickElement(okBtn!);
      assert(
        !document.querySelector('.message-collapsed')
          && dismissedActions.length === 1
          && dismissedActions[0].runId === 'run-submit-restore-1'
          && dismissedActions[0].messageId === 'interrupted-action-run-submit-restore-1',
        `Dismissing restored submit session did not persist dismissal (${JSON.stringify(dismissedActions)})`,
      );
      checks.push('Phase 3 Cold Restart: restored interrupted submit session chip, preserved Issue/PR links, and persisted dismissal');
    }
  }
}
