import { useEffect, useRef, useState } from 'react';
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  RefObject,
} from 'react';
import {
  LOCAL_KEEP_DEFAULT_HEIGHT_PX,
  LOCAL_KEEP_PEEK_HEIGHT_PX,
  clampLocalKeepHeight,
  readSavedLocalKeepHeight,
  writeSavedLocalKeepHeight,
} from './changesPanelSelection';

interface UseLocalKeepResizeParams {
  panelRef: RefObject<HTMLElement | null>;
  localKeepExpanded: boolean;
  keptCount: number;
  layoutMode: 'idle' | 'peek' | 'hidden';
}

export function useLocalKeepResize({
  panelRef,
  localKeepExpanded,
  keptCount,
  layoutMode,
}: UseLocalKeepResizeParams) {
  const [preferredLocalKeepHeight, setPreferredLocalKeepHeight] = useState<number>(
    () => readSavedLocalKeepHeight() ?? LOCAL_KEEP_DEFAULT_HEIGHT_PX,
  );
  const [panelHeight, setPanelHeight] = useState(0);
  const [resizingLocalKeep, setResizingLocalKeep] = useState(false);
  const localKeepListRef = useRef<HTMLDivElement>(null);
  const cleanupResizeRef = useRef<(() => void) | null>(null);

  const canResizeLocalKeep = localKeepExpanded && keptCount > 0 && layoutMode === 'idle';
  const effectiveLocalKeepHeight = layoutMode === 'peek'
    ? LOCAL_KEEP_PEEK_HEIGHT_PX
    : clampLocalKeepHeight(preferredLocalKeepHeight, panelHeight);

  useEffect(() => {
    if (!canResizeLocalKeep) {
      cleanupResizeRef.current?.();
    }
  }, [canResizeLocalKeep]);

  useEffect(() => {
    if (!canResizeLocalKeep) return;
    const panelEl = panelRef.current;
    if (!panelEl) return;
    const syncHeight = () => {
      const measured = Math.round(panelEl.getBoundingClientRect().height);
      if (measured > 0) {
        setPanelHeight((prev) => (prev === measured ? prev : measured));
      }
    };
    syncHeight();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => syncHeight());
    observer.observe(panelEl);
    return () => observer.disconnect();
  }, [panelRef, canResizeLocalKeep]);

  useEffect(() => () => {
    cleanupResizeRef.current?.();
  }, []);

  function handleLocalKeepResizeStart(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || !canResizeLocalKeep) return;
    event.preventDefault();
    event.stopPropagation();
    cleanupResizeRef.current?.();
    const measuredPanelHeight = panelRef.current?.getBoundingClientRect().height ?? panelHeight;
    if (measuredPanelHeight > 0) setPanelHeight(measuredPanelHeight);
    const listHeight = localKeepListRef.current?.getBoundingClientRect().height ?? 0;
    const startHeight = listHeight > 0 ? Math.round(listHeight) : effectiveLocalKeepHeight;
    const pointerId = event.pointerId;
    const startY = event.clientY;
    let latestHeight = startHeight;

    const previousCursor = document.body.style.cursor;
    const previousSelect = document.body.style.userSelect;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
    setResizingLocalKeep(true);

    const teardown = () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousSelect;
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', finishResize);
      window.removeEventListener('pointercancel', finishResize);
      window.removeEventListener('blur', handleBlur);
      cleanupResizeRef.current = null;
      setResizingLocalKeep(false);
    };

    const handleMove = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      const currentPanelHeight = panelRef.current?.getBoundingClientRect().height ?? measuredPanelHeight;
      const nextHeight = clampLocalKeepHeight(startHeight + (startY - moveEvent.clientY), currentPanelHeight);
      latestHeight = nextHeight;
      setPreferredLocalKeepHeight(nextHeight);
    };

    const finishResize = (endEvent: PointerEvent) => {
      if (endEvent.pointerId !== pointerId) return;
      teardown();
      writeSavedLocalKeepHeight(latestHeight);
    };

    const handleBlur = () => {
      teardown();
      writeSavedLocalKeepHeight(latestHeight);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', finishResize);
    window.addEventListener('pointercancel', finishResize);
    window.addEventListener('blur', handleBlur);
    cleanupResizeRef.current = teardown;
  }

  function handleLocalKeepResizeReset() {
    setPreferredLocalKeepHeight(LOCAL_KEEP_DEFAULT_HEIGHT_PX);
    writeSavedLocalKeepHeight(null);
  }

  function handleLocalKeepResizeKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (!canResizeLocalKeep) return;
    const currentPanelHeight = panelRef.current?.getBoundingClientRect().height ?? panelHeight;
    if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      event.preventDefault();
      const step = event.key === 'ArrowUp' ? 24 : -24;
      const next = clampLocalKeepHeight(effectiveLocalKeepHeight + step, currentPanelHeight);
      setPreferredLocalKeepHeight(next);
      writeSavedLocalKeepHeight(next);
    } else if (event.key === 'Home' || event.key === '0') {
      event.preventDefault();
      handleLocalKeepResizeReset();
    }
  }

  return {
    localKeepListRef,
    canResizeLocalKeep,
    effectiveLocalKeepHeight,
    resizingLocalKeep,
    handleLocalKeepResizeStart,
    handleLocalKeepResizeReset,
    handleLocalKeepResizeKeyDown,
  };
}
