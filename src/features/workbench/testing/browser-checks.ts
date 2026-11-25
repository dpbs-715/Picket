import { act, createElement } from 'react';
import type { CPushCommandReply } from '../../../../shared/cpush-protocol';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitOpenFileReply } from '../../../../shared/ide-protocol';
import type {
  ModelMutateReply,
  ModelSaveReply,
  ProviderRevealKeyReply,
  ProviderSaveReply,
  ProviderUpdateReply,
  ReviewScopeReply,
} from '../../../../shared/model-protocol';
import type { WorkbenchRestoreReply, WorkbenchSaveRequest } from '../../../../shared/workbench-persistence-protocol';
import type { Root } from 'react-dom/client';
import type { WindowController } from '../../../desktop/window';
import { getMessages, LANGUAGE_OPTIONS } from '../../../i18n/messages';
import type { ModelConfiguration, ProviderConnection } from '../../settings/modelProviders';
import { Workbench } from '../Workbench';
import { disconnectedBackend } from '../commands';
import type { WorkbenchBackend, WorkbenchCommand } from '../commands';
import { targetOf } from '../state/workbench';
import type { WorkbenchEvent, WorkbenchState } from '../state/workbench';
import { fixture, releaseQuestion } from './fixtures';
import { repositoryReply, repositorySnapshot } from './repository-fixture';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const tick = () => act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 0)); });
function assert(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent?.trim() === label,
  );
  assert(found, `Missing button: ${label}`);
  return found;
}
async function click(label: string) {
  const target = button(label);
  await act(async () => { target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); });
  await act(async () => { button(label).click(); });
}
async function clickElement(target: HTMLElement) {
  await act(async () => { target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); });
  await act(async () => { target.click(); });
}
function type(element: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value);
    element.dispatchEvent(new Event('input', { bubbles: true }));
  });
}
const text = () => document.querySelector('.workbench')!.textContent ?? '';

export async function verifyWorkbench(root: Root) {
  const checks: string[] = [];
  const zh = getMessages('zh');
  const zhLanguageLabel = LANGUAGE_OPTIONS.find((item) => item.id === 'zh')!.label;
  let key = 0;
  async function mount(
    initialState: WorkbenchState,
    backend: WorkbenchBackend = disconnectedBackend,
    windowController?: WindowController,
  ) {
    await act(async () => {
      root.render(
        createElement(Workbench, {
          key: ++key,
          initialState,
          backend,
          ...(windowController ? { windowController } : {}),
        }),
      );
    });
  }

  // 1. Default English zero-model empty state, extra-instructions box, Configure Model guidance, and cancel return:
  await mount(fixture('empty'));
  assert(document.documentElement.lang === 'en', 'Default document lang should be en');
  assert(text().includes('Open Your Git Repository') && !text().includes('my-order-service'), 'Default empty state leaked sample repository');
  assert(!document.querySelector('.changes-heading') && !document.querySelector('[aria-label="Switch to directory tree view"]') && !document.querySelector('[aria-label="Refresh Changes"]'), 'Empty repository state must not render changes toolbar');
  assert(document.querySelector<HTMLButtonElement>('.repository-open-button')?.classList.contains('primary'), 'Open Local Repository should be the single primary action in upper empty state');
  assert(!document.querySelector('.plan-empty button.primary'), 'Lower workspace in empty state must not compete visually with Open Local Repository');
  assert(!document.querySelector('.action-dropdown') && !text().includes('Write Manually'), 'Manual commit dropdown and Write Manually button must be removed');
  const footerConfigBtn = document.querySelector<HTMLButtonElement>('.action-footer button.primary')!;
  assert(footerConfigBtn.textContent?.includes('Configure Model') && !footerConfigBtn.disabled, 'Zero-model footer primary button must guide to Configure Model');
  assert(!document.querySelector('.prompt-textarea') && button('+ Prompt').getAttribute('aria-expanded') === 'false', 'Prompt box must stay hidden by default until + Prompt is clicked');
  assert(!text().includes('Or click C-Push below') && !text().includes('Configure a model first'), 'Minimalist Commit Plan must not render verbose filler descriptions');
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
  assert(button('Prompt').getAttribute('aria-expanded') === 'true', 'Non-empty prompt must mark Prompt toggle active');
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
  await click('Add First Provider');
  await click('Cancel');
  assert(document.querySelector('[aria-label="Close Settings"]'), 'Cancelling Add Provider opened from Settings should return to Settings');
  await click('Close Settings');
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
  assert(text().includes(zh.changes.emptyRepoTitle) && text().includes(zh.commit.configureModelButton) && text().includes(zh.commit.promptToggle), 'Workbench labels should switch to Chinese');
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

  // 3. Input focus, selection range, prompt preservation, message priority, and lower-workspace dock overlay:
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
  await act(async () => { subscription.receive?.({ type: 'message', target: targetOf(typingState.worktrees[0]), message: { id: 'review', kind: 'review', title: 'Review finished', description: 'Secondary update' } }); });
  assert(text().includes('+1'), 'Review notification after question should appear as secondary count');
  assert(document.activeElement === promptArea, 'Secondary notification stole prompt textarea focus');
  await click('Expand message: Include release notes in this commit?');
  const lower = document.querySelector('.lower-workspace')!.getBoundingClientRect();
  const expanded = document.querySelector('.message-expanded')!.getBoundingClientRect();
  assert(Math.abs(lower.top - expanded.top) <= 1 && Math.abs(lower.bottom - expanded.bottom) <= 1, 'Expanded message dock exceeded lower workspace bounds');
  assert(document.querySelector('.context-header')!.getBoundingClientRect().bottom <= expanded.top, 'Expanded message dock covered header');
  assert(document.querySelector('.action-footer')!.getBoundingClientRect().top >= expanded.bottom, 'Expanded message dock covered footer');
  const reply = document.querySelector<HTMLInputElement>('[aria-label="Reply to active message"]')!;
  type(reply, 'Commit docs separately'); await tick();
  await click('Collapse Message'); await click('Expand message: Include release notes in this commit?');
  assert(document.querySelector<HTMLInputElement>('[aria-label="Reply to active message"]')!.value === 'Commit docs separately', 'Collapsing message dock lost custom reply draft');
  await click('Commit separately & push together');
  assert(text().includes('Include release notes in this commit?'), 'Failed respond must not fabricate resolved question');
  await click('View');
  assert(button('Code Review').getAttribute('aria-selected') === 'true', 'Secondary review View action did not switch to Code Review tab');
  checks.push('Input focus, cursor selection, prompt preservation, message priority, and lower-workspace dock overlay');

  // 4. Tri-state checkboxes, partial staged stats, directory tree grouping, and worktree isolation:
  await mount(fixture('changed'));
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

  // 5. One-click C-Push direct dispatch, verbatim confirmedPlan dispatch, and outdated plan discard & re-plan:
  const calls: WorkbenchCommand[] = [];
  let finishCPush: (() => void) | undefined;
  const directState = fixture('typing');
  directState.worktrees[0].plan = undefined;
  await mount(directState, { ...disconnectedBackend, execute(command) { calls.push(command); return new Promise<void>((resolve) => { finishCPush = resolve; }); } });
  await act(async () => { button('C-Push').click(); button('C-Push').click(); });
  assert(calls.length === 1 && calls[0].kind === 'cPush' && !calls[0].confirmedPlan, 'Direct C-Push without pre-generated plan duplicated dispatch or fabricated confirmedPlan');
  assert(!document.querySelector('.scope-menu'), 'C-Push must not open manual scope confirmation popover');
  if (calls[0].kind === 'cPush') {
    assert(!calls[0].fileIds.includes('lock') && calls[0].target.worktreeId === 'pay-tree' && calls[0].locale === 'en' && calls[0].commitLanguage === 'en', 'C-Push expanded scope, mismatched target, or omitted locale/commitLanguage');
  }
  await act(async () => { finishCPush?.(); });

  // Valid pre-generated plan edited by user -> C-Push forwards verbatim `confirmedPlan`:
  const planCalls: WorkbenchCommand[] = [];
  await mount(fixture('typing'), { ...disconnectedBackend, execute(command) { planCalls.push(command); return new Promise<void>((resolve) => { finishCPush = resolve; }); } });
  const planBox = document.querySelector<HTMLTextAreaElement>('.plan-group-textarea')!;
  type(planBox, 'feat(order): edited verbatim plan\n\n- custom bullet 1'); await tick();
  await click('C-Push');
  const firstPlanCall = planCalls.at(0);
  assert(
    planCalls.length === 1
      && firstPlanCall?.kind === 'cPush'
      && firstPlanCall.confirmedPlan?.groups[0].message === 'feat(order): edited verbatim plan\n\n- custom bullet 1',
    'C-Push with valid pre-generated plan did not send edited confirmedPlan verbatim',
  );
  await act(async () => { finishCPush?.(); });

  // Scope change marks plan Outdated -> clicking C-Push discards outdated plan and re-plans (`confirmedPlan === undefined`):
  const outdatedCalls: WorkbenchCommand[] = [];
  await mount(fixture('typing'), { ...disconnectedBackend, execute(command) { outdatedCalls.push(command); return new Promise<void>((resolve) => { finishCPush = resolve; }); } });
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.click(); });
  assert(text().includes('Outdated') && !document.querySelector('.commit-plan-panel .change-notice'), 'Changing selected files must show Outdated badge without verbose notice banner');
  await click('C-Push');
  const firstOutdatedCall = outdatedCalls.at(0);
  assert(
    outdatedCalls.length === 1
      && firstOutdatedCall?.kind === 'cPush'
      && firstOutdatedCall.confirmedPlan === undefined
      && firstOutdatedCall.fileIds.includes('lock'),
    'Clicking C-Push with an outdated plan must discard the stale plan and re-plan the new selection',
  );
  await act(async () => { finishCPush?.(); });
  checks.push('One-click C-Push direct dispatch, verbatim edited confirmedPlan, and outdated plan discard & re-plan');

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
      if (command.kind === 'cPush' || command.kind === 'planCPush' || command.kind === 'review') {
        throw new Error('Rate limit or quota exceeded (402/429). Check your account balance.');
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('Regenerate Plan');
  const noticeModelBtn = [...document.querySelectorAll<HTMLButtonElement>('.operation-notice button')].find((b) => b.textContent?.trim() === 'Model Settings');
  assert(!!noticeModelBtn && text().includes('402/429'), 'Quota/auth model error in notice must show 1-step Model Settings button');
  await clickElement(noticeModelBtn!);
  assert(!!document.querySelector('#settings-models-section'), 'Clicking Model Settings from notice must open Settings at Models section');
  await click('Close Settings');
  await act(async () => { document.querySelector<HTMLButtonElement>('#review-tab')!.click(); });
  await click('Review Selected Scope');
  const reviewModelBtn = [...document.querySelectorAll<HTMLButtonElement>('.review-empty button')].find((b) => b.textContent?.trim() === 'Model Settings');
  assert(!!reviewModelBtn, 'Model error in Code Review panel must also show 1-step Model Settings button');
  await clickElement(reviewModelBtn!);
  assert(!!document.querySelector('#settings-models-section'), 'Clicking Model Settings from Review panel must open Settings');
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
  await click('Settings'); await click('Add First Provider');
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
      && text().includes('No models under this connection yet.')
      && document.querySelectorAll('.provider-card').length === 2,
    'Deleting the last model under Anthropic must keep the empty Anthropic connection card and fall back to GPT-4.1',
  );
  assert(modelCalls.length === 1 && modelCalls[0].kind === 'deleteModel', 'Deleting or editing model triggered unexpected selectModel');
  await click('Close Settings');
  assert(button('GPT-4.1'), 'Settings and ActionFooter did not share active model');
  await click('Settings');
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
  await click('Settings'); await click('Add First Provider'); await click('Ollama (Local)'); await click('Save & Use');
  assert(!document.querySelector('.overlay') && button('Qwen 2.5 Coder'), 'Saving first provider should close both overlays and activate model');
  checks.push('Add Provider save receipt, auto-activation, and return to workbench');

  const pendingTests: { resolve: (description: string) => void }[] = [];
  await mount(fixture('no-model'), { ...disconnectedBackend, testModel() { return new Promise((resolve) => pendingTests.push({ resolve })); } });
  await click('Settings'); await click('Add First Provider'); await click('Ollama (Local)'); await click('Test Connection');
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
    assert(document.querySelector('.lower-workspace')!.getBoundingClientRect().height > 150, 'Lower workspace collapsed in compact height');
  }
  checks.push('Long paths, independent file scrolling, and 600/700px viewport layout');

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
  await click('Add First Provider');
  assert(document.querySelectorAll('.overlay-heading h1')[1].getBoundingClientRect().left >= 84, 'Add Provider title overlapped native window controls');
  assert(!document.querySelector('.overlay-heading button[data-tauri-drag-region]'), 'Overlay close button must not carry drag region');
  document.documentElement.classList.remove('native-titlebar');
  checks.push('Native titlebar inset clearance, long repository badge truncation, and overlay drag boundaries');

  // 12. Repository reads, pre-open prompt migration, focus refresh, read error recovery, and late refresh isolation:
  let readSequence = 0, refreshCount = 0, failRefresh = false;
  const reads: WorkbenchBackend = { ...disconnectedBackend, repositoryReadsAvailable: true, async execute(command) {
    if (command.kind !== 'openRepository' && command.kind !== 'refresh') return disconnectedBackend.execute(command);
    if (command.kind === 'refresh') { refreshCount++; if (failRefresh) throw new Error('Working directory temporarily unreadable'); }
    const reply = repositoryReply(command.requestId, ++readSequence);
    if (command.kind === 'refresh') { reply.target = command.target; reply.snapshot!.activeWorktreeId = command.target.worktreeId; }
    return reply;
  } };
  const emptyWithModel = fixture('empty');
  emptyWithModel.connections = [{ id: 'conn-anthropic', name: 'Anthropic', provider: 'anthropic', keyHint: 'sk-a…test' }];
  emptyWithModel.models = [{ id: 'test-claude', connectionId: 'conn-anthropic', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }];
  emptyWithModel.activeModelId = 'test-claude';
  await mount(emptyWithModel, reads);
  await click('+ Prompt');
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  type(promptArea, 'Instruction written prior to opening repo'); await tick();
  await click('Open Local Repository…');
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  assert(text().includes('real-contract-fixture') && promptArea.value === 'Instruction written prior to opening repo', 'Opening repository did not display snapshot or lost pre-open prompt');
  assert(
    document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.checked
      && text().includes('5/5 selected'),
    'All normal changes including lockfiles must start selected by default on real snapshot',
  );
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.click(); });
  promptArea.focus(); promptArea.setSelectionRange(1, 5);
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  assert(refreshCount === 1 && document.activeElement === promptArea && promptArea.selectionStart === 1 && promptArea.selectionEnd === 5, 'Window focus refresh did not run or stole focus/selection');
  assert(promptArea.value === 'Instruction written prior to opening repo' && !document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.checked, 'Refresh overwrote prompt or manually unchecked selection');
  checks.push('Open repository receipt, initial prompt migration, default normal-file auto-selection, and window focus refresh');

  failRefresh = true; await click('Refresh Changes');
  assert(text().includes('Worktree Status Unavailable') && !document.querySelector('.changes-heading input[type="checkbox"]') && button('C-Push').disabled && promptArea.value === 'Instruction written prior to opening repo', 'Read failure did not hide select-all, preserve prompt, or disable C-Push');
  failRefresh = false; await click('Retry Read');
  assert(!text().includes('Worktree Status Unavailable') && document.querySelector<HTMLInputElement>('[aria-label="Select src/service/Order.ts"]'), 'Retry Read did not restore file list');
  checks.push('Read error honesty, prompt preservation, and Retry Read recovery');

  const delayed: { finish?: (reply: ReturnType<typeof repositoryReply>) => void; requestId?: string; target?: ReturnType<typeof targetOf> } = {};
  await mount(emptyWithModel, { ...reads, execute(command) {
    if (command.kind === 'refresh') return new Promise((resolve) => { delayed.finish = resolve; delayed.requestId = command.requestId; delayed.target = command.target; });
    return reads.execute(command);
  } });
  await click('Open Local Repository…');
  await click('Refresh Changes');
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')].find((btn) => btn.textContent?.includes('Worktrees'))!.click();
  });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="worktree"] button')].find((btn) => btn.textContent?.includes('fix/npe'))!.click();
  });
  assert(document.querySelector('.branch-anchor')!.textContent?.includes('fix/npe'), 'Worktree switch did not apply');
  assert(delayed.requestId && delayed.target && delayed.finish, 'Controlled refresh did not start');
  const lateReply = repositoryReply(delayed.requestId, ++readSequence); lateReply.target = delayed.target;
  await act(async () => { delayed.finish!(lateReply); });
  assert(document.querySelector('.branch-anchor')!.textContent?.includes('fix/npe'), 'Late refresh stole active worktree view');
  checks.push('Late refresh during worktree switch updates only target worktree without stealing view');

  await mount(fixture('empty'), { ...reads, async execute(command) {
    if (command.kind === 'openRepository') return { protocolVersion: 1, requestId: command.requestId, snapshot: null };
    return reads.execute(command);
  } });
  await click('+ Prompt');
  type(document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!, 'Kept after cancel'); await tick();
  await click('Open Local Repository…');
  assert(document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'Kept after cancel' && text().includes('Open Your Git Repository'), 'Cancelling directory picker lost prompt or faked open');
  let rejectNonGit = false;
  await mount(fixture('empty'), { ...reads, async execute(command) {
    if (command.kind === 'openRepository' && rejectNonGit) {
      throw new Error('Cannot open "plain-folder": directory is not a Git repository (missing .git)');
    }
    return reads.execute(command);
  } });
  await click('Open Local Repository…');
  rejectNonGit = true;
  await click('real-contract-fixture');
  await click('Open Local Repository…');
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  assert(
    (document.querySelector('.operation-notice')?.textContent ?? '').includes('not a Git repository')
      && text().includes('real-contract-fixture'),
    'Window focus auto-refresh must not wipe non-Git directory error notice',
  );
  checks.push('Directory picker cancel, non-Git folder error preservation across focus refresh');

  // 13. Branch switch, remote tracking branch checkout, and occupied worktree jump:
  let branchCallCount = 0;
  const branchResponse: { finish?: (reply: GitBranchReply) => void; command?: Extract<WorkbenchCommand, { kind: 'switchBranch' | 'checkoutRemoteBranch' }> } = {};
  const branches: WorkbenchBackend = { ...reads, execute(command) {
    if (command.kind !== 'switchBranch' && command.kind !== 'checkoutRemoteBranch') return reads.execute(command);
    branchCallCount++; branchResponse.command = command;
    return new Promise<GitBranchReply>((resolve) => { branchResponse.finish = resolve; });
  } };
  await mount(fixture('empty'), branches); await click('Open Local Repository…');
  await click('+ Prompt');
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  type(promptArea, 'Instruction during branch operation'); await tick();
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.click(); document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  assert(!text().includes('New Branch') && !text().includes('New Worktree'), 'Branch menu must not render removed New Branch/Worktree items');
  await act(async () => {
    const targetBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((btn) => btn.textContent?.includes('feat/invoice'))!;
    targetBtn.click(); targetBtn.click();
  });
  assert(branchCallCount === 1 && branchResponse.command?.kind === 'switchBranch' && branchResponse.command.branch === 'feat/invoice' && branchResponse.command.branchToken, 'switchBranch duplicated or omitted branchToken');
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/pay'), 'UI faked branch switch before Git reply');
  type(promptArea, 'Newer instruction typed while waiting for Git'); await tick();
  const branchSnapshot = repositoryReply(branchResponse.command!.requestId, ++readSequence).snapshot!;
  branchSnapshot.worktrees[0].branch = 'feat/invoice'; branchSnapshot.worktrees[0].branchToken = 'updated-branch-token';
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: branchResponse.command!.requestId, target: branchResponse.command!.target, action: 'switchBranch', outcome: 'completed', snapshot: branchSnapshot }); });
  assert(!document.querySelector('.branch-menu') && document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/invoice'), 'Branch switch did not adopt Git snapshot or close menu');
  assert(promptArea.value === 'Newer instruction typed while waiting for Git' && !document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.checked, 'Branch switch reply overwrote newer prompt or selection');
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('.branch-menu .branch-group-toggle')].find((btn) => btn.textContent?.includes('Remote Branches'))!.click();
  });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="remote"] button')].find((btn) => btn.textContent?.includes('mirror/feat/reconcile'))!.click();
  });
  const remoteCmd = branchResponse.command as Extract<WorkbenchCommand, { kind: 'switchBranch' | 'checkoutRemoteBranch' }> | undefined;
  assert((branchCallCount as number) === 2 && remoteCmd?.kind === 'checkoutRemoteBranch' && remoteCmd.branch === 'mirror/feat/reconcile', 'Clicking remote branch did not dispatch checkoutRemoteBranch');
  const remoteCheckoutSnapshot = repositoryReply(remoteCmd.requestId, ++readSequence).snapshot!;
  remoteCheckoutSnapshot.worktrees[0].branch = 'feat/reconcile';
  remoteCheckoutSnapshot.worktrees[0].branchToken = 'remote-checkout-token';
  remoteCheckoutSnapshot.repository.branches.push({ name: 'feat/reconcile', worktreeId: 'pay-tree' });
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: remoteCmd.requestId, target: remoteCmd.target, action: 'checkoutRemoteBranch', outcome: 'completed', snapshot: remoteCheckoutSnapshot }); });
  assert(!document.querySelector('.branch-menu') && document.querySelector('.branch-anchor > button')?.textContent?.includes('feat/reconcile'), 'Remote branch checkout did not update local tracking branch');
  await act(async () => { document.querySelector<HTMLButtonElement>('.branch-anchor > button')!.click(); });
  await act(async () => {
    [...document.querySelectorAll<HTMLButtonElement>('[data-branch-group="local"] button')].find((btn) => btn.textContent?.includes('fix/npe'))!.click();
  });
  assert((branchCallCount as number) === 2 && document.querySelector('.branch-anchor > button')?.textContent?.includes('fix/npe'), 'Clicking occupied branch did not jump to existing worktree');
  checks.push('Switch local branch, checkout remote tracking branch, preserve prompt/selection, and jump to occupied worktree');

  branchCallCount = 0;
  await mount(fixture('empty'), branches); await click('Open Local Repository…');
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
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('fix/npe'), 'Late branch reply stole active worktree view');
  checks.push('Late branch reply updates only source worktree when view switched mid-flight');

  // 14. Per-project Editor & Commit Message Language isolation (`orders` vs `cli`) + Preview Plan & C-Push forwarding:
  const isolatedOpenCalls: Extract<WorkbenchCommand, { kind: 'openFile' }>[] = [];
  const isolatedAiCalls: Extract<WorkbenchCommand, { kind: 'planCPush' | 'cPush' }>[] = [];
  let failOpen = false;
  const multiProjectState = fixture('changed');
  multiProjectState.worktrees = multiProjectState.worktrees.map((tree) => ({
    ...tree,
    files: tree.files.map((file) => ({ ...file, commitToken: `tok-${tree.worktreeId}-${file.id}` })),
  }));
  await mount(multiProjectState, {
    ...disconnectedBackend,
    async execute(command) {
      if (command.kind === 'openFile') {
        isolatedOpenCalls.push(command);
        if (failOpen) throw new Error(`Executable not found for ${command.editor}. Prompt and selection preserved.`);
        const reply: GitOpenFileReply = {
          protocolVersion: 1, kind: 'openFile', requestId: command.requestId, target: command.target,
          editor: command.editor, filePath: command.line ? 'src/api/pay.ts' : 'src/service/Order.ts',
          action: command.line ? 'goto' : 'stagedDiff', ...(command.line ? { line: command.line } : {}),
        };
        return reply;
      }
      if (command.kind === 'planCPush') {
        isolatedAiCalls.push(command);
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
          preparedPlan: {
            summary: `Planned (${command.commitLanguage})`,
            groups: [{ id: 'g1', fileIds: command.fileIds, message: `feat(scope): planned-${command.commitLanguage}\n\n- bullet` }],
            skippedFileIds: [],
          },
          session: {
            runId: command.requestId, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: command.modelId,
            frozenFileIds: command.fileIds, committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'idle', message: null, canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
          },
        };
      }
      if (command.kind === 'cPush') {
        isolatedAiCalls.push(command);
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
          session: {
            runId: command.requestId, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: command.modelId,
            frozenFileIds: command.fileIds, committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'done', canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
            message: { id: `done-${command.requestId}`, kind: 'done', title: `Done (${command.commitLanguage})`, description: 'Done' },
          },
        };
      }
      return disconnectedBackend.execute(command);
    },
  });
  const orderRow = () => document.querySelector('input[aria-label="Select src/service/Order.ts"]')!.closest<HTMLElement>('.file-row')!;
  await act(async () => {
    orderRow().querySelector<HTMLButtonElement>('.file-open')!.click();
  });
  assert(orderRow().classList.contains('is-selected') && !isolatedOpenCalls.at(0), 'Single-clicking file row must highlight row without launching IDE');
  await act(async () => {
    orderRow().querySelector<HTMLButtonElement>('.file-diff-link')!.click();
  });
  assert(isolatedOpenCalls.length === 1 && isolatedOpenCalls[0].editor === 'idea' && text().includes('Opened staged diff in IDEA: Order.ts'), 'Clicking right-side diff link did not open staged diff in IDEA');
  await act(async () => { document.querySelector<HTMLButtonElement>('#review-tab')!.click(); });
  await act(async () => { document.querySelector<HTMLButtonElement>('.review-finding')!.click(); });
  assert(isolatedOpenCalls.at(1)?.line === 46 && text().includes('Jumped to pay.ts:46 in IDEA'), 'Review finding click did not jump to line in IDEA');
  await act(async () => { document.querySelector<HTMLButtonElement>('#commit-tab')!.click(); });

  await click('Settings');
  assert(document.querySelector('.project-badge')?.textContent?.trim() === 'my-order-service', 'Settings project badge should show active repository name');
  await clickElement([...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="editor"] button')].find((b) => b.textContent?.includes('RustRover'))!);
  await clickElement([...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="commit-language"] button')].find((b) => b.textContent?.includes(zhLanguageLabel))!);
  await click('Close Settings');
  assert((document.querySelector('.file-hover')?.textContent ?? '').includes('RustRover Staged Diff'), 'orders hover label should switch to RustRover');
  await act(async () => {
    orderRow().querySelector<HTMLButtonElement>('.file-open')!.dispatchEvent(
      new MouseEvent('dblclick', { bubbles: true, cancelable: true }),
    );
  });
  assert(isolatedOpenCalls.at(-1)?.editor === 'rustrover', 'Double-clicking file row should launch rustrover diff');
  await click('Regenerate Plan');
  assert(isolatedAiCalls.at(-1)?.kind === 'planCPush' && isolatedAiCalls.at(-1)?.commitLanguage === 'zh' && isolatedAiCalls.at(-1)?.locale === 'en', 'orders planCPush should send commitLanguage: zh while keeping locale: en');

  await click('my-order-service');
  await clickElement([...document.querySelectorAll<HTMLButtonElement>('.repository-menu button')].find((b) => b.textContent?.includes('picket-cli'))!);
  await click('Settings');
  assert(
    document.querySelector('.project-badge')?.textContent?.trim() === 'picket-cli'
      && document.querySelector('[data-setting-group="editor"] button[aria-pressed="true"]')?.textContent?.includes('IntelliJ IDEA')
      && document.querySelector('[data-setting-group="commit-language"] button[aria-pressed="true"]')?.textContent?.includes('English'),
    'Second repository picket-cli must keep its own independent editor and commit language settings',
  );
  await clickElement([...document.querySelectorAll<HTMLButtonElement>('[data-setting-group="editor"] button')].find((b) => b.textContent?.includes('Cursor'))!);
  await click('Close Settings');
  assert((document.querySelector('.file-hover')?.textContent ?? '').includes('Cursor Staged Diff'), 'cli hover label should switch to Cursor');
  failOpen = true;
  await act(async () => {
    document.querySelector('input[aria-label="Select src/service/Order.ts"]')!.closest('.file-row')!.querySelector<HTMLButtonElement>('.file-diff-link')!.click();
  });
  assert(isolatedOpenCalls.at(-1)?.editor === 'cursor' && text().includes('Executable not found for cursor'), 'cli file open should launch cursor and surface missing CLI error');
  failOpen = false;
  await click('C-Push');
  assert(isolatedAiCalls.at(-1)?.kind === 'cPush' && isolatedAiCalls.at(-1)?.commitLanguage === 'en', 'cli C-Push should send commitLanguage: en');

  await click('picket-cli');
  await clickElement([...document.querySelectorAll<HTMLButtonElement>('.repository-menu button')].find((b) => b.textContent?.includes('my-order-service'))!);
  assert((document.querySelector('.file-hover')?.textContent ?? '').includes('RustRover Staged Diff'), 'Switching back to orders must retain RustRover');
  await click('C-Push');
  assert(isolatedAiCalls.at(-1)?.kind === 'cPush' && isolatedAiCalls.at(-1)?.commitLanguage === 'zh', 'Switching back to orders must retain commitLanguage: zh');
  checks.push('External IDE launch, per-project Editor & Commit Message Language isolation across repositories');

  // 15. Cross-restart v2 persistence of per-project settings, prompts, selections, and installedEditors filtering:
  const savedRequests: WorkbenchSaveRequest[] = [];
  const restoredOrders = repositorySnapshot(1);
  const restoredCli = repositorySnapshot(1);
  restoredCli.repository = {
    id: 'cli',
    name: 'picket-cli',
    branches: [{ name: 'main', worktreeId: 'cli-tree' }],
    remoteBranches: [],
  };
  restoredCli.activeWorktreeId = 'cli-tree';
  restoredCli.worktrees = [{
    ...restoredCli.worktrees[0],
    repositoryId: 'cli',
    worktreeId: 'cli-tree',
    label: 'cli',
    branch: 'main',
  }];
  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 2,
            locale: 'en',
            editor: 'vscode',
            defaultCommitLanguage: 'en',
            repositorySettings: [
              { repositoryId: 'orders', editor: 'rustrover', commitLanguage: 'zh' },
              { repositoryId: 'cli', editor: 'cursor', commitLanguage: 'en' },
            ],
            activeWorktreeId: 'pay-tree',
            emptyPrompt: '',
            snapshots: [restoredOrders, restoredCli],
            warning: null,
            worktrees: [{
              repositoryId: 'orders',
              worktreeId: 'pay-tree',
              promptText: 'Restored extra instruction after restart',
              selectedIds: ['order'],
              scopeVersion: 'stale-before-restart',
              tab: 'commit',
            }],
          };
        },
        async saveWorkbenchState(request) { savedRequests.push(request); },
      },
    }));
  });
  await tick();
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  assert(promptArea.value === 'Restored extra instruction after restart', 'Cold restart did not restore v2 promptText');
  assert(document.querySelector<HTMLInputElement>('[aria-label="Select src/service/Order.ts"]')!.checked && !document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.checked, 'Cold restart did not restore saved file selection');
  assert(savedRequests.length === 0, 'Unmodified cold hydration must not immediately rewrite disk');
  await click('Settings');
  assert(
    document.querySelector<HTMLButtonElement>('[data-setting-group="editor"] button[aria-pressed="true"]')?.textContent?.includes('RustRover')
      && document.querySelector<HTMLButtonElement>('[data-setting-group="commit-language"] button[aria-pressed="true"]')?.textContent?.includes(zhLanguageLabel),
    'Cold restart did not restore orders per-project editor and commitLanguage',
  );
  await click('WebStorm'); await click('Close Settings');
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.click(); });
  assert(
    savedRequests.length >= 2
      && savedRequests.at(-1)?.repositorySettings?.find((item) => item.repositoryId === 'orders')?.editor === 'webstorm'
      && savedRequests.at(-1)?.repositorySettings?.find((item) => item.repositoryId === 'cli')?.editor === 'cursor'
      && savedRequests.at(-1)?.worktrees[0].selectedIds.includes('pay'),
    'Changing per-project editor and selection did not persist updated repositorySettings while preserving other repositories',
  );
  checks.push('Cold-restart v2 hydration and persistence of per-project settings, prompts, and selections');

  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 2, editor: 'cursor', installedEditors: ['idea', 'rustrover', 'webstorm'],
            activeWorktreeId: null, emptyPrompt: '', snapshots: [], worktrees: [], warning: null,
          };
        },
      },
    }));
  });
  await tick();
  await click('Settings');
  const gridText = document.querySelector('[data-setting-group="editor"]')?.textContent ?? '';
  assert(
    gridText.includes('IntelliJ IDEA') && gridText.includes('RustRover') && gridText.includes('WebStorm')
      && !gridText.includes('VS Code') && !gridText.includes('Cursor')
      && document.querySelector<HTMLButtonElement>('[data-setting-group="editor"] button[aria-pressed="true"]')?.textContent?.includes('IntelliJ IDEA'),
    'Installed editor detection did not hide uninstalled editors or fall back from uninstalled preference',
  );
  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 2, editor: 'idea', installedEditors: [],
            activeWorktreeId: null, emptyPrompt: '', snapshots: [], worktrees: [], warning: null,
          };
        },
      },
    }));
  });
  await tick();
  await click('Settings');
  assert(!document.querySelector('[data-setting-group="editor"]') && text().includes('No supported editors detected'), 'Zero installed editors did not render empty state');
  checks.push('Dynamic installed IDE filtering, fallback, and zero-editor empty state');

  // 16. Preview Plan loading spinner, API key redaction, prompt forwarding to Review, Q&A during Preview Plan, and cancellation:
  const cancelledIds: string[] = [];
  let finishPlan: ((reply: CPushCommandReply) => void) | undefined;
  const m3State = fixture('typing');
  m3State.worktrees[0].plan = undefined;
  m3State.worktrees[0].prompt = { text: 'split lockfile separately and check callback auth', revision: 0 };
  m3State.worktrees[0].files = m3State.worktrees[0].files.map((file) => ({ ...file, commitToken: `tok-${file.id}` }));
  await mount(m3State, {
    ...disconnectedBackend,
    async testModel(input) {
      const secret = 'apiKey' in input ? input.apiKey : '';
      throw new Error(`Upstream auth error: Bearer ${secret} rejected`);
    },
    async cancelRequest(requestId) { cancelledIds.push(requestId); },
    execute(command) {
      if (command.kind === 'planCPush') {
        assert(command.selections?.length === command.fileIds.length && command.selections[0].commitToken === 'tok-order', 'Preview Plan did not pass per-file commitTokens');
        return new Promise<CPushCommandReply>((resolve) => { finishPlan = resolve; });
      }
      if (command.kind === 'review') {
        return new Promise<ReviewScopeReply>(() => {});
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('Settings'); await click('Provider');
  const keyField = document.querySelector<HTMLInputElement>('.model-form input[type="password"]')!;
  type(keyField, 'sk-form-secret-5566'); await tick();
  await click('Test Connection');
  assert(text().includes('[REDACTED]') && !text().includes('sk-form-secret-5566'), 'Model form error leaked plaintext API key');
  await click('Cancel'); await click('Close Settings');

  await click('Preview Plan');
  assert(
    text().includes('Generating commit plan…')
      && !!document.querySelector('.commit-generating .spinning')
      && (document.querySelector('#commit-tab')?.textContent ?? '').includes('Planning')
      && !!document.querySelector('.prompt-textarea'),
    'Generating commit plan should show centered spinner in plan region while keeping extra-instructions input visible above',
  );
  await click('Cancel');
  assert(
    cancelledIds.length === 1
      && !text().includes('Generating commit plan…')
      && !!document.querySelector('.plan-empty'),
    'Cancel during Preview Plan did not invoke cancelRequest or restore empty plan state',
  );

  // Late `planCPush` reply after user edits prompt must NOT overwrite prompt or populate a stale plan:
  await click('Preview Plan');
  const activePlanTarget = targetOf(m3State.worktrees[0]);
  promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  type(promptArea, 'updated extra instruction while planning'); await tick();
  await act(async () => {
    finishPlan!({
      protocolVersion: 1,
      kind: 'cPush',
      requestId: 'ignored',
      preparedPlan: {
        summary: 'Stale plan',
        groups: [{ id: 'g-stale', fileIds: ['order'], message: 'feat: stale plan' }],
        skippedFileIds: [],
      },
      session: {
        runId: 'ignored', repositoryId: activePlanTarget.repositoryId, worktreeId: activePlanTarget.worktreeId,
        sessionId: activePlanTarget.sessionId, frozenScopeVersion: activePlanTarget.scopeVersion, modelId: 'test-claude',
        frozenFileIds: ['order'], committedFileIds: [], skippedFileIds: [],
        sessionStatus: 'idle', message: null, canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
      },
    });
  });
  assert(promptArea.value === 'updated extra instruction while planning' && !document.querySelector('.plan-group-textarea'), 'Late planCPush reply must not overwrite newer prompt or populate stale plan');

  let capturedPlanCmd: Extract<WorkbenchCommand, { kind: 'planCPush' }> | undefined;
  let capturedRevCmd: Extract<WorkbenchCommand, { kind: 'review' }> | undefined;
  await mount(m3State, {
    ...disconnectedBackend,
    async cancelRequest(requestId) { cancelledIds.push(requestId); },
    async execute(command) {
      if (command.kind === 'planCPush') {
        capturedPlanCmd = command;
        return {
          protocolVersion: 1,
          kind: 'cPush',
          requestId: command.requestId,
          preparedPlan: {
            summary: 'Prepared 1 commit',
            groups: [{
              id: 'g1',
              fileIds: command.fileIds,
              message: 'feat(order): add payment callback idempotency lock\n\n- keep staged Order.ts changes\n- add backoff retry and signature error code',
            }],
            skippedFileIds: [],
          },
          session: {
            runId: command.requestId, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: command.modelId,
            frozenFileIds: command.fileIds, committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'idle', message: null, canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
          },
        };
      }
      if (command.kind === 'review') {
        capturedRevCmd = command;
        return new Promise<ReviewScopeReply>(() => {});
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('Preview Plan');
  const expectedPlannedMessage = 'feat(order): add payment callback idempotency lock\n\n- keep staged Order.ts changes\n- add backoff retry and signature error code';
  assert(
    capturedPlanCmd?.modelId === 'test-claude'
      && capturedPlanCmd.prompt === 'split lockfile separately and check callback auth'
      && document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'split lockfile separately and check callback auth'
      && document.querySelector<HTMLTextAreaElement>('.plan-group-textarea')?.value === expectedPlannedMessage,
    'Preview Plan did not populate editable plan below prompt or altered prompt input',
  );
  await act(async () => { document.querySelector<HTMLButtonElement>('#review-tab')!.click(); });
  assert(!document.querySelector('.review-empty [data-help-tip]'), 'Review empty state must not render a ? HelpTip next to Review Selected Scope');
  await click('Review Selected Scope');
  assert(capturedRevCmd?.prompt === 'split lockfile separately and check callback auth', 'Code Review did not forward user extra instructions');
  await click('Cancel Review');
  assert(cancelledIds.includes(capturedRevCmd!.requestId) && !!document.querySelector('.review-empty'), 'Cancel Review did not restore review empty state');
  checks.push('Preview Plan loading spinner, API key redaction, prompt forwarding to Code Review, and cancel restoration');

  const lockOnlyState = fixture('typing');
  lockOnlyState.worktrees[0].selectedIds = ['lock'];
  lockOnlyState.worktrees[0].files = lockOnlyState.worktrees[0].files.map((file) => ({ ...file, commitToken: `tok-${file.id}` }));
  await mount(lockOnlyState, {
    ...disconnectedBackend,
    async execute(command) {
      if (command.kind === 'review') {
        return {
          protocolVersion: 2, kind: 'review', requestId: command.requestId, target: command.target,
          modelId: command.modelId, findings: [], skippedPaths: ['pnpm-lock.yaml (lockfile skipped by default)'], reviewedFileIds: [],
        };
      }
      return disconnectedBackend.execute(command);
    },
  });
  await act(async () => { document.querySelector<HTMLButtonElement>('#review-tab')!.click(); });
  assert(!document.querySelector('.review-empty [data-help-tip]'), 'Review empty state must not render a ? HelpTip');
  await click('Review Selected Scope');
  assert(text().includes('No files in selected scope were reviewed') && !text().includes('Review passed') && text().includes('Uncovered') && text().includes('pnpm-lock.yaml (lockfile skipped by default)'), 'Lockfile-only review must report Uncovered rather than Review passed');
  checks.push('M3 lockfile-filtered review coverage honesty without claiming Review passed');

  // 17. M4 C-Push frozen scope, background question non-interruption, step pause/retry, and Q&A log:
  const cPushSub: { receive?: (event: WorkbenchEvent) => void } = {};
  const cPushCmds: WorkbenchCommand[] = [];
  const cPushState = fixture('typing');
  cPushState.worktrees[0].plan = undefined;
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
                { state: 'done', label: 'Commit 1/2: a3f91c2 feat(order): add idempotency lock' },
                { state: 'failed', label: 'Commit 2/2 paused: docs: update release notes' },
              ],
              qaLog: [{ questionId: command.messageId, question: 'Include release notes in this commit?', answer: 'Commit separately & push together' }],
              commits: [{ oid: 'a3f91c2000', shortOid: 'a3f91c2', title: 'feat(order): add idempotency lock', branch: 'feat/pay', fileIds: ['order', 'pay', 'retry'] }],
              message: {
                id: 'err-step-2', kind: 'error', title: 'Completed 1/2 commits; commit 2 paused', scope: 'Commit 2/2',
                description: 'Commit hook rejected commit 2. Landed commits are preserved; retry remaining steps after fixing.',
                commits: [{ hash: 'a3f91c2', title: 'feat(order): add idempotency lock' }],
                options: [{ id: 'retry-remaining', label: 'Retry Remaining Commits', recommended: true }],
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
              { state: 'done', label: 'Commit 1/2: a3f91c2 feat(order): add idempotency lock' },
              { state: 'done', label: 'Commit 2/2: b84d120 docs: update release notes' },
            ],
            qaLog: [{ questionId: 'q-m4', question: 'Include release notes in this commit?', answer: 'Commit separately & push together' }],
            commits: [
              { oid: 'a3f91c2000', shortOid: 'a3f91c2', title: 'feat(order): add idempotency lock', branch: 'feat/pay', fileIds: ['order', 'pay', 'retry'] },
              { oid: 'b84d120000', shortOid: 'b84d120', title: 'docs: update release notes', branch: 'feat/pay', fileIds: ['release'] },
            ],
            message: {
              id: 'done-m4', kind: 'done', title: 'Completed 2 local split commits', scope: 'feat/pay',
              description: 'Unselected lockfile and unstaged drafts remain untouched.',
              commits: [{ hash: 'a3f91c2', title: 'feat(order): add idempotency lock' }, { hash: 'b84d120', title: 'docs: update release notes' }],
            },
          },
        };
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('C-Push');
  assert(cPushCmds.length === 1 && cPushCmds[0].kind === 'cPush' && cPushCmds[0].selections?.length === 4, 'C-Push omitted per-file commitTokens');
  const startedCmd = cPushCmds[0];
  await click('Collapse Message');
  const m4Editor = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  m4Editor.focus();
  type(m4Editor, 'Next C-Push instruction typed mid-run'); await tick();
  await act(async () => { document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.click(); });
  await act(async () => {
    resolveInitialCPush!({
      protocolVersion: 1, kind: 'cPush', requestId: startedCmd.requestId,
      session: {
        runId: startedCmd.requestId, repositoryId: startedCmd.target.repositoryId, worktreeId: startedCmd.target.worktreeId,
        sessionId: startedCmd.target.sessionId, frozenScopeVersion: startedCmd.target.scopeVersion, modelId: 'test-claude',
        frozenFileIds: ['order', 'pay', 'retry', 'release'], committedFileIds: [], skippedFileIds: [],
        sessionStatus: 'needsDecision', canRetryRemaining: false,
        timeline: [{ state: 'done', label: 'Froze scope for 4 selected files' }, { state: 'active', label: 'Waiting for decision: Include release notes in this commit?' }],
        qaLog: [], commits: [],
        message: { ...releaseQuestion(), id: 'q-m4' },
      },
    });
  });
  assert(document.activeElement === m4Editor && !document.querySelector('.message-expanded') && m4Editor.value === 'Next C-Push instruction typed mid-run', 'C-Push background question stole focus or auto-expanded');
  assert(button('C-Push · Decide'), 'needsDecision status did not sync to primary footer button');
  await click('C-Push · Decide');
  assert(cPushCmds.length === 1 && !!document.querySelector('.message-expanded'), 'Clicking resume button must expand dock rather than starting a second C-Push');
  await click('Commit separately & push together');
  const firstRespond = cPushCmds[1];
  assert(firstRespond?.kind === 'respond' && firstRespond.runId === startedCmd.requestId && firstRespond.target.scopeVersion === startedCmd.target.scopeVersion, 'Mid-run checkbox toggle mutated frozen C-Push target');
  assert(text().includes('Completed 1/2 commits; commit 2 paused') && !!document.querySelector('[aria-label="Session execution steps"]'), 'Paused step state or timeline did not render');
  await click('Retry Remaining Commits');
  assert(text().includes('Completed 2 local split commits') && text().includes('b84d120') && !!document.querySelector('[aria-label="Session Q&A log"]'), 'Retrying remaining steps did not finish or omitted Q&A log');
  assert(m4Editor.value === 'Next C-Push instruction typed mid-run', 'C-Push completion wiped prompt edited mid-run');
  checks.push('M4 C-Push scope freeze, non-interrupting question, partial-step retry, and Q&A log');

  // 18. M5 Pull & Rebase, C-Push push-failure prompt preservation + retry-push-only, manual IDE merge, and focus reconciliation:
  const m5Sub: { receive?: (event: WorkbenchEvent) => void } = {};
  const m5Cmds: WorkbenchCommand[] = [];
  let reconcileCalls = 0;
  let pullCreatesConflict = false;
  const m5InitState = fixture('empty');
  m5InitState.connections = [{ id: 'conn-anthropic', name: 'Anthropic', provider: 'anthropic', keyHint: 'sk-a…test' }];
  m5InitState.models = [{ id: 'test-claude', connectionId: 'conn-anthropic', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }];
  m5InitState.activeModelId = 'test-claude';
  await mount(m5InitState, {
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
            sessionId: target.sessionId, frozenScopeVersion: target.scopeVersion, modelId: 'test-claude',
            frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'done', autoCollapse: true, canRetryRemaining: false,
            timeline: [{ state: 'done', label: 'Detected merge and push completed in terminal to mirror/feat/pay' }],
            qaLog: [], commits: [],
            message: { id: 'reconciled-done', kind: 'done', title: 'Detected merge and push completed in terminal', description: 'Local and remote HEAD match.' },
          },
        },
      });
    },
    async execute(command) {
      m5Cmds.push(command);
      if (command.kind === 'pull') {
        const pullSnap = repositoryReply(command.requestId, ++readSequence).snapshot!;
        if (pullCreatesConflict) {
          return {
            protocolVersion: 1, kind: 'cPush', requestId: command.requestId, repositorySnapshot: pullSnap,
            session: {
              runId: 'conflict-run-1', repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
              sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: 'test-claude',
              frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
              sessionStatus: 'conflict', canRetryRemaining: false,
              timeline: [{ state: 'failed', label: 'Rebase conflict on src/service/Order.ts' }],
              qaLog: [], commits: [],
              message: {
                id: 'conflict-msg-1', kind: 'conflict', title: 'Merge conflict while syncing remote changes', scope: 'mirror/feat/pay',
                description: 'Choose a merge strategy; Agent never modifies conflicted files without authorization.',
                files: [{ id: 'order', label: 'src/service/Order.ts', badge: 'Conflict' }],
                options: [
                  { id: 'manual-merge', label: 'Merge Manually', recommended: true },
                  { id: 'abort-rebase', label: 'Abort Rebase' },
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
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: 'test-claude',
            frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'idle', message: null, canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
          },
        };
      }
      if (command.kind === 'cPush') {
        const localCommittedSnap = repositoryReply(command.requestId, ++readSequence).snapshot!;
        localCommittedSnap.worktrees[0].remote = { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 3, behind: 0, localOnly: true };
        localCommittedSnap.worktrees[0].files = localCommittedSnap.worktrees[0].files.filter((f) => f.id !== 'retry');
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId, repositorySnapshot: localCommittedSnap,
          session: {
            runId: command.requestId, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: command.modelId,
            frozenFileIds: command.fileIds, committedFileIds: command.fileIds, skippedFileIds: [],
            sessionStatus: 'pushFailed', canRetryRemaining: false,
            timeline: [
              { state: 'done', label: 'Local commit c9012ab: feat(order): commit and push' },
              { state: 'failed', label: 'Push to mirror/feat/pay failed' },
            ],
            qaLog: [],
            commits: [{ oid: 'c9012ab000', shortOid: 'c9012ab', title: 'feat(order): commit and push', branch: 'feat/pay', fileIds: command.fileIds }],
            message: {
              id: 'push-err-1', kind: 'pushFailed', title: 'Committed locally, push failed', scope: 'mirror/feat/pay',
              description: 'Local commit preserved; Retry Push only pushes existing commits.',
              commits: [{ hash: 'c9012ab', title: 'feat(order): commit and push' }],
              options: [{ id: 'retry-push', label: 'Retry Push', recommended: true }],
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
              sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: 'test-claude',
              frozenFileIds: ['order', 'retry'], committedFileIds: ['order', 'retry'], skippedFileIds: [],
              sessionStatus: 'done', canRetryRemaining: false,
              timeline: [{ state: 'done', label: '1 commit pushed to mirror/feat/pay' }],
              qaLog: [],
              commits: [{ oid: 'c9012ab000', shortOid: 'c9012ab', title: 'feat(order): commit and push', branch: 'feat/pay', fileIds: ['order', 'retry'] }],
              message: { id: 'push-done-1', kind: 'done', title: '1 commit pushed to mirror/feat/pay', description: 'Remote push completed.' },
            },
          };
        }
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
          session: {
            runId: command.runId!, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: 'test-claude',
            frozenFileIds: [], committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'waitingManual', canRetryRemaining: false,
            timeline: [{ state: 'active', label: 'Waiting for manual merge in external IDE…' }],
            qaLog: [], commits: [],
            message: {
              id: 'waiting-manual-1', kind: 'waitingManual', title: 'Waiting for manual merge in IDE',
              description: 'Refocusing Picket after resolving conflicts automatically reconciles Git and remote state.',
              files: [{ id: 'order', label: 'src/service/Order.ts', badge: 'Conflict' }],
            },
          },
        };
      }
      return reads.execute(command);
    },
  });
  await click('Open Local Repository…');
  await click('Remote Details');
  assert(text().includes('not fetched over network yet'), 'localOnly remote state should display local-tracking-ref tooltip');
  await click('Pull & Rebase');
  assert(text().includes('Pull & rebase complete') && text().includes('↓ 0'), 'Pull & Rebase completion did not update behind count');
  await click('Remote Details');
  assert(!text().includes('not fetched over network yet'), 'Network-synced remote should clear localOnly tooltip');
  await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });

  await act(async () => {
    document.querySelector<HTMLInputElement>('[aria-label="Select src/api/pay.ts"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="Select RELEASE_NOTES.md"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="Select pnpm-lock.yaml"]')!.click();
  });
  await click('+ Prompt');
  const m5Prompt = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
  type(m5Prompt, 'keep staged hunk atomic'); await tick();
  m5Prompt.blur();
  await click('C-Push');
  assert(text().includes('Committed locally, push failed') && text().includes('c9012ab') && button('Retry Push'), 'Push failure did not surface preserved local commit or Retry Push');
  assert(document.querySelector<HTMLTextAreaElement>('.prompt-textarea')?.value === 'keep staged hunk atomic', 'Push failure must preserve extra instructions while Retry Push is pending');
  const cPushCmdCountBeforeRetry = m5Cmds.filter((c) => c.kind === 'cPush').length;
  await click('Collapse Message');
  await click('Retry Push');
  assert(!!document.querySelector('.message-expanded') && m5Cmds.filter((c) => c.kind === 'cPush').length === cPushCmdCountBeforeRetry, 'Footer Retry Push resume button did not expand dock');
  await click('Retry Push');
  assert(
    m5Cmds.filter((c) => c.kind === 'cPush').length === cPushCmdCountBeforeRetry
      && text().includes('1 commit pushed to mirror/feat/pay')
      && text().includes('↑ 0')
      && !document.querySelector('.prompt-textarea'),
    'Retrying push duplicated C-Push, failed to clear ahead count, or failed to clear/collapse extra instructions after push succeeded',
  );

  pullCreatesConflict = true;
  await click('Collapse Message');
  await click('Remote Details');
  await click('Pull & Rebase');
  assert(text().includes('Merge conflict while syncing remote changes') && !!document.querySelector('.message-expanded'), 'Conflict during Pull & Rebase did not expand conflict card');
  await click('Merge Manually');
  assert(text().includes('Waiting for manual merge in IDE') && button('C-Push · Decide'), 'Switching to manual merge did not enter waitingManual');
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  assert(reconcileCalls >= 1 && !document.querySelector('.message-expanded') && text().includes('Detected merge and push completed in terminal'), 'Window focus did not reconcile or auto-collapse completed terminal merge');
  checks.push('M5 Pull & Rebase, C-Push pushFailed prompt preservation + retry-push-only, manual merge handoff, and focus reconciliation');

  // 19. Local Keep fixed group: default collapsed, right-click & drag move-in/move-out, select-all tri-state, gentle notice, conflict visibility, and cold-restart hydration:
  const localKeepOpenCalls: string[] = [];
  const localKeepSetCalls: Array<{ paths: string[]; keep: boolean }> = [];
  const localKeepState = fixture();
  localKeepState.worktrees = localKeepState.worktrees.map((tree) => ({
    ...tree,
    files: tree.files.map((file) => ({ ...file, commitToken: `tok-lk-${tree.worktreeId}-${file.id}` })),
  }));
  await mount(localKeepState, {
    ...disconnectedBackend,
    repositoryReadsAvailable: true,
    async execute(command) {
      if (command.kind === 'openFile') {
        localKeepOpenCalls.push(command.fileId);
        return {
          protocolVersion: 1,
          kind: 'openFile',
          requestId: command.requestId,
          target: command.target,
          editor: command.editor,
          filePath: 'src/service/Order.ts',
          action: 'diff',
        } satisfies GitOpenFileReply;
      }
      if (command.kind === 'setLocalKeep') {
        localKeepSetCalls.push({ paths: command.paths, keep: command.keep });
        const snap = repositorySnapshot(++readSequence);
        const currentKept = new Set<string>(
          localKeepSetCalls.reduce<string[]>((acc, item) => {
            const next = new Set(acc);
            for (const p of item.paths) {
              if (item.keep) next.add(p);
              else next.delete(p);
            }
            return [...next];
          }, []),
        );
        snap.repository.localKeepPaths = [...currentKept].sort();
        snap.worktrees = snap.worktrees.map((wt) => ({
          ...wt,
          files: wt.files.map((f) => ({
            ...f,
            localKeep: currentKept.has(f.path),
            commitToken: `tok-seq${readSequence}-${f.id}-${currentKept.has(f.path) ? 'keep' : 'norm'}`,
          })),
        }));
        return {
          protocolVersion: 1,
          kind: 'localKeep',
          requestId: command.requestId,
          target: command.target,
          snapshot: snap,
        };
      }
      return disconnectedBackend.execute(command);
    },
  });

  const keepHeader = () => document.querySelector<HTMLButtonElement>('.local-keep-header')!;
  const keepCount = () => document.querySelector('.local-keep-count')?.textContent?.trim();
  assert(
    keepHeader().getAttribute('aria-expanded') === 'false' && keepCount() === '0',
    'Local Keep group must render at bottom collapsed by default with 0 count',
  );

  // IDEA-style single-click, Shift+Click range, Cmd+Click multi-select, and batch right-click Keep Locally:
  const rowByPath = (path: string) => document.querySelector<HTMLElement>(`.file-row[data-file-path="${path}"]`)!;
  const clickRowWithMods = async (path: string, init: MouseEventInit = {}) => {
    await act(async () => {
      rowByPath(path).querySelector<HTMLButtonElement>('.file-open')!.dispatchEvent(
        new MouseEvent('click', { bubbles: true, cancelable: true, ...init }),
      );
    });
  };
  await clickRowWithMods('src/service/Order.ts');
  assert(
    rowByPath('src/service/Order.ts').classList.contains('is-selected')
      && document.querySelectorAll('.file-row.is-selected').length === 1
      && localKeepOpenCalls.length === 0,
    'Single click must select only the clicked file row without opening IDE',
  );
  await clickRowWithMods('src/utils/retry.ts', { shiftKey: true });
  assert(
    rowByPath('src/service/Order.ts').classList.contains('is-selected')
      && rowByPath('src/api/pay.ts').classList.contains('is-selected')
      && rowByPath('src/utils/retry.ts').classList.contains('is-selected')
      && document.querySelectorAll('.file-row.is-selected').length === 3,
    'Shift+Click must select the contiguous range of file rows',
  );
  await clickRowWithMods('src/api/pay.ts', { metaKey: true });
  assert(
    !rowByPath('src/api/pay.ts').classList.contains('is-selected')
      && document.querySelectorAll('.file-row.is-selected').length === 2,
    'Cmd+Click on a selected row must toggle it off',
  );
  await clickRowWithMods('src/api/pay.ts', { metaKey: true });
  assert(document.querySelectorAll('.file-row.is-selected').length === 3, 'Cmd+Click on an unselected row must add it to selection');
  await act(async () => {
    rowByPath('src/api/pay.ts').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 90, clientY: 150 }));
  });
  const batchMenuItems = [...document.querySelectorAll<HTMLButtonElement>('.file-context-menu [role="menuitem"]')];
  assert(
    batchMenuItems.length === 1
      && batchMenuItems[0].textContent?.includes('Keep Locally (Do Not Commit)')
      && batchMenuItems[0].textContent?.includes('3'),
    'Right-clicking multi-selected rows must show single Keep Locally menu item with batch count 3',
  );
  await clickElement(batchMenuItems[0]);
  assert(
    keepCount() === '3' && localKeepOpenCalls.length === 0,
    'Clicking Keep Locally on multi-selected rows must move all 3 files to Local Keep simultaneously',
  );
  await clickElement(keepHeader());
  await clickRowWithMods('src/service/Order.ts');
  await clickRowWithMods('src/utils/retry.ts', { shiftKey: true });
  await act(async () => {
    rowByPath('src/service/Order.ts').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 90, clientY: 220 }));
  });
  const restoreBatchItem = document.querySelector<HTMLButtonElement>('.file-context-menu [role="menuitem"]')!;
  assert(
    document.querySelectorAll('.file-context-menu [role="menuitem"]').length === 1
      && restoreBatchItem.textContent?.includes('Move Back to Changes')
      && restoreBatchItem.textContent?.includes('3'),
    'Right-clicking multi-selected Local Keep rows must show Move Back to Changes with count 3',
  );
  await clickElement(restoreBatchItem);
  assert(keepCount() === '0', 'Batch Move Back to Changes must restore all 3 files');
  await clickElement(keepHeader());
  assert(keepHeader().getAttribute('aria-expanded') === 'false', 'Local Keep should collapse back after batch restore test');

  const notesRow = () => document.querySelector<HTMLElement>('.file-row[data-file-path="RELEASE_NOTES.md"]')!;
  await act(async () => {
    notesRow().dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 90, clientY: 160 }));
  });
  const menuItem = document.querySelector<HTMLButtonElement>('.file-context-menu [role="menuitem"]')!;
  assert(menuItem?.textContent?.includes('Keep Locally (Do Not Commit)'), 'Right-clicking normal file did not open Keep Locally menu item');
  await clickElement(menuItem);
  assert(
    keepCount() === '1'
      && keepHeader().getAttribute('aria-expanded') === 'false'
      && !document.querySelector('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]')
      && localKeepOpenCalls.length === 0,
    'Context-menu Keep Locally must move file to collapsed Local Keep without opening IDE',
  );

  const topSelectAll = () => document.querySelector<HTMLInputElement>('.changes-summary .file-checkbox')!;
  assert(topSelectAll().indeterminate && !topSelectAll().checked, 'Top Select All should be indeterminate when lockfile in normal changes is unselected');
  await act(async () => { topSelectAll().click(); });
  assert(
    topSelectAll().checked && !topSelectAll().indeterminate && text().includes('4/4 selected'),
    'Selecting all normal changes must mark top Select All checked even when Local Keep file is unselected',
  );
  assert(!document.querySelector('.local-keep-notice'), 'Gentle Local Keep notice must not appear when no Local Keep file is checked');

  await clickElement(keepHeader());
  assert(keepHeader().getAttribute('aria-expanded') === 'true' && !!document.querySelector('.local-keep-list .file-row[data-file-path="RELEASE_NOTES.md"]'), 'Expanding Local Keep did not reveal kept file row');
  const keptCheckbox = () => document.querySelector<HTMLInputElement>('.local-keep-list input[aria-label="Select RELEASE_NOTES.md"]')!;
  assert(!keptCheckbox().checked, 'Select All must never select Local Keep files');
  await act(async () => { keptCheckbox().click(); });
  assert(
    keptCheckbox().checked
      && !!document.querySelector('.local-keep-notice')
      && text().includes('1 Local Keep file is checked and will be included'),
    'Checking a Local Keep file did not display the inline gentle notice',
  );
  await clickElement(document.querySelector<HTMLButtonElement>('.local-keep-notice button')!);
  assert(!keptCheckbox().checked && !document.querySelector('.local-keep-notice') && topSelectAll().checked, 'Uncheck button in gentle notice should uncheck only Local Keep files');

  await act(async () => { keptCheckbox().click(); });
  assert(keptCheckbox().checked && topSelectAll().checked, 'Top Select All should remain checked when all normal changes are selected');
  await act(async () => { topSelectAll().click(); });
  assert(
    !topSelectAll().checked && !topSelectAll().indeterminate && !keptCheckbox().checked && text().includes('No changes selected'),
    'Deselect All must also clear manually checked Local Keep files',
  );

  const keptRow = document.querySelector<HTMLElement>('.local-keep-list .file-row[data-file-path="RELEASE_NOTES.md"]')!;
  const keptRowRect = keptRow.getBoundingClientRect();
  const normalZone = document.querySelector<HTMLElement>('.normal-changes-zone')!;
  const normalZoneRect = normalZone.getBoundingClientRect();
  await act(async () => {
    keptRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 10, clientX: keptRowRect.left + 20, clientY: keptRowRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 10, clientX: keptRowRect.left + 28, clientY: keptRowRect.top + 18 }));
  });
  assert(normalZone.classList.contains('drop-zone-ready'), 'Dragging a Local Keep file did not highlight normal changes zone as ready');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 10, clientX: normalZoneRect.left + 40, clientY: normalZoneRect.top + 16 }));
  });
  assert(normalZone.classList.contains('drop-zone-over') && text().includes('Drop to move back to changes'), 'Hovering normal changes zone during drag did not show active drop highlight');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 10, clientX: normalZoneRect.left + 40, clientY: normalZoneRect.top + 16 }));
  });
  assert(
    keepCount() === '0'
      && !!document.querySelector('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]')
      && localKeepOpenCalls.length === 0,
    'Pointer drag-drop back to normal changes failed or accidentally opened IDE',
  );

  await clickElement(keepHeader());
  assert(keepHeader().getAttribute('aria-expanded') === 'false', 'Local Keep should collapse on header click');
  const payRow = document.querySelector<HTMLElement>('.normal-changes-zone .file-row[data-file-path="src/api/pay.ts"]')!;
  const payRect = payRow.getBoundingClientRect();
  const keepRect = keepHeader().getBoundingClientRect();
  const keepSection = document.querySelector<HTMLElement>('.local-keep-section')!;
  await act(async () => {
    payRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 11, clientX: payRect.left + 20, clientY: payRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 11, clientX: payRect.left + 32, clientY: payRect.top + 22 }));
  });
  assert(keepSection.classList.contains('drop-zone-ready') && !!document.querySelector('.file-drag-ghost'), 'Pointer drag past threshold did not activate drag ghost or Local Keep drop zone');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 11, clientX: keepRect.left + 40, clientY: keepRect.top + 12 }));
  });
  assert(keepSection.classList.contains('drop-zone-over') && text().includes('Drop to keep locally'), 'Pointer hover over collapsed Local Keep header did not show active drop highlight');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 11, clientX: keepRect.left + 40, clientY: keepRect.top + 12 }));
    keepHeader().click();
  });
  assert(
    keepCount() === '1'
      && keepHeader().getAttribute('aria-expanded') === 'false'
      && localKeepOpenCalls.length === 0,
    'Pointer drop onto collapsed Local Keep header must move file without toggling expand or opening IDE',
  );

  // Fixed-bottom Local Keep + drag-to-edge auto-scrolling on overflowing file lists:
  await mount(fixture('long-paths'));
  const changesPanelEl = document.querySelector<HTMLElement>('.changes-panel')!;
  const scrollList = document.querySelector<HTMLElement>('.changes-list')!;
  const pinnedKeepSection = document.querySelector<HTMLElement>('.local-keep-section')!;
  const pinnedKeepHeader = document.querySelector<HTMLButtonElement>('.local-keep-header')!;
  assert(
    scrollList.scrollHeight > scrollList.clientHeight + 300
      && scrollList.scrollTop === 0
      && !scrollList.contains(pinnedKeepSection)
      && Math.abs(pinnedKeepSection.getBoundingClientRect().bottom - changesPanelEl.getBoundingClientRect().bottom) <= 2,
    'Local Keep must be fixed at the bottom of .changes-panel outside .changes-list even when file list overflows at scrollTop=0',
  );
  // Dragging a file directly onto the fixed bottom Local Keep bar at scrollTop=0 immediately moves it without scrolling:
  const midRow = document.querySelector<HTMLElement>('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]')!;
  const midRect = midRow.getBoundingClientRect();
  const pinnedKeepRect = pinnedKeepHeader.getBoundingClientRect();
  let nativeDragCancelled = false;
  await act(async () => {
    const nativeDragStart = new DragEvent('dragstart', { bubbles: true, cancelable: true });
    midRow.dispatchEvent(nativeDragStart);
    nativeDragCancelled = nativeDragStart.defaultPrevented;
    midRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 21, clientX: midRect.left + 24, clientY: midRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 21, clientX: pinnedKeepRect.left + 40, clientY: pinnedKeepRect.top + 12 }));
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 21, clientX: pinnedKeepRect.left + 40, clientY: pinnedKeepRect.top + 12 }));
  });
  assert(nativeDragCancelled && keepCount() === '1' && scrollList.scrollTop === 0, 'Dropping directly onto fixed-bottom Local Keep at scrollTop=0 must move the file immediately');

  // Holding dragged file near bottom edge of .changes-list (above Local Keep) auto-scrolls .changes-list downward:
  const topRow = document.querySelector<HTMLElement>('.normal-changes-zone .file-row')!;
  const topRowRect = topRow.getBoundingClientRect();
  const listRect = scrollList.getBoundingClientRect();
  await act(async () => {
    topRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 22, clientX: topRowRect.left + 24, clientY: topRowRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 22, clientX: listRect.left + 60, clientY: listRect.bottom - 4 }));
  });
  await act(async () => {
    await new Promise<void>((resolve) => {
      const deadline = performance.now() + 1800;
      const waitBottom = () => {
        if (scrollList.scrollTop + scrollList.clientHeight >= scrollList.scrollHeight - 2 || performance.now() >= deadline) {
          resolve();
          return;
        }
        requestAnimationFrame(waitBottom);
      };
      requestAnimationFrame(waitBottom);
    });
  });
  assert(scrollList.scrollTop > 300, `Holding near bottom edge of .changes-list must auto-scroll down (scrollTop=${scrollList.scrollTop})`);
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 22, clientX: listRect.left + 60, clientY: listRect.bottom - 4 }));
  });

  // Expand fixed Local Keep at bottom, grab kept row, drag near top edge of .changes-list -> auto-scrolls back to top and drops back:
  await clickElement(pinnedKeepHeader);
  const keptNotesRow = document.querySelector<HTMLElement>('.local-keep-list .file-row[data-file-path="RELEASE_NOTES.md"]')!;
  const keptNotesRect = keptNotesRow.getBoundingClientRect();
  const listRectAfterExpand = scrollList.getBoundingClientRect();
  await act(async () => {
    keptNotesRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 23, clientX: keptNotesRect.left + 24, clientY: keptNotesRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 23, clientX: listRectAfterExpand.left + 60, clientY: listRectAfterExpand.top + 4 }));
  });
  await act(async () => {
    await new Promise<void>((resolve) => {
      const deadline = performance.now() + 1800;
      const waitTop = () => {
        if (scrollList.scrollTop <= 2 || performance.now() >= deadline) {
          resolve();
          return;
        }
        requestAnimationFrame(waitTop);
      };
      requestAnimationFrame(waitTop);
    });
  });
  assert(scrollList.scrollTop <= 2, `Holding dragged file near top edge must auto-scroll back to top (scrollTop=${scrollList.scrollTop})`);
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 23, clientX: listRectAfterExpand.left + 60, clientY: listRectAfterExpand.top + 24 }));
  });
  assert(keepCount() === '0', 'Dropping back into normal changes after auto-scrolling up must restore the file');

  const conflictKeepState = fixture('conflict');
  conflictKeepState.worktrees[0].files = conflictKeepState.worktrees[0].files.map((file) =>
    file.status === 'U' ? { ...file, localKeep: true } : file,
  );
  await mount(conflictKeepState);
  assert(
    keepHeader().getAttribute('aria-expanded') === 'false'
      && !!document.querySelector('.local-keep-conflict-badge')
      && (document.querySelector('.local-keep-conflict-badge')?.textContent ?? '').includes('conflict'),
    'Collapsed Local Keep header must surface conflict badge when a conflicted file is inside Local Keep',
  );

  const coldKeepSnap = repositorySnapshot(++readSequence);
  coldKeepSnap.repository.localKeepPaths = ['RELEASE_NOTES.md'];
  coldKeepSnap.worktrees[0].files = coldKeepSnap.worktrees[0].files.map((f) =>
    f.path === 'RELEASE_NOTES.md' ? { ...f, localKeep: true } : f,
  );
  await act(async () => {
    root.render(createElement(Workbench, {
      key: ++key,
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 2,
            locale: 'en',
            editor: 'idea',
            defaultCommitLanguage: 'en',
            repositorySettings: [{ repositoryId: 'orders', editor: 'idea', commitLanguage: 'en', localKeepPaths: ['RELEASE_NOTES.md'] }],
            activeWorktreeId: 'pay-tree',
            emptyPrompt: '',
            snapshots: [coldKeepSnap],
            worktrees: [],
            warning: null,
          };
        },
        async saveWorkbenchState() {},
      },
    }));
  });
  await tick();
  assert(
    keepHeader().getAttribute('aria-expanded') === 'false'
      && keepCount() === '1'
      && !document.querySelector('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]'),
    'Cold-start hydration did not restore Local Keep group collapsed with kept file excluded from normal changes',
  );
  checks.push('Local Keep fixed group: collapsed default, right-click & drag move-in/move-out, select-all tri-state, gentle notice, conflict badge, and cold-restart restore');

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

  // Interactive compact <-> full workflow with controllable desktop window mock:
  const createControlledWindow = (initialPos = { x: 180, y: 120 }, initialSize = { width: 400, height: 700 }) => {
    let winPinned = false;
    let winPos = { ...initialPos };
    let winSize = { ...initialSize };
    let geometryListener: ((snap: { position: { x: number; y: number }; size: { width: number; height: number } }) => void) | undefined;
    let pendingGate: { promise: Promise<void>; release: () => void } | null = null;
    const notifications: { title: string; body: string }[] = [];
    const history: { pinned: boolean; compactMode: boolean; pos: { x: number; y: number }; size: { width: number; height: number } }[] = [];
    const controller: WindowController = {
      isDesktopWindow: true,
      async readSnapshot() {
        return { pinned: winPinned, focused: true, scaleFactor: 2, position: { ...winPos }, size: { ...winSize } };
      },
      async setPinned(nextPinned) {
        winPinned = nextPinned;
        return winPinned;
      },
      async applyWindowMode(opts) {
        const previousPosition = { ...winPos };
        const previousSize = { ...winSize };
        if (pendingGate) {
          const gate = pendingGate;
          pendingGate = null;
          await gate.promise;
        }
        winPinned = opts.pinned;
        winSize = opts.compactMode
          ? { width: opts.compactWidth ?? 400, height: opts.compactHeight ?? 86 }
          : { width: opts.fullSize.width, height: opts.fullSize.height };
        if (opts.placement === 'restorePosition' && opts.targetPosition) {
          winPos = { ...opts.targetPosition };
        }
        // placement === 'anchorTopLeft' keeps winPos (top-left) unchanged!
        history.push({ pinned: winPinned, compactMode: opts.compactMode, pos: { ...winPos }, size: { ...winSize } });
        return {
          pinned: winPinned,
          previousPosition,
          previousSize,
          position: { ...winPos },
          size: { ...winSize },
        };
      },
      async adjustCompactHeight(height) {
        winSize = { width: Math.max(400, winSize.width), height };
      },
      subscribeGeometry(listener) {
        geometryListener = listener;
        return () => { geometryListener = undefined; };
      },
      async notifyBackgroundAttention(payload) {
        notifications.push(payload);
        return true;
      },
    };
    return {
      controller,
      notifications,
      history,
      getPos: () => ({ ...winPos }),
      getSize: () => ({ ...winSize }),
      getPinned: () => winPinned,
      pauseNextApply() {
        let release!: () => void;
        const promise = new Promise<void>((resolve) => { release = resolve; });
        pendingGate = { promise, release };
        return release;
      },
      dragTo(x: number, y: number) {
        winPos = { x, y };
        geometryListener?.({ position: { ...winPos }, size: { ...winSize } });
      },
      resizeTo(width: number, height: number) {
        winSize = { width, height };
        geometryListener?.({ position: { ...winPos }, size: { ...winSize } });
      },
    };
  };

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
  modeState.worktrees[0].plan = undefined;
  modeState.worktrees[0].branchToken = 'branch-token-compact';
  await mount(
    modeState,
    {
      ...disconnectedBackend,
      async execute(command) {
        compactCommands.push(command);
        if (command.kind === 'pull') {
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
      && compactCommands.some((command) => command.kind === 'pull' && command.target.worktreeId === 'pay-tree')
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

  // (b) Click "Plan 2 · Outdated ▸" pill -> expands to Commit Plan tab & focuses .plan-region
  const outdatedPlanCompact = fixture('changed');
  outdatedPlanCompact.worktrees[0].tab = 'review';
  outdatedPlanCompact.windowPrefs = { ...outdatedPlanCompact.windowPrefs, pinned: true, compactMode: true };
  await mount(outdatedPlanCompact, disconnectedBackend, jumpWin.controller);
  assert(text().includes('Plan 2 \u00b7 Outdated \u25b8'), 'Compact Row 2 must display outdated plan pill alongside selection trigger');
  await clickElement(document.querySelector<HTMLButtonElement>('[data-expand-target="plan"]')!);
  await tick();
  assert(
    !document.querySelector('.compact-bar')
      && document.querySelector<HTMLButtonElement>('#commit-tab')?.getAttribute('aria-selected') === 'true'
      && !!document.querySelector('.plan-region')?.contains(document.activeElement),
    'Clicking Plan ▸ pill in Compact Mode must switch to Commit Plan tab and focus .plan-region',
  );

  // (c) Zero-model Compact Mode -> primary button shows "Configure Model", clicking expands & opens AddModelForm:
  const zeroModelCompact = fixture('no-model');
  zeroModelCompact.windowPrefs = { ...zeroModelCompact.windowPrefs, pinned: true, compactMode: true };
  await mount(zeroModelCompact, disconnectedBackend, jumpWin.controller);
  const compactPrimaryBtn = document.querySelector<HTMLButtonElement>('.compact-bottom button.primary')!;
  assert(compactPrimaryBtn.textContent?.includes('Configure Model') && text().includes('No model'), 'Zero-model Compact Mode must show No model badge and Configure Model button');
  await clickElement(compactPrimaryBtn);
  await tick();
  assert(!document.querySelector('.compact-bar') && !!document.querySelector('.model-form'), 'Clicking Configure Model in Compact Mode must expand and open AddModelForm');

  // (d) Blocking states in Compact Mode (Detached HEAD & Read Error) disable C-Push and explain reason in Row 2:
  const detachedCompact = fixture('compact');
  detachedCompact.worktrees[0].plan = undefined;
  detachedCompact.worktrees[0].detached = true;
  await mount(detachedCompact, disconnectedBackend, jumpWin.controller);
  assert(
    text().includes('Detached HEAD \u25b8')
      && document.querySelector<HTMLButtonElement>('.compact-bottom button.primary')?.disabled === true,
    'Detached HEAD in Compact Mode must explain reason in Row 2 and disable C-Push',
  );
  checks.push('Pinned Compact Mode: browser-preview boundary, 2-row ~86px layout, project & file drawers, ▸ jump targets, non-auto-expand, and background notifications');

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
    text().includes('Applies to all 2 models under this connection.'),
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

  return {
    environment: 'Chrome headless · development fixtures only (not real Git or live model acceptance)',
    viewport: { width: innerWidth, height: innerHeight },
    checks,
  };
}
