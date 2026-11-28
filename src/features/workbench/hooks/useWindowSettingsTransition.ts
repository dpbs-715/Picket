import type { Dispatch, MutableRefObject, SetStateAction } from 'react';
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
  defaultWindowPreferences,
  type WorkbenchState,
  type WorkbenchSurface,
} from '../state/workbench';
import type { NotifyFn, SettingsSession, WorkbenchDispatch } from '../types';

export type WindowTransitionKind = 'idle' | 'expanding' | 'collapsing';

export interface WindowTransitionState {
  kind: 'expanding' | 'collapsing';
  heightPx: number;
  mountWorkbenchDuringExpand?: boolean;
}

export function measureWorkbenchHeight(fallback: number): number {
  if (typeof document === 'undefined') return fallback;
  const measured = Math.round(document.querySelector<HTMLElement>('.workbench')?.getBoundingClientRect().height ?? 0);
  return measured > WINDOW_DIMENSIONS.compactDefaultHeight ? measured : fallback;
}

export function commitBeforeWindowResize(update: () => void): void {
  try {
    flushSync(update);
  } catch {
    update();
  }
}

interface UseWindowSettingsTransitionParams {
  locale: WorkbenchState['locale'];
  latestState: MutableRefObject<WorkbenchState>;
  windowController: WindowController;
  dispatch: WorkbenchDispatch;
  notify: NotifyFn;
  onClearOverlays: () => void;
  settingsSessionRef: MutableRefObject<SettingsSession | null>;
  setSettingsSession: Dispatch<SetStateAction<SettingsSession | null>>;
  setWindowTransition: Dispatch<SetStateAction<WindowTransitionState | null>>;
  setPinBusy: Dispatch<SetStateAction<boolean>>;
  pinInFlight: MutableRefObject<boolean>;
  activeSurfaceRef: MutableRefObject<WorkbenchSurface>;
  desiredCompactHeightRef: MutableRefObject<number>;
  collapsedCompactHeightRef: MutableRefObject<number>;
}

export function useWindowSettingsTransition({
  locale,
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
}: UseWindowSettingsTransitionParams) {
  const t = getMessages(locale);

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
          locale,
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
        locale,
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
        locale,
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

  return {
    openSettingsFromWorkbench,
    clearWorkbenchSettingsSession,
    openSettingsFromCompact,
    closeSettings,
  };
}
