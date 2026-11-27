import { useLayoutEffect, useRef } from 'react';
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import { useI18n } from '../../../i18n';
import { Icon } from '../../../ui/controls';
import { splitPath, type Change, type Worktree } from '../state/workbench';
import { splitFileNameExt, treeGuideOffsets, treeIndentPx } from './changesPanelTree';

export function FileName({ name }: { name: string }) {
  const { stem, ext } = splitFileNameExt(name);
  return (
    <span className="mono file-name">
      <span className="file-stem">{stem}</span>
      {ext && <span className="file-ext">{ext}</span>}
    </span>
  );
}

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
  treeDepth?: number;
  editorShort: string;
  isRowSelected: boolean;
  isDraggingThis: boolean;
  onSelect(fileId: string): void;
  onOpenFile(fileId: string): void;
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
  treeDepth,
  editorShort,
  isRowSelected,
  isDraggingThis,
  onSelect,
  onOpenFile,
  onPointerDown,
  onContextMenu,
  onRowClick,
  onRowDoubleClick,
  consumeSuppressedClick,
}: ChangeFileRowProps) {
  const { t } = useI18n();
  const { name, directory } = splitPath(file.path);
  const hasDirectory = !grouped && Boolean(directory);
  const effectiveDepth = grouped ? (treeDepth ?? 1) : 0;
  const guideOffsets = grouped ? treeGuideOffsets(effectiveDepth) : [];
  const action = file.status === 'U'
    ? t.changes.ideActions.merge
    : file.range.kind === 'partial'
      ? t.changes.ideActions.stagedDiff
      : file.status === 'A'
        ? t.changes.ideActions.open
        : t.changes.ideActions.diff;

  return (
    <div
      className={`file-row ${grouped ? 'grouped' : ''} ${hasDirectory ? 'has-directory' : ''} ${file.status === 'U' ? 'conflicted' : ''} ${file.localKeep ? 'local-keep-row' : ''} ${isRowSelected ? 'is-selected' : ''} ${isDraggingThis ? 'dragging' : ''}`}
      data-file-path={file.path}
      data-selected={isRowSelected ? 'true' : undefined}
      style={grouped ? { paddingLeft: `${treeIndentPx(effectiveDepth)}px` } : undefined}
      onPointerDown={(event) => onPointerDown(event, file, name)}
      onContextMenu={(event) => onContextMenu(event, file)}
      onClickCapture={(event) => {
        consumeSuppressedClick(event);
      }}
    >
      {guideOffsets.map((leftPx) => (
        <span key={leftPx} className="tree-guide" style={{ left: `${leftPx}px` }} aria-hidden="true" />
      ))}
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
          <span className="file-primary-line">
            <FileName name={name} />
            {file.status === 'U'
              ? <span className="file-badge danger">{t.changes.conflictBadge}</span>
              : file.range.kind === 'partial' && <span className="file-badge warning">{t.changes.partialStagedBadge}</span>}
          </span>
          {hasDirectory && <span className="mono file-directory truncate">{directory}</span>}
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
    </div>
  );
}
