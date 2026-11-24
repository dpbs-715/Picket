import { describe, expect, it } from 'vitest';
import { getMessages } from '../../../i18n/messages';
import { fixture, releaseQuestion, sampleFindings } from '../testing/fixtures';
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
  isModelErrorText,
  isPlanOutdated,
  localKeepChanges,
  localKeepPathsFor,
  normalChanges,
  orderedMessages,
  primaryAction,
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

  it('classifies model errors accurately across connection, 401/403 auth, 402/429 quota, and timeout in en and zh', () => {
    expect(isModelErrorText('Authentication failed (401/403). Verify your API key and permissions.')).toBe(true);
    expect(isModelErrorText('Rate limit or quota exceeded (402/429). Check your account balance.')).toBe(true);
    expect(isModelErrorText('Unable to connect to model service (http://127.0.0.1:8080).')).toBe(true);
    expect(isModelErrorText('Model request timed out.')).toBe(true);
    expect(isModelErrorText('\u9274\u6743\u5931\u8d25\uff08401/403\uff09')).toBe(true);
    expect(isModelErrorText('\u989d\u5ea6\u4e0d\u8db3\uff08402/429\uff09')).toBe(true);
    expect(isModelErrorText('Working directory is not a Git repository')).toBe(false);
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

  it('toggles selection without mutating file metadata; selectAll and directory selectFiles update scopeVersion and invalidate in-flight plan/review requests', () => {
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
    liveState.worktrees[0].plan = {
      id: 'plan-live',
      baseScopeVersion: liveState.worktrees[0].scopeVersion,
      modelId: 'test-claude',
      summary: '1 commit',
      groups: [{ id: 'g1', fileIds: ['order'], message: 'feat: live' }],
      skippedFileIds: [],
    };
    const beforeScope = liveState.worktrees[0].scopeVersion;
    const withReq = apply(liveState, { type: 'request', target: targetOf(liveState.worktrees[0]), kind: 'plan', requestId: 'plan-select-all' });
    const allSelected = apply(withReq, { type: 'selectAll', worktreeId: 'pay-tree' });
    expect(allSelected.worktrees[0].selectedIds).toEqual(beforeFiles.map((file) => file.id));
    expect(allSelected.worktrees[0].scopeVersion).not.toBe(beforeScope);
    expect(isPlanOutdated(allSelected.worktrees[0])).toBe(true);
    expect(allSelected.worktrees[0].requests.plan).toBeUndefined();

    const clearedAll = apply(allSelected, { type: 'selectAll', worktreeId: 'pay-tree' });
    expect(clearedAll.worktrees[0].selectedIds).toEqual([]);
    expect(clearedAll.worktrees[0].scopeVersion).not.toBe(allSelected.worktrees[0].scopeVersion);

    let dirState = structuredClone(liveState);
    dirState = apply(dirState, { type: 'request', target: targetOf(dirState.worktrees[0]), kind: 'review', requestId: 'rev-dir-select' });
    const rootSelected = apply(dirState, { type: 'selectFiles', worktreeId: 'pay-tree', fileIds: ['release', 'lock'] });
    expect(rootSelected.worktrees[0].selectedIds).toEqual(['order', 'pay', 'retry', 'release', 'lock']);
    expect(rootSelected.worktrees[0].scopeVersion).not.toBe(beforeScope);
    expect(isPlanOutdated(rootSelected.worktrees[0])).toBe(true);
    expect(rootSelected.worktrees[0].requests.review).toBeUndefined();
    expect(rootSelected.worktrees[0].review.status).toBe('idle');

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

  it('preserves per-worktree prompt, selection, active tab, and replies across switches', () => {
    let state = fixture();
    state = apply(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'own instruction' });
    state = apply(state, { type: 'reply', worktreeId: 'pay-tree', messageId: 'question', text: 'split docs commit' });
    state = apply(state, { type: 'tab', worktreeId: 'pay-tree', tab: 'review' });
    state = apply(state, { type: 'switch', worktreeId: 'fix-tree' });
    expect(state.worktrees[1].prompt.text).toBe('write concise bullets');
    state = apply(state, { type: 'switch', worktreeId: 'pay-tree' });
    expect(state.worktrees[0]).toMatchObject({ prompt: { text: 'own instruction' }, tab: 'review', replies: { question: 'split docs commit' } });
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

describe('Background events and prompt/plan preservation', () => {
  it('incoming question never auto-expands dock, switches tab, or alters prompt; review stays secondary', () => {
    const state = fixture('typing');
    const tree = state.worktrees[0];
    let next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(tree), message: releaseQuestion(), sessionStatus: 'needsDecision' } });
    next = apply(next, { type: 'event', event: { type: 'message', target: targetOf(tree), message: { id: 'review', kind: 'review', title: 'Review complete', description: '1 high-risk' } } });
    expect(next.worktrees[0].prompt).toEqual(tree.prompt);
    expect(next.worktrees[0].tab).toBe('commit');
    expect(next.worktrees[0].dockExpanded).toBe(false);
    expect(orderedMessages(next.worktrees[0].messages)[0].kind).toBe('question');
    expect(orderedMessages(next.worktrees[0].messages)).toHaveLength(2);
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

  it('discards late planPrepared response after prompt edit, plan clear, newer request, or selection change', () => {
    let state = fixture('typing');
    state.worktrees[0] = { ...state.worktrees[0], plan: undefined };
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'plan', requestId: 'plan-1' });
    state = apply(state, { type: 'prompt', worktreeId: 'pay-tree', text: 'updated extra instruction' });
    state = apply(state, {
      type: 'event',
      event: { type: 'planPrepared', target, requestId: 'plan-1', modelId: 'test-claude', plan: { summary: 'stale', groups: [{ id: 'g1', fileIds: ['order'], message: 'feat: stale' }], skippedFileIds: [] } },
    });
    expect(state.worktrees[0].plan).toBeUndefined();
    expect(state.worktrees[0].requests.plan).toBeUndefined();

    state = apply(state, { type: 'request', target, kind: 'plan', requestId: 'old' });
    state = apply(state, { type: 'request', target, kind: 'plan', requestId: 'new' });
    state = apply(state, {
      type: 'event',
      event: { type: 'planPrepared', target, requestId: 'old', modelId: 'test-claude', plan: { summary: 'old', groups: [{ id: 'g1', fileIds: ['order'], message: 'feat: old' }], skippedFileIds: [] } },
    });
    expect(state.worktrees[0].requests.plan?.id).toBe('new');
    state = apply(state, { type: 'planCleared', worktreeId: 'pay-tree' });
    state = apply(state, {
      type: 'event',
      event: { type: 'planPrepared', target, requestId: 'new', modelId: 'test-claude', plan: { summary: 'cleared', groups: [{ id: 'g1', fileIds: ['order'], message: 'feat: cleared' }], skippedFileIds: [] } },
    });
    expect(state.worktrees[0].plan).toBeUndefined();
  });

  it('marks plan outdated only when selected files change, ignoring unselected changes', () => {
    const state = fixture('typing');
    const tree = state.worktrees[0];
    expect(isPlanOutdated(tree)).toBe(false);
    const unrelated = apply(state, { type: 'event', event: { type: 'files', target: targetOf(tree), nextScopeVersion: tree.scopeVersion, files: tree.files, changedPaths: ['pnpm-lock.yaml'] } });
    expect(isPlanOutdated(unrelated.worktrees[0])).toBe(false);
    const related = apply(state, { type: 'event', event: { type: 'files', target: targetOf(tree), nextScopeVersion: 'scope-2', files: tree.files, changedPaths: ['src/api/pay.ts'] } });
    expect(isPlanOutdated(related.worktrees[0])).toBe(true);
  });

  it('re-review replaces findings and skipped entries (both ASCII and fullwidth parens) only for re-reviewed files', () => {
    let state = fixture('changed');
    state.worktrees[0].review.skippedPaths = ['src/service/Order.ts (oversized diff)', 'pnpm-lock.yaml\uff08lockfile\uff09'];
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'review', requestId: 'review-order', fileIds: ['order'] });
    state = apply(state, { type: 'event', event: { type: 'reviewed', target, requestId: 'review-order', findings: [], skippedPaths: [] } });
    expect(state.worktrees[0].review.findings).toEqual([sampleFindings()[0]]);
    expect(state.worktrees[0].review.changedPaths).toEqual([]);
    expect(state.worktrees[0].review.skippedPaths).toEqual(['pnpm-lock.yaml\uff08lockfile\uff09']);
  });

  it('preserves previous passed review status when re-review fails', () => {
    let state = fixture();
    state.worktrees[0].review = { status: 'done', findings: [], skippedPaths: ['pnpm-lock.yaml'], changedPaths: [], scopeFileIds: ['order', 'pay'] };
    const target = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target, kind: 'review', requestId: 'failed-review' });
    state = apply(state, { type: 'requestFailed', target, kind: 'review', requestId: 'failed-review' });
    expect(state.worktrees[0].review.status).toBe('done');
    expect(state.worktrees[0].review.skippedPaths).toEqual(['pnpm-lock.yaml']);
  });

  it('collapses dock and removes stale question while preserving secondary review notice when terminal push completes', () => {
    const state = fixture('review-during-question');
    state.worktrees[0].dockExpanded = true;
    const next = apply(state, { type: 'event', event: { type: 'message', target: targetOf(state.worktrees[0]), sessionStatus: 'done',
      message: { id: 'terminal-pushed', kind: 'done', title: 'Remote verified', description: 'Question resolved' } } });
    expect(next.worktrees[0].messages.map((message) => message.kind)).toEqual(['review', 'done']);
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

  it('shares Local Keep across worktrees of the same repository, unchecks newly kept files, and invalidates in-flight plan requests', () => {
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
    state.worktrees[0].plan = { ...state.worktrees[0].plan!, baseScopeVersion: state.worktrees[0].scopeVersion };
    state.worktrees[1].scopeVersion = readScopeVersion(state.worktrees[1]);

    const staleTarget = targetOf(state.worktrees[0]);
    state = apply(state, { type: 'request', target: staleTarget, kind: 'plan', requestId: 'plan-before-keep' });

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
    expect(isPlanOutdated(payTree)).toBe(true);
    expect(payTree.requests.plan).toBeUndefined();
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

describe('Pinned Compact Mode state transitions, status summaries, and non-auto-expand invariants', () => {
  it('defaults to compact on Pin, remembers manual expand while pinned, resets on manual collapse, and exits compact on Unpin', () => {
    let prefs = defaultWindowPreferences();
    expect(prefs).toEqual({
      pinned: false,
      compactMode: false,
      pinnedExpanded: false,
      fullSize: { width: 400, height: 700 },
    });

    // 1. Clicking Pin enters compactMode by default:
    prefs = computeNextPinPreferences(prefs, true);
    expect(prefs).toMatchObject({ pinned: true, compactMode: true, pinnedExpanded: false });

    // 2. Manually expanding while pinned remembers pinnedExpanded = true without unpinning:
    prefs = computeNextCompactPreferences(prefs, false);
    expect(prefs).toMatchObject({ pinned: true, compactMode: false, pinnedExpanded: true });

    // 3. Unpinning exits compact mode while preserving pinnedExpanded = true:
    prefs = computeNextPinPreferences(prefs, false);
    expect(prefs).toMatchObject({ pinned: false, compactMode: false, pinnedExpanded: true });

    // 4. Next Pin remembers expanded preference (stays compactMode = false):
    prefs = computeNextPinPreferences(prefs, true);
    expect(prefs).toMatchObject({ pinned: true, compactMode: false, pinnedExpanded: true });

    // 5. Manually collapsing while pinned enters compactMode = true and resets pinnedExpanded = false:
    prefs = computeNextCompactPreferences(prefs, true);
    expect(prefs).toMatchObject({ pinned: true, compactMode: true, pinnedExpanded: false });

    // 6. Unpinning from compact mode exits compact mode:
    prefs = computeNextPinPreferences(prefs, false);
    expect(prefs).toMatchObject({ pinned: false, compactMode: false, pinnedExpanded: false });
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

  it('formats compactStatusSummary accurately across ready, plan, step progress, actionable sessions, and blocking errors in en and zh', () => {
    expect(compactStatusSummary(undefined, false)).toEqual({
      tone: 'muted',
      label: 'Open a repository \u25b8',
      expandTarget: 'repo',
    });

    const readyTree = { ...fixture('typing').worktrees[0], plan: undefined };
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

    // Pre-generated plan (valid vs outdated):
    const plannedTree = fixture('typing').worktrees[0];
    expect(compactStatusSummary(plannedTree, true, 'zh')).toEqual({
      tone: 'ready',
      label: '\u8ba1\u5212\uff1a1 \u4e2a\u63d0\u4ea4 \u25b8',
      expandTarget: 'plan',
    });
    const outdatedTree = fixture('changed').worktrees[0];
    expect(compactStatusSummary(outdatedTree, true, 'zh')).toEqual({
      tone: 'warning',
      label: '\u8ba1\u5212\uff1a2 \u4e2a\u63d0\u4ea4\uff08\u5df2\u8fc7\u671f\uff09\u25b8',
      expandTarget: 'plan',
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
});
