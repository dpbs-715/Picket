export type DropZone = 'normal-changes' | 'local-keep';

export interface DragState {
  paths: string[];
  name: string;
  fromLocalKeep: boolean;
  overZone: DropZone | null;
  clientX: number;
  clientY: number;
}

export interface PointerCandidate {
  pointerId: number;
  startX: number;
  startY: number;
  paths: string[];
  primaryPath: string;
  name: string;
  fromLocalKeep: boolean;
  selectSingleOnDragStart: boolean;
}

export interface ContextMenuState {
  x: number;
  y: number;
  paths: string[];
  primaryPath: string;
  isLocalKeep: boolean;
}

export const DRAG_THRESHOLD_PX = 5;
export const DRAG_AUTOSCROLL_EDGE_PX = 32;
export const LOCAL_KEEP_DEFAULT_HEIGHT_PX = 180;
export const LOCAL_KEEP_MIN_HEIGHT_PX = 68;
export const LOCAL_KEEP_PEEK_HEIGHT_PX = 56;
export const LOCAL_KEEP_HEIGHT_STORAGE_KEY = 'picket.localKeepHeight';
const CHANGES_CHROME_RESERVE_PX = 64;
const CHANGES_NORMAL_MIN_HEIGHT_PX = 96;
const DRAG_AUTOSCROLL_OUTSIDE_LIMIT_PX = 72;
const DRAG_AUTOSCROLL_MIN_PX_PER_FRAME = 2.5;
const DRAG_AUTOSCROLL_MAX_PX_PER_FRAME = 14;

export function clampLocalKeepHeight(desiredPx: number, panelHeightPx: number): number {
  const safeDesired = Number.isFinite(desiredPx) ? desiredPx : LOCAL_KEEP_DEFAULT_HEIGHT_PX;
  if (!Number.isFinite(panelHeightPx) || panelHeightPx <= 0) {
    return Math.round(Math.max(LOCAL_KEEP_MIN_HEIGHT_PX, Math.min(safeDesired, 360)));
  }
  const maxAvailable = Math.max(
    LOCAL_KEEP_MIN_HEIGHT_PX,
    Math.floor(panelHeightPx - CHANGES_CHROME_RESERVE_PX - CHANGES_NORMAL_MIN_HEIGHT_PX),
  );
  return Math.round(Math.max(LOCAL_KEEP_MIN_HEIGHT_PX, Math.min(safeDesired, maxAvailable)));
}

export function readSavedLocalKeepHeight(): number | null {
  if (typeof window === 'undefined' || !window.localStorage) return null;
  try {
    const raw = window.localStorage.getItem(LOCAL_KEEP_HEIGHT_STORAGE_KEY);
    if (!raw) return null;
    const parsed = Number(raw);
    return Number.isFinite(parsed) && parsed >= LOCAL_KEEP_MIN_HEIGHT_PX ? Math.round(parsed) : null;
  } catch {
    return null;
  }
}

export function writeSavedLocalKeepHeight(heightPx: number | null): void {
  if (typeof window === 'undefined' || !window.localStorage) return;
  try {
    if (heightPx === null) {
      window.localStorage.removeItem(LOCAL_KEEP_HEIGHT_STORAGE_KEY);
    } else {
      window.localStorage.setItem(LOCAL_KEEP_HEIGHT_STORAGE_KEY, String(Math.round(heightPx)));
    }
  } catch {
    // Ignore storage quota or private-mode restrictions.
  }
}

export interface DragAutoScrollOrigin {
  startY: number;
  leftTopEdge?: boolean;
  leftBottomEdge?: boolean;
}

export function computeChangesDragScrollDelta(
  clientY: number,
  rect: { top: number; bottom: number; height: number },
  scroll: { scrollTop: number; scrollHeight: number; clientHeight: number },
  frameScale = 1,
  origin?: DragAutoScrollOrigin,
): number {
  const maxScrollTop = Math.max(0, scroll.scrollHeight - scroll.clientHeight);
  if (maxScrollTop <= 0 || rect.height <= 0) return 0;

  const effectiveEdge = Math.min(DRAG_AUTOSCROLL_EDGE_PX, rect.height / 3);
  if (effectiveEdge <= 0) return 0;

  const topThreshold = rect.top + effectiveEdge;
  const bottomThreshold = rect.bottom - effectiveEdge;
  const scale = Math.min(2.5, Math.max(0.5, frameScale));

  if (clientY < topThreshold && clientY >= rect.top - DRAG_AUTOSCROLL_OUTSIDE_LIMIT_PX && scroll.scrollTop > 0) {
    if (origin && origin.startY < topThreshold && !origin.leftTopEdge && clientY > origin.startY - 4 && clientY > rect.top + 4) {
      return 0;
    }
    const ratio = Math.min(1, (topThreshold - clientY) / effectiveEdge);
    const speed = DRAG_AUTOSCROLL_MIN_PX_PER_FRAME
      + (DRAG_AUTOSCROLL_MAX_PX_PER_FRAME - DRAG_AUTOSCROLL_MIN_PX_PER_FRAME) * ratio * ratio;
    return -Math.min(scroll.scrollTop, Math.ceil(speed * scale));
  }

  if (clientY > bottomThreshold && clientY <= rect.bottom + DRAG_AUTOSCROLL_OUTSIDE_LIMIT_PX && scroll.scrollTop < maxScrollTop) {
    if (origin && origin.startY > bottomThreshold && !origin.leftBottomEdge && clientY < origin.startY + 4 && clientY < rect.bottom - 4) {
      return 0;
    }
    const ratio = Math.min(1, (clientY - bottomThreshold) / effectiveEdge);
    const speed = DRAG_AUTOSCROLL_MIN_PX_PER_FRAME
      + (DRAG_AUTOSCROLL_MAX_PX_PER_FRAME - DRAG_AUTOSCROLL_MIN_PX_PER_FRAME) * ratio * ratio;
    return Math.min(maxScrollTop - scroll.scrollTop, Math.ceil(speed * scale));
  }

  return 0;
}

export function resolveRowSelectionOnClick(params: {
  currentSelected: string[];
  anchorPath: string | null;
  clickedPath: string;
  orderedPaths: string[];
  metaOrCtrl: boolean;
  shift: boolean;
}): { selectedPaths: string[]; anchorPath: string } {
  const { currentSelected, anchorPath, clickedPath, orderedPaths, metaOrCtrl, shift } = params;
  const visibleSet = new Set(orderedPaths);
  const cleanSelected = currentSelected.filter((path) => visibleSet.has(path));

  if (shift) {
    const anchorIndex = anchorPath ? orderedPaths.indexOf(anchorPath) : -1;
    const clickedIndex = orderedPaths.indexOf(clickedPath);
    if (anchorIndex !== -1 && clickedIndex !== -1) {
      const start = Math.min(anchorIndex, clickedIndex);
      const end = Math.max(anchorIndex, clickedIndex);
      const rangePaths = orderedPaths.slice(start, end + 1);
      const selectedPaths = metaOrCtrl
        ? [...new Set([...cleanSelected, ...rangePaths])]
        : rangePaths;
      return { selectedPaths, anchorPath: anchorPath! };
    }
    return { selectedPaths: [clickedPath], anchorPath: clickedPath };
  }

  if (metaOrCtrl) {
    const exists = cleanSelected.includes(clickedPath);
    const selectedPaths = exists
      ? cleanSelected.filter((path) => path !== clickedPath)
      : [...cleanSelected, clickedPath];
    return { selectedPaths, anchorPath: clickedPath };
  }

  return { selectedPaths: [clickedPath], anchorPath: clickedPath };
}

export function resolveContextMenuSelection(params: {
  currentSelected: string[];
  anchorPath: string | null;
  clickedPath: string;
  orderedPaths: string[];
}): { selectedPaths: string[]; anchorPath: string | null; targetPaths: string[] } {
  const { currentSelected, anchorPath, clickedPath, orderedPaths } = params;
  const visibleSet = new Set(orderedPaths);
  const cleanSelected = currentSelected.filter((path) => visibleSet.has(path));

  if (cleanSelected.includes(clickedPath)) {
    const targetPaths = orderedPaths.filter((path) => cleanSelected.includes(path));
    return { selectedPaths: cleanSelected, anchorPath, targetPaths };
  }

  return {
    selectedPaths: [clickedPath],
    anchorPath: clickedPath,
    targetPaths: [clickedPath],
  };
}

export function targetDropZone(fromLocalKeep: boolean): DropZone {
  return fromLocalKeep ? 'normal-changes' : 'local-keep';
}

export function readDropZoneAtPoint(clientX: number, clientY: number): DropZone | null {
  if (typeof document === 'undefined' || typeof document.elementFromPoint !== 'function') return null;
  const hit = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-drop-zone]')?.dataset.dropZone;
  return hit === 'local-keep' || hit === 'normal-changes' ? hit : null;
}
