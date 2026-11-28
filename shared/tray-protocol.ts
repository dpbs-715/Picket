import type {
  TrayMainWindowAction,
  TrayRepositorySummary,
  TrayRunningServiceSummary,
  TraySessionStatus,
  TrayStateSnapshot,
  TrayUiContextUpdate,
} from './generated';

export type {
  TrayMainWindowAction,
  TrayRepositorySummary,
  TrayRunningServiceSummary,
  TraySessionStatus,
  TrayStateSnapshot,
  TrayUiContextUpdate,
};

export const TRAY_STATE_EVENT = 'picket:tray-state';
export const TRAY_SNAPSHOTS_SYNCED_EVENT = 'picket:tray-snapshots-synced';
export const TRAY_MAIN_ACTION_EVENT = 'picket:tray-main-action';
export const TRAY_POPOVER_SHOWN_EVENT = 'picket:tray-popover-shown';
