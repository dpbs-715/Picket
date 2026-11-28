import { useEffect, useLayoutEffect, type MutableRefObject } from 'react';
import { normalizeFullWindowSize, type WindowController } from '../../../desktop/window';
import type { WorkbenchState, WorkbenchSurface } from '../state/workbench';
import type { PendingModeFocus, SettingsSession, WorkbenchDispatch } from '../types';
import type { WindowTransitionState } from './useWindowSettingsTransition';

interface UseWindowSyncEffectsParams {
  hydrated: boolean;
  initialState: WorkbenchState | undefined;
  backendHasLoadState: boolean;
  latestState: MutableRefObject<WorkbenchState>;
  windowController: WindowController;
  dispatch: WorkbenchDispatch;
  pinInFlight: MutableRefObject<boolean>;
  settingsSessionRef: MutableRefObject<SettingsSession | null>;
  workbenchReadyNotifiedRef: MutableRefObject<boolean>;
  pendingModeFocusRef: MutableRefObject<PendingModeFocus | null>;
  presentationSurface: WorkbenchSurface;
  windowTransition: WindowTransitionState | null;
}

export function useWindowSyncEffects({
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
  presentationSurface,
  windowTransition,
}: UseWindowSyncEffectsParams) {
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
  }, [hydrated, windowController, workbenchReadyNotifiedRef]);

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
  }, [presentationSurface, windowTransition, pendingModeFocusRef]);

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
  }, [backendHasLoadState, dispatch, hydrated, initialState, latestState, pinInFlight, settingsSessionRef, windowController]);
}
