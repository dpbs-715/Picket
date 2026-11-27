import { useEffect, useRef, type Dispatch, type MutableRefObject, type SetStateAction } from 'react';
import type { TrayMainWindowAction } from '../../../../shared/tray-protocol';
import {
  subscribeTrayMainAction,
  subscribeTraySnapshotsSynced,
  syncTrayContext,
} from '../../../desktop/tray';
import { normalizeFullWindowSize, type WindowController } from '../../../desktop/window';
import { getMessages } from '../../../i18n';
import type { WorkbenchBackend, WorkbenchCommand } from '../commands';
import {
  resolvePersistenceKeys,
  serializeWorkbenchPersistence,
  type PersistenceKeySnapshot,
} from '../state/persistence';
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
  inFlight: MutableRefObject<Set<string>>;
  pinInFlight: MutableRefObject<boolean>;
  desiredCompactHeightRef: MutableRefObject<number>;
  activeSurfaceRef: MutableRefObject<WorkbenchSurface>;
  isEditingCommitInput: () => boolean;
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
  inFlight,
  pinInFlight,
  desiredCompactHeightRef,
  activeSurfaceRef,
  isEditingCommitInput,
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
  const persistenceSnapshotRef = useRef<PersistenceKeySnapshot | null>(null);
  const executeRef = useRef(execute);
  executeRef.current = execute;
  const trayActionHandlerRef = useRef<(action: TrayMainWindowAction) => void>(() => {});

  const isEditingRef = useRef(isEditingCommitInput);
  isEditingRef.current = isEditingCommitInput;

  useEffect(
    () => backend.subscribe?.((event) => {
      const suppressExpand = (event.type === 'customActionProgress' || event.type === 'cPushProgress')
        && (isEditingRef.current() || latestState.current.windowPrefs.compactMode);
      dispatch({ type: 'event', event, ...(suppressExpand ? { suppressExpand: true } : {}) });
    }),
    [backend, dispatch, latestState],
  );

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

  const keySnapshot = resolvePersistenceKeys(state, persistenceSnapshotRef.current);
  persistenceSnapshotRef.current = keySnapshot;
  const { persistenceKey, structuralKey } = keySnapshot;

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
  const hasActiveWorktree = Boolean(worktree?.worktreeId);
  useEffect(() => {
    if (!backend.repositoryReadsAvailable || !hasActiveWorktree) return;
    let wasHidden = typeof document !== 'undefined' && document.visibilityState === 'hidden';
    const refresh = () => {
      const activeTree = worktreeRef.current;
      if (!activeTree || inFlight.current.has('global:openRepository')) return;
      void executeRef.current(
        { kind: 'refresh', target: targetOf(activeTree), requestId: crypto.randomUUID(), locale: latestState.current.locale },
        true,
      );
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        wasHidden = true;
        return;
      }
      if (document.visibilityState === 'visible' && wasHidden) {
        wasHidden = false;
        refresh();
      }
    };
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [backend, hasActiveWorktree, inFlight, latestState]);

  // Rust is the authoritative source of Tray state; only push lightweight UI context changes.
  useEffect(() => {
    if (!hydrated || initialState || !windowController.isDesktopWindow) return;
    void syncTrayContext({
      locale: state.locale,
      theme: state.theme,
      activeWorktreeId: state.activeWorktreeId,
    }).catch(() => {});
  }, [hydrated, initialState, state.activeWorktreeId, state.locale, state.theme, windowController.isDesktopWindow]);

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
    const unsubSnapshots = subscribeTraySnapshotsSynced((snapshots) => {
      if (snapshots.length > 0) {
        dispatch({ type: 'traySnapshotsSynced', snapshots });
      }
    });
    const unsubAction = subscribeTrayMainAction((action) => {
      trayActionHandlerRef.current(action);
    });
    return () => {
      unsubSnapshots();
      unsubAction();
    };
  }, [dispatch, initialState, windowController.isDesktopWindow]);
}
