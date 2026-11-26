import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import type { Change, Worktree } from '../state/workbench';
import {
  DRAG_AUTOSCROLL_EDGE_PX,
  DRAG_THRESHOLD_PX,
  computeChangesDragScrollDelta,
  readDropZoneAtPoint,
  resolveContextMenuSelection,
  resolveRowSelectionOnClick,
  targetDropZone,
  type ContextMenuState,
  type DragAutoScrollOrigin,
  type DragState,
  type DropZone,
  type PointerCandidate,
} from './changesPanelSelection';

interface UseChangesPanelInteractionsParams {
  worktree?: Worktree;
  localKeepExpanded: boolean;
  orderedVisiblePaths: string[];
  onSetLocalKeep(paths: string[], keep: boolean): void;
  onOpenFile(fileId: string): void;
}

export function useChangesPanelInteractions({
  worktree,
  localKeepExpanded,
  orderedVisiblePaths,
  onSetLocalKeep,
  onOpenFile,
}: UseChangesPanelInteractionsParams) {
  const [selectedRowPaths, setSelectedRowPaths] = useState<string[]>([]);
  const [selectionAnchorPath, setSelectionAnchorPath] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [menuPosition, setMenuPosition] = useState<{ left: number; top: number } | null>(null);
  const [dragState, setDragState] = useState<DragState | null>(null);

  const panelRef = useRef<HTMLElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const localKeepRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const pointerCandidateRef = useRef<PointerCandidate | null>(null);
  const dragPointRef = useRef<{ x: number; y: number } | null>(null);
  const dragScrollOriginRef = useRef<DragAutoScrollOrigin | null>(null);
  const dragStateRef = useRef<DragState | null>(null);
  dragStateRef.current = dragState;
  const localKeepExpandedRef = useRef(localKeepExpanded);
  localKeepExpandedRef.current = localKeepExpanded;
  const suppressClickRef = useRef(false);
  const suppressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestSetLocalKeep = useRef(onSetLocalKeep);
  latestSetLocalKeep.current = onSetLocalKeep;

  function armClickSuppression() {
    suppressClickRef.current = true;
    if (suppressTimerRef.current !== null) clearTimeout(suppressTimerRef.current);
    suppressTimerRef.current = setTimeout(() => {
      suppressClickRef.current = false;
      suppressTimerRef.current = null;
    }, 0);
  }

  function consumeSuppressedClick(event: { preventDefault(): void; stopPropagation(): void }): boolean {
    if (!suppressClickRef.current) return false;
    suppressClickRef.current = false;
    if (suppressTimerRef.current !== null) {
      clearTimeout(suppressTimerRef.current);
      suppressTimerRef.current = null;
    }
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  useEffect(() => () => {
    if (suppressTimerRef.current !== null) clearTimeout(suppressTimerRef.current);
  }, []);

  useEffect(() => {
    setSelectedRowPaths([]);
    setSelectionAnchorPath(null);
    setContextMenu(null);
    setDragState(null);
    dragStateRef.current = null;
    dragPointRef.current = null;
    dragScrollOriginRef.current = null;
    pointerCandidateRef.current = null;
  }, [worktree?.worktreeId]);

  useLayoutEffect(() => {
    if (!contextMenu) {
      setMenuPosition(null);
      return;
    }
    const width = menuRef.current?.offsetWidth ?? 210;
    const height = menuRef.current?.offsetHeight ?? 38;
    const maxLeft = Math.max(8, window.innerWidth - width - 8);
    const maxTop = Math.max(8, window.innerHeight - height - 8);
    setMenuPosition({
      left: Math.min(Math.max(8, contextMenu.x), maxLeft),
      top: Math.min(Math.max(8, contextMenu.y), maxTop),
    });
  }, [contextMenu]);

  useEffect(() => {
    if (!contextMenu) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !menuRef.current?.contains(event.target)) {
        setContextMenu(null);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        setContextMenu(null);
      }
    };
    const handleDismiss = () => setContextMenu(null);
    document.addEventListener('pointerdown', handlePointerDown);
    document.addEventListener('keydown', handleKeyDown, true);
    window.addEventListener('blur', handleDismiss);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown);
      document.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('blur', handleDismiss);
    };
  }, [contextMenu]);

  useEffect(() => {
    const resolveDropZone = (clientX: number, clientY: number): DropZone | null => {
      const keepEl = localKeepRef.current;
      if (keepEl) {
        const keepRect = keepEl.getBoundingClientRect();
        if (
          keepRect.width > 0
          && keepRect.height > 0
          && clientX >= keepRect.left - 8
          && clientX <= keepRect.right + 8
          && clientY >= keepRect.top
          && clientY <= keepRect.bottom + 8
        ) {
          return 'local-keep';
        }
      }
      const panelEl = panelRef.current;
      const listEl = listRef.current;
      if (panelEl && listEl) {
        const panelRect = panelEl.getBoundingClientRect();
        const keepTop = keepEl ? keepEl.getBoundingClientRect().top : panelRect.bottom;
        if (
          panelRect.width > 0
          && panelRect.height > 0
          && clientX >= panelRect.left - 8
          && clientX <= panelRect.right + 8
          && clientY >= panelRect.top - 4
          && clientY < keepTop
        ) {
          return 'normal-changes';
        }
      }
      return readDropZoneAtPoint(clientX, clientY);
    };

    const updateScrollOriginGates = (clientY: number) => {
      const origin = dragScrollOriginRef.current;
      const listEl = listRef.current;
      if (!origin || !listEl) return;
      const rect = listEl.getBoundingClientRect();
      const effectiveEdge = Math.min(DRAG_AUTOSCROLL_EDGE_PX, rect.height / 3);
      if (clientY >= rect.top + effectiveEdge) origin.leftTopEdge = true;
      if (clientY <= rect.bottom - effectiveEdge) origin.leftBottomEdge = true;
    };

    const finishPointer = (event: PointerEvent, commitDrop: boolean) => {
      const candidate = pointerCandidateRef.current;
      const currentDrag = dragStateRef.current;
      if (!candidate && !currentDrag) return;
      if (candidate && event.pointerId !== candidate.pointerId) return;
      pointerCandidateRef.current = null;
      if (currentDrag) {
        armClickSuppression();
        const expectedZone = targetDropZone(currentDrag.fromLocalKeep);
        const hitZone = commitDrop
          ? (resolveDropZone(event.clientX, event.clientY) ?? currentDrag.overZone)
          : null;
        dragPointRef.current = null;
        dragScrollOriginRef.current = null;
        dragStateRef.current = null;
        setDragState(null);
        if (commitDrop && hitZone === expectedZone) {
          const nextKeep = expectedZone === 'local-keep';
          if (nextKeep && !localKeepExpandedRef.current) {
            const movedSet = new Set(currentDrag.paths);
            setSelectedRowPaths((prev) => prev.filter((path) => !movedSet.has(path)));
          }
          latestSetLocalKeep.current(currentDrag.paths, nextKeep);
        }
      }
    };

    const handlePointerMove = (event: PointerEvent) => {
      const candidate = pointerCandidateRef.current;
      const currentDrag = dragStateRef.current;
      if (!candidate && !currentDrag) return;
      if (candidate && event.pointerId === candidate.pointerId && !currentDrag) {
        const distance = Math.hypot(event.clientX - candidate.startX, event.clientY - candidate.startY);
        if (distance < DRAG_THRESHOLD_PX) return;
        armClickSuppression();
        setContextMenu(null);
        if (candidate.selectSingleOnDragStart) {
          setSelectedRowPaths([candidate.primaryPath]);
          setSelectionAnchorPath(candidate.primaryPath);
        }
        const expectedZone = targetDropZone(candidate.fromLocalKeep);
        const hitZone = resolveDropZone(event.clientX, event.clientY);
        const started: DragState = {
          paths: candidate.paths,
          name: candidate.name,
          fromLocalKeep: candidate.fromLocalKeep,
          overZone: hitZone === expectedZone ? expectedZone : null,
          clientX: event.clientX,
          clientY: event.clientY,
        };
        dragScrollOriginRef.current = { startY: candidate.startY };
        updateScrollOriginGates(event.clientY);
        dragPointRef.current = { x: event.clientX, y: event.clientY };
        dragStateRef.current = started;
        setDragState(started);
        return;
      }
      if (currentDrag) {
        updateScrollOriginGates(event.clientY);
        dragPointRef.current = { x: event.clientX, y: event.clientY };
        const expectedZone = targetDropZone(currentDrag.fromLocalKeep);
        const hitZone = resolveDropZone(event.clientX, event.clientY);
        const nextOver = hitZone === expectedZone ? expectedZone : null;
        const updated: DragState = {
          ...currentDrag,
          overZone: nextOver,
          clientX: event.clientX,
          clientY: event.clientY,
        };
        dragStateRef.current = updated;
        setDragState(updated);
      }
    };
    const handlePointerUp = (event: PointerEvent) => finishPointer(event, true);
    const handlePointerCancel = (event: PointerEvent) => finishPointer(event, false);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && dragStateRef.current) {
        event.stopPropagation();
        pointerCandidateRef.current = null;
        dragPointRef.current = null;
        dragScrollOriginRef.current = null;
        armClickSuppression();
        dragStateRef.current = null;
        setDragState(null);
      }
    };
    const handleWindowBlur = () => {
      pointerCandidateRef.current = null;
      if (dragStateRef.current) {
        dragPointRef.current = null;
        dragScrollOriginRef.current = null;
        dragStateRef.current = null;
        setDragState(null);
      }
    };
    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    window.addEventListener('pointercancel', handlePointerCancel);
    window.addEventListener('blur', handleWindowBlur);
    window.addEventListener('keydown', handleKeyDown, true);
    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      window.removeEventListener('pointercancel', handlePointerCancel);
      window.removeEventListener('blur', handleWindowBlur);
      window.removeEventListener('keydown', handleKeyDown, true);
    };
  }, []);

  const isDragging = dragState !== null;
  useEffect(() => {
    if (!isDragging) return;
    let rafId = 0;
    let lastTime: number | null = null;

    const tick = (now: number) => {
      const dt = lastTime === null ? 16.67 : now - lastTime;
      lastTime = now;
      const point = dragPointRef.current;
      const listEl = listRef.current;
      const activeDrag = dragStateRef.current;
      if (point && listEl && activeDrag && activeDrag.overZone !== 'local-keep') {
        const delta = computeChangesDragScrollDelta(
          point.y,
          listEl.getBoundingClientRect(),
          {
            scrollTop: listEl.scrollTop,
            scrollHeight: listEl.scrollHeight,
            clientHeight: listEl.clientHeight,
          },
          dt / 16.67,
          dragScrollOriginRef.current ?? undefined,
        );
        if (delta !== 0) {
          listEl.scrollTop += delta;
        }
      }
      rafId = window.requestAnimationFrame(tick);
    };

    rafId = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(rafId);
    };
  }, [isDragging]);

  const visiblePathSet = new Set(orderedVisiblePaths);
  const activeSelectedRowPaths = selectedRowPaths.filter((path) => visiblePathSet.has(path));
  const activeSelectedRowSet = new Set(activeSelectedRowPaths);

  function resolveDragPathsForFile(file: Change): string[] {
    if (!worktree || !activeSelectedRowSet.has(file.path)) {
      return [file.path];
    }
    const fileMap = new Map(worktree.files.map((entry) => [entry.path, entry]));
    const matching = orderedVisiblePaths.filter((path) => {
      if (!activeSelectedRowSet.has(path)) return false;
      const entry = fileMap.get(path);
      return !!entry && Boolean(entry.localKeep) === Boolean(file.localKeep);
    });
    return matching.length > 0 ? matching : [file.path];
  }

  function handleFileContextMenu(event: ReactMouseEvent<HTMLDivElement>, file: Change) {
    event.preventDefault();
    event.stopPropagation();
    pointerCandidateRef.current = null;
    const resolved = resolveContextMenuSelection({
      currentSelected: activeSelectedRowPaths,
      anchorPath: selectionAnchorPath,
      clickedPath: file.path,
      orderedPaths: orderedVisiblePaths,
    });
    setSelectedRowPaths(resolved.selectedPaths);
    setSelectionAnchorPath(resolved.anchorPath);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      paths: resolved.targetPaths,
      primaryPath: file.path,
      isLocalKeep: !!file.localKeep,
    });
  }

  function handleFilePointerDown(event: ReactPointerEvent<HTMLDivElement>, file: Change, name: string) {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) {
      pointerCandidateRef.current = null;
      return;
    }
    if (event.target instanceof Element && event.target.closest('.file-checkbox, .finding-count')) {
      pointerCandidateRef.current = null;
      return;
    }
    const alreadySelected = activeSelectedRowSet.has(file.path);
    pointerCandidateRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      paths: resolveDragPathsForFile(file),
      primaryPath: file.path,
      name,
      fromLocalKeep: !!file.localKeep,
      selectSingleOnDragStart: !alreadySelected,
    };
  }

  function handleFileRowClick(event: ReactMouseEvent<HTMLButtonElement>, file: Change) {
    if (consumeSuppressedClick(event)) return;
    const next = resolveRowSelectionOnClick({
      currentSelected: activeSelectedRowPaths,
      anchorPath: selectionAnchorPath,
      clickedPath: file.path,
      orderedPaths: orderedVisiblePaths,
      metaOrCtrl: event.metaKey || event.ctrlKey,
      shift: event.shiftKey,
    });
    setSelectedRowPaths(next.selectedPaths);
    setSelectionAnchorPath(next.anchorPath);
  }

  function handleFileRowDoubleClick(event: ReactMouseEvent<HTMLButtonElement>, file: Change) {
    if (consumeSuppressedClick(event)) return;
    if (event.metaKey || event.ctrlKey || event.shiftKey) return;
    onOpenFile(file.id);
  }

  function handleContextMenuToggleLocalKeep() {
    if (!contextMenu) return;
    const targetPaths = contextMenu.paths;
    const nextKeep = !contextMenu.isLocalKeep;
    setContextMenu(null);
    if (nextKeep && !localKeepExpanded) {
      const movedSet = new Set(targetPaths);
      setSelectedRowPaths((prev) => prev.filter((path) => !movedSet.has(path)));
    }
    onSetLocalKeep(targetPaths, nextKeep);
  }

  return {
    panelRef,
    listRef,
    localKeepRef,
    menuRef,
    contextMenu,
    menuPosition,
    dragState,
    activeSelectedRowSet,
    dismissContextMenu: () => setContextMenu(null),
    consumeSuppressedClick,
    handleFileContextMenu,
    handleFilePointerDown,
    handleFileRowClick,
    handleFileRowDoubleClick,
    handleContextMenuToggleLocalKeep,
  };
}
