import { isValidRepoRelativePath } from '../../../../shared/git-read-protocol';
import { DEFAULT_APP_LOCALE, DEFAULT_COMMIT_LANGUAGE } from '../../../../shared/language-protocol';
import { DEFAULT_APP_THEME } from '../../../../shared/theme-protocol';
import {
  WINDOW_DIMENSIONS,
  type PersistedWindowPreferences,
  type WorkbenchRestoreReply,
  type WorkbenchSaveRequest,
} from '../../../../shared/workbench-persistence-protocol';
import { computeNormalUnselectedIds, defaultWindowPreferences } from './selectors';
import { mergeSnapshot, normalizeWorktreeLocalKeep, projectWorktree, readScopeVersion } from './snapshotMerge';
import type { EditorId, RepositorySettings, WorkbenchState, Worktree } from './types';

export function serializeWorkbenchPersistence(state: WorkbenchState, revision: number): WorkbenchSaveRequest {
  const windowPreferences: PersistedWindowPreferences = {
    pinned: state.windowPrefs.pinned,
    compactMode: state.windowPrefs.compactMode,
    pinnedExpanded: state.windowPrefs.pinnedExpanded,
    ...(state.windowPrefs.fullSize ? { fullSize: { ...state.windowPrefs.fullSize } } : {}),
    ...(state.windowPrefs.fullPosition ? { fullPosition: { ...state.windowPrefs.fullPosition } } : {}),
    ...(state.windowPrefs.compactPosition ? { compactPosition: { ...state.windowPrefs.compactPosition } } : {}),
  };
  return {
    revision,
    locale: state.locale,
    theme: state.theme,
    editor: state.editor,
    defaultCommitLanguage: state.defaultCommitLanguage,
    repositorySettings: Object.entries(state.repositorySettings).map(([repositoryId, cfg]) => ({
      repositoryId,
      editor: cfg.editor,
      commitLanguage: cfg.commitLanguage,
      ...(cfg.localKeepPaths?.length ? { localKeepPaths: [...cfg.localKeepPaths] } : {}),
      ...(cfg.customActions?.length ? { customActions: cfg.customActions.map((item) => ({ ...item })) } : {}),
      ...(cfg.activeActionId ? { activeActionId: cfg.activeActionId } : {}),
    })),
    windowPreferences,
    activeWorktreeId: state.activeWorktreeId,
    emptyPrompt: state.emptyPrompt,
    worktrees: state.worktrees.map((tree) => ({
      repositoryId: tree.repositoryId,
      worktreeId: tree.worktreeId,
      promptText: tree.prompt.text,
      selectedIds: [...tree.selectedIds],
      unselectedIds: [...(tree.unselectedIds ?? computeNormalUnselectedIds(tree.files, tree.selectedIds))],
      scopeVersion: tree.scopeVersion,
    })),
  };
}

export function hydrateWorkbench(state: WorkbenchState, restored: WorkbenchRestoreReply): WorkbenchState {
  const persistedByWorktree = new Map(restored.worktrees.map((item) => [item.worktreeId, item]));
  const cPushByWorktree = new Map((restored.cPushSessions ?? []).map((item) => [item.worktreeId, item]));
  const actionByWorktree = new Map((restored.actionSessions ?? []).map((item) => [item.worktreeId, item]));
  const userEditedEmpty = state.emptyPrompt.length > 0;
  const hydratedConnections = restored.connections === undefined
    ? state.connections
    : state.connections.length > 0
      ? [...restored.connections.filter((item) => !state.connections.some((existing) => existing.id === item.id)), ...state.connections]
      : restored.connections;
  const hydratedModels = restored.models === undefined
    ? state.models
    : state.models.length > 0
      ? [...restored.models.filter((item) => !state.models.some((existing) => existing.id === item.id)), ...state.models]
      : restored.models;
  const hydratedActiveModelId = restored.models === undefined
    ? state.activeModelId
    : state.activeModelId ?? restored.activeModelId ?? hydratedModels[0]?.id ?? null;
  const installedEditors = restored.installedEditors ?? state.installedEditors;
  const resolveEditor = (desired: EditorId): EditorId =>
    installedEditors && installedEditors.length > 0 && !installedEditors.includes(desired)
      ? installedEditors[0]
      : desired;
  const restoredLocale = restored.locale ?? DEFAULT_APP_LOCALE;
  const restoredTheme = restored.theme ?? DEFAULT_APP_THEME;
  const restoredDefaultCommitLang = restored.defaultCommitLanguage ?? DEFAULT_COMMIT_LANGUAGE;
  const locale = state.settingsModified ? state.locale : restoredLocale;
  const theme = state.settingsModified ? state.theme : restoredTheme;
  const defaultCommitLanguage = state.settingsModified ? state.defaultCommitLanguage : restoredDefaultCommitLang;
  const desiredEditor = state.settingsModified ? state.editor : restored.editor;
  const effectiveEditor = resolveEditor(desiredEditor);
  const repositorySettings: Record<string, RepositorySettings> = {};
  for (const entry of restored.repositorySettings ?? []) {
    const keepPaths = entry.localKeepPaths?.filter(isValidRepoRelativePath) ?? [];
    const customActions = entry.customActions?.length ? entry.customActions : undefined;
    const activeActionId = entry.activeActionId && customActions?.some((item) => item.id === entry.activeActionId)
      ? entry.activeActionId
      : undefined;
    repositorySettings[entry.repositoryId] = {
      editor: resolveEditor(entry.editor),
      commitLanguage: entry.commitLanguage,
      ...(keepPaths.length ? { localKeepPaths: [...new Set(keepPaths)].sort() } : {}),
      ...(customActions ? { customActions } : {}),
      ...(activeActionId ? { activeActionId } : {}),
    };
  }
  for (const snapshot of restored.snapshots) {
    const snapKeep = snapshot.repository.localKeepPaths?.filter(isValidRepoRelativePath);
    const existing = repositorySettings[snapshot.repository.id];
    const mergedKeep = snapKeep !== undefined
      ? [...new Set(snapKeep)].sort()
      : existing?.localKeepPaths;
    repositorySettings[snapshot.repository.id] = {
      editor: existing?.editor ?? resolveEditor(restored.editor),
      commitLanguage: existing?.commitLanguage ?? restoredDefaultCommitLang,
      ...(mergedKeep?.length ? { localKeepPaths: mergedKeep } : {}),
      ...(existing?.customActions?.length ? { customActions: existing.customActions } : {}),
      ...(existing?.activeActionId ? { activeActionId: existing.activeActionId } : {}),
    };
  }
  if (state.settingsModified) {
    for (const [repositoryId, cfg] of Object.entries(state.repositorySettings)) {
      const existing = repositorySettings[repositoryId];
      const keepPaths = cfg.localKeepPaths ?? existing?.localKeepPaths;
      const customActions = cfg.customActions ?? existing?.customActions;
      const activeActionId = cfg.activeActionId !== undefined ? cfg.activeActionId : existing?.activeActionId;
      repositorySettings[repositoryId] = {
        editor: resolveEditor(cfg.editor),
        commitLanguage: cfg.commitLanguage,
        ...(keepPaths?.length ? { localKeepPaths: keepPaths } : {}),
        ...(customActions?.length ? { customActions } : {}),
        ...(activeActionId ? { activeActionId } : {}),
      };
    }
  }
  const restoredWindowPrefs = restored.windowPreferences
    ? {
        ...defaultWindowPreferences(),
        ...restored.windowPreferences,
        fullSize: restored.windowPreferences.fullSize
          ? {
              width: Math.max(WINDOW_DIMENSIONS.minFullWidth, restored.windowPreferences.fullSize.width),
              height: Math.max(WINDOW_DIMENSIONS.minFullHeight, restored.windowPreferences.fullSize.height),
            }
          : defaultWindowPreferences().fullSize,
      }
    : state.windowPrefs;
  let next: WorkbenchState = {
    ...state,
    locale,
    theme,
    editor: effectiveEditor,
    defaultCommitLanguage,
    repositorySettings,
    windowPrefs: state.settingsModified ? state.windowPrefs : restoredWindowPrefs,
    ...(installedEditors !== undefined ? { installedEditors } : {}),
    emptyPrompt: userEditedEmpty ? state.emptyPrompt : restored.emptyPrompt,
    connections: hydratedConnections,
    models: hydratedModels,
    activeModelId: hydratedActiveModelId,
  };
  for (const snapshot of restored.snapshots) {
    if (next.repositories.some((repo) => repo.id === snapshot.repository.id)) {
      next = mergeSnapshot(next, snapshot, false);
      continue;
    }
    const repoKeepPaths = repositorySettings[snapshot.repository.id]?.localKeepPaths ?? [];
    const keepSet = new Set(repoKeepPaths);
    const hasSnapshotKeepList = snapshot.repository.localKeepPaths !== undefined || repoKeepPaths.length > 0;
    const worktrees = snapshot.worktrees.map((rawIncoming): Worktree => {
      const incoming = normalizeWorktreeLocalKeep(rawIncoming, keepSet, hasSnapshotKeepList);
      const base = projectWorktree(incoming);
      const saved = persistedByWorktree.get(incoming.worktreeId);
      const savedCPush = cPushByWorktree.get(incoming.worktreeId);
      const savedAction = actionByWorktree.get(incoming.worktreeId);
      const withSavedPrompt: Worktree = (!saved || incoming.readError) ? base : (() => {
        const selectedIds = saved.unselectedIds !== undefined
          ? (() => {
              const savedSelectedSet = new Set(saved.selectedIds);
              const savedUnselectedSet = new Set(saved.unselectedIds);
              return incoming.files
                .filter((file) => (file.localKeep ? savedSelectedSet.has(file.id) : !savedUnselectedSet.has(file.id)))
                .map((file) => file.id);
            })()
          : saved.selectedIds.filter((id) => incoming.files.some((file) => file.id === id));
        const unselectedIds = computeNormalUnselectedIds(incoming.files, selectedIds);
        const scopeVersion = readScopeVersion({ ...base, selectedIds });
        return {
          ...base,
          selectedIds,
          unselectedIds,
          scopeVersion,
          prompt: { text: saved.promptText, revision: 0 },
        };
      })();
      if (savedCPush && savedCPush.sessionStatus !== 'idle') {
        return {
          ...withSavedPrompt,
          sessionId: savedCPush.sessionId,
          sessionStatus: savedCPush.sessionStatus,
          messages: savedCPush.message ? [savedCPush.message] : [],
          dockExpanded: false,
          cPush: {
            runId: savedCPush.runId,
            frozenTarget: {
              repositoryId: savedCPush.repositoryId,
              worktreeId: savedCPush.worktreeId,
              sessionId: savedCPush.sessionId,
              scopeVersion: savedCPush.frozenScopeVersion,
            },
            frozenFileIds: savedCPush.frozenFileIds,
            promptRevisionAtStart: withSavedPrompt.prompt.revision,
            modelId: savedCPush.modelId,
            timeline: savedCPush.timeline,
            qaLog: savedCPush.qaLog,
            commits: savedCPush.commits,
            canRetryRemaining: savedCPush.canRetryRemaining,
          },
        };
      }
      if (savedAction?.message) {
        return {
          ...withSavedPrompt,
          sessionStatus: savedAction.status === 'done' ? 'done' : 'idle',
          messages: [savedAction.message],
          dockExpanded: false,
          customAction: {
            runId: savedAction.runId,
            actionId: savedAction.actionId,
            actionLabel: savedAction.actionLabel,
            timeline: savedAction.timeline,
            commits: savedAction.commits,
            links: savedAction.links,
          },
        };
      }
      return withSavedPrompt;
    });
    next = {
      ...next,
      repositories: [
        ...next.repositories,
        {
          ...snapshot.repository,
          ...(repoKeepPaths.length ? { localKeepPaths: repoKeepPaths } : { localKeepPaths: undefined }),
          readSequence: snapshot.readSequence,
        },
      ],
      worktrees: [...next.worktrees, ...worktrees],
    };
  }
  if (state.navigationRevision === 0 && state.activeWorktreeId === null) {
    next = {
      ...next,
      activeWorktreeId: restored.activeWorktreeId && next.worktrees.some((tree) => tree.worktreeId === restored.activeWorktreeId)
        ? restored.activeWorktreeId
        : next.worktrees[0]?.worktreeId ?? null,
    };
  }
  return next;
}
