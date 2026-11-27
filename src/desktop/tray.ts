import { isTauri } from '@tauri-apps/api/core';
import { invokeDesktop } from './errors';
import { listen } from '@tauri-apps/api/event';
import type { GitRepositorySnapshot } from '../../shared/git-read-protocol';
import {
  TRAY_MAIN_ACTION_EVENT,
  TRAY_POPOVER_SHOWN_EVENT,
  TRAY_SNAPSHOTS_SYNCED_EVENT,
  TRAY_STATE_EVENT,
  type TrayMainWindowAction,
  type TrayStateSnapshot,
  type TrayUiContextUpdate,
} from '../../shared/tray-protocol';

export async function readTrayState(): Promise<TrayStateSnapshot> {
  return invokeDesktop<TrayStateSnapshot>('read_tray_state');
}

export async function syncTrayContext(context: TrayUiContextUpdate): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('sync_tray_context', {
    context: {
      ...(context.locale ? { locale: context.locale } : {}),
      ...(context.theme ? { theme: context.theme } : {}),
      ...(context.activeWorktreeId ? { activeWorktreeId: context.activeWorktreeId } : {}),
    },
  });
}

export async function requestTrayRefresh(): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('request_tray_refresh');
}

export async function resizeTrayPopover(height: number): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('resize_tray_popover', { height });
}

export async function hideTrayPopover(): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('hide_tray_popover');
}

export async function openMainWindowFromTray(action?: TrayMainWindowAction): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('tray_open_main_window', { action: action ?? { kind: 'show' } });
}

export async function quitAppFromTray(): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('tray_quit_app');
}

function subscribeTauriEvent<T>(eventName: string, listener: (payload: T) => void): () => void {
  if (!isTauri()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | undefined;
  void listen<T>(eventName, (event) => {
    if (!disposed) listener(event.payload);
  })
    .then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    })
    .catch(() => {});
  return () => {
    disposed = true;
    unlisten?.();
  };
}

export function subscribeTrayState(listener: (snapshot: TrayStateSnapshot) => void): () => void {
  return subscribeTauriEvent<TrayStateSnapshot>(TRAY_STATE_EVENT, listener);
}

export function subscribeTrayPopoverShown(listener: () => void): () => void {
  return subscribeTauriEvent<void>(TRAY_POPOVER_SHOWN_EVENT, () => listener());
}

export function subscribeTraySnapshotsSynced(
  listener: (snapshots: GitRepositorySnapshot[]) => void,
): () => void {
  return subscribeTauriEvent<GitRepositorySnapshot[]>(TRAY_SNAPSHOTS_SYNCED_EVENT, listener);
}

export function subscribeTrayMainAction(
  listener: (action: TrayMainWindowAction) => void,
): () => void {
  return subscribeTauriEvent<TrayMainWindowAction>(TRAY_MAIN_ACTION_EVENT, listener);
}
