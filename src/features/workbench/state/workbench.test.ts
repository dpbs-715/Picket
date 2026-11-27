import { describe, expect, it } from 'vitest';
import { DesktopCommandError, isModelCommandError, toDesktopCommandError } from '../../../desktop/errors';
import { getMessages } from '../../../i18n/messages';
import { fixture, releaseQuestion } from '../testing/fixtures';
import { readScopeVersion } from './repositories';
import {
  compactStatusSummary,
  computeNextCompactPreferences,
  computeNextPinPreferences,
  defaultWindowPreferences,
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  emptyWorkbench,
  groupFiles,
  localKeepChanges,
  localKeepPathsFor,
  normalChanges,
  orderedMessages,
  primaryAction,
  resolveWorkbenchPresentation,
  scopeSummary,
  selectedFiles,
  selectedLocalKeepChanges,
  targetOf,
  workbenchReducer,
} from './workbench';
import type { WorkbenchAction, WorkbenchState } from './workbench';

function apply(state: WorkbenchState, action: WorkbenchAction) { return workbenchReducer(state, action); }

describe('Workbench selection, per-project settings, zero-model guidance, and localization state', () => {
  it('starts in English default state and guides zero-model users to Configure Model', () => {
    expect(emptyWorkbench()).toMatchObject({
      repositories: [],
      worktrees: [],
      models: [],
      activeWorktreeId: null,
      activeModelId: null,
      locale: 'en',
      editor: 'idea',
      defaultCommitLanguage: 'en',
      repositorySettings: {},
    });
    expect(primaryAction(undefined, false)).toEqual({
      label: 'Configure Model',
      configureModel: true,
      resume: false,
      disabled: false,
    });
    expect(primaryAction(undefined, false, 'zh')).toEqual({
      label: getMessages('zh').primaryActions.configureModel,
      configureModel: true,
      resume: false,
      disabled: false,
    });
  });

  it('classifies structured model errors by category instead of localized text matching', () => {
    expect(isModelCommandError(new DesktopCommandError({
      code: 'modelAuthFailed',
      category: 'model',
      message: 'Authentication failed (401/403). Verify your API key and permissions.',
    }))).toBe(true);
    expect(isModelCommandError(toDesktopCommandError({
      code: 'modelRateLimited',
      category: 'model',
      message: '\u989d\u5ea6\u4e0d\u8db3\uff08402/429\uff09',
      retryable: true,
    }))).toBe(true);
    expect(isModelCommandError(new DesktopCommandError({
      code: 'gitFailed',
      category: 'git',
      message: 'Working directory is not a Git repository',
    }))).toBe(false);
    expect(isModelCommandError(new Error('Authentication failed (401/403) without structured category'))).toBe(false);
  });

  it('isolates per-repository editor and commitLanguage while updating fallback editor for unconfigured repos', () => {
    let state = fixture();
    state = apply(state, { type: 'editor', editor: 'rustrover', repositoryId: 'orders' });
    state = apply(state, { type: 'commitLanguage', commitLanguage: 'zh', repositoryId: 'orders' });
    state = apply(state, { type: 'editor', editor: 'cursor', repositoryId: 'cli' });
    state = apply(state, { type: 'commitLanguage', commitLanguage: 'en', repositoryId: 'cli' });

    expect(effectiveEditorFor(state, 'orders')).toBe('rustrover');
    expect(effectiveCommitLanguageFor(state, 'orders')).toBe('zh');
    expect(effectiveEditorFor(state, 'cli')).toBe('cursor');
    expect(effectiveCommitLanguageFor(state, 'cli')).toBe('en');
    expect(effectiveEditorFor(state, 'future-repo')).toBe('cursor');
    expect(effectiveCommitLanguageFor(state, 'future-repo')).toBe('en');
    expect(state.settingsModified).toBe(true);

    const filtered = { ...state, installedEditors: ['idea', 'vscode'] as WorkbenchState['installedEditors'] };
    expect(effectiveEditorFor(filtered, 'orders')).toBe('idea');
  });

  it('toggles selection without mutating file metadata; selectAll and directory selectFiles update scopeVersion', () => {
    const state = fixture();
    const beforeFiles = structuredClone(state.worktrees[0].files);
    const next = apply(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'lock' });
    expect(next.worktrees[0].files).toEqual(beforeFiles);
    expect(state.worktrees[0].selectedIds).not.toContain('lock');
    expect(next.worktrees[0].selectedIds).toContain('lock');
    expect(next.worktrees[1]).toEqual(state.worktrees[1]);

    const liveState = structuredClone(state);
    liveState.worktrees[0].head = 'head-1';
    liveState.worktrees[0].statusVersion = 'status-1';
    liveState.worktrees[0].scopeVersion = readScopeVersion(liveState.worktrees[0]);
    const beforeScope = liveState.worktrees[0].scopeVersion;
    const allSelected = apply(liveState, { type: 'selectAll', worktreeId: 'pay-tree' });
    expect(allSelected.worktrees[0].selectedIds).toEqual(beforeFiles.map((file) => file.id));
    expect(allSelected.worktrees[0].scopeVersion).not.toBe(beforeScope);

    const clearedAll = apply(allSelected, { type: 'selectAll', worktreeId: 'pay-tree' });
    expect(clearedAll.worktrees[0].selectedIds).toEqual([]);
    expect(clearedAll.worktrees[0].scopeVersion).not.toBe(allSelected.worktrees[0].scopeVersion);

    const rootSelected = apply(liveState, { type: 'selectFiles', worktreeId: 'pay-tree', fileIds: ['release', 'lock'] });
    expect(rootSelected.worktrees[0].selectedIds).toEqual(['order', 'pay', 'retry', 'release', 'lock']);
    expect(rootSelected.worktrees[0].scopeVersion).not.toBe(beforeScope);

    const rootCleared = apply(rootSelected, { type: 'selectFiles', worktreeId: 'pay-tree', fileIds: ['release', 'lock'] });
    expect(rootCleared.worktrees[0].selectedIds).toEqual(['order', 'pay', 'retry']);
    expect(rootCleared.worktrees[0].scopeVersion).not.toBe(rootSelected.worktrees[0].scopeVersion);
  });

  it('uses only staged stats (+28 -4) for partial staged files and localizes scopeSummary and root directory group', () => {
    const tree = fixture().worktrees[0];
    expect(scopeSummary(tree)).toBe('4/5 selected · partial staged');
    expect(scopeSummary(tree, 'zh')).toBe(getMessages('zh').changes.selectedSummary(4, 5, true, 0));
    expect(selectedFiles(tree)[0].range).toEqual({ kind: 'partial', added: 28, removed: 4 });
    expect(scopeSummary({ ...tree, selectedIds: [] })).toBe('No changes selected');
    expect(groupFiles(tree.files).find((group) => group.directory === '(root)')?.files.map((file) => file.id)).toEqual(['release', 'lock']);
    expect(groupFiles(tree.files, getMessages('zh').changes.rootDirectory).find((group) => group.directory === getMessages('zh').changes.rootDirectory)?.files.map((file) => file.id)).toEqual(['release', 'lock']);
  });

  it('preserves per-worktree prompt, selection, and replies across switches', () => {
    let state = fixture();
    state = apply(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'own instruction' });
    state = apply(state, { type: 'reply', worktreeId: 'pay-tree', messageId: 'question', text: 'split docs commit' });
    state = apply(state, { type: 'switch', worktreeId: 'fix-tree' });
    expect(state.worktrees[1].prompt.text).toBe('write concise bullets');
    state = apply(state, { type: 'switch', worktreeId: 'pay-tree' });
    expect(state.worktrees[0]).toMatchObject({ prompt: { text: 'own instruction' }, replies: { question: 'split docs commit' } });
    expect(state.worktrees[0].selectedIds).toHaveLength(4);
  });

  it('computes primaryAction labels for zero-model, empty selection, and resumable sessions', () => {
    expect(primaryAction(fixture('no-model').worktrees[0], false)).toEqual({
      label: 'Configure Model',
      configureModel: true,
      resume: false,
      disabled: false,
    });
    expect(primaryAction(fixture('unselected').worktrees[0], true).disabled).toBe(true);
    const failed = fixture('push-failed').worktrees[0];
    expect(primaryAction({ ...failed, selectedIds: [] }, false)).toEqual({
      label: 'Retry Push',
      configureModel: false,
      resume: true,
      disabled: false,
    });
    expect(primaryAction(fixture('conflict').worktrees[0], true).label).toBe('Resolve Conflict');
  });

  it('falls back to remaining model when deleting active model and clears to null when last model is deleted', () => {
    const state = fixture();
    const remaining = apply(state, { type: 'modelDeleted', modelId: 'test-claude' });
    expect(remaining.activeModelId).toBe('test-gpt');
    expect(apply(remaining, { type: 'modelDeleted', modelId: 'test-gpt' }).activeModelId).toBeNull();
  });
});

describe('Background events and prompt preservation', () => {
  it('incoming question never auto-expands dock or alters prompt', () => {
    const state = fixture('typing');
    const tree = state.worktrees[0];
    const next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(tree), message: releaseQuestion(), sessionStatus: 'needsDecision' } });
    expect(next.worktrees[0].prompt).toEqual(tree.prompt);
    expect(next.worktrees[0].dockExpanded).toBe(false);
    expect(orderedMessages(next.worktrees[0].messages)[0].kind).toBe('question');
    expect(orderedMessages(next.worktrees[0].messages)).toHaveLength(1);
  });

  it.each(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const)('rejects late event with mismatched %s', (field) => {
    const state = fixture();
    const target = { ...targetOf(state.worktrees[0]), [field]: 'obsolete' };
    const next = apply(state, { type: 'event', event: { type: 'message', target, message: releaseQuestion() } });
    expect(next).toEqual(state);
  });

  it('routes valid message for another worktree only to that target worktree', () => {
    const state = fixture();
    const next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(state.worktrees[1]), message: releaseQuestion() } });
    expect(next.worktrees[0]).toEqual(state.worktrees[0]);
    expect(next.worktrees[1].messages).toHaveLength(1);
  });

  it('collapses dock and removes stale question when terminal push completes', () => {
    const state = fixture('question');
    state.worktrees[0].dockExpanded = true;
    const next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(state.worktrees[0]), sessionStatus: 'done',
      message: { id: 'terminal-pushed', kind: 'done', title: 'Remote verified', description: 'Question resolved' } } });
    expect(next.worktrees[0].messages.map((message) => message.kind)).toEqual(['done']);
    expect(orderedMessages(next.worktrees[0].messages)[0].kind).toBe('done');
    expect(next.worktrees[0].dockExpanded).toBe(false);
    expect(next.worktrees[0].sessionStatus).toBe('done');
  });

  it('deduplicates repeated messages by id without stacking duplicates', () => {
    let state = fixture('question');
    const event = { type: 'message' as const, target: targetOf(state.worktrees[0]), message: releaseQuestion() };
    state = apply(apply(state, { type: 'event', event }), { type: 'event', event });
    expect(state.worktrees[0].messages).toHaveLength(1);
  });

  it('shares Local Keep across worktrees of the same repository, unchecks newly kept files, and updates scopeVersion', () => {
    let state = fixture('typing');
    state.worktrees[1] = {
      ...state.worktrees[1],
      statusVersion: 'fix-status-1',
      files: [
        ...state.worktrees[1].files,
        {
          id: 'fix-notes',
          path: 'RELEASE_NOTES.md',
          status: 'A',
          range: { kind: 'unstaged', added: 4, removed: 0 },
          contentVersion: 'v-notes',
        },
      ],
      selectedIds: [...state.worktrees[1].selectedIds, 'fix-notes'],
    };
    state.worktrees[0] = { ...state.worktrees[0], statusVersion: 'pay-status-1' };
    state.worktrees[0].scopeVersion = readScopeVersion(state.worktrees[0]);
    state.worktrees[1].scopeVersion = readScopeVersion(state.worktrees[1]);

    const staleTarget = targetOf(state.worktrees[0]);

    state = apply(state, {
      type: 'localKeepUpdated',
      repositoryId: 'orders',
      paths: ['RELEASE_NOTES.md'],
      keep: true,
    });

    expect(localKeepPathsFor(state, 'orders')).toEqual(['RELEASE_NOTES.md']);
    expect(localKeepPathsFor(state, 'cli')).toEqual([]);

    const payTree = state.worktrees[0];
    const fixTree = state.worktrees[1];
    expect(localKeepChanges(payTree).map((f) => f.path)).toEqual(['RELEASE_NOTES.md']);
    expect(localKeepChanges(fixTree).map((f) => f.path)).toEqual(['RELEASE_NOTES.md']);
    expect(payTree.selectedIds).not.toContain('release');
    expect(fixTree.selectedIds).not.toContain('fix-notes');
    expect(payTree.scopeVersion).not.toBe(staleTarget.scopeVersion);
  });

  it('computes selectAll strictly from normal changes, clears manually checked kept files on deselect, and evaluates renames by new path', () => {
    let state = fixture();
    state.worktrees[0] = {
      ...state.worktrees[0],
      statusVersion: 'pay-status-rename',
      files: [
        ...state.worktrees[0].files,
        {
          id: 'renamed-config',
          path: 'config/prod.json',
          previousPath: 'config/local.json',
          status: 'R',
          range: { kind: 'staged', added: 1, removed: 1 },
          contentVersion: 'v-ren',
        },
      ],
    };
    state = apply(state, {
      type: 'localKeepUpdated',
      repositoryId: 'orders',
      paths: ['RELEASE_NOTES.md', 'config/local.json'],
      keep: true,
    });

    const renamed = state.worktrees[0].files.find((f) => f.id === 'renamed-config')!;
    expect(Boolean(renamed.localKeep)).toBe(false);
    expect(normalChanges(state.worktrees[0]).map((f) => f.id)).toEqual(['order', 'pay', 'retry', 'lock', 'renamed-config']);
    expect(localKeepChanges(state.worktrees[0]).map((f) => f.id)).toEqual(['release']);

    state = apply(state, { type: 'selectAll', worktreeId: 'pay-tree' });
    expect(state.worktrees[0].selectedIds).toEqual(['order', 'pay', 'retry', 'lock', 'renamed-config']);
    expect(scopeSummary(state.worktrees[0])).toBe('5/5 selected · partial staged');

    state = apply(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'release' });
    expect(selectedLocalKeepChanges(state.worktrees[0]).map((f) => f.id)).toEqual(['release']);
    expect(scopeSummary(state.worktrees[0])).toBe('6/6 selected · partial staged');

    state = apply(state, { type: 'selectAll', worktreeId: 'pay-tree' });
    expect(state.worktrees[0].selectedIds).toEqual([]);
  });
});

describe('Compact Mode and Pin state transitions, status summaries, and non-auto-expand invariants', () => {
  it('keeps Pin and Compact Mode toggles completely independent across all four combinations', () => {
    let prefs = defaultWindowPreferences();
    expect(prefs).toEqual({
      pinned: false,
      compactMode: false,
      pinnedExpanded: false,
      fullSize: { width: 400, height: 700 },
    });

    // 1. Pinning stays in Full Mode without changing compactMode:
    prefs = computeNextPinPreferences(prefs, true);
    expect(prefs).toMatchObject({ pinned: true, compactMode: false });

    // 2. Collapsing enters Compact Mode while preserving pinned = true:
    prefs = computeNextCompactPreferences(prefs, true);
    expect(prefs).toMatchObject({ pinned: true, compactMode: true });

    // 3. Unpinning stays in Compact Mode without forcing Full Mode:
    prefs = computeNextPinPreferences(prefs, false);
    expect(prefs).toMatchObject({ pinned: false, compactMode: true });

    // 4. Expanding returns to Full Mode while preserving pinned = false:
    prefs = computeNextCompactPreferences(prefs, false);
    expect(prefs).toMatchObject({ pinned: false, compactMode: false });

    // 5. Collapsing while unpinned enters Compact Mode without forcing pinned = true:
    prefs = computeNextCompactPreferences(prefs, true);
    expect(prefs).toMatchObject({ pinned: false, compactMode: true });
  });

  it('tracks full and compact window positions independently and never overwrites fullSize while compact', () => {
    let state = fixture('typing');
    state = apply(state, {
      type: 'windowGeometryObserved',
      position: { x: 140, y: 90 },
      size: { width: 420, height: 660 },
    });
    expect(state.windowPrefs.fullPosition).toEqual({ x: 140, y: 90 });
    expect(state.windowPrefs.fullSize).toEqual({ width: 420, height: 660 });

    state = apply(state, {
      type: 'windowPreferencesUpdated',
      preferences: {
        ...state.windowPrefs,
        pinned: true,
        compactMode: true,
        compactPosition: { x: 140, y: 90 },
      },
    });
    // Dragging in compact mode updates compactPosition only and never shrinks fullSize to 140:
    state = apply(state, {
      type: 'windowGeometryObserved',
      position: { x: 880, y: 40 },
      size: { width: 400, height: 140 },
    });
    expect(state.windowPrefs.compactPosition).toEqual({ x: 880, y: 40 });
    expect(state.windowPrefs.fullPosition).toEqual({ x: 140, y: 90 });
    expect(state.windowPrefs.fullSize).toEqual({ width: 420, height: 660 });
  });

  it('shares a single prompt state across modes and never auto-expands compactMode on question, pushFailed, conflict, or done events', () => {
    let state = fixture('compact');
    expect(state.windowPrefs.compactMode).toBe(true);

    // Editing prompt in compact mode mutates the exact same worktree prompt state:
    state = apply(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'shared prompt across modes' });
    expect(state.worktrees[0].prompt.text).toBe('shared prompt across modes');

    const target = targetOf(state.worktrees[0]);
    for (const [status, kind, id] of [
      ['needsDecision', 'question', 'q-1'],
      ['pushFailed', 'pushFailed', 'pf-1'],
      ['conflict', 'conflict', 'cf-1'],
      ['done', 'done', 'dn-1'],
    ] as const) {
      state = apply(state, {
        type: 'event',
        event: {
          type: 'message',
          target,
          sessionStatus: status,
          message: { id, kind, title: `Event ${kind}`, description: 'Background update' },
        },
      });
      expect(state.windowPrefs.compactMode).toBe(true);
      expect(state.worktrees[0].prompt.text).toBe('shared prompt across modes');
    }
  });

  it('formats compactStatusSummary accurately across ready, step progress, actionable sessions, and blocking errors in en and zh', () => {
    expect(compactStatusSummary(undefined, false)).toEqual({
      tone: 'muted',
      label: 'Open a repository \u25b8',
      expandTarget: 'repo',
    });

    const readyTree = fixture('typing').worktrees[0];
    expect(compactStatusSummary(readyTree, true, 'en')).toEqual({
      tone: 'ready',
      label: 'Selected 4/5 \u00b7 Ready',
      expandTarget: 'changes',
    });
    expect(compactStatusSummary(readyTree, true, 'zh')).toEqual({
      tone: 'ready',
      label: '\u5df2\u9009 4/5 \u00b7 \u5c31\u7eea',
      expandTarget: 'changes',
    });

    // Committing step progress:
    const stepTree = {
      ...fixture('running').worktrees[0],
      messages: [{
        id: 'progress-run-1-step-2',
        kind: 'progress' as const,
        title: 'Writing commit 2/3',
        description: 'feat: split',
        progress: { completed: 2, total: 5, label: 'Write commit 2/3 \u00b7 Verify scope & Git hooks' },
      }],
    };
    expect(compactStatusSummary(stepTree, true, 'en')).toEqual({
      tone: 'running',
      label: 'Committing 2/3\u2026',
    });
    expect(compactStatusSummary(stepTree, true, 'zh')).toEqual({
      tone: 'running',
      label: '\u6b63\u5728\u63d0\u4ea4 2/3\u2026',
    });

    // Actionable sessions:
    expect(compactStatusSummary(fixture('question').worktrees[0], true, 'zh')).toEqual({
      tone: 'warning',
      label: '\u5f85\u4f60\u51b3\u5b9a \u25b8',
      expandTarget: 'dock',
    });
    expect(compactStatusSummary(fixture('push-failed').worktrees[0], true, 'zh')).toEqual({
      tone: 'danger',
      label: '\u63a8\u9001\u5931\u8d25 \u25b8 \u91cd\u8bd5',
      expandTarget: 'dock',
    });
    expect(compactStatusSummary(fixture('conflict').worktrees[0], true, 'zh')).toEqual({
      tone: 'danger',
      label: '\u6709\u51b2\u7a81 \u25b8',
      expandTarget: 'dock',
    });

    // Blocking conditions (Detached HEAD & Read Error):
    expect(compactStatusSummary({ ...readyTree, detached: true }, true, 'en')).toEqual({
      tone: 'warning',
      label: 'Detached HEAD \u25b8',
      expandTarget: 'changes',
    });
    expect(compactStatusSummary({ ...readyTree, readError: 'Git index locked' }, true, 'en')).toEqual({
      tone: 'danger',
      label: 'Status unavailable \u25b8',
      expandTarget: 'changes',
    });
  });

  it('decouples window size mode, rendered surface, and settings return target cleanly', () => {
    // 1. Compact bar (settings closed):
    expect(resolveWorkbenchPresentation(true, null)).toEqual({
      surface: 'compact',
      windowSizeMode: 'compact',
      mountWorkbenchContent: false,
      showSettings: false,
    });

    // 2. Settings opened from Compact Mode: full-sized window, standalone Settings, NEVER mounts workbench content:
    expect(resolveWorkbenchPresentation(true, 'compact')).toEqual({
      surface: 'standaloneSettings',
      windowSizeMode: 'full',
      mountWorkbenchContent: false,
      showSettings: true,
    });

    // 3. Full Workbench (settings closed):
    expect(resolveWorkbenchPresentation(false, null)).toEqual({
      surface: 'workbench',
      windowSizeMode: 'full',
      mountWorkbenchContent: true,
      showSettings: false,
    });

    // 4. Settings opened from Full Workbench: full-sized window, Settings overlaid on mounted workbench content:
    expect(resolveWorkbenchPresentation(false, 'workbench')).toEqual({
      surface: 'workbench',
      windowSizeMode: 'full',
      mountWorkbenchContent: true,
      showSettings: true,
    });
  });
});
