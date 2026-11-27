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
  const [pinBusy, setPinBusy] = useState(false);
  const pinInFlight = useRef(false);
  const desiredCompactHeightRef = useRef<number>(WINDOW_DIMENSIONS.compactDefaultHeight);
  const collapsedCompactHeightRef = useRef<number>(WINDOW_DIMENSIONS.compactDefaultHeight);
  const pendingModeFocusRef = useRef<PendingModeFocus | null>(null);
  const workbenchReadyNotifiedRef = useRef(false);

  const t = getMessages(state.locale);
  const pinned = state.windowPrefs.pinned;
  const compactMode = state.windowPrefs.compactMode;
  const presentation = resolveWorkbenchPresentation(compactMode, settingsSession?.returnTo ?? null);
  const settingsOpen = presentation.showSettings;
  const activeSurfaceRef = useRef<WorkbenchSurface>(presentation.surface);
  activeSurfaceRef.current = presentation.surface;

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
    setPinBusy(true);
    // Switch surface directly to standalone Settings BEFORE enlarging the window so neither
    // a stretched CompactBar nor the full workbench content can ever flash during expansion.
    settingsSessionRef.current = provisionalSession;
    setSettingsSession(provisionalSession);

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

    pinInFlight.current = true;
    setPinBusy(true);
    try {
      // Shrink and move back to pre-settings compact geometry FIRST while standalone Settings
      // remains mounted, then swap directly to CompactBar without ever mounting full workbench.
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
      settingsSessionRef.current = null;
      setSettingsSession(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    } finally {
      pinInFlight.current = false;
      setPinBusy(false);
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

    pinInFlight.current = true;
    setPinBusy(true);
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
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  async function expandFromCompact(target?: CompactExpandTarget | 'addModel') {
    if (pinInFlight.current || activeSurfaceRef.current !== 'compact') return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPrefs = computeNextCompactPreferences(currentPrefs, false);
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);

    pinInFlight.current = true;
    setPinBusy(true);
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
      if (target === 'dock' && worktree) {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true });
      } else if (target === 'addModel') {
        openConfigureModel();
      }
      pinInFlight.current = false;
      setPinBusy(false);
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
      pinInFlight.current = false;
      setPinBusy(false);
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
  }, [presentation.surface]);

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
