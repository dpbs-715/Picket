import { describe, expect, it } from 'vitest';
import {
  decodeWorkbenchRestoreReply,
  type WorkbenchRestoreReply,
} from '../../../../shared/workbench-persistence-protocol';
import { repositorySnapshot } from '../testing/repository-fixture';
import { readScopeVersion, resolvePersistenceKeys, serializeWorkbenchPersistence } from './repositories';
import { effectiveCommitLanguageFor, effectiveEditorFor, emptyWorkbench, workbenchReducer } from './workbench';

describe('Workbench cross-restart v2 persistence projection and hydration', () => {
  it('restores locale, per-project settings, active worktree, prompts, and selections', () => {
    const snapshot = repositorySnapshot(1);
    const unchangedScope = readScopeVersion({
      ...snapshot.worktrees[1],
      selectedIds: [],
      scopeVersion: snapshot.worktrees[1].statusVersion,
    });
    const restored: WorkbenchRestoreReply = {
      protocolVersion: 2,
      editor: 'webstorm',
      activeWorktreeId: 'fix-tree',
      emptyPrompt: 'empty-repo extra instruction',
      snapshots: [snapshot],
      warning: null,
      worktrees: [
        {
          repositoryId: 'orders',
          worktreeId: 'pay-tree',
          promptText: 'split lockfile separately',
          selectedIds: ['order'],
          scopeVersion: 'old-scope-before-external-edit',
          tab: 'review',
        },
        {
          repositoryId: 'orders',
          worktreeId: 'fix-tree',
          promptText: 'unchanged prompt',
          selectedIds: [],
          scopeVersion: unchangedScope,
          tab: 'commit',
        },
      ],
    };

    const hydrated = workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored });
    expect(hydrated.locale).toBe('en');
    expect(hydrated.defaultCommitLanguage).toBe('en');
    expect(hydrated.editor).toBe('webstorm');
    expect(hydrated.repositorySettings.orders).toEqual({ editor: 'webstorm', commitLanguage: 'en' });
    expect(hydrated.activeWorktreeId).toBe('fix-tree');
    expect(hydrated.emptyPrompt).toBe('empty-repo extra instruction');

    const payTree = hydrated.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;
    expect(payTree.selectedIds).toEqual(['order']);
    expect(payTree.prompt.text).toBe('split lockfile separately');

    const fixTree = hydrated.worktrees.find((tree) => tree.worktreeId === 'fix-tree')!;
    expect(fixTree.selectedIds).toEqual([]);
    expect(fixTree.prompt.text).toBe('unchanged prompt');
  });

  it('isolates per-repository editor and commitLanguage across projects and round-trips across cold restart', () => {
    const ordersSnapshot = repositorySnapshot(1);
    const cliSnapshot = repositorySnapshot(1);
    cliSnapshot.repository = {
      id: 'cli',
      name: 'picket-cli',
      branches: [{ name: 'main', worktreeId: 'cli-tree' }],
      remoteBranches: [],
    };
    cliSnapshot.activeWorktreeId = 'cli-tree';
    cliSnapshot.worktrees = [{
      ...cliSnapshot.worktrees[0],
      repositoryId: 'cli',
      worktreeId: 'cli-tree',
      label: 'cli',
      branch: 'main',
    }];

    let state = emptyWorkbench();
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'open-orders' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: { protocolVersion: 1, requestId: 'open-orders', snapshot: ordersSnapshot } });
    state = workbenchReducer(state, { type: 'editor', editor: 'rustrover', repositoryId: 'orders' });
    state = workbenchReducer(state, { type: 'commitLanguage', commitLanguage: 'zh', repositoryId: 'orders' });

    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'open-cli' });
    state = workbenchReducer(state, { type: 'repositoryReadFinished', key: 'open', reply: { protocolVersion: 1, requestId: 'open-cli', snapshot: cliSnapshot } });
    state = workbenchReducer(state, { type: 'editor', editor: 'cursor', repositoryId: 'cli' });
    state = workbenchReducer(state, { type: 'commitLanguage', commitLanguage: 'en', repositoryId: 'cli' });
    state = workbenchReducer(state, { type: 'locale', locale: 'zh' });

    expect(effectiveEditorFor(state, 'orders')).toBe('rustrover');
    expect(effectiveCommitLanguageFor(state, 'orders')).toBe('zh');
    expect(effectiveEditorFor(state, 'cli')).toBe('cursor');
    expect(effectiveCommitLanguageFor(state, 'cli')).toBe('en');

    const saved = serializeWorkbenchPersistence(state, 3);
    expect(saved.locale).toBe('zh');
    expect(saved.repositorySettings).toEqual([
      { repositoryId: 'orders', editor: 'rustrover', commitLanguage: 'zh' },
      { repositoryId: 'cli', editor: 'cursor', commitLanguage: 'en' },
    ]);

    const coldRestored = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        locale: saved.locale,
        editor: saved.editor,
        defaultCommitLanguage: saved.defaultCommitLanguage,
        repositorySettings: saved.repositorySettings,
        activeWorktreeId: saved.activeWorktreeId,
        emptyPrompt: saved.emptyPrompt,
        worktrees: saved.worktrees,
        snapshots: [ordersSnapshot, cliSnapshot],
        warning: null,
      },
    });

    expect(coldRestored.locale).toBe('zh');
    expect(effectiveEditorFor(coldRestored, 'orders')).toBe('rustrover');
    expect(effectiveCommitLanguageFor(coldRestored, 'orders')).toBe('zh');
    expect(effectiveEditorFor(coldRestored, 'cli')).toBe('cursor');
    expect(effectiveCommitLanguageFor(coldRestored, 'cli')).toBe('en');
  });

  it('does not overwrite settings or prompts changed by the user before async hydration arrives', () => {
    let state = emptyWorkbench();
    state = workbenchReducer(state, { type: 'locale', locale: 'zh' });
    state = workbenchReducer(state, { type: 'editor', editor: 'cursor' });
    state = workbenchReducer(state, { type: 'commitLanguage', commitLanguage: 'zh' });
    state = workbenchReducer(state, { type: 'prompt', worktreeId: null, text: 'freshly typed instruction' });
    state = workbenchReducer(state, {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        locale: 'en',
        editor: 'idea',
        defaultCommitLanguage: 'en',
        activeWorktreeId: null,
        emptyPrompt: 'stale instruction',
        snapshots: [],
        worktrees: [],
        warning: null,
      },
    });
    expect(state.locale).toBe('zh');
    expect(state.editor).toBe('cursor');
    expect(state.defaultCommitLanguage).toBe('zh');
    expect(state.emptyPrompt).toBe('freshly typed instruction');
  });

  it('serializes only allowlisted v2 persistence fields without paths, write tokens, credentials, or legacy draft fields', () => {
    const snapshot = repositorySnapshot(1);
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        editor: 'vscode',
        activeWorktreeId: 'pay-tree',
        emptyPrompt: '',
        snapshots: [snapshot],
        worktrees: [],
        warning: null,
      },
    });
    const payload = serializeWorkbenchPersistence(hydrated, 5);
    const serialized = JSON.stringify(payload);
    expect(payload.revision).toBe(5);
    expect(payload.locale).toBe('en');
    expect(payload.editor).toBe('vscode');
    expect(payload.defaultCommitLanguage).toBe('en');
    expect(serialized).not.toMatch(/commitToken|branchToken|path|apiKey|draftText|emptyDraft/);
  });

  it('filters against installedEditors and falls back to the first installed editor when saved editor is unavailable', () => {
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        editor: 'cursor',
        repositorySettings: [{ repositoryId: 'orders', editor: 'vscode', commitLanguage: 'zh' }],
        installedEditors: ['idea', 'rustrover', 'webstorm'],
        activeWorktreeId: null,
        emptyPrompt: '',
        snapshots: [],
        worktrees: [],
        warning: null,
      },
    });
    expect(hydrated.installedEditors).toEqual(['idea', 'rustrover', 'webstorm']);
    expect(hydrated.editor).toBe('idea');
    expect(effectiveEditorFor(hydrated, 'orders')).toBe('idea');
    expect(effectiveCommitLanguageFor(hydrated, 'orders')).toBe('zh');
  });

  it('hydrates repository-wide localKeepPaths across worktrees on cold restart and re-applies when clean kept files reappear', () => {
    const snapshot = repositorySnapshot(1);
    snapshot.repository.localKeepPaths = ['RELEASE_NOTES.md', 'scripts/dev-only.sh'];
    snapshot.worktrees[1].files.push({
      id: 'fix-notes',
      path: 'RELEASE_NOTES.md',
      status: 'M',
      gitStatus: '.M',
      range: { kind: 'unstaged', added: 2, removed: 0 },
      binary: false,
      ignoredForReview: false,
      localKeep: true,
      contentVersion: 'v-fix-notes',
      commitToken: 'tok-fix-notes',
    });

    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        editor: 'idea',
        activeWorktreeId: 'pay-tree',
        emptyPrompt: '',
        snapshots: [snapshot],
        worktrees: [],
        warning: null,
      },
    });

    expect(hydrated.repositorySettings.orders?.localKeepPaths).toEqual(['RELEASE_NOTES.md', 'scripts/dev-only.sh']);
    const payTree = hydrated.worktrees.find((t) => t.worktreeId === 'pay-tree')!;
    const fixTree = hydrated.worktrees.find((t) => t.worktreeId === 'fix-tree')!;
    expect(payTree.files.find((f) => f.path === 'RELEASE_NOTES.md')?.localKeep).toBe(true);
    expect(fixTree.files.find((f) => f.path === 'RELEASE_NOTES.md')?.localKeep).toBe(true);
    expect(payTree.selectedIds).not.toContain('release');
    expect(fixTree.selectedIds).not.toContain('fix-notes');

    const nextSnap = repositorySnapshot(2);
    nextSnap.repository.localKeepPaths = ['RELEASE_NOTES.md', 'scripts/dev-only.sh'];
    nextSnap.worktrees[0].files.push({
      id: 'dev-script',
      path: 'scripts/dev-only.sh',
      status: 'A',
      gitStatus: '??',
      range: { kind: 'unstaged', added: 5, removed: 0 },
      binary: false,
      ignoredForReview: false,
      localKeep: true,
      contentVersion: 'v-dev-script',
      commitToken: 'tok-dev-script',
    });
    let refreshed = workbenchReducer(hydrated, { type: 'repositoryReadStarted', key: 'orders', requestId: 'ref-2' });
    refreshed = workbenchReducer(refreshed, {
      type: 'repositoryReadFinished',
      key: 'orders',
      reply: { protocolVersion: 1, requestId: 'ref-2', target: { repositoryId: 'orders', worktreeId: 'pay-tree', sessionId: payTree.sessionId, scopeVersion: payTree.scopeVersion }, snapshot: nextSnap },
    });
    const refreshedPay = refreshed.worktrees.find((t) => t.worktreeId === 'pay-tree')!;
    expect(refreshedPay.files.find((f) => f.path === 'scripts/dev-only.sh')?.localKeep).toBe(true);
    expect(refreshedPay.selectedIds).not.toContain('dev-script');
  });

  it('persists and cold-restores windowPreferences and rejects out-of-range window geometry payloads', () => {
    let state = emptyWorkbench();
    state = workbenchReducer(state, {
      type: 'windowPreferencesUpdated',
      preferences: {
        pinned: true,
        compactMode: true,
        pinnedExpanded: false,
        fullSize: { width: 430, height: 640 },
        fullPosition: { x: 160, y: 110 },
        compactPosition: { x: 920, y: 48 },
      },
    });
    const saved = serializeWorkbenchPersistence(state, 9);
    expect(saved.windowPreferences).toEqual({
      pinned: true,
      compactMode: true,
      pinnedExpanded: false,
      fullSize: { width: 430, height: 640 },
      fullPosition: { x: 160, y: 110 },
      compactPosition: { x: 920, y: 48 },
    });

    const decoded = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: 'en',
      editor: 'idea',
      defaultCommitLanguage: 'en',
      windowPreferences: saved.windowPreferences,
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
    });
    const hydrated = workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored: decoded });
    expect(hydrated.windowPrefs).toEqual(saved.windowPreferences);

    // Rejects sub-480 fullSize height or non-integer coordinates:
    expect(() =>
      decodeWorkbenchRestoreReply({
        protocolVersion: 2,
        editor: 'idea',
        windowPreferences: {
          pinned: false,
          compactMode: false,
          pinnedExpanded: false,
          fullSize: { width: 400, height: 479 },
        },
        activeWorktreeId: null,
        emptyPrompt: '',
        worktrees: [],
        snapshots: [],
        warning: null,
      }),
    ).toThrow();
  });

  it('preserves manually unchecked files across cold restart while auto-selecting newly modified normal files', () => {
    const initialSnapshot = repositorySnapshot(1);
    let state = emptyWorkbench();
    state = workbenchReducer(state, { type: 'repositoryReadStarted', key: 'open', requestId: 'open-1' });
    state = workbenchReducer(state, {
      type: 'repositoryReadFinished',
      key: 'open',
      reply: { protocolVersion: 1, requestId: 'open-1', snapshot: initialSnapshot },
    });
    // Uncheck pnpm-lock.yaml manually:
    state = workbenchReducer(state, { type: 'select', worktreeId: 'pay-tree', fileId: 'lock' });
    const saved = serializeWorkbenchPersistence(state, 11);
    expect(saved.worktrees.find((item) => item.worktreeId === 'pay-tree')?.unselectedIds).toEqual(['lock']);

    // While Picket is closed, a brand-new normal file appears on disk:
    const restartedSnapshot = repositorySnapshot(1);
    restartedSnapshot.worktrees[0].files.push({
      id: 'fresh-feature',
      path: 'src/freshFeature.ts',
      status: 'A',
      gitStatus: '??',
      range: { kind: 'unstaged', added: 24, removed: 0 },
      binary: false,
      ignoredForReview: false,
      contentVersion: 'v-fresh',
      commitToken: 'tok-fresh',
    });

    const decoded = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: saved.locale,
      editor: saved.editor,
      defaultCommitLanguage: saved.defaultCommitLanguage,
      repositorySettings: saved.repositorySettings,
      windowPreferences: saved.windowPreferences,
      activeWorktreeId: saved.activeWorktreeId,
      emptyPrompt: saved.emptyPrompt,
      worktrees: saved.worktrees,
      snapshots: [restartedSnapshot],
      warning: null,
    });
    const hydrated = workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored: decoded });
    const payTree = hydrated.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;
    expect(payTree.selectedIds).toEqual(['order', 'pay', 'retry', 'release', 'fresh-feature']);
    expect(payTree.unselectedIds).toEqual(['lock']);
  });

  it('persists and cold-restores theme preference (system, dark, light) and defaults missing theme to system', () => {
    let state = emptyWorkbench();
    expect(state.theme).toBe('system');

    state = workbenchReducer(state, { type: 'theme', theme: 'light' });
    expect(state.theme).toBe('light');
    expect(state.settingsModified).toBe(true);

    const saved = serializeWorkbenchPersistence(state, 3);
    expect(saved.theme).toBe('light');

    const decodedLight = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: saved.locale,
      theme: saved.theme,
      editor: saved.editor,
      defaultCommitLanguage: saved.defaultCommitLanguage,
      repositorySettings: saved.repositorySettings,
      windowPreferences: saved.windowPreferences,
      activeWorktreeId: saved.activeWorktreeId,
      emptyPrompt: saved.emptyPrompt,
      worktrees: saved.worktrees,
      snapshots: [],
      warning: null,
    });
    expect(decodedLight.theme).toBe('light');
    expect(workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored: decodedLight }).theme).toBe('light');

    // Legacy v2 files without a theme field decode cleanly and hydrate to 'system':
    const decodedLegacy = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: 'en',
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
    });
    expect(decodedLegacy.theme).toBeUndefined();
    expect(workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored: decodedLegacy }).theme).toBe('system');

    // Invalid theme values are rejected by protocol decoder:
    expect(() => decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: 'en',
      theme: 'solarized',
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
    })).toThrow(/protocol mismatch/i);
  });

  it('reuses cached persistence keys when only high-frequency runtime workflow state changes', () => {
    const snapshot = repositorySnapshot(1);
    const base = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 2,
        editor: 'idea',
        activeWorktreeId: 'pay-tree',
        emptyPrompt: '',
        worktrees: [],
        snapshots: [snapshot],
        warning: null,
      },
    });
    const first = resolvePersistenceKeys(base, null);

    // High-frequency streaming updates (activeRun, messages, dockExpanded, replies) must reuse cached snapshot reference:
    const afterStart = workbenchReducer(base, {
      type: 'actionStarted',
      worktreeId: 'pay-tree',
      profile: 'customAction',
      runId: 'run-stream-1',
      actionId: 'act-1',
      actionLabel: 'Review',
      frozenTarget: {
        repositoryId: 'orders',
        worktreeId: 'pay-tree',
        sessionId: base.worktrees[0].sessionId,
        scopeVersion: base.worktrees[0].scopeVersion,
      },
      frozenFileIds: [],
      modelId: 'm-1',
      expandDock: true,
    });
    const afterTick = workbenchReducer(afterStart, {
      type: 'event',
      event: {
        type: 'actionProgress',
        progress: {
          protocolVersion: 2,
          session: {
            runId: 'run-stream-1',
            repositoryId: 'orders',
            worktreeId: 'pay-tree',
            sessionId: afterStart.worktrees[0].sessionId,
            actionId: 'act-1',
            actionLabel: 'Review',
            profile: 'customAction',
            frozenScopeVersion: afterStart.worktrees[0].scopeVersion,
            modelId: 'm-1',
            frozenFileIds: [],
            committedFileIds: [],
            skippedFileIds: [],
            status: 'running',
            summary: '',
            message: {
              id: 'progress-run-stream-1',
              kind: 'progress',
              title: 'Review',
              description: 'Checking step 2',
            },
            timeline: [
              { state: 'done', label: 'Step 1' },
              { state: 'active', label: 'Step 2' },
            ],
            qaLog: [],
            commits: [],
            links: [],
            canRetryRemaining: false,
          },
        },
      },
    });
    expect(resolvePersistenceKeys(afterStart, first)).toBe(first);
    expect(resolvePersistenceKeys(afterTick, first)).toBe(first);

    // Persisted changes (prompt text or file selection) produce a fresh key:
    const afterPrompt = workbenchReducer(afterTick, { type: 'prompt', worktreeId: 'pay-tree', text: 'extra note' });
    const promptKeys = resolvePersistenceKeys(afterPrompt, first);
    expect(promptKeys).not.toBe(first);
    expect(promptKeys.persistenceKey).not.toBe(first.persistenceKey);
    expect(promptKeys.structuralKey).toBe(first.structuralKey);

    const afterSelect = workbenchReducer(afterPrompt, { type: 'select', worktreeId: 'pay-tree', fileId: 'order' });
    const selectKeys = resolvePersistenceKeys(afterSelect, promptKeys);
    expect(selectKeys).not.toBe(promptKeys);
    expect(selectKeys.structuralKey).not.toBe(promptKeys.structuralKey);
  });

  it('persists and cold-restores global actionApprovalMode (ask, auto, allowAll) and defaults legacy payloads to auto', () => {
    let state = emptyWorkbench();
    expect(state.actionApprovalMode).toBe('auto');

    state = workbenchReducer(state, { type: 'actionApprovalMode', mode: 'ask' });
    expect(state.actionApprovalMode).toBe('ask');
    expect(state.settingsModified).toBe(true);

    const saved = serializeWorkbenchPersistence(state, 4);
    expect(saved.actionApprovalMode).toBe('ask');

    const decoded = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: saved.locale,
      theme: saved.theme,
      actionApprovalMode: saved.actionApprovalMode,
      editor: saved.editor,
      defaultCommitLanguage: saved.defaultCommitLanguage,
      repositorySettings: saved.repositorySettings,
      windowPreferences: saved.windowPreferences,
      activeWorktreeId: saved.activeWorktreeId,
      emptyPrompt: saved.emptyPrompt,
      worktrees: saved.worktrees,
      snapshots: [],
      warning: null,
    });
    expect(decoded.actionApprovalMode).toBe('ask');
    expect(workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored: decoded }).actionApprovalMode).toBe('ask');

    // Legacy payloads without actionApprovalMode default to 'auto':
    const decodedLegacy = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: 'en',
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
    });
    expect(decodedLegacy.actionApprovalMode).toBeUndefined();
    expect(workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored: decodedLegacy }).actionApprovalMode).toBe('auto');

    // Invalid mode values are rejected by protocol decoder:
    expect(() => decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      locale: 'en',
      actionApprovalMode: 'yolo',
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
    })).toThrow(/protocol mismatch/i);
  });
});
