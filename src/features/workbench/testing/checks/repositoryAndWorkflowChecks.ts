import { act, createElement } from 'react';
import type { GitBranchReply } from '../../../../../shared/git-branch-protocol';
import type { GitOpenFileReply } from '../../../../../shared/ide-protocol';
import type { WorkbenchRestoreReply, WorkbenchSaveRequest } from '../../../../../shared/workbench-persistence-protocol';
import { Workbench } from '../../Workbench';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchBackend, WorkbenchCommand } from '../../commands';
import { targetOf } from '../../state/workbench';
import type { WorkbenchEvent } from '../../state/workbench';
import { fixture, releaseQuestion } from '../fixtures';
import { repositoryReply, repositorySnapshot } from '../repository-fixture';
import { assert, button, click, clickElement, openSettingsTab, text, tick, type, type BrowserCheckHarness } from './harness';

export async function runRepositoryAndWorkflowChecks(harness: BrowserCheckHarness) {
  const { root, checks, zhLanguageLabel, nextKey, nextReadSequence, mount } = harness;
  // 12. Repository reads, pre-open prompt migration, focus refresh, read error recovery, and late refresh isolation:
  let refreshCount = 0, failRefresh = false;
  const reads: WorkbenchBackend = { ...disconnectedBackend, repositoryReadsAvailable: true, async execute(command) {
    if (command.kind !== 'openRepository' && command.kind !== 'refresh') return disconnectedBackend.execute(command);
    if (command.kind === 'refresh') { refreshCount++; if (failRefresh) throw new Error('Working directory temporarily unreadable'); }
    const reply = repositoryReply(command.requestId, nextReadSequence());
    if (command.kind === 'refresh') { reply.target = command.target; reply.snapshot!.activeWorktreeId = command.target.worktreeId; }
    return reply;
  } };
  const emptyWithModel = fixture('empty');
  emptyWithModel.connections = [{ id: 'conn-anthropic', name: 'Anthropic', provider: 'anthropic', keyHint: 'sk-a…test' }];
  emptyWithModel.models = [{ id: 'test-claude', connectionId: 'conn-anthropic', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }];
  emptyWithModel.activeModelId = 'test-claude';
  await mount(emptyWithModel, reads);
  await click('+ Prompt');
  let promptArea = document.querySelector<HTMLTextAreaElement>('.prompt-textarea')!;
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
  const lateReply = repositoryReply(delayed.requestId, nextReadSequence()); lateReply.target = delayed.target;
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
  const branchSnapshot = repositoryReply(branchResponse.command!.requestId, nextReadSequence()).snapshot!;
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
  const remoteCheckoutSnapshot = repositoryReply(remoteCmd.requestId, nextReadSequence()).snapshot!;
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
  const lateBranchSnapshot = repositoryReply(branchResponse.command!.requestId, nextReadSequence()).snapshot!;
  lateBranchSnapshot.worktrees[0].branch = 'feat/invoice';
  await act(async () => { branchResponse.finish!({ protocolVersion: 1, kind: 'branch', requestId: branchResponse.command!.requestId, target: branchResponse.command!.target, action: 'switchBranch', outcome: 'completed', snapshot: lateBranchSnapshot }); });
  assert(document.querySelector('.branch-anchor > button')?.textContent?.includes('fix/npe'), 'Late branch reply stole active worktree view');
  checks.push('Late branch reply updates only source worktree when view switched mid-flight');

  // 14. Per-project Editor & Commit Message Language isolation (`orders` vs `cli`) + C-Push forwarding:
  const isolatedOpenCalls: Extract<WorkbenchCommand, { kind: 'openFile' }>[] = [];
  const isolatedAiCalls: Extract<WorkbenchCommand, { kind: 'cPush' }>[] = [];
  let failOpen = false;
  const multiProjectState = fixture('typing');
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
          editor: command.editor, filePath: command.path ?? (command.line ? 'src/api/pay.ts' : 'src/service/Order.ts'),
          action: command.line ? 'goto' : 'stagedDiff', ...(command.line ? { line: command.line } : {}),
        };
        return reply;
      }
      if (command.kind === 'cPush') {
        isolatedAiCalls.push(command);
        return {
          protocolVersion: 1, kind: 'cPush', requestId: command.requestId,
          session: {
            runId: command.requestId, repositoryId: command.target.repositoryId, worktreeId: command.target.worktreeId,
            sessionId: command.target.sessionId, frozenScopeVersion: command.target.scopeVersion, modelId: command.modelId,
            frozenFileIds: command.fileIds, committedFileIds: [], skippedFileIds: [],
            sessionStatus: 'done', autoCollapse: true, canRetryRemaining: false, timeline: [], qaLog: [], commits: [],
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
  await click('C-Push');
  assert(isolatedAiCalls.at(-1)?.kind === 'cPush' && isolatedAiCalls.at(-1)?.commitLanguage === 'zh' && isolatedAiCalls.at(-1)?.locale === 'en', 'orders C-Push should send commitLanguage: zh while keeping locale: en');

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
      key: nextKey(),
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
      key: nextKey(),
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
      key: nextKey(),
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

  // 16. API key redaction, rich message_user Direction 2 sheet (all 7 blocks + XSS safety + clean-tree IDE jump + fullHeight + dismissal):
  const richOpenCalls: Extract<WorkbenchCommand, { kind: 'openFile' }>[] = [];
  const richState = fixture('rich-card');
  // Also verify clean working tree allows jumping from findings & files blocks via direct `path` + `line`:
  richState.worktrees[0].files = [];
  richState.worktrees[0].selectedIds = [];
  richState.worktrees[0].messages[0].card!.blocks!.unshift(
    {
      type: 'markdown',
      content: 'Checked **4 files** with `dcr` and [PR #128](https://github.com/acme/orders/pull/128). <img src=x onerror=alert(1)>',
    },
    {
      type: 'callout',
      tone: 'warning',
      title: 'Attention',
      content: 'One high-risk finding blocks release.',
    },
  );
  await mount(richState, {
    ...disconnectedBackend,
    async testModel(input) {
      const secret = 'apiKey' in input ? input.apiKey : '';
      throw new Error(`Upstream auth error: Bearer ${secret} rejected`);
    },
    async execute(command) {
      if (command.kind === 'openFile') {
        richOpenCalls.push(command);
        return {
          protocolVersion: 1,
          kind: 'openFile',
          requestId: command.requestId,
          target: command.target,
          editor: command.editor,
          filePath: command.path ?? 'src/api/pay.ts',
          action: 'goto',
          ...(command.line ? { line: command.line } : {}),
        } satisfies GitOpenFileReply;
      }
      return disconnectedBackend.execute(command);
    },
  });
  await click('Settings'); await openSettingsTab('models'); await click('Provider');
  const keyField = document.querySelector<HTMLInputElement>('.model-form input[type="password"]')!;
  type(keyField, 'sk-form-secret-5566'); await tick();
  await click('Test Connection');
  assert(text().includes('[REDACTED]') && !text().includes('sk-form-secret-5566'), 'Model form error leaked plaintext API key');
  await click('Cancel'); await click('Close Settings');

  assert(
    !!document.querySelector('.d2-sheet.tone-warning')
      && !!document.querySelector('.d2-md strong')
      && !document.querySelector('.d2-md img')
      && text().includes('<img src=x onerror=alert(1)>')
      && !!document.querySelector('.d2-callout.is-warning')
      && document.querySelectorAll('.d2-fact-seg').length === 4
      && document.querySelectorAll('.d2-group-row.is-clickable').length === 2
      && document.querySelectorAll('.d2-checklist-row').length === 2
      && !document.querySelector('.d2-code-well pre'),
    'Direction 2 MessageDock must render all 7 safe blocks (with >8-line code collapsed by default and zero raw HTML execution)',
  );
  // Click finding row on clean tree -> invokes openFile with direct `path` and `line` (no fileId needed):
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-group-row.is-clickable[data-finding-path="src/api/pay.ts"]')!);
  assert(
    richOpenCalls.length === 1
      && richOpenCalls[0].path === 'src/api/pay.ts'
      && richOpenCalls[0].line === 46
      && richOpenCalls[0].fileId === undefined
      && text().includes('Jumped to pay.ts:46 in IDEA'),
    'Clicking findings row in MessageDock must jump to path:line in IDE even on a clean working tree',
  );
  // Toggle skipped files disclosure:
  const skippedToggle = document.querySelector<HTMLButtonElement>('.d2-skipped-wrap .d2-skipped-toggle')!;
  await clickElement(skippedToggle);
  assert(skippedToggle.getAttribute('aria-expanded') === 'true' && text().includes('pnpm-lock.yaml'), 'Skipped files disclosure did not reveal skipped lockfile reason');
  // Expand collapsed >8-line code block and copy:
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-code-title-btn')!);
  assert(!!document.querySelector('.d2-code-well pre')?.textContent?.includes('line 10'), 'Clicking collapsed code block header did not expand code preview');
  // Click files block pill -> invokes openFile with direct path/line:
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-blocks .d2-pill')!);
  assert((richOpenCalls.length as number) === 2 && richOpenCalls[1]?.path === 'src/api/pay.ts' && richOpenCalls[1]?.line === 46, 'Clicking files block pill did not trigger openFile path jump');
  // Trace details collapse by default when card blocks exist, and expand on click:
  assert(!document.querySelector('[aria-label="Session execution steps"]'), 'Trace timeline should stay collapsed by default when card has structured blocks');
  await clickElement(document.querySelector<HTMLButtonElement>('.d2-trace-disclosure .d2-skipped-toggle')!);
  assert(!!document.querySelector('[aria-label="Session execution steps"]'), 'Clicking Details toggle did not expand session execution timeline');
  // No bulky Got it button; top-right × dismisses completed card cleanly:
  assert(![...document.querySelectorAll('button')].some((b) => b.textContent?.trim() === 'Got it'), 'Direction 2 sheet must not render legacy bottom Got it button');
  await click('Dismiss completed notice');
  assert(!document.querySelector('.message-expanded') && !document.querySelector('.message-collapsed'), 'Top-right dismiss icon must dismiss rich card cleanly');
  checks.push('API key redaction, Direction 2 MessageDock with all 7 safe blocks, clean-tree findings/files IDE jump, collapsible code/trace, and header dismissal');

  // 17. M4 C-Push frozen scope, background question non-interruption, step pause/retry, and Q&A log:
  const cPushSub: { receive?: (event: WorkbenchEvent) => void } = {};
  const cPushCmds: WorkbenchCommand[] = [];
  const cPushState = fixture('typing');
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
      const pushedSnap = repositoryReply(`reconcile-${reconcileCalls}`, nextReadSequence()).snapshot!;
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
        const pullSnap = repositoryReply(command.requestId, nextReadSequence()).snapshot!;
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
        const localCommittedSnap = repositoryReply(command.requestId, nextReadSequence()).snapshot!;
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
          const pushedSnap = repositoryReply(command.requestId, nextReadSequence()).snapshot!;
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
}
