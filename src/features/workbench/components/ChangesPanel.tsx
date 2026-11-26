import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { Collapse, MOTION_DURATION, MOTION_EASE, MotionPresence, m, useInstantMotion } from '../../../ui/motion';
import {
  groupFiles,
  localKeepChanges,
  normalChanges,
  scopeSummary,
  selectedLocalKeepChanges,
  splitPath,
} from '../state/workbench';
import type { Change, Worktree } from '../state/workbench';

type DropZone = 'normal-changes' | 'local-keep';

interface DragState {
  paths: string[];
  name: string;
  fromLocalKeep: boolean;
  overZone: DropZone | null;
  clientX: number;
  clientY: number;
}

interface PointerCandidate {
  pointerId: number;
  startX: number;
  startY: number;
  paths: string[];
  primaryPath: string;
  name: string;
  fromLocalKeep: boolean;
  selectSingleOnDragStart: boolean;
}

interface ContextMenuState {
  x: number;
  y: number;
  paths: string[];
  primaryPath: string;
  isLocalKeep: boolean;
}

interface Props {
  worktree?: Worktree;
  editorShort: string;
  opening: boolean;
  repositoryReadsAvailable: boolean;
  onOpenRepository(): void;
  onSelect(fileId: string): void;
  onSelectAll(): void;
  onSelectFiles(fileIds: string[]): void;
  onSetLocalKeep(paths: string[], keep: boolean): void;
  onOpenFile(fileId: string): void;
  onReview(): void;
  onRefresh(): void;
  refreshing: boolean;
}

const DRAG_THRESHOLD_PX = 5;
export const DRAG_AUTOSCROLL_EDGE_PX = 32;
const DRAG_AUTOSCROLL_OUTSIDE_LIMIT_PX = 72;
const DRAG_AUTOSCROLL_MIN_PX_PER_FRAME = 2.5;
const DRAG_AUTOSCROLL_MAX_PX_PER_FRAME = 14;

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

function targetDropZone(fromLocalKeep: boolean): DropZone {
  return fromLocalKeep ? 'normal-changes' : 'local-keep';
}

function readDropZoneAtPoint(clientX: number, clientY: number): DropZone | null {
  if (typeof document === 'undefined' || typeof document.elementFromPoint !== 'function') return null;
  const hit = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-drop-zone]')?.dataset.dropZone;
  return hit === 'local-keep' || hit === 'normal-changes' ? hit : null;
}

function TriStateCheckbox(props: {
  checked: boolean;
  indeterminate: boolean;
  disabled?: boolean;
  label: string;
  onChange(): void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (ref.current) {
      ref.current.indeterminate = props.indeterminate;
    }
  }, [props.indeterminate, props.checked]);
  return (
    <input
      ref={ref}
      type="checkbox"
      className="file-checkbox"
      checked={props.checked}
      disabled={props.disabled}
      aria-checked={props.indeterminate ? 'mixed' : props.checked}
      aria-label={props.label}
      onChange={props.onChange}
    />
  );
}

export function ChangesPanel(props: Props) {
  const { locale, t } = useI18n();
  const instantMotion = useInstantMotion();
  const [view, setView] = useState<'list' | 'tree'>('list');
  const [localKeepExpanded, setLocalKeepExpanded] = useState(false);
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
  const latestSetLocalKeep = useRef(props.onSetLocalKeep);
  latestSetLocalKeep.current = props.onSetLocalKeep;

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
  }, [props.worktree?.worktreeId]);

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

  const worktree = props.worktree;
  const hasChanges = !!worktree && !worktree.readError && worktree.files.length > 0;
  const normal = worktree ? normalChanges(worktree) : [];
  const kept = worktree ? localKeepChanges(worktree) : [];
  const selectedKept = worktree ? selectedLocalKeepChanges(worktree) : [];
  const normalSelectedCount = normal.filter((file) => worktree!.selectedIds.includes(file.id)).length;
  const allSelected = normal.length > 0 && normalSelectedCount === normal.length;
  const partiallySelected = normal.length > 0
    ? normalSelectedCount > 0 && normalSelectedCount < normal.length
    : selectedKept.length > 0;
  const selectAllDisabled = normal.length === 0 && selectedKept.length === 0;
  const shouldDeselectOnToggle = allSelected || (normal.length === 0 && selectedKept.length > 0);
  const keptConflictCount = kept.filter((file) => file.status === 'U').length;
  const activeTargetZone = dragState ? targetDropZone(dragState.fromLocalKeep) : null;
  const groupedNormal = worktree && view === 'tree' ? groupFiles(normal, t.changes.rootDirectory) : [];
  const orderedNormalPaths = view === 'list'
    ? normal.map((file) => file.path)
    : groupedNormal.flatMap((group) => group.files.map((file) => file.path));
  const orderedVisiblePaths = localKeepExpanded
    ? [...orderedNormalPaths, ...kept.map((file) => file.path)]
    : orderedNormalPaths;
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
    props.onOpenFile(file.id);
  }

  function fileRow(file: Change, grouped = false) {
    const { name, directory } = splitPath(file.path);
    const findings = worktree!.review.findings.filter((finding) => finding.fileId === file.id);
    const hasHigh = findings.some((finding) => finding.severity === 'high');
    const isRowSelected = activeSelectedRowSet.has(file.path);
    const isDraggingThis = dragState?.paths.includes(file.path) ?? false;
    const action = file.status === 'U'
      ? t.changes.ideActions.merge
      : file.range.kind === 'partial'
        ? t.changes.ideActions.stagedDiff
        : file.status === 'A'
          ? t.changes.ideActions.open
          : t.changes.ideActions.diff;
    return (
      <div
        className={`file-row ${grouped ? 'grouped' : ''} ${file.status === 'U' ? 'conflicted' : ''} ${file.ignoredForReview && !worktree!.selectedIds.includes(file.id) ? 'ignored' : ''} ${file.localKeep ? 'local-keep-row' : ''} ${isRowSelected ? 'is-selected' : ''} ${isDraggingThis ? 'dragging' : ''}`}
        key={file.id}
        data-file-path={file.path}
        data-selected={isRowSelected ? 'true' : undefined}
        onPointerDown={(event) => handleFilePointerDown(event, file, name)}
        onContextMenu={(event) => handleFileContextMenu(event, file)}
        onClickCapture={(event) => {
          consumeSuppressedClick(event);
        }}
      >
        <input
          type="checkbox"
          className="file-checkbox"
          checked={worktree!.selectedIds.includes(file.id)}
          aria-label={t.changes.selectFile(file.path)}
          onChange={() => props.onSelect(file.id)}
        />
        <button
          type="button"
          className="file-open"
          title={`${file.previousPath ? `${file.previousPath} → ` : ''}${file.path}`}
          onClick={(event) => handleFileRowClick(event, file)}
          onDoubleClick={(event) => handleFileRowDoubleClick(event, file)}
        >
          <span className={`git-status git-${file.status}`} aria-label={t.changes.gitStatus(file.status)}>
            {file.status === 'U' ? '!' : file.status}
          </span>
          <span className="file-label">
            <span className="mono file-name truncate">{name}</span>
            {!grouped && directory && <span className="mono file-directory truncate">{directory}</span>}
            {file.status === 'U'
              ? <span className="file-badge danger">{t.changes.conflictBadge}</span>
              : file.range.kind === 'partial' && <span className="file-badge warning">{t.changes.partialStagedBadge}</span>}
            {file.ignoredForReview && <span className="file-badge muted">{t.changes.skipReviewBadge}</span>}
          </span>
          <span className="file-stats mono">
            {file.binary
              ? <span className="muted">{t.common.binary}</span>
              : <><span className="success">+{file.range.added}</span> <span className="danger">-{file.range.removed}</span></>}
          </span>
        </button>
        <button
          type="button"
          className="file-hover file-diff-link"
          aria-label={`${props.editorShort} ${action}: ${file.path}`}
          title={`${props.editorShort} ${action}`}
          onClick={(event) => {
            event.stopPropagation();
            if (consumeSuppressedClick(event)) return;
            props.onOpenFile(file.id);
          }}
          onDoubleClick={(event) => {
            event.stopPropagation();
          }}
        >
          {props.editorShort} {action}<Icon name="external" />
        </button>
        {!!findings.length && (
          <button
            type="button"
            className={`finding-count ${hasHigh ? 'danger' : 'warning'}`}
            aria-label={t.changes.findingCountLabel(file.path, findings.length)}
            onClick={props.onReview}
          >
            <span className="status-dot" />{findings.length}
          </button>
        )}
      </div>
    );
  }

  return (
    <section
      ref={panelRef}
      className="changes-panel"
      aria-label={t.changes.panelLabel}
      onDragStart={(event) => event.preventDefault()}
    >
      {worktree && (
        <div
          className="changes-heading"
          data-drop-zone="normal-changes"
        >
          <div className="changes-summary min-width-zero">
            {hasChanges && (
              <TriStateCheckbox
                checked={allSelected}
                indeterminate={partiallySelected}
                disabled={selectAllDisabled}
                label={shouldDeselectOnToggle ? t.changes.deselectAll : t.changes.selectAll}
                onChange={props.onSelectAll}
              />
            )}
            <span className="muted truncate">{scopeSummary(worktree, locale)}</span>
          </div>
          <div className="row">
            {hasChanges && (
              <IconButton
                icon={view === 'list' ? 'tree' : 'list'}
                label={view === 'list' ? t.changes.switchToTreeView : t.changes.switchToListView}
                onClick={() => setView(view === 'list' ? 'tree' : 'list')}
              />
            )}
            <IconButton
              icon="refresh"
              label={t.changes.refreshChanges}
              disabled={props.refreshing}
              className={props.refreshing ? 'spinning' : ''}
              onClick={props.onRefresh}
            />
          </div>
        </div>
      )}
      {hasChanges && selectedKept.length > 0 && (
        <div className="change-notice local-keep-notice" role="status">
          <span className="truncate">{t.changes.localKeepSelectedNotice(selectedKept.length)}</span>
          <button
            type="button"
            className="accent"
            onClick={() => props.onSelectFiles(selectedKept.map((file) => file.id))}
          >
            {t.changes.uncheckLocalKeepAction}
          </button>
        </div>
      )}
      <div
        ref={listRef}
        className="changes-list scroll-area"
        data-drop-zone="normal-changes"
        onScroll={() => { if (contextMenu) setContextMenu(null); }}
      >
        {!worktree ? (
          <div className="empty-state repository-empty">
            <span className="empty-badge" aria-hidden="true"><Icon name="folder" /></span>
            <div className="empty-copy">
              <p className="empty-title">{t.changes.emptyRepoTitle}</p>
              {!props.repositoryReadsAvailable && <p className="muted help">{t.changes.browserPreviewHint}</p>}
            </div>
            <button type="button" className="primary repository-open-button" disabled={props.opening} onClick={props.onOpenRepository}>
              {props.opening ? t.changes.openingRepo : t.changes.openLocalRepository}
            </button>
          </div>
        ) : worktree.readError ? (
          <div className="empty-state">
            <span className="empty-badge warning" aria-hidden="true"><Icon name="alert" /></span>
            <div className="empty-copy">
              <p className="empty-title">{t.changes.worktreeUnavailableTitle}</p>
              <p className="warning help">{worktree.readError}</p>
            </div>
            <button type="button" className="secondary compact" disabled={props.refreshing} onClick={props.onRefresh}>
              {t.changes.retryRead}
            </button>
          </div>
        ) : !worktree.files.length ? (
          <div className="empty-state">
            <span className="empty-badge success" aria-hidden="true"><Icon name="check" /></span>
            <div className="empty-copy"><p className="empty-title">{t.changes.workingTreeClean}</p></div>
          </div>
        ) : (
          <div
            className={`normal-changes-zone ${activeTargetZone === 'normal-changes' ? 'drop-zone-ready' : ''} ${dragState?.overZone === 'normal-changes' ? 'drop-zone-over' : ''}`}
            data-drop-zone="normal-changes"
          >
            {dragState?.overZone === 'normal-changes' && (
              <div className="drop-zone-banner accent">{t.changes.dropToChangesHint}</div>
            )}
            {normal.length === 0 ? (
              <p className="normal-changes-empty muted help">{t.changes.noNormalChanges}</p>
            ) : view === 'list' ? (
              normal.map((file) => fileRow(file))
            ) : (
              groupedNormal.map((group) => {
                const groupSelectedCount = group.files.filter((file) => worktree.selectedIds.includes(file.id)).length;
                const groupAllSelected = group.files.length > 0 && groupSelectedCount === group.files.length;
                const groupIndeterminate = groupSelectedCount > 0 && groupSelectedCount < group.files.length;
                return (
                  <div key={group.directory}>
                    <div className="directory-heading mono">
                      <TriStateCheckbox
                        checked={groupAllSelected}
                        indeterminate={groupIndeterminate}
                        label={groupAllSelected ? t.changes.deselectDirectory(group.directory) : t.changes.selectDirectory(group.directory)}
                        onChange={() => props.onSelectFiles(group.files.map((file) => file.id))}
                      />
                      <span className="directory-label min-width-zero">
                        <Icon name="folder" />
                        <span className="truncate">{group.directory}</span>
                      </span>
                    </div>
                    {group.files.map((file) => fileRow(file, true))}
                  </div>
                );
              })
            )}
          </div>
        )}
      </div>
      {hasChanges && (
        <div
          ref={localKeepRef}
          className={`local-keep-section ${activeTargetZone === 'local-keep' ? 'drop-zone-ready' : ''} ${dragState?.overZone === 'local-keep' ? 'drop-zone-over' : ''}`}
          data-drop-zone="local-keep"
        >
          <button
            type="button"
            className="local-keep-header"
            aria-expanded={localKeepExpanded}
            onClick={(event) => {
              if (consumeSuppressedClick(event)) return;
              setLocalKeepExpanded((prev) => !prev);
            }}
          >
            <span className="local-keep-heading min-width-zero">
              <RotatingChevron open={localKeepExpanded} />
              <span className="local-keep-title truncate">{t.changes.localKeepTitle}</span>
              {keptConflictCount > 0 && (
                <span className="local-keep-conflict-badge danger">{t.changes.localKeepConflicts(keptConflictCount)}</span>
              )}
              {dragState?.overZone === 'local-keep' && (
                <span className="drop-zone-pill accent">{t.changes.dropToLocalKeepHint}</span>
              )}
            </span>
            <span className="dynamic-count local-keep-count">{kept.length}</span>
          </button>
          <Collapse open={localKeepExpanded}>
            <div
              className="local-keep-list scroll-area"
              onScroll={() => { if (contextMenu) setContextMenu(null); }}
            >
              {kept.length === 0
                ? <p className="local-keep-empty muted help">{t.changes.localKeepEmptyHint}</p>
                : kept.map((file) => fileRow(file, false))}
            </div>
          </Collapse>
        </div>
      )}
      <MotionPresence>
        {contextMenu && (
          <m.div
            key={`ctx:${contextMenu.primaryPath}:${contextMenu.x}:${contextMenu.y}`}
            ref={menuRef}
            role="menu"
            aria-label={
              contextMenu.paths.length > 1
                ? t.changes.batchFileActionsMenuLabel(contextMenu.paths.length)
                : t.changes.fileActionsMenuLabel(contextMenu.primaryPath)
            }
            className="popover file-context-menu"
            style={{
              left: `${menuPosition?.left ?? contextMenu.x}px`,
              top: `${menuPosition?.top ?? contextMenu.y}px`,
            }}
            initial={instantMotion ? false : { opacity: 0, scale: 0.96 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={instantMotion ? undefined : { opacity: 0, scale: 0.96 }}
            transition={instantMotion ? { duration: 0 } : { duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
          >
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                const targetPaths = contextMenu.paths;
                const nextKeep = !contextMenu.isLocalKeep;
                setContextMenu(null);
                if (nextKeep && !localKeepExpanded) {
                  const movedSet = new Set(targetPaths);
                  setSelectedRowPaths((prev) => prev.filter((path) => !movedSet.has(path)));
                }
                props.onSetLocalKeep(targetPaths, nextKeep);
              }}
            >
              <span>{contextMenu.isLocalKeep ? t.changes.moveBackToChangesAction : t.changes.keepLocallyAction}</span>
              {contextMenu.paths.length > 1 && (
                <span className="dynamic-count">{contextMenu.paths.length}</span>
              )}
            </button>
          </m.div>
        )}
      </MotionPresence>
      {dragState && (
        <div
          className="file-drag-ghost mono"
          aria-hidden="true"
          style={{
            left: `${dragState.clientX + 12}px`,
            top: `${dragState.clientY + 12}px`,
          }}
        >
          <span className="truncate">{dragState.name}</span>
          {dragState.paths.length > 1 && (
            <span className="dynamic-count">+{dragState.paths.length - 1}</span>
          )}
        </div>
      )}
    </section>
  );
}
