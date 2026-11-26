import { useLayoutEffect, useRef } from 'react';
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import { useI18n } from '../../../i18n';
import { Icon } from '../../../ui/controls';
import { splitPath, type Change, type Worktree } from '../state/workbench';

export function TriStateCheckbox(props: {
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

interface ChangeFileRowProps {
  file: Change;
  worktree: Worktree;
  grouped?: boolean;
  editorShort: string;
  isRowSelected: boolean;
  isDraggingThis: boolean;
  onSelect(fileId: string): void;
  onOpenFile(fileId: string): void;
  onReview(): void;
  onPointerDown(event: ReactPointerEvent<HTMLDivElement>, file: Change, name: string): void;
  onContextMenu(event: ReactMouseEvent<HTMLDivElement>, file: Change): void;
  onRowClick(event: ReactMouseEvent<HTMLButtonElement>, file: Change): void;
  onRowDoubleClick(event: ReactMouseEvent<HTMLButtonElement>, file: Change): void;
  consumeSuppressedClick(event: { preventDefault(): void; stopPropagation(): void }): boolean;
}

export function ChangeFileRow({
  file,
  worktree,
  grouped = false,
  editorShort,
  isRowSelected,
  isDraggingThis,
  onSelect,
  onOpenFile,
  onReview,
  onPointerDown,
  onContextMenu,
  onRowClick,
  onRowDoubleClick,
  consumeSuppressedClick,
}: ChangeFileRowProps) {
  const { t } = useI18n();
  const { name, directory } = splitPath(file.path);
  const findings = worktree.review.findings.filter((finding) => finding.fileId === file.id);
  const hasHigh = findings.some((finding) => finding.severity === 'high');
  const action = file.status === 'U'
    ? t.changes.ideActions.merge
    : file.range.kind === 'partial'
      ? t.changes.ideActions.stagedDiff
      : file.status === 'A'
        ? t.changes.ideActions.open
        : t.changes.ideActions.diff;

  return (
    <div
      className={`file-row ${grouped ? 'grouped' : ''} ${file.status === 'U' ? 'conflicted' : ''} ${file.ignoredForReview && !worktree.selectedIds.includes(file.id) ? 'ignored' : ''} ${file.localKeep ? 'local-keep-row' : ''} ${isRowSelected ? 'is-selected' : ''} ${isDraggingThis ? 'dragging' : ''}`}
      data-file-path={file.path}
      data-selected={isRowSelected ? 'true' : undefined}
      onPointerDown={(event) => onPointerDown(event, file, name)}
      onContextMenu={(event) => onContextMenu(event, file)}
      onClickCapture={(event) => {
        consumeSuppressedClick(event);
      }}
    >
      <input
        type="checkbox"
        className="file-checkbox"
        checked={worktree.selectedIds.includes(file.id)}
        aria-label={t.changes.selectFile(file.path)}
        onChange={() => onSelect(file.id)}
      />
      <button
        type="button"
        className="file-open"
        title={`${file.previousPath ? `${file.previousPath} → ` : ''}${file.path}`}
        onClick={(event) => onRowClick(event, file)}
        onDoubleClick={(event) => onRowDoubleClick(event, file)}
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
        aria-label={`${editorShort} ${action}: ${file.path}`}
        title={`${editorShort} ${action}`}
        onClick={(event) => {
          event.stopPropagation();
          if (consumeSuppressedClick(event)) return;
          onOpenFile(file.id);
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
        }}
      >
        {editorShort} {action}<Icon name="external" />
      </button>
      {!!findings.length && (
        <button
          type="button"
          className={`finding-count ${hasHigh ? 'danger' : 'warning'}`}
          aria-label={t.changes.findingCountLabel(file.path, findings.length)}
          onClick={onReview}
        >
          <span className="status-dot" />{findings.length}
        </button>
      )}
    </div>
  );
}
