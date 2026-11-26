import { useState } from 'react';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { Collapse } from '../../../ui/motion';
import {
  groupFiles,
  localKeepChanges,
  normalChanges,
  scopeSummary,
  selectedLocalKeepChanges,
  type Change,
  type Worktree,
} from '../state/workbench';
import { ChangeFileRow, TriStateCheckbox } from './ChangeFileRow';
import { ChangesPanelOverlays } from './ChangesPanelOverlays';
import { targetDropZone } from './changesPanelSelection';
import { useChangesPanelInteractions } from './useChangesPanelInteractions';

export {
  DRAG_AUTOSCROLL_EDGE_PX,
  computeChangesDragScrollDelta,
  resolveContextMenuSelection,
  resolveRowSelectionOnClick,
  type DragAutoScrollOrigin,
} from './changesPanelSelection';

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

export function ChangesPanel(props: Props) {
  const { locale, t } = useI18n();
  const [view, setView] = useState<'list' | 'tree'>('list');
  const [localKeepExpanded, setLocalKeepExpanded] = useState(false);

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
  const groupedNormal = worktree && view === 'tree' ? groupFiles(normal, t.changes.rootDirectory) : [];
  const orderedNormalPaths = view === 'list'
    ? normal.map((file) => file.path)
    : groupedNormal.flatMap((group) => group.files.map((file) => file.path));
  const orderedVisiblePaths = localKeepExpanded
    ? [...orderedNormalPaths, ...kept.map((file) => file.path)]
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

  function renderFileRow(file: Change, grouped = false) {
    return (
      <ChangeFileRow
        key={file.id}
        file={file}
        worktree={worktree!}
        grouped={grouped}
        editorShort={props.editorShort}
        isRowSelected={activeSelectedRowSet.has(file.path)}
        isDraggingThis={dragState?.paths.includes(file.path) ?? false}
        onSelect={props.onSelect}
        onOpenFile={props.onOpenFile}
        onReview={props.onReview}
        onPointerDown={handleFilePointerDown}
        onContextMenu={handleFileContextMenu}
        onRowClick={handleFileRowClick}
        onRowDoubleClick={handleFileRowDoubleClick}
        consumeSuppressedClick={consumeSuppressedClick}
      />
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
                    {group.files.map((file) => renderFileRow(file, true))}
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
              onScroll={() => { if (contextMenu) dismissContextMenu(); }}
            >
              {kept.length === 0
                ? <p className="local-keep-empty muted help">{t.changes.localKeepEmptyHint}</p>
                : kept.map((file) => renderFileRow(file, false))}
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
