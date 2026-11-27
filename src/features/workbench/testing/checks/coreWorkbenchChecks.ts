import { act } from 'react';
import type { ModelMutateReply, ModelSaveReply, ProviderSaveReply } from '../../../../../shared/model-protocol';
import type { ModelConfiguration, ProviderConnection } from '../../../settings/modelProviders';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchCommand } from '../../commands';
import { targetOf } from '../../state/workbench';
import type { WorkbenchEvent } from '../../state/workbench';
import { fixture, releaseQuestion } from '../fixtures';
import { assert, button, click, clickElement, openSettingsTab, text, tick, type, type BrowserCheckHarness } from './harness';

export async function runCoreWorkbenchChecks(harness: BrowserCheckHarness) {
  const { checks, zh, zhLanguageLabel, mount } = harness;
  // 1. Default English zero-model empty state, extra-instructions box, Configure Model guidance, and cancel return:
  await mount(fixture('empty'));
  assert(document.documentElement.lang === 'en', 'Default document lang should be en');
  assert(text().includes('Open Your Git Repository') && !text().includes('my-order-service'), 'Default empty state leaked sample repository');
  assert(!document.querySelector('.changes-heading') && !document.querySelector('[aria-label="Switch to directory tree view"]') && !document.querySelector('[aria-label="Refresh Changes"]'), 'Empty repository state must not render changes toolbar');
  assert(document.querySelector<HTMLButtonElement>('.repository-open-button')?.classList.contains('primary'), 'Open Local Repository should be the single primary action in upper empty state');
  assert(document.querySelector('.changes-panel')?.getAttribute('data-layout-mode') === 'idle' && !document.querySelector('.lower-workspace .tab-list'), 'Idle workbench must expand ChangesPanel without legacy commit/review tabs');
  assert(!document.querySelector('.action-dropdown') && !text().includes('Write Manually'), 'Manual commit dropdown and Write Manually button must be removed');
  const footerConfigBtn = document.querySelector<HTMLButtonElement>('.action-footer button.primary')!;
  assert(footerConfigBtn.textContent?.includes('Configure Model') && !footerConfigBtn.disabled, 'Zero-model footer primary button must guide to Configure Model');
  assert(
    !document.querySelector('.prompt-textarea')
      && button('+ Prompt').getAttribute('aria-expanded') === 'false'
      && !!document.querySelector('.action-footer .prompt-footer-toggle')
      && document.querySelector('.action-context-strip')!.getBoundingClientRect().height === 0,
    'Idle default state must place prompt icon inside footer with zero extra strip height until opened',
  );
  assert(!text().includes('Or click C-Push below') && !text().includes('Configure a model first'), 'Minimalist context strip must not render verbose filler descriptions');
  await click('Open Local Repository…');
  assert(text().includes('not connected in browser preview'), `Disconnected openRepository should explain preview boundary: ${text()}`);
  await click('+ Prompt');
  let promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  assert(!!promptArea && document.activeElement === promptArea, 'Clicking + Prompt must reveal and focus prompt textarea');
  await click('+ Prompt');
  assert(!document.querySelector('.prompt-textarea'), 'Clicking + Prompt again while empty must collapse prompt box');
  await click('+ Prompt');
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  type(promptArea, 'Extra instruction written before opening repository'); await tick();
  assert(
    button('Prompt').getAttribute('aria-expanded') === 'true'
      && !!document.querySelector('.action-footer .prompt-footer-dot'),
    'Non-empty prompt must mark footer prompt icon active with indicator dot',
  );
  await clickElement(footerConfigBtn);
  assert(!!document.querySelector('.model-form[data-form-mode="create-provider"]') && document.querySelector<HTMLInputElement>('input[type="password"]')?.value === '', 'Clicking Configure Model in footer must open Add Provider form with empty masked key');
  await click('Cancel');
  assert(!document.querySelector('.overlay'), 'Cancelling Add Provider opened from footer should return directly to workbench');
  await click('Settings');
  assert(
    document.querySelector('[data-setting-group="commit-language"] button[aria-pressed="true"]')?.textContent?.trim() === 'English'
      && document.querySelector('[data-setting-group="app-language"] button[aria-pressed="true"]')?.textContent?.trim() === 'English',
    'Default commit message language and app language must both be English',
  );
  await openSettingsTab('models');
  await click('Add First Provider');
  await clickElement(document.querySelector<HTMLButtonElement>('[data-overlay-back]')!);
  assert(document.querySelector('[aria-label="Close Settings"]') && !!document.querySelector('#settings-models-section'), 'Back button on Add Provider should return to Settings Models tab');
  await click('Add First Provider');
  await click('Cancel');
  assert(document.querySelector('[aria-label="Close Settings"]'), 'Cancelling Add Provider opened from Settings should return to Settings');
  await click('Add First Provider');
  await click('Close Add Provider');
  assert(!document.querySelector('.overlay'), 'Clicking × on nested Add Provider should close straight back to workbench');
  assert(document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'Extra instruction written before opening repository', 'Opening and closing Settings must preserve extra instructions');
  await click('Clear Extra Instructions');
  assert(!document.querySelector('.prompt-textarea') && button('+ Prompt').getAttribute('aria-expanded') === 'false', 'Clearing extra instructions via × must clear and collapse prompt box');
  await mount(fixture('clean'));
  assert(text().includes('Working Tree Clean') && !!document.querySelector('[aria-label="Refresh Changes"]') && !document.querySelector('[aria-label="Switch to directory tree view"]') && !document.querySelector('.changes-heading input[type="checkbox"]'), 'Clean worktree should keep Refresh while hiding select-all and tree toggle');
  checks.push('Default English zero-model empty state, extra instructions box, Configure Model guidance, and cancel return');

  // 2. App Language switching ('en' -> 'zh' -> 'en') and locale forwarding:
  const localeCommands: WorkbenchCommand[] = [];
  await mount(fixture('empty'), {
    ...disconnectedBackend,
    async execute(command) {
      localeCommands.push(command);
      return disconnectedBackend.execute(command);
    },
  });
  await click('Settings');
  const appLangZhBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="app-language"] button')]
    .find((item) => item.textContent?.includes(zhLanguageLabel))!;
  await clickElement(appLangZhBtn);
  assert(document.documentElement.getAttribute('lang') === 'zh-CN' && text().includes(zh.settings.title) && text().includes(zh.settings.appLanguage), 'Switching App Language to zh should update document lang and Settings labels');
  await click(zh.common.closeOverlay(zh.settings.title));
  assert(
    text().includes(zh.changes.emptyRepoTitle)
      && text().includes(zh.commit.configureModelButton)
      && !!document.querySelector(`[aria-label="${zh.commit.promptToggle}"]`),
    'Workbench labels should switch to Chinese',
  );
  await click(zh.changes.openLocalRepository);
  assert(localeCommands.at(-1)?.locale === 'zh' && text().includes(zh.desktop.disconnectedGit), 'Backend command in zh locale should receive locale: zh and localized error');
  await click(zh.header.settings);
  const appLangEnBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="app-language"] button')]
    .find((item) => item.textContent?.includes('English'))!;
  await clickElement(appLangEnBtn);
  assert(document.documentElement.getAttribute('lang') === 'en' && text().includes('Settings'), 'Switching App Language back to English should restore English UI');
  await click('Close Settings');
  assert(text().includes('Open Your Git Repository') && text().includes('Configure Model'), 'Main workbench should restore English labels');
  checks.push('App Language live switching between English and Chinese with backend locale forwarding');

  // 3. Input focus, selection range, prompt preservation, message priority, and draggable lower-workspace dock:
  window.localStorage.removeItem('picket.dockHeight');
  const subscription: { receive?: (event: WorkbenchEvent) => void } = {};
  const typingState = fixture('typing');
  await mount(typingState, { ...disconnectedBackend, subscribe(receive) { subscription.receive = receive; return () => { subscription.receive = undefined; }; } });
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  promptArea.focus(); type(promptArea, 'Continuous typing must not be interrupted'); await tick();
  promptArea.setSelectionRange(3, 10);
  await act(async () => { subscription.receive?.({ type: 'message', target: targetOf(typingState.worktrees[0]), message: releaseQuestion(), sessionStatus: 'needsDecision' }); });
  assert(document.activeElement === promptArea, 'Background question stole prompt textarea focus');
  assert(promptArea.value === 'Continuous typing must not be interrupted' && promptArea.selectionStart === 3 && promptArea.selectionEnd === 10, 'Background question altered prompt text or cursor selection');
  assert(!document.querySelector('.message-expanded'), 'Background question must not auto-expand while user is typing');
  await act(async () => { subscription.receive?.({ type: 'message', target: targetOf(typingState.worktrees[0]), message: { id: 'secondary-done', kind: 'done', title: 'Background note', description: 'Secondary update' } }); });
  assert(text().includes('+1'), 'Secondary notification after question should appear as secondary count');
  assert(document.activeElement === promptArea, 'Secondary notification stole prompt textarea focus');
  await click('Expand message: Include release notes in this commit?');
  const lower = document.querySelector('.lower-workspace')!.getBoundingClientRect();
  const dockWrap = document.querySelector('.d2-wrap')!.getBoundingClientRect();
  const expanded = document.querySelector('.message-expanded')!.getBoundingClientRect();
  assert(Math.abs(lower.top - dockWrap.top) <= 1 && Math.abs(lower.bottom - dockWrap.bottom) <= 1, 'Expanded message dock wrapper exceeded lower workspace bounds');
  assert(expanded.top >= lower.top && expanded.top <= lower.top + 8 && Math.abs(lower.bottom - expanded.bottom) <= 1, 'Expanded floating sheet exceeded lower workspace bounds');
  assert(document.querySelector('.changes-panel')?.getAttribute('data-layout-mode') === 'peek', 'Expanded message dock must shift ChangesPanel to peek mode');
  assert(document.querySelector('.context-header')!.getBoundingClientRect().bottom <= expanded.top, 'Expanded message dock covered header');
  assert(document.querySelector('.action-footer')!.getBoundingClientRect().top >= expanded.bottom, 'Expanded message dock covered footer');
  const reply = document.querySelector<HTMLInputElement>('[aria-label="Reply to active message"]')!;
  type(reply, 'Commit docs separately'); await tick();
  await click('Collapse Message');
  assert(document.querySelector('.changes-panel')?.getAttribute('data-layout-mode') === 'idle', 'Collapsing message dock must restore ChangesPanel to idle mode');
  await click('Expand message: Include release notes in this commit?');
  assert(document.querySelector<HTMLInputElement>('[aria-label="Reply to active message"]')!.value === 'Commit docs separately', 'Collapsing message dock lost custom reply draft');
  await click('Commit separately & push together');
  assert(text().includes('Include release notes in this commit?'), 'Failed respond must not fabricate resolved question');
  assert(text().includes('Background note'), 'Secondary message did not render inside expanded dock');
  assert(
    document.querySelectorAll('.d2-header-tools .d2-ibtn').length === 1
      && !document.querySelector('[aria-label="Expand to full height"]'),
    'MessageDock header must omit redundant expand/restore button when top handle is draggable',
  );
  const grabHandle = document.querySelector<HTMLElement>('.d2-grab-zone')!;
  assert(grabHandle.getAttribute('role') === 'separator', 'Top grab handle must be an accessible horizontal separator');
  const changesBeforeDrag = document.querySelector<HTMLElement>('.changes-panel')!.getBoundingClientRect().height;
  const grabRect = grabHandle.getBoundingClientRect();
  const dragStartY = grabRect.top + 6;
  // Drag handle downward by 110px -> shrinks dock and reveals more of ChangesPanel:
  await act(async () => {
    grabHandle.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 81, clientX: grabRect.left + 80, clientY: dragStartY }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 81, clientX: grabRect.left + 80, clientY: dragStartY + 110 }));
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 81, clientX: grabRect.left + 80, clientY: dragStartY + 110 }));
  });
  const changesAfterDownDrag = document.querySelector<HTMLElement>('.changes-panel')!.getBoundingClientRect().height;
  assert(
    changesAfterDownDrag >= changesBeforeDrag + 85
      && document.querySelector('.changes-panel')?.getAttribute('data-layout-mode') === 'peek'
      && window.localStorage.getItem('picket.dockHeight') !== null,
    `Dragging grab handle downward must grow ChangesPanel and persist dock height (before=${changesBeforeDrag}, after=${changesAfterDownDrag})`,
  );
  // Drag handle upward to top edge -> snaps cleanly into full-height reading mode and hides ChangesPanel:
  const grabRect2 = grabHandle.getBoundingClientRect();
  await act(async () => {
    grabHandle.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 82, clientX: grabRect2.left + 80, clientY: grabRect2.top + 6 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 82, clientX: grabRect2.left + 80, clientY: grabRect2.top - 320 }));
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 82, clientX: grabRect2.left + 80, clientY: grabRect2.top - 320 }));
  });
  assert(
    document.querySelector('.d2-wrap')?.classList.contains('is-full-height')
      && document.querySelector('.changes-panel')?.getAttribute('data-layout-mode') === 'hidden'
      && window.localStorage.getItem('picket.dockHeight') === 'full',
    'Dragging grab handle to top edge must snap to full-height reading mode and hide ChangesPanel',
  );
  // Double-click handle -> resets back to default peek height and clears saved preference:
  await act(async () => {
    grabHandle.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
  });
  assert(
    !document.querySelector('.d2-wrap')?.classList.contains('is-full-height')
      && document.querySelector('.changes-panel')?.getAttribute('data-layout-mode') === 'peek'
      && window.localStorage.getItem('picket.dockHeight') === null,
    'Double-clicking grab handle must restore default peek height and clear persisted override',
  );
  checks.push('Input focus, cursor selection, prompt preservation, message priority, floating sheet bounds, and draggable resize handle');

  // 4. Tri-state checkboxes, partial staged stats, directory tree grouping, and worktree isolation:
  await mount(fixture('typing'));
  assert(!document.querySelector('.changes-heading h2') && !document.querySelector('.changes-select-all'), 'Changes header must not render redundant heading or text select-all button');
  const checkbox = document.querySelector<HTMLInputElement>('[aria-label="Select src/service/Order.ts"]')!;
  const topCheckbox = document.querySelector<HTMLInputElement>('.changes-heading input[type="checkbox"]')!;
  assert(checkbox.checked, 'Fixture initial selection mismatch');
  assert(
    topCheckbox && topCheckbox.getAttribute('aria-label') === 'Select all changes' && !topCheckbox.checked && topCheckbox.indeterminate
      && Math.abs(topCheckbox.getBoundingClientRect().left - checkbox.getBoundingClientRect().left) < 0.5,
    'Header tri-state select-all checkbox must align with file checkboxes and start indeterminate',
  );
  await act(async () => { topCheckbox.click(); });
  assert(
    topCheckbox.checked && !topCheckbox.indeterminate && topCheckbox.getAttribute('aria-label') === 'Deselect all changes'
      && document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.checked,
    'Clicking indeterminate select-all must select all files including lockfile',
  );
  await act(async () => { topCheckbox.click(); });
  assert(!topCheckbox.checked && !topCheckbox.indeterminate && topCheckbox.getAttribute('aria-label') === 'Select all changes' && !checkbox.checked && text().includes('No changes selected'), 'Clicking checked select-all must clear all selections');
  await click('Switch to directory tree view');
  assert(text().includes('(root)'), 'Directory tree view missing (root) group');
  await act(async () => {
    document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="Select RELEASE_NOTES.md"]')!.click();
  });
  const rootDirCheckbox = document.querySelector<HTMLInputElement>('[aria-label="Select directory (root)"]')!;
  const groupedCheckbox = document.querySelector<HTMLInputElement>('.file-row.grouped .file-checkbox')!;
  assert(
    rootDirCheckbox && !rootDirCheckbox.checked && rootDirCheckbox.indeterminate
      && Math.abs(rootDirCheckbox.getBoundingClientRect().left - topCheckbox.getBoundingClientRect().left) < 0.5
      && groupedCheckbox.getBoundingClientRect().left > rootDirCheckbox.getBoundingClientRect().right,
    'Directory checkbox must align with header select-all, show indeterminate state, and indent child files',
  );
  await act(async () => { rootDirCheckbox.click(); });
  assert(
    rootDirCheckbox.checked && !rootDirCheckbox.indeterminate && rootDirCheckbox.getAttribute('aria-label') === 'Deselect directory (root)'
      && document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.checked
      && document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.checked
      && !document.querySelector('[role="alert"]'),
    'Clicking indeterminate directory checkbox must select all files in that directory without triggering IDE launch',
  );
  await act(async () => { rootDirCheckbox.click(); });
  assert(
    !rootDirCheckbox.checked && !rootDirCheckbox.indeterminate
      && !document.querySelector<HTMLInputElement>('[aria-label="Select RELEASE_NOTES.md"]')!.checked
      && !document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.checked
      && document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.checked,
    'Deselecting a directory must not clear selected files in other directories',
  );
  await click('Switch to list view');
  assert(!document.querySelector<HTMLInputElement>('[aria-label="Select src/service/Order.ts"]')!.checked && document.querySelector('.file-stats')?.textContent?.includes('+28'), 'Partial staged stats mismatch');
  await click('C-Push Session History');
  const historyEntry = [...document.querySelectorAll<HTMLButtonElement>('.history-entry')].find((entry) => entry.textContent?.includes('fix/npe'))!;
  await act(async () => { historyEntry.click(); });
  assert(document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'write concise bullets', 'Switching worktrees leaked prompt');
  await click('C-Push Session History');
  await act(async () => { document.querySelectorAll<HTMLButtonElement>('.history-entry')[0].click(); });
  assert(!document.querySelector<HTMLInputElement>('[aria-label="Select src/service/Order.ts"]')!.checked, 'Worktree selection did not persist across switch');
  checks.push('Selection decoupled from file open, partial staged stats, directory grouping, and worktree isolation');

  // 5. One-click C-Push direct dispatch, deduplication, extra prompt forwarding, and scope isolation:
  const calls: WorkbenchCommand[] = [];
  let finishCPush: (() => void) | undefined;
  const directState = fixture('typing');
  await mount(directState, { ...disconnectedBackend, execute(command) { calls.push(command); return new Promise<void>((resolve) => { finishCPush = resolve; }); } });
  await act(async () => { button('C-Push').click(); button('C-Push').click(); });
  assert(calls.length === 1 && calls[0].kind === 'cPush', 'Direct one-click C-Push duplicated dispatch');
  assert(!document.querySelector('.scope-menu') && !document.querySelector('.plan-group-textarea'), 'C-Push must run directly without scope popover or Preview Plan UI');
  if (calls[0].kind === 'cPush') {
    assert(
      !calls[0].fileIds.includes('lock')
        && calls[0].target.worktreeId === 'pay-tree'
        && calls[0].locale === 'en'
        && calls[0].commitLanguage === 'en'
        && calls[0].prompt === 'split release notes into its own commit and write detailed bullets',
      'C-Push expanded scope, mismatched target, or omitted prompt/locale/commitLanguage',
    );
  }
  await act(async () => { finishCPush?.(); });
  checks.push('One-click C-Push direct dispatch, deduplication, extra prompt forwarding, and exact scope isolation');

  // 6. Zero-model blocks commits, detached HEAD intercepts C-Push, and model errors offer 1-step Model Settings jump:
  const zeroModelCalls: WorkbenchCommand[] = [];
  await mount(fixture('no-model'), { ...disconnectedBackend, async execute(cmd) { zeroModelCalls.push(cmd); } });
  assert(!document.querySelector('.action-dropdown') && button('Configure Model') && zeroModelCalls.length === 0, 'Zero-model state must not offer C-Push or manual commit');
  await clickElement(document.querySelector<HTMLButtonElement>('.action-footer button.primary')!);
  assert(!!document.querySelector('.model-form'), 'Zero-model primary button must open model configuration');

  const detached = fixture('typing');
  detached.worktrees[0].detached = true;
  detached.worktrees[0].branch = '(detached)';
  const detachedCalls: WorkbenchCommand[] = [];
  await mount(detached, { ...disconnectedBackend, async execute(cmd) { detachedCalls.push(cmd); } });
  await click('C-Push');
  assert(detachedCalls.length === 0 && text().includes('Detached HEAD'), 'Detached HEAD must block C-Push before IPC');

  await mount(fixture('typing'), {
    ...disconnectedBackend,
    async execute(command) {
      if (command.kind === 'cPush') {
        throw new Error('Rate limit or quota exceeded (402/429). Check your account balance.');
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('C-Push');
  const noticeModelBtn = [...document.querySelectorAll<HTMLButtonElement>('.operation-notice button')].find((b) => b.textContent?.trim() === 'Model Settings');
  assert(!!noticeModelBtn && text().includes('402/429'), 'Quota/auth model error in notice must show 1-step Model Settings button');
  await clickElement(noticeModelBtn!);
  assert(!!document.querySelector('#settings-models-section'), 'Clicking Model Settings from notice must open Settings at Models section');
  await click('Close Settings');
  checks.push('Zero-model block, detached HEAD interception, and 1-step Model Settings jump on model errors');

  // 7. Single branch trigger, IDEA-style collapsible Local/Remote branch groups, and protected branch direct C-Push:
  const single = fixture('single');
  single.repositories[1].remoteBranches!.push({
    name: 'origin/103-shan-dao-bug-xiu-gai-very-long-branch-name',
    remote: 'origin',
    localName: '103-shan-dao-bug-xiu-gai-very-long-branch-name',
  });
  await mount(single);
  const outerBranchLabel = document.querySelector<HTMLElement>('.branch-anchor > button .truncate')!;
  assert(!document.querySelector('.worktree-anchor') && outerBranchLabel.scrollWidth <= outerBranchLabel.clientWidth, 'Header should render single branch trigger without truncating short branch names');
  assert(!document.querySelector('.context-bottom > .context-path > .branch-tag') && !button('C-Push').disabled, 'Protected branch main should not render redundant outer Protected badge while allowing C-Push');
  await click('main');
  const groupToggles = [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')];
  assert(
    groupToggles.length === 2
      && groupToggles[0].textContent?.includes('Local Branches') && groupToggles[0].getAttribute('aria-expanded') === 'true'
      && groupToggles[1].textContent?.includes('Remote Branches') && groupToggles[1].getAttribute('aria-expanded') === 'false'
      && document.querySelector('.branch-menu .branch-tag')?.textContent === 'Protected'
      && !text().includes('New Branch')
      && !text().includes('New Worktree'),
    'Single-worktree branch menu should render IDEA-style Local/Remote collapsible sections and Protected badge',
  );
  await act(async () => { groupToggles[1].click(); });
  const branchMenuRect = document.querySelector('.branch-menu')!.getBoundingClientRect();
  const remoteItems = [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="remote"] .branch-item')];
  assert(
    groupToggles[1].getAttribute('aria-expanded') === 'true'
      && (document.querySelector('[data-branch-group="remote"]')?.textContent ?? '').includes('origin/release/v1')
      && branchMenuRect.left >= 0 && branchMenuRect.right <= innerWidth
      && remoteItems.every((item) => item.getBoundingClientRect().height <= 32),
    'Expanded Remote Branches section should stay within viewport bounds without wrapping item metadata',
  );
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
  checks.push('Single branch entry, IDEA-style Local/Remote collapsible menu, and Protected branch direct C-Push');

  // 8. Provider connection fields, collapsible Advanced options (API Format + token defaults), and disconnected boundaries:
  await mount(fixture('no-model'));
  await click('Settings'); await openSettingsTab('models'); await click('Add First Provider');
  const keyHelpTip = document.querySelector<HTMLElement>('[aria-label="API Key storage info"]')!;
  assert(
    !!keyHelpTip && getComputedStyle(keyHelpTip.querySelector('.help-tip-bubble')!).visibility === 'hidden'
      && !document.querySelector('.model-form > p.help')
      && !document.querySelector('select[aria-label="API Format"]'),
    'Add Provider form should collapse Advanced fields by default, hide tooltip bubbles until hover/focus, and omit redundant footer paragraphs',
  );
  await clickElement(document.querySelector<HTMLButtonElement>('.advanced-toggle')!);
  const formatSelect = document.querySelector<HTMLSelectElement>('select[aria-label="API Format"]')!;
  const ctxInput = document.querySelector<HTMLInputElement>('input[aria-label="Context Window"]')!;
  const maxOutInput = document.querySelector<HTMLInputElement>('input[aria-label="Max Output Tokens"]')!;
  assert(formatSelect && formatSelect.value === 'anthropic-messages', 'Anthropic default API format should be anthropic-messages');
  assert(ctxInput.value === '200000' && maxOutInput.value === '16384', 'Anthropic default token limits should load from shared defaults');
  await click('Custom Compatible');
  assert((formatSelect.value as string) === 'openai-completions', 'Custom Compatible default API format should be openai-completions');
  assert((ctxInput.value as string) === '128000' && (maxOutInput.value as string) === '16384', 'Custom Compatible should load its default token limits');
  await act(async () => {
    formatSelect.value = 'openai-responses';
    formatSelect.dispatchEvent(new Event('change', { bubbles: true }));
  });
  assert((formatSelect.value as string) === 'openai-responses', 'API Format select should switch protocol');
  assert((ctxInput.value as string) === '200000' && (maxOutInput.value as string) === '32768', 'Switching API format before manual edits should update default token limits');
  await click('Ollama (Local)');
  assert(!document.querySelector('input[type="password"]'), 'Ollama must not ask for an API key');
  assert(document.querySelector('input[value="http://127.0.0.1:11434/v1"]'), 'Ollama default baseUrl mismatch');
  await click('Test Connection');
  assert(text().includes('no connection request was sent'), 'Disconnected preview must not fake successful model test');
  await click('Save Anyway');
  assert(text().includes('no configuration or API key was saved'), 'Disconnected preview must not fake saved provider');
  await click('Anthropic');
  assert(document.querySelector<HTMLInputElement>('input[type="password"]')?.value === '', 'Switching providers leaked API key');
  checks.push('Provider connection fields, collapsible Advanced options, zero-model boundaries, and honest disconnected errors');

  // 9. Grouped provider cards, editing child model without key input, keeping empty connection on last model delete, deleting connection, and auto-activation:
  const modelCalls: WorkbenchCommand[] = [];
  let editedPayload: unknown;
  const typingFixture = fixture('typing');
  let mockConnections: ProviderConnection[] = typingFixture.connections.map((item) => ({ ...item }));
  let mockModels: ModelConfiguration[] = typingFixture.models.map((item) => ({ ...item }));
  let mockActiveModelId: string | null = typingFixture.activeModelId;
  await mount(typingFixture, {
    ...disconnectedBackend,
    async execute(command): Promise<ModelMutateReply | void> {
      modelCalls.push(command);
      if (command.kind === 'deleteModel') {
        mockModels = mockModels.filter((item) => item.id !== command.modelId);
        if (mockActiveModelId === command.modelId) {
          mockActiveModelId = mockModels[0]?.id ?? null;
        }
        return {
          protocolVersion: 2,
          connections: mockConnections.map((item) => ({ ...item })),
          models: mockModels.map((item) => ({ ...item })),
          activeModelId: mockActiveModelId,
        };
      }
      if (command.kind === 'deleteProviderConnection') {
        mockConnections = mockConnections.filter((item) => item.id !== command.connectionId);
        mockModels = mockModels.filter((item) => item.connectionId !== command.connectionId);
        if (!mockModels.some((item) => item.id === mockActiveModelId)) {
          mockActiveModelId = mockModels[0]?.id ?? null;
        }
        return {
          protocolVersion: 2,
          connections: mockConnections.map((item) => ({ ...item })),
          models: mockModels.map((item) => ({ ...item })),
          activeModelId: mockActiveModelId,
        };
      }
      return undefined;
    },
    async saveModel(input): Promise<ModelSaveReply> {
      editedPayload = input;
      const parent = mockConnections.find((item) => item.id === input.connectionId)!;
      const updated: ModelConfiguration = {
        id: input.id ?? 'new-model-id',
        connectionId: parent.id,
        name: input.name,
        provider: parent.provider,
        apiFormat: input.apiFormat,
        modelId: input.modelId,
        contextWindow: input.contextWindow,
        maxOutputTokens: input.maxOutputTokens,
        ...(parent.baseUrl ? { baseUrl: parent.baseUrl } : {}),
      };
      mockModels = input.id
        ? mockModels.map((item) => (item.id === input.id ? updated : item))
        : [...mockModels, updated];
      if (!mockActiveModelId) mockActiveModelId = updated.id;
      return {
        protocolVersion: 2,
        model: updated,
        connections: mockConnections.map((item) => ({ ...item })),
        models: mockModels.map((item) => ({ ...item })),
        activeModelId: mockActiveModelId,
      };
    },
  });
  await click('Settings');
  await openSettingsTab('models');
  assert(document.querySelectorAll('.provider-card').length === 2, 'Settings should group models into 2 provider connection cards');
  const activeSelect = document.querySelector<HTMLButtonElement>('.model-row.is-active .model-select')!;
  assert(activeSelect && getComputedStyle(activeSelect).backgroundColor === 'rgba(0, 0, 0, 0)', 'Active model background should belong to .model-row card rather than inner button');
  await click('Edit model GPT-4.1');
  assert(text().includes('Edit Model') && !document.querySelector('input[type="password"]'), 'Edit Model must not show API Key input');
  const editModelIdInput = document.querySelector<HTMLInputElement>('.model-form [data-field="model-id"]')!;
  type(editModelIdInput, 'gpt-4.1-mini'); await tick();
  await clickElement(document.querySelector<HTMLButtonElement>('.advanced-toggle')!);
  const editCtxInput = document.querySelector<HTMLInputElement>('input[aria-label="Context Window"]')!;
  const editMaxOutInput = document.querySelector<HTMLInputElement>('input[aria-label="Max Output Tokens"]')!;
  type(editCtxInput, '256000');
  type(editMaxOutInput, '24576'); await tick();
  const editFormatSelect = document.querySelector<HTMLSelectElement>('select[aria-label="API Format"]')!;
  await act(async () => {
    editFormatSelect.value = 'openai-completions';
    editFormatSelect.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await click('Save Changes');
  assert(
    !!editedPayload && (editedPayload as { id?: string; connectionId?: string; apiFormat?: string }).id === 'test-gpt'
      && (editedPayload as { connectionId?: string }).connectionId === 'conn-openai'
      && (editedPayload as { apiFormat?: string }).apiFormat === 'openai-completions'
      && (editedPayload as { contextWindow?: number }).contextWindow === 256_000
      && (editedPayload as { maxOutputTokens?: number }).maxOutputTokens === 24_576
      && !!document.querySelector('[aria-label="Close Settings"]')
      && text().includes('gpt-4.1-mini')
      && text().includes('256K/24K'),
    'Editing child model should keep connection intact, return to Settings, and preserve custom token limits',
  );
  await click('Delete model Claude Sonnet 4');
  assert(
    text().includes('GPT-4.1')
      && !text().includes('Claude Sonnet 4')
      && text().includes('No models yet.')
      && document.querySelectorAll('.provider-card').length === 2,
    'Deleting the last model under Anthropic must keep the empty Anthropic connection card and fall back to GPT-4.1',
  );
  assert(modelCalls.length === 1 && modelCalls[0].kind === 'deleteModel', 'Deleting or editing model triggered unexpected selectModel');
  await click('Close Settings');
  assert(button('GPT-4.1'), 'Settings and ActionFooter did not share active model');
  await click('Settings');
  await openSettingsTab('models');
  await click('Delete connection Anthropic');
  assert(!!document.querySelector('.provider-delete-confirm'), 'Deleting connection must show inline confirmation');
  await click('Delete Connection');
  assert(document.querySelectorAll('.provider-card').length === 1, 'Confirming delete must remove the provider connection card');
  await click('Delete connection OpenAI');
  await click('Delete Connection');
  assert(!!document.querySelector('.model-empty') && ![...document.querySelectorAll('.settings-content button')].some((item) => item.textContent?.trim() === 'Provider'), 'Zero-connection empty state should not duplicate header Provider button');
  await click('Close Settings');
  assert(!document.querySelector('.model-button') && button('Add Model'), 'Deleting last connection left stale model dropdown');
  checks.push('Grouped provider cards, child model editing, empty connection preservation, connection delete confirmation, and zero-model fallback');

  await mount(fixture('no-model'), {
    ...disconnectedBackend,
    async createProviderConnection(input): Promise<ProviderSaveReply> {
      assert(input.apiKey === '' && input.provider === 'ollama' && input.locale === 'en', 'Hidden key must not enter createProviderConnection request');
      const connection: ProviderConnection = { id: 'conn-ollama-saved', name: input.connectionName, provider: input.provider, baseUrl: input.baseUrl };
      const model: ModelConfiguration = { id: 'fixture-saved', connectionId: connection.id, name: input.modelName, provider: input.provider, modelId: input.modelId, baseUrl: input.baseUrl };
      return { protocolVersion: 2, connection, model, connections: [connection], models: [model], activeModelId: model.id };
    },
  });
  await click('Settings'); await openSettingsTab('models'); await click('Add First Provider'); await click('Ollama (Local)'); await click('Save & Use');
  assert(!document.querySelector('.overlay') && button('Qwen 2.5 Coder'), 'Saving first provider should close both overlays and activate model');
  checks.push('Add Provider save receipt, auto-activation, and return to workbench');

  const pendingTests: { resolve: (description: string) => void }[] = [];
  await mount(fixture('no-model'), { ...disconnectedBackend, testModel() { return new Promise((resolve) => pendingTests.push({ resolve })); } });
  await click('Settings'); await openSettingsTab('models'); await click('Add First Provider'); await click('Ollama (Local)'); await click('Test Connection');
  const modelInput = document.querySelector<HTMLInputElement>('.model-form [data-field="model-id"]')!;
  type(modelInput, 'changed-model'); await tick(); await click('Test Connection');
  await act(async () => { pendingTests[0].resolve('Stale connection test result'); });
  assert(!text().includes('Stale connection test result') && text().includes('Testing…'), 'Stale test result polluted updated form fields');
  await act(async () => { pendingTests[1].resolve('Fresh connection test result (fixture)'); });
  assert(text().includes('Fresh connection test result (fixture)'), 'Fresh connection result did not render');
  type(modelInput, 'changed-again'); await tick();
  assert(!text().includes('Fresh connection test result (fixture)'), 'Changing connection field did not clear previous test status');
  checks.push('Connection field mutation invalidates stale test results');

  // 10. Long paths (including escaped Unicode + spaces) & 600/700px height responsiveness:
  await mount(fixture('long-paths'));
  const workbench = document.querySelector<HTMLElement>('.workbench')!;
  assert(workbench.scrollWidth <= workbench.clientWidth, 'Long file paths caused horizontal overflow');
  assert(document.querySelector('.changes-list')!.scrollHeight > document.querySelector('.changes-list')!.clientHeight, 'Changes list did not scroll independently');
  for (const height of [600, 700]) {
    document.getElementById('root')!.style.height = `${height}px`; await tick();
    const changesRect = document.querySelector('.changes-panel')!.getBoundingClientRect();
    const footerRect = document.querySelector('.action-footer')!.getBoundingClientRect();
    assert(changesRect.height > 300 && footerRect.bottom <= height + 1, 'Idle ChangesPanel did not expand or footer overflowed compact height');
  }
  await mount(fixture('question'));
  for (const height of [600, 700]) {
    document.getElementById('root')!.style.height = `${height}px`; await tick();
    assert(document.querySelector('.lower-workspace')!.getBoundingClientRect().height > 250, 'Expanded dock lower workspace collapsed in compact height');
    assert(Math.abs(document.querySelector('.changes-panel')!.getBoundingClientRect().height - 116) <= 2, 'ChangesPanel peek height mismatch when dock is expanded');
  }
  checks.push('Long paths, independent file scrolling, and 600/700px idle/expanded viewport layout');

  // 11. Native macOS overlaid titlebar clearance & long repository badge truncation in Settings:
  document.documentElement.classList.add('native-titlebar');
  const compact = fixture('typing');
  compact.repositories[0].name = 'a-repository-with-a-very-long-name-that-must-truncate';
  compact.connections = []; compact.models = []; compact.activeModelId = null;
  await mount(compact);
  const firstRow = document.querySelector<HTMLElement>('.context-top')!;
  const repositoryButton = document.querySelector<HTMLButtonElement>('.repository-button')!;
  assert(firstRow.getBoundingClientRect().height === 38, 'Overlaid titlebar must keep 38px header height');
  assert(repositoryButton.getBoundingClientRect().left >= 78, 'Repository button intruded into traffic light inset');
  assert(repositoryButton.scrollWidth <= repositoryButton.clientWidth, 'Long repository name overflowed header tools');
  assert(document.querySelector('.header-tools')!.getBoundingClientRect().right <= innerWidth, 'Header tools overflowed narrow window');
  assert(firstRow.hasAttribute('data-tauri-drag-region') && !repositoryButton.hasAttribute('data-tauri-drag-region'), 'Drag region must not cover repository button');
  await click(compact.repositories[0].name);
  const repositoryMenu = document.querySelector('.repository-menu')!.getBoundingClientRect();
  assert(repositoryMenu.left >= 0 && repositoryMenu.right <= innerWidth, 'Repository menu overflowed window');
  await click('Settings');
  assert(document.querySelector('.overlay-heading h1')!.getBoundingClientRect().left >= 84, 'Settings title overlapped native window controls');
  const projectBadge = document.querySelector<HTMLElement>('.project-badge')!;
  assert(projectBadge && projectBadge.getBoundingClientRect().right <= innerWidth - 14, 'Long repository badge in Settings overflowed narrow window');
  await openSettingsTab('models');
  await click('Add First Provider');
  assert(document.querySelector<HTMLElement>('[data-overlay-back]')!.getBoundingClientRect().left >= 80 && document.querySelectorAll('.overlay-heading h1')[1].getBoundingClientRect().left >= 84, 'Add Provider nav header overlapped native window controls');
  assert(!document.querySelector('.overlay-heading button[data-tauri-drag-region]'), 'Overlay close button must not carry drag region');
  document.documentElement.classList.remove('native-titlebar');
  checks.push('Native titlebar inset clearance, long repository badge truncation, and overlay drag boundaries');
}
