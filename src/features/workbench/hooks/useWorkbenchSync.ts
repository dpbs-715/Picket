import { useEffect, useRef, type Dispatch, type MutableRefObject, type SetStateAction } from 'react';
import type { TrayMainWindowAction } from '../../../../shared/tray-protocol';
import {
  subscribeTrayMainAction,
  subscribeTrayRefreshRequested,
  syncTrayState,
} from '../../../desktop/tray';
import { normalizeFullWindowSize, type WindowController } from '../../../desktop/window';
import { getMessages } from '../../../i18n';
import { buildTrayStateSnapshot } from '../../tray/trayModel';
import type { WorkbenchBackend, WorkbenchCommand } from '../commands';
import { serializeWorkbenchPersistence } from '../state/repositories';
import {
  orderedMessages,
  targetOf,
  type CompactExpandTarget,
  type WorkbenchState,
  type WorkbenchSurface,
  type Worktree,
} from '../state/workbench';
import type { NoticeItem, WorkbenchDispatch } from '../types';

interface UseWorkbenchSyncParams {
  state: WorkbenchState;
  latestState: MutableRefObject<WorkbenchState>;
  worktree: Worktree | undefined;
  hydrated: boolean;
  setHydrated: Dispatch<SetStateAction<boolean>>;
  initialState: WorkbenchState | undefined;
  backend: WorkbenchBackend;
  windowController: WindowController;
  dispatch: WorkbenchDispatch;
  setFeedback: Dispatch<SetStateAction<Record<string, NoticeItem>>>;
  pending: string[];
  inFlight: MutableRefObject<Set<string>>;
  hasUpdate: boolean;
  pinInFlight: MutableRefObject<boolean>;
  desiredCompactHeightRef: MutableRefObject<number>;
  activeSurfaceRef: MutableRefObject<WorkbenchSurface>;
  execute: (command: WorkbenchCommand, preserveFeedback?: boolean) => Promise<boolean>;
  openSettingsFromWorkbench: (focusModels?: boolean) => void;
  openSettingsFromCompact: (options?: { focusModels?: boolean; drawerWasOpen?: boolean }) => Promise<void>;
  clearWorkbenchSettingsSession: () => void;
  onClearOverlays: () => void;
  expandFromCompact: (target?: CompactExpandTarget | 'addModel') => Promise<void>;
}

export function useWorkbenchSync({
  state,
  latestState,
  worktree,
  hydrated,
  setHydrated,
  initialState,
  backend,
  windowController,
  dispatch,
  setFeedback,
  pending,
  inFlight,
  hasUpdate,
  pinInFlight,
  desiredCompactHeightRef,
  activeSurfaceRef,
  execute,
  openSettingsFromWorkbench,
  openSettingsFromCompact,
  clearWorkbenchSettingsSession,
  onClearOverlays,
  expandFromCompact,
}: UseWorkbenchSyncParams) {
  const previousSessionStatusRef = useRef<Record<string, Worktree['sessionStatus']> | null>(null);
  const saveRevision = useRef(0);
  const lastPersistedKey = useRef<string | null>(null);
  const lastStructuralKey = useRef<string | null>(null);
  const lastTraySnapshotKey = useRef<string | null>(null);
  const executeRef = useRef(execute);
  executeRef.current = execute;
  const trayActionHandlerRef = useRef<(action: TrayMainWindowAction) => void>(() => {});
  const trayRefreshHandlerRef = useRef<() => void>(() => {});

  useEffect(() => backend.subscribe?.((event) => dispatch({ type: 'event', event })), [backend, dispatch]);

  useEffect(() => {
    if (initialState || !backend.loadWorkbenchState) return;
    let mounted = true;
    void backend.loadWorkbenchState().then(async (restored) => {
      if (!mounted) return;
      let effectiveRestored = restored;
      if (windowController.isDesktopWindow) {
        if (restored.windowPreferences && !latestState.current.settingsModified && !pinInFlight.current) {
          const saved = restored.windowPreferences;
          const targetPos = saved.compactMode ? saved.compactPosition : saved.fullPosition;
          try {
            const applied = await windowController.applyWindowMode({
              pinned: saved.pinned,
              compactMode: saved.compactMode,
              compactHeight: desiredCompactHeightRef.current,
              fullSize: normalizeFullWindowSize(saved.fullSize),
              placement: targetPos ? 'restorePosition' : 'anchorTopLeft',
              targetPosition: targetPos,
              instant: true,
              locale: restored.locale ?? latestState.current.locale,
            });
            effectiveRestored = {
              ...restored,
              windowPreferences: {
                ...saved,
                pinned: applied.pinned,
                fullSize: normalizeFullWindowSize(saved.fullSize),
                ...(saved.compactMode
                  ? { compactPosition: applied.position }
                  : { fullPosition: applied.position }),
              },
            };
          } catch {
            // Ignore window restore failure so repository/model hydration still completes cleanly.
          }
        }
      }
      if (!mounted) return;
      dispatch({ type: 'workbenchHydrated', restored: effectiveRestored });
      if (restored.warning) {
        const key = restored.activeWorktreeId ?? restored.snapshots[0]?.activeWorktreeId ?? 'empty';
        setFeedback((previous) => ({ ...previous, [key]: { text: restored.warning! } }));
      }
      if (windowController.isDesktopWindow && !restored.windowPreferences && !latestState.current.settingsModified && !pinInFlight.current) {
        try {
          const snap = await windowController.readSnapshot();
          if (mounted && snap && !latestState.current.settingsModified && !pinInFlight.current) {
            dispatch({
              type: 'windowSnapshotSynced',
              pinned: snap.pinned,
              size: normalizeFullWindowSize(snap.size),
              position: snap.position,
            });
          }
        } catch {
          if (mounted) {
            setFeedback((previous) => ({
              ...previous,
              [latestState.current.activeWorktreeId ?? 'empty']: { text: getMessages(latestState.current.locale).workbench.readPinFailedFallback },
            }));
          }
        }
      }
      if (mounted) setHydrated(true);
    }).catch((error: unknown) => {
      if (!mounted) return;
      const message = error instanceof Error ? error.message : getMessages(latestState.current.locale).workbench.loadStateFailedFallback;
      setFeedback((previous) => ({ ...previous, empty: { text: message } }));
      setHydrated(true);
    });
    return () => { mounted = false; };
  }, [backend, desiredCompactHeightRef, dispatch, initialState, latestState, pinInFlight, setFeedback, setHydrated, windowController]);

  useEffect(() => {
    if (!hydrated) return;
    const nextMap: Record<string, Worktree['sessionStatus']> = {};
    for (const tree of state.worktrees) {
      nextMap[tree.worktreeId] = tree.sessionStatus;
    }
    const previousMap = previousSessionStatusRef.current;
    previousSessionStatusRef.current = nextMap;
    if (!previousMap) return;

    const notifCatalog = getMessages(state.locale).compact.notifications;
    for (const tree of state.worktrees) {
      const prevStatus = previousMap[tree.worktreeId];
      if (prevStatus === tree.sessionStatus) continue;
      if (tree.sessionStatus !== 'needsDecision' && tree.sessionStatus !== 'pushFailed' && tree.sessionStatus !== 'conflict') {
        continue;
      }
      const title = tree.sessionStatus === 'conflict'
        ? notifCatalog.conflictTitle
        : tree.sessionStatus === 'pushFailed'
          ? notifCatalog.pushFailedTitle
          : notifCatalog.needsDecisionTitle;
      const topMessage = orderedMessages(tree.messages)[0];
      const body = topMessage?.title || notifCatalog.fallbackBody(tree.branch);
      void windowController.notifyBackgroundAttention({ title, body });
    }
  }, [hydrated, state.locale, state.worktrees, windowController]);

  const persistenceSnapshot = serializeWorkbenchPersistence(state, 0);
  const persistenceKey = JSON.stringify(persistenceSnapshot);
  const structuralKey = JSON.stringify([
    state.locale,
    state.editor,
    state.defaultCommitLanguage,
    state.repositorySettings,
    state.windowPrefs.pinned,
    state.windowPrefs.compactMode,
    state.windowPrefs.pinnedExpanded,
    state.activeWorktreeId,
    state.worktrees.map((tree) => [tree.worktreeId, tree.selectedIds, tree.scopeVersion, tree.tab]),
  ]);

  useEffect(() => {
    if (!hydrated || !backend.saveWorkbenchState) return;
    if (lastPersistedKey.current === null) {
      lastStructuralKey.current = structuralKey;
      if (!state.settingsModified && state.navigationRevision === 0 && !state.emptyPrompt) {
        lastPersistedKey.current = persistenceKey;
        return;
      }
    }
    if (persistenceKey === lastPersistedKey.current) {
      lastStructuralKey.current = structuralKey;
      return;
    }
    const immediate = structuralKey !== lastStructuralKey.current;
    lastStructuralKey.current = structuralKey;
    const persistNow = () => {
      const current = serializeWorkbenchPersistence(latestState.current, 0);
      const currentKey = JSON.stringify(current);
      if (currentKey === lastPersistedKey.current) return;
      lastPersistedKey.current = currentKey;
      const revision = ++saveRevision.current;
      void backend.saveWorkbenchState!({ ...current, revision }).catch((error: unknown) => {
        const message = error instanceof Error
          ? error.message
          : getMessages(latestState.current.locale).workbench.saveStateFailedFallback;
        setFeedback((previous) => ({ ...previous, [latestState.current.activeWorktreeId ?? 'empty']: { text: message } }));
      });
    };
    if (immediate) {
      persistNow();
      return;
    }
    const timer = setTimeout(persistNow, 80);
    return () => clearTimeout(timer);
  }, [backend, hydrated, latestState, persistenceKey, setFeedback, state.emptyPrompt, state.navigationRevision, state.settingsModified, structuralKey]);

  useEffect(() => {
    if (!hydrated || !backend.saveWorkbenchState) return;
    const flush = () => {
      const current = serializeWorkbenchPersistence(latestState.current, 0);
      const currentKey = JSON.stringify(current);
      if (currentKey === lastPersistedKey.current) return;
      lastPersistedKey.current = currentKey;
      const revision = ++saveRevision.current;
      void backend.saveWorkbenchState!({ ...current, revision }).catch(() => {});
    };
    window.addEventListener('pagehide', flush);
    return () => window.removeEventListener('pagehide', flush);
  }, [backend, hydrated, latestState]);

  const worktreeRef = useRef(worktree);
  worktreeRef.current = worktree;
  useEffect(() => {
    if (!backend.repositoryReadsAvailable || !worktree?.worktreeId) return;
    const refresh = () => {
      const activeTree = worktreeRef.current;
      if (!activeTree || inFlight.current.has('global:openRepository')) return;
      void executeRef.current(
        { kind: 'refresh', target: targetOf(activeTree), requestId: crypto.randomUUID(), locale: latestState.current.locale },
        true,
      );
    };
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [backend, inFlight, latestState, worktree?.worktreeId, worktree?.scopeVersion]);

  const anyRepoRefreshing = pending.some((key) => key.endsWith(':refresh'));
  const traySnapshot = buildTrayStateSnapshot(state, {
    hasUpdate,
    refreshing: anyRepoRefreshing,
  });
  const traySnapshotKey = JSON.stringify(traySnapshot);

  useEffect(() => {
    if (!hydrated || initialState || !windowController.isDesktopWindow) return;
    if (traySnapshotKey === lastTraySnapshotKey.current) return;
    lastTraySnapshotKey.current = traySnapshotKey;
    void syncTrayState(traySnapshot).catch(() => {});
  }, [hydrated, initialState, traySnapshot, traySnapshotKey, windowController.isDesktopWindow]);

  trayRefreshHandlerRef.current = () => {
    if (!backend.repositoryReadsAvailable || inFlight.current.has('global:openRepository')) return;
    const current = latestState.current;
    const seenRepos = new Set<string>();
    const activeTree = current.worktrees.find((tree) => tree.worktreeId === current.activeWorktreeId);
    const orderedTrees = activeTree
      ? [activeTree, ...current.worktrees.filter((tree) => tree.worktreeId !== activeTree.worktreeId)]
      : current.worktrees;
    for (const tree of orderedTrees) {
      if (seenRepos.has(tree.repositoryId)) continue;
      seenRepos.add(tree.repositoryId);
      if (inFlight.current.has(`${tree.repositoryId}:refresh`) || inFlight.current.has(`${tree.repositoryId}:write`)) {
        continue;
      }
      void execute(
        { kind: 'refresh', target: targetOf(tree), requestId: crypto.randomUUID(), locale: current.locale },
        true,
      );
    }
  };

  trayActionHandlerRef.current = (action) => {
    if (action.kind === 'openSettings') {
      if (activeSurfaceRef.current === 'compact') {
        void openSettingsFromCompact({ focusModels: false });
      } else {
        openSettingsFromWorkbench(false);
      }
      return;
    }
    if (action.kind === 'focusWorktree') {
      const targetTree = latestState.current.worktrees.find((tree) => tree.worktreeId === action.worktreeId);
      if (!targetTree) return;
      clearWorkbenchSettingsSession();
      onClearOverlays();
      dispatch({ type: 'switch', worktreeId: targetTree.worktreeId, expand: action.openDock || undefined });
      if (action.openDock && activeSurfaceRef.current === 'compact') {
        void expandFromCompact('dock');
      }
      if (backend.repositoryReadsAvailable) {
        void execute(
          { kind: 'refresh', target: targetOf(targetTree), requestId: crypto.randomUUID(), locale: latestState.current.locale },
          true,
        );
      }
    }
  };

  useEffect(() => {
    if (!windowController.isDesktopWindow || initialState) return;
    const unsubRefresh = subscribeTrayRefreshRequested(() => {
      trayRefreshHandlerRef.current();
    });
    const unsubAction = subscribeTrayMainAction((action) => {
      trayActionHandlerRef.current(action);
    });
    return () => {
      unsubRefresh();
      unsubAction();
    };
  }, [initialState, windowController.isDesktopWindow]);
}
