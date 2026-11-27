import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { Collapse } from '../../../ui/motion';
import {
  localKeepChanges,
  normalChanges,
  scopeSummary,
  selectedLocalKeepChanges,
  type Change,
  type Worktree,
} from '../state/workbench';
import { ChangeFileRow, TriStateCheckbox } from './ChangeFileRow';
import { ChangesPanelOverlays } from './ChangesPanelOverlays';
import {
  LOCAL_KEEP_DEFAULT_HEIGHT_PX,
  LOCAL_KEEP_PEEK_HEIGHT_PX,
  clampLocalKeepHeight,
  readSavedLocalKeepHeight,
  targetDropZone,
  writeSavedLocalKeepHeight,
} from './changesPanelSelection';
import {
  buildDirectoryTree,
  collectVisibleTreePaths,
  formatDirectoryLabel,
  treeGuideOffsets,
  treeIndentPx,
  type DirectoryTreeNode,
} from './changesPanelTree';
import { useChangesPanelInteractions } from './useChangesPanelInteractions';

export {
  DRAG_AUTOSCROLL_EDGE_PX,
  LOCAL_KEEP_DEFAULT_HEIGHT_PX,
  LOCAL_KEEP_MIN_HEIGHT_PX,
  clampLocalKeepHeight,
  computeChangesDragScrollDelta,
  resolveContextMenuSelection,
  resolveRowSelectionOnClick,
  type DragAutoScrollOrigin,
} from './changesPanelSelection';
export {
  buildDirectoryTree,
  collectVisibleTreePaths,
  formatDirectoryLabel,
  splitFileNameExt,
  treeGuideOffsets,
  treeIndentPx,
  type DirectoryTreeNode,
} from './changesPanelTree';

interface Props {
  worktree?: Worktree;
  editorShort: string;
  opening: boolean;
  repositoryReadsAvailable: boolean;
  layoutMode?: 'idle' | 'peek' | 'hidden';
  peekHeightPx?: number | null;
  resizingDock?: boolean;
  onOpenRepository(): void;
  onSelect(fileId: string): void;
  onSelectAll(): void;
  onSelectFiles(fileIds: string[]): void;
  onSetLocalKeep(paths: string[], keep: boolean): void;
  onOpenFile(fileId: string): void;
  onRefresh(): void;
  refreshing: boolean;
}

export function ChangesPanel(props: Props) {
  const { locale, t } = useI18n();
  const [view, setView] = useState<'list' | 'tree'>('list');
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(() => new Set());
  const [localKeepExpanded, setLocalKeepExpanded] = useState(false);
  const [preferredLocalKeepHeight, setPreferredLocalKeepHeight] = useState<number>(
    () => readSavedLocalKeepHeight() ?? LOCAL_KEEP_DEFAULT_HEIGHT_PX,
  );
  const [panelHeight, setPanelHeight] = useState(0);
  const [resizingLocalKeep, setResizingLocalKeep] = useState(false);
  const localKeepListRef = useRef<HTMLDivElement>(null);
  const cleanupResizeRef = useRef<(() => void) | null>(null);

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
  const normalTreeKey = `${worktree?.worktreeId ?? ''}:normal`;
  const keepTreeKey = `${worktree?.worktreeId ?? ''}:keep`;
  const normalTree = worktree && view === 'tree' ? buildDirectoryTree(normal, t.changes.rootDirectory) : [];
  const keptTree = worktree && view === 'tree' ? buildDirectoryTree(kept, t.changes.rootDirectory) : [];
  const orderedNormalPaths = view === 'list'
    ? normal.map((file) => file.path)
    : collectVisibleTreePaths(normalTree, collapsedDirs, normalTreeKey);
  const orderedKeptPaths = view === 'list'
    ? kept.map((file) => file.path)
    : collectVisibleTreePaths(keptTree, collapsedDirs, keepTreeKey);
  const orderedVisiblePaths = localKeepExpanded
    ? [...orderedNormalPaths, ...orderedKeptPaths]
    : orderedNormalPaths;

  const {
    panelRef,
    listRef,
    localKeepRef,
    menuRef,
    contextMenu,
    menuPosition,
    dragState,
    activeSelectedRowSet,
    dismissContextMenu,
    consumeSuppressedClick,
    handleFileContextMenu,
    handleFilePointerDown,
    handleFileRowClick,
    handleFileRowDoubleClick,
    handleContextMenuToggleLocalKeep,
  } = useChangesPanelInteractions({
    worktree,
    localKeepExpanded,
    orderedVisiblePaths,
    onSetLocalKeep: props.onSetLocalKeep,
    onOpenFile: props.onOpenFile,
  });

  const activeTargetZone = dragState ? targetDropZone(dragState.fromLocalKeep) : null;

  function toggleDirectoryCollapsed(dirKey: string) {
    setCollapsedDirs((prev) => {
      const next = new Set(prev);
      if (next.has(dirKey)) next.delete(dirKey);
      else next.add(dirKey);
      return next;
    });
  }

  function renderFileRow(file: Change, grouped = false, treeDepth?: number) {
    return (
      <ChangeFileRow
        key={file.id}
        file={file}
        worktree={worktree!}
        grouped={grouped}
        treeDepth={treeDepth}
        editorShort={props.editorShort}
        isRowSelected={activeSelectedRowSet.has(file.path)}
        isDraggingThis={dragState?.paths.includes(file.path) ?? false}
        onSelect={props.onSelect}
        onOpenFile={props.onOpenFile}
        onPointerDown={handleFilePointerDown}
        onContextMenu={handleFileContextMenu}
        onRowClick={handleFileRowClick}
        onRowDoubleClick={handleFileRowDoubleClick}
        consumeSuppressedClick={consumeSuppressedClick}
      />
    );
  }

  function renderDirectoryNodes(nodes: readonly DirectoryTreeNode[], keyPrefix: string, depth = 0) {
    return nodes.map((node) => {
      const nodeKey = `${keyPrefix}:${node.id}`;
      const isCollapsed = collapsedDirs.has(nodeKey);
      const groupSelectedCount = node.allFiles.filter((file) => worktree!.selectedIds.includes(file.id)).length;
      const groupAllSelected = node.allFiles.length > 0 && groupSelectedCount === node.allFiles.length;
      const groupIndeterminate = groupSelectedCount > 0 && groupSelectedCount < node.allFiles.length;
      const isModuleHead = depth === 0 && node.children.length > 0;
      const conflictCount = node.allFiles.filter((file) => file.status === 'U').length;
      const formatted = formatDirectoryLabel(node.segments);
      const guides = treeGuideOffsets(depth);
      const showCount = isModuleHead || isCollapsed;

      return (
        <div key={nodeKey} className="directory-branch">
          <div
            className={`directory-heading mono ${isModuleHead ? 'is-module' : ''} ${isCollapsed ? 'is-collapsed' : ''}`}
            data-dir-path={node.fullPath}
            data-depth={depth}
            style={{ paddingLeft: `${treeIndentPx(depth)}px` }}
          >
            {guides.map((leftPx) => (
              <span key={leftPx} className="tree-guide" style={{ left: `${leftPx}px` }} aria-hidden="true" />
            ))}
            <TriStateCheckbox
              checked={groupAllSelected}
              indeterminate={groupIndeterminate}
              label={groupAllSelected ? t.changes.deselectDirectory(node.fullPath) : t.changes.selectDirectory(node.fullPath)}
              onChange={() => props.onSelectFiles(node.allFiles.map((file) => file.id))}
            />
            <button
              type="button"
              className="directory-toggle min-width-zero"
              aria-expanded={!isCollapsed}
              aria-label={isCollapsed ? t.changes.expandDirectory(node.fullPath) : t.changes.collapseDirectory(node.fullPath)}
              title={node.fullPath}
              onClick={() => toggleDirectoryCollapsed(nodeKey)}
            >
              <RotatingChevron open={!isCollapsed} className="directory-chevron" />
              <Icon name="folder" className="directory-folder-icon" />
              <span className="directory-path min-width-zero">
                {formatted.prefix && (
                  <>
                    <span className="directory-prefix">{formatted.prefix}</span>
                    <span className="directory-sep">/</span>
                  </>
                )}
                <span className="directory-leaf">{formatted.leaf}</span>
              </span>
              {isCollapsed && conflictCount > 0 && (
                <span className="directory-conflict-dot" title={t.changes.localKeepConflicts(conflictCount)} aria-hidden="true" />
              )}
              {showCount && <span className="directory-count">{node.allFiles.length}</span>}
            </button>
          </div>
          {!isCollapsed && (
            <>
              {node.children.length > 0 && renderDirectoryNodes(node.children, keyPrefix, depth + 1)}
              {node.files.map((file) => renderFileRow(file, true, depth + 1))}
            </>
          )}
        </div>
      );
    });
  }

  const layoutMode = props.layoutMode ?? 'idle';
  const canResizeLocalKeep = localKeepExpanded && kept.length > 0 && layoutMode === 'idle';
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

  return (
    <section
      ref={panelRef}
      className={`changes-panel is-${layoutMode} ${props.resizingDock ? 'is-dock-resizing' : ''}`}
      data-layout-mode={layoutMode}
      style={
        layoutMode === 'peek' && props.peekHeightPx !== null && props.peekHeightPx !== undefined
          ? { flexBasis: `${props.peekHeightPx}px`, height: `${props.peekHeightPx}px` }
          : undefined
      }
      inert={layoutMode === 'hidden'}
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
        onScroll={() => { if (contextMenu) dismissContextMenu(); }}
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
              normal.map((file) => renderFileRow(file))
            ) : (
              renderDirectoryNodes(normalTree, normalTreeKey)
            )}
          </div>
        )}
      </div>
      {hasChanges && (
        <div
          ref={localKeepRef}
          className={`local-keep-section ${activeTargetZone === 'local-keep' ? 'drop-zone-ready' : ''} ${dragState?.overZone === 'local-keep' ? 'drop-zone-over' : ''} ${resizingLocalKeep ? 'is-resizing' : ''}`}
          data-drop-zone="local-keep"
        >
          {canResizeLocalKeep && (
            <div
              role="separator"
              aria-orientation="horizontal"
              aria-label={t.changes.resizeLocalKeepLabel}
              aria-valuenow={effectiveLocalKeepHeight}
              title={t.changes.resizeLocalKeepTitle}
              tabIndex={0}
              className={`local-keep-resizer ${resizingLocalKeep ? 'is-resizing' : ''}`}
              onPointerDown={handleLocalKeepResizeStart}
              onDoubleClick={handleLocalKeepResizeReset}
              onKeyDown={handleLocalKeepResizeKeyDown}
            >
              <span className="local-keep-resizer-pill" aria-hidden="true" />
            </div>
          )}
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
              ref={localKeepListRef}
              className="local-keep-list scroll-area"
              style={{ maxHeight: `${effectiveLocalKeepHeight}px` }}
              onScroll={() => { if (contextMenu) dismissContextMenu(); }}
            >
              {kept.length === 0
                ? <p className="local-keep-empty muted help">{t.changes.localKeepEmptyHint}</p>
                : view === 'list'
                  ? kept.map((file) => renderFileRow(file, false))
                  : renderDirectoryNodes(keptTree, keepTreeKey)}
            </div>
          </Collapse>
        </div>
      )}
      <ChangesPanelOverlays
        contextMenu={contextMenu}
        menuPosition={menuPosition}
        menuRef={menuRef}
        dragState={dragState}
        onToggleLocalKeep={handleContextMenuToggleLocalKeep}
      />
    </section>
  );
}
