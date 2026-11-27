export type DockSizePreference = null | 'full' | number;

export const DOCK_SIZE_STORAGE_KEY = 'picket.dockHeight';
export const DOCK_MIN_HEIGHT_PX = 180;
export const DOCK_FULL_SNAP_MARGIN_PX = 44;

export interface ResolvedDockSplit {
  fullHeight: boolean;
  changesHeightPx: number | null;
  dockHeightPx: number | null;
}

export function resolveDockSplit(
  preference: DockSizePreference,
  availableHeightPx: number,
): ResolvedDockSplit {
  if (preference === null) {
    return { fullHeight: false, changesHeightPx: null, dockHeightPx: null };
  }
  if (preference === 'full') {
    return {
      fullHeight: true,
      changesHeightPx: 0,
      dockHeightPx: availableHeightPx > 0 ? Math.round(availableHeightPx) : null,
    };
  }
  if (!Number.isFinite(availableHeightPx) || availableHeightPx <= 0) {
    return {
      fullHeight: false,
      changesHeightPx: null,
      dockHeightPx: Math.round(Math.max(DOCK_MIN_HEIGHT_PX, preference)),
    };
  }

  const minDock = Math.min(DOCK_MIN_HEIGHT_PX, Math.floor(availableHeightPx));
  const maxPeekDock = Math.max(minDock, Math.floor(availableHeightPx - DOCK_FULL_SNAP_MARGIN_PX));
  if (preference >= availableHeightPx - Math.floor(DOCK_FULL_SNAP_MARGIN_PX / 2)) {
    return { fullHeight: true, changesHeightPx: 0, dockHeightPx: Math.round(availableHeightPx) };
  }

  const clampedDock = Math.round(Math.max(minDock, Math.min(preference, maxPeekDock)));
  return {
    fullHeight: false,
    changesHeightPx: Math.max(0, Math.round(availableHeightPx - clampedDock)),
    dockHeightPx: clampedDock,
  };
}

export function computeDraggedDockPreference(
  desiredDockHeightPx: number,
  availableHeightPx: number,
): DockSizePreference {
  if (!Number.isFinite(availableHeightPx) || availableHeightPx <= 0) {
    return Math.round(Math.max(DOCK_MIN_HEIGHT_PX, desiredDockHeightPx));
  }
  if (availableHeightPx - desiredDockHeightPx <= DOCK_FULL_SNAP_MARGIN_PX) {
    return 'full';
  }
  const minDock = Math.min(DOCK_MIN_HEIGHT_PX, Math.floor(availableHeightPx));
  const maxPeekDock = Math.max(minDock, Math.floor(availableHeightPx - DOCK_FULL_SNAP_MARGIN_PX));
  return Math.round(Math.max(minDock, Math.min(desiredDockHeightPx, maxPeekDock)));
}

export function readSavedDockSize(): DockSizePreference {
  if (typeof window === 'undefined' || !window.localStorage) return null;
  try {
    const raw = window.localStorage.getItem(DOCK_SIZE_STORAGE_KEY);
    if (!raw) return null;
    if (raw === 'full') return 'full';
    const parsed = Number(raw);
    return Number.isFinite(parsed) && parsed >= DOCK_MIN_HEIGHT_PX ? Math.round(parsed) : null;
  } catch {
    return null;
  }
}

export function writeSavedDockSize(preference: DockSizePreference): void {
  if (typeof window === 'undefined' || !window.localStorage) return;
  try {
    if (preference === null) {
      window.localStorage.removeItem(DOCK_SIZE_STORAGE_KEY);
    } else if (preference === 'full') {
      window.localStorage.setItem(DOCK_SIZE_STORAGE_KEY, 'full');
    } else {
      window.localStorage.setItem(DOCK_SIZE_STORAGE_KEY, String(Math.round(preference)));
    }
  } catch {
    // Ignore storage quota or private-mode restrictions.
  }
}
