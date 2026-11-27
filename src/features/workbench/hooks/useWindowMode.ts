import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from 'react';
import { flushSync } from 'react-dom';
import { WINDOW_DIMENSIONS, type WindowSize } from '../../../../shared/workbench-persistence-protocol';
import {
  clampCompactHeight,
  normalizeCompactWidth,
  normalizeFullWindowSize,
  type WindowController,
} from '../../../desktop/window';
import { getMessages } from '../../../i18n';
import {
  computeNextCompactPreferences,
  computeNextPinPreferences,
  defaultWindowPreferences,
  resolveWorkbenchPresentation,
  type CompactExpandTarget,
  type WorkbenchState,
  type WorkbenchSurface,
  type Worktree,
} from '../state/workbench';
import type {
  ModelOverlayState,
  NotifyFn,
  PendingModeFocus,
  SettingsSession,
  WorkbenchDispatch,
} from '../types';

interface UseWindowModeParams {
  state: WorkbenchState;
  latestState: MutableRefObject<WorkbenchState>;
  worktree: Worktree | undefined;
  hydrated: boolean;
  initialState: WorkbenchState | undefined;
  backendHasLoadState: boolean;
  windowController: WindowController;
  dispatch: WorkbenchDispatch;
  notify: NotifyFn;
  setModelOverlay: Dispatch<SetStateAction<ModelOverlayState>>;
  onClearOverlays: () => void;
}

export type WindowTransitionKind = 'idle' | 'expanding' | 'collapsing';

interface WindowTransitionState {
  kind: 'expanding' | 'collapsing';
  heightPx: number;
  mountWorkbenchDuringExpand?: boolean;
}

function measureWorkbenchHeight(fallback: number): number {
  if (typeof document === 'undefined') return fallback;
  const measured = Math.round(document.querySelector<HTMLElement>('.workbench')?.getBoundingClientRect().height ?? 0);
  return measured > WINDOW_DIMENSIONS.compactDefaultHeight ? measured : fallback;
}

function commitBeforeWindowResize(update: () => void): void {
  try {
    flushSync(update);
  } catch {
    update();
  }
}

export function useWindowMode({
  state,
  latestState,
  worktree,
  hydrated,
  initialState,
  backendHasLoadState,
  windowController,
  dispatch,
  notify,
  setModelOverlay,
  onClearOverlays,
}: UseWindowModeParams) {
  const [settingsSession, setSettingsSession] = useState<SettingsSession | null>(null);
  const settingsSessionRef = useRef<SettingsSession | null>(null);
  settingsSessionRef.current = settingsSession;
  const [windowTransition, setWindowTransition] = useState<WindowTransitionState | null>(null);
  const [pinBusy, setPinBusy] = useState(false);
  const pinInFlight = useRef(false);
  const desiredCompactHeightRef = useRef<number>(WINDOW_DIMENSIONS.compactDefaultHeight);
  const collapsedCompactHeightRef = useRef<number>(WINDOW_DIMENSIONS.compactDefaultHeight);
  const pendingModeFocusRef = useRef<PendingModeFocus | null>(null);
  const workbenchReadyNotifiedRef = useRef(false);

  const t = getMessages(state.locale);
  const pinned = state.windowPrefs.pinned;
  const compactMode = state.windowPrefs.compactMode;
  const effectiveCompactMode = windowTransition?.mountWorkbenchDuringExpand ? false : compactMode;
  const presentation = resolveWorkbenchPresentation(effectiveCompactMode, settingsSession?.returnTo ?? null);
  const settingsOpen = presentation.showSettings;
  const activeSurfaceRef = useRef<WorkbenchSurface>(presentation.surface);
  activeSurfaceRef.current = presentation.surface;
  const transitionKind: WindowTransitionKind = windowTransition?.kind ?? 'idle';
  const transitionHeightPx = windowTransition?.heightPx ?? null;

  function openSettingsFromWorkbench(focusModels = false) {
    const next: SettingsSession = { returnTo: 'workbench', focusModels };
    settingsSessionRef.current = next;
    setSettingsSession(next);
  }

  function clearWorkbenchSettingsSession() {
    if (settingsSessionRef.current?.returnTo === 'workbench') {
      settingsSessionRef.current = null;
      setSettingsSession(null);
    }
  }

  async function openSettingsFromCompact(options: { focusModels?: boolean; drawerWasOpen?: boolean } = {}) {
    if (pinInFlight.current || activeSurfaceRef.current !== 'compact') return;
    const currentPrefs = latestState.current.windowPrefs;
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);

    if (!windowController.isDesktopWindow) {
      try {
        await windowController.applyWindowMode({
          pinned: currentPrefs.pinned,
          compactMode: false,
          fullSize,
          placement: 'anchorTopLeft',
          locale: state.locale,
        });
      } catch (error) {
        notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
      }
      return;
    }

    const focusModels = Boolean(options.focusModels);
    const fallbackCompactHeight = clampCompactHeight(collapsedCompactHeightRef.current);
    const provisionalSession: SettingsSession = {
      returnTo: 'compact',
      focusModels,
      ...(currentPrefs.compactPosition
        ? {
            compactSnapshot: {
              position: currentPrefs.compactPosition,
              size: { width: WINDOW_DIMENSIONS.compactWidth, height: fallbackCompactHeight },
            },
          }
        : {}),
    };

    pinInFlight.current = true;
    // Synchronously mount standalone Settings at fullSize.height BEFORE enlarging the native window
    // so neither a stretched CompactBar nor the full workbench content can ever flash during expansion.
    settingsSessionRef.current = provisionalSession;
    commitBeforeWindowResize(() => {
      setPinBusy(true);
      setWindowTransition({ kind: 'expanding', heightPx: fullSize.height });
      setSettingsSession(provisionalSession);
    });

    try {
      const applied = await windowController.applyWindowMode({
        pinned: currentPrefs.pinned,
        compactMode: false,
        fullSize,
        placement: 'anchorTopLeft',
        locale: state.locale,
      });
      const isTransientlyExpanded = Boolean(options.drawerWasOpen)
        || desiredCompactHeightRef.current > fallbackCompactHeight + 2;
      const compactSize: WindowSize = {
        width: normalizeCompactWidth(applied.previousSize.width),
        height: isTransientlyExpanded
          ? fallbackCompactHeight
          : clampCompactHeight(applied.previousSize.height),
      };
      const settledSession: SettingsSession = {
        returnTo: 'compact',
        focusModels,
        compactSnapshot: {
          position: applied.previousPosition,
          size: compactSize,
        },
      };
      settingsSessionRef.current = settledSession;
      setSettingsSession(settledSession);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...latestState.current.windowPrefs,
          pinned: applied.pinned,
          compactMode: true,
          compactPosition: applied.previousPosition,
        },
      });
    } catch (error) {
      settingsSessionRef.current = null;
      setSettingsSession(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    } finally {
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
    }
  }

  async function closeSettings() {
    const session = settingsSessionRef.current;
    if (!session) return;

    if (session.returnTo === 'workbench') {
      settingsSessionRef.current = null;
      setSettingsSession(null);
      return;
    }

    if (pinInFlight.current) return;
    const currentPrefs = latestState.current.windowPrefs;
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);
    const snapshot = session.compactSnapshot;
    const targetPosition = snapshot?.position ?? currentPrefs.compactPosition;
    const targetWidth = normalizeCompactWidth(snapshot?.size.width);
    const targetHeight = clampCompactHeight(snapshot?.size.height ?? collapsedCompactHeightRef.current);
    const freezeHeight = measureWorkbenchHeight(fullSize.height);

    pinInFlight.current = true;
    commitBeforeWindowResize(() => {
      setPinBusy(true);
      setWindowTransition({ kind: 'collapsing', heightPx: freezeHeight });
    });
    try {
      // Shrink and move back to pre-settings compact geometry FIRST while standalone Settings
      // remains mounted and height-locked, then swap directly to CompactBar without ever mounting full workbench.
      const applied = await windowController.applyWindowMode({
        pinned: currentPrefs.pinned,
        compactMode: true,
        compactWidth: targetWidth,
        compactHeight: targetHeight,
        fullSize,
        placement: targetPosition ? 'restorePosition' : 'anchorTopLeft',
        ...(targetPosition ? { targetPosition } : {}),
        locale: state.locale,
      });
      desiredCompactHeightRef.current = applied.size.height;
      collapsedCompactHeightRef.current = applied.size.height;
      onClearOverlays();
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
      settingsSessionRef.current = null;
      setSettingsSession(null);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...latestState.current.windowPrefs,
          pinned: applied.pinned,
          compactMode: true,
          compactPosition: applied.position,
        },
      });
    } catch (error) {
      onClearOverlays();
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
      settingsSessionRef.current = null;
      setSettingsSession(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    } finally {
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
    }
  }

  function openConfigureModel() {
    if (latestState.current.connections.length === 0) {
      setModelOverlay({ kind: 'createProvider' });
    } else {
      openSettingsFromWorkbench(true);
    }
  }

  function openConfigureModelFromCompact() {
    if (latestState.current.connections.length === 0) {
      void expandFromCompact('addModel');
    } else {
      void openSettingsFromCompact({ focusModels: true });
    }
  }

  const handleDesiredCompactHeight = useCallback((height: number, drawerOpen = false) => {
    desiredCompactHeightRef.current = height;
    if (!drawerOpen) {
      collapsedCompactHeightRef.current = height;
    }
    if (activeSurfaceRef.current !== 'compact' || !windowController.isDesktopWindow || pinInFlight.current) {
      return;
    }
    void windowController.adjustCompactHeight(height, latestState.current.locale).catch(() => {});
  }, [latestState, windowController]);

  function captureModeSwitchFocus(overrideTarget?: CompactExpandTarget | 'addModel') {
    pendingModeFocusRef.current = overrideTarget ? { kind: overrideTarget } : null;
  }

  async function togglePin() {
    if (pinInFlight.current) return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPinned = !currentPrefs.pinned;

    pinInFlight.current = true;
    setPinBusy(true);
    try {
      const appliedPinned = await windowController.setPinned(nextPinned, state.locale);
      pinInFlight.current = false;
      setPinBusy(false);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: computeNextPinPreferences(latestState.current.windowPrefs, appliedPinned),
      });
    } catch (error) {
      pinInFlight.current = false;
      setPinBusy(false);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  async function collapseToCompact() {
    if (pinInFlight.current || latestState.current.windowPrefs.compactMode) return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPrefs = computeNextCompactPreferences(currentPrefs, true);
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);
    const freezeHeight = measureWorkbenchHeight(fullSize.height);

    pinInFlight.current = true;
    if (windowController.isDesktopWindow) {
      commitBeforeWindowResize(() => {
        setPinBusy(true);
        setWindowTransition({ kind: 'collapsing', heightPx: freezeHeight });
      });
    } else {
      setPinBusy(true);
    }
    try {
      const applied = await windowController.applyWindowMode({
        pinned: nextPrefs.pinned,
        compactMode: true,
        compactHeight: WINDOW_DIMENSIONS.compactDefaultHeight,
        fullSize,
        placement: 'anchorTopLeft',
        locale: state.locale,
      });
      desiredCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      collapsedCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      captureModeSwitchFocus();
      settingsSessionRef.current = null;
      setSettingsSession(null);
      onClearOverlays();
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...nextPrefs,
          pinned: applied.pinned,
          fullSize: normalizeFullWindowSize(applied.previousSize),
          fullPosition: applied.previousPosition,
          compactPosition: applied.position,
        },
      });
    } catch (error) {
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  async function expandFromCompact(target?: CompactExpandTarget | 'addModel') {
    if (pinInFlight.current || activeSurfaceRef.current !== 'compact') return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPrefs = computeNextCompactPreferences(currentPrefs, false);
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);

    if (!windowController.isDesktopWindow) {
      pinInFlight.current = true;
      setPinBusy(true);
      try {
        await windowController.applyWindowMode({
          pinned: nextPrefs.pinned,
          compactMode: false,
          fullSize,
          placement: 'anchorTopLeft',
          locale: state.locale,
        });
      } catch (error) {
        notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
      } finally {
        pinInFlight.current = false;
        setPinBusy(false);
      }
      return;
    }

    const shouldPreExpandDock = Boolean(target === 'dock' && worktree && !worktree.dockExpanded);
    pinInFlight.current = true;
    // Synchronously mount full WorkbenchContent locked to fullSize.height BEFORE expanding the native
    // window so the OS frame reveals the already-painted full layout smoothly without blank flashes or reflow.
    commitBeforeWindowResize(() => {
      setPinBusy(true);
      setWindowTransition({
        kind: 'expanding',
        heightPx: fullSize.height,
        mountWorkbenchDuringExpand: true,
      });
      if (shouldPreExpandDock && worktree) {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true });
      }
    });
    try {
      const applied = await windowController.applyWindowMode({
        pinned: nextPrefs.pinned,
        compactMode: false,
        fullSize,
        placement: 'anchorTopLeft',
        locale: state.locale,
      });
      desiredCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      collapsedCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      captureModeSwitchFocus(target);
      if (target === 'dock' && worktree && !shouldPreExpandDock) {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true });
      } else if (target === 'addModel') {
        openConfigureModel();
      }
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...nextPrefs,
          pinned: applied.pinned,
          fullSize: normalizeFullWindowSize(applied.size),
          compactPosition: applied.previousPosition,
          fullPosition: applied.position,
        },
      });
    } catch (error) {
      if (shouldPreExpandDock && worktree) {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: false });
      }
      pinInFlight.current = false;
      setPinBusy(false);
      setWindowTransition(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  useLayoutEffect(() => {
    if (!hydrated || !windowController.isDesktopWindow || workbenchReadyNotifiedRef.current) return;
    // Defer by one short macrotask (rather than rAF, which WebKit can throttle in hidden windows)
    // so the hydrated DOM has flushed before signaling the startup splash to leap and reveal.
    const timer = window.setTimeout(() => {
      if (workbenchReadyNotifiedRef.current) return;
      workbenchReadyNotifiedRef.current = true;
      void windowController.notifyWorkbenchReady?.();
    }, 16);
    return () => window.clearTimeout(timer);
  }, [hydrated, windowController]);

  useLayoutEffect(() => {
    if (windowTransition) return;
    const pendingFocus = pendingModeFocusRef.current;
    pendingModeFocusRef.current = null;
    if (!pendingFocus || typeof document === 'undefined') return;
    if (pendingFocus.kind === 'changes') {
      const panel = document.querySelector<HTMLElement>('.changes-panel');
      panel?.scrollIntoView({ block: 'nearest' });
      (panel?.querySelector<HTMLElement>('input[type="checkbox"], button') ?? panel)?.focus();
      return;
    }
    if (pendingFocus.kind === 'dock') {
      const dock = document.querySelector<HTMLElement>('.d2-sheet, .message-expanded, .message-collapsed');
      (dock?.querySelector<HTMLElement>('.d2-bottom button, .d2-bottom input, .message-actions button, .message-reply input, button') ?? dock)?.focus();
      return;
    }
    if (pendingFocus.kind === 'repo') {
      document.querySelector<HTMLElement>('.repository-open-button, .repository-button')?.focus();
    }
  }, [presentation.surface, windowTransition]);

  useEffect(() => {
    if (!hydrated || !windowController.isDesktopWindow) return;
    let mounted = true;
    if ((initialState || !backendHasLoadState)
      && !latestState.current.settingsModified
      && !latestState.current.windowPrefs.compactMode
      && !latestState.current.windowPrefs.fullPosition
      && !pinInFlight.current) {
      void windowController.readSnapshot().then((snap) => {
        if (!mounted || !snap || latestState.current.settingsModified || pinInFlight.current) return;
        dispatch({
          type: 'windowSnapshotSynced',
          pinned: snap.pinned,
          size: normalizeFullWindowSize(snap.size),
          position: snap.position,
        });
      }).catch(() => {});
    }
    const unsubscribeGeometry = windowController.subscribeGeometry?.((observed) => {
      if (!mounted || settingsSessionRef.current?.returnTo === 'compact') return;
      dispatch({ type: 'windowGeometryObserved', position: observed.position, size: observed.size });
    });
    const unsubscribeTap = windowController.subscribeNotificationTap?.(() => {
      // Focusing the window on notification click must never auto-expand compact mode.
    });
    return () => {
      mounted = false;
      unsubscribeGeometry?.();
      unsubscribeTap?.();
    };
  }, [backendHasLoadState, dispatch, hydrated, initialState, latestState, windowController]);

  return {
    settingsSession,
    settingsSessionRef,
    pinBusy,
    pinInFlight,
    transitionKind,
    transitionHeightPx,
    desiredCompactHeightRef,
    pinned,
    compactMode,
    presentation,
    settingsOpen,
    activeSurfaceRef,
    openSettingsFromWorkbench,
    openSettingsFromCompact,
    closeSettings,
    clearWorkbenchSettingsSession,
    openConfigureModel,
    openConfigureModelFromCompact,
    handleDesiredCompactHeight,
    togglePin,
    collapseToCompact,
    expandFromCompact,
  };
}
