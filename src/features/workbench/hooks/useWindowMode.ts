import {
  useCallback,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from 'react';
import { WINDOW_DIMENSIONS } from '../../../../shared/workbench-persistence-protocol';
import {
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
import {
  commitBeforeWindowResize,
  measureWorkbenchHeight,
  useWindowSettingsTransition,
  type WindowTransitionKind,
  type WindowTransitionState,
} from './useWindowSettingsTransition';
import { useWindowSyncEffects } from './useWindowSyncEffects';

export type { WindowTransitionKind } from './useWindowSettingsTransition';

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

  const {
    openSettingsFromWorkbench,
    clearWorkbenchSettingsSession,
    openSettingsFromCompact,
    closeSettings,
  } = useWindowSettingsTransition({
    locale: state.locale,
    latestState,
    windowController,
    dispatch,
    notify,
    onClearOverlays,
    settingsSessionRef,
    setSettingsSession,
    setWindowTransition,
    setPinBusy,
    pinInFlight,
    activeSurfaceRef,
    desiredCompactHeightRef,
    collapsedCompactHeightRef,
  });

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

  useWindowSyncEffects({
    hydrated,
    initialState,
    backendHasLoadState,
    latestState,
    windowController,
    dispatch,
    pinInFlight,
    settingsSessionRef,
    workbenchReadyNotifiedRef,
    pendingModeFocusRef,
    presentationSurface: presentation.surface,
    windowTransition,
  });

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
