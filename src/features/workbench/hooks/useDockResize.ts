import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent, RefObject } from 'react';
import {
  computeDraggedDockPreference,
  readSavedDockSize,
  resolveDockSplit,
  writeSavedDockSize,
  type DockSizePreference,
} from '../components/dockResize';

interface UseDockResizeParams {
  hasExpandedDock: boolean;
  canMeasureDock: boolean;
  workbenchContentRef: RefObject<HTMLDivElement | null>;
  lowerWorkspaceRef: RefObject<HTMLElement | null>;
}

export function useDockResize({
  hasExpandedDock,
  canMeasureDock,
  workbenchContentRef,
  lowerWorkspaceRef,
}: UseDockResizeParams) {
  const [dockSizePref, setDockSizePref] = useState<DockSizePreference>(() => readSavedDockSize());
  const [dockAvailableHeight, setDockAvailableHeight] = useState(0);
  const [resizingDock, setResizingDock] = useState(false);
  const cleanupDockResizeRef = useRef<(() => void) | null>(null);

  const dockSplit = hasExpandedDock
    ? resolveDockSplit(dockSizePref, dockAvailableHeight)
    : { fullHeight: false, changesHeightPx: null, dockHeightPx: null };
  const changesLayoutMode: 'idle' | 'peek' | 'hidden' = !hasExpandedDock
    ? 'idle'
    : dockSplit.fullHeight
      ? 'hidden'
      : 'peek';

  function measureDockArea(): { available: number; dock: number } {
    const contentEl = workbenchContentRef.current;
    const changesEl = contentEl?.querySelector<HTMLElement>('.changes-panel');
    const lowerEl = lowerWorkspaceRef.current;
    const changesH = changesEl?.getBoundingClientRect().height ?? 0;
    const lowerH = lowerEl?.getBoundingClientRect().height ?? 0;
    return {
      available: Math.round(changesH + lowerH),
      dock: Math.round(lowerH),
    };
  }

  useEffect(() => () => {
    cleanupDockResizeRef.current?.();
  }, []);

  useEffect(() => {
    if (!canMeasureDock) {
      cleanupDockResizeRef.current?.();
    }
  }, [canMeasureDock]);

  useLayoutEffect(() => {
    if (!canMeasureDock) return;
    const syncAvailable = () => {
      const { available } = measureDockArea();
      if (available > 0) {
        setDockAvailableHeight((prev) => (prev === available ? prev : available));
      }
    };
    syncAvailable();
    const contentEl = workbenchContentRef.current;
    if (!contentEl || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => syncAvailable());
    observer.observe(contentEl);
    return () => observer.disconnect();
  }, [canMeasureDock]);

  function handleDockResizeStart(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || !hasExpandedDock) return;
    event.preventDefault();
    event.stopPropagation();
    cleanupDockResizeRef.current?.();
    const initial = measureDockArea();
    const measuredAvailable = initial.available || dockAvailableHeight;
    const startDockHeight = initial.dock > 0
      ? initial.dock
      : (dockSplit.dockHeightPx ?? Math.round(measuredAvailable * 0.75));
    const pointerId = event.pointerId;
    const startY = event.clientY;
    let latestPref: DockSizePreference = dockSizePref;
    let didDrag = false;

    const previousCursor = document.body.style.cursor;
    const previousSelect = document.body.style.userSelect;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
    setResizingDock(true);

    const teardown = () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousSelect;
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', finishResize);
      window.removeEventListener('pointercancel', finishResize);
      window.removeEventListener('blur', handleBlur);
      cleanupDockResizeRef.current = null;
      setResizingDock(false);
    };

    const handleMove = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      const deltaY = startY - moveEvent.clientY;
      if (!didDrag && Math.abs(deltaY) < 3) return;
      didDrag = true;
      const currentAvailable = measureDockArea().available || measuredAvailable;
      if (currentAvailable > 0) setDockAvailableHeight(currentAvailable);
      const nextPref = computeDraggedDockPreference(startDockHeight + deltaY, currentAvailable);
      latestPref = nextPref;
      setDockSizePref(nextPref);
    };

    const finishResize = (endEvent: PointerEvent) => {
      if (endEvent.pointerId !== pointerId) return;
      teardown();
      if (didDrag) writeSavedDockSize(latestPref);
    };

    const handleBlur = () => {
      teardown();
      if (didDrag) writeSavedDockSize(latestPref);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', finishResize);
    window.addEventListener('pointercancel', finishResize);
    window.addEventListener('blur', handleBlur);
    cleanupDockResizeRef.current = teardown;
  }

  function handleDockResizeDoubleClick() {
    if (!hasExpandedDock) return;
    const next: DockSizePreference = dockSizePref === null ? 'full' : null;
    setDockSizePref(next);
    writeSavedDockSize(next);
  }

  function handleDockResizeKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (!hasExpandedDock) return;
    const measured = measureDockArea();
    const available = measured.available || dockAvailableHeight;
    if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      event.preventDefault();
      const base = dockSizePref === 'full'
        ? available
        : measured.dock || dockSplit.dockHeightPx || Math.round(available * 0.75);
      const delta = event.key === 'ArrowUp' ? 32 : (dockSizePref === 'full' ? -56 : -32);
      const next = computeDraggedDockPreference(base + delta, available);
      if (available > 0) setDockAvailableHeight(available);
      setDockSizePref(next);
      writeSavedDockSize(next);
    } else if (event.key === 'Home' || event.key === '0') {
      event.preventDefault();
      setDockSizePref(null);
      writeSavedDockSize(null);
    }
  }

  return {
    dockSplit,
    changesLayoutMode,
    resizingDock,
    handleDockResizeStart,
    handleDockResizeDoubleClick,
    handleDockResizeKeyDown,
  };
}
