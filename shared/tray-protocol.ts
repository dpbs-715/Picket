import type { AppLocale } from './language-protocol';

export type TraySessionStatus =
  | 'idle'
  | 'running'
  | 'needsDecision'
  | 'conflict'
  | 'waitingManual'
  | 'pushFailed'
  | 'done';

export interface TrayRepositorySummary {
  repositoryId: string;
  worktreeId: string;
  name: string;
  branch: string;
  detached: boolean;
  ahead: number;
  behind: number;
  hasUpstream: boolean;
  changeCount: number;
  localKeepCount: number;
  worktreeCount: number;
  active: boolean;
  readError?: string;
  sessionStatus: TraySessionStatus;
  statusDetail?: string;
}

export interface TrayStateSnapshot {
  protocolVersion: 1;
  locale: AppLocale;
  hasUpdate: boolean;
  refreshing: boolean;
  repositories: TrayRepositorySummary[];
}

export type TrayMainWindowAction =
  | { kind: 'show' }
  | { kind: 'openSettings' }
  | { kind: 'focusWorktree'; repositoryId: string; worktreeId: string; openDock: boolean };

export const TRAY_STATE_EVENT = 'picket:tray-state';
export const TRAY_REFRESH_REQUESTED_EVENT = 'picket:tray-refresh-requested';
export const TRAY_MAIN_ACTION_EVENT = 'picket:tray-main-action';
export const TRAY_POPOVER_SHOWN_EVENT = 'picket:tray-popover-shown';
