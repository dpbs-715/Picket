import type { RefObject } from 'react';
import { useI18n } from '../../../i18n';
import { MOTION_DURATION, MOTION_EASE, MotionPresence, m, useInstantMotion } from '../../../ui/motion';
import type { ContextMenuState, DragState } from './changesPanelSelection';

interface ChangesPanelOverlaysProps {
  contextMenu: ContextMenuState | null;
  menuPosition: { left: number; top: number } | null;
  menuRef: RefObject<HTMLDivElement | null>;
  dragState: DragState | null;
  onToggleLocalKeep(): void;
}

export function ChangesPanelOverlays({
  contextMenu,
  menuPosition,
  menuRef,
  dragState,
  onToggleLocalKeep,
}: ChangesPanelOverlaysProps) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
    <>
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
              onClick={onToggleLocalKeep}
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
    </>
  );
}
