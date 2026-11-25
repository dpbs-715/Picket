import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  TRAY_MAIN_ACTION_EVENT,
  TRAY_POPOVER_SHOWN_EVENT,
  TRAY_REFRESH_REQUESTED_EVENT,
  TRAY_STATE_EVENT,
  type TrayMainWindowAction,
  type TrayStateSnapshot,
} from '../../shared/tray-protocol';

export async function readTrayState(): Promise<TrayStateSnapshot> {
  return invoke<TrayStateSnapshot>('read_tray_state');
}

export async function syncTrayState(snapshot: TrayStateSnapshot): Promise<void> {
  if (!isTauri()) return;
  await invoke('sync_tray_state', { snapshot });
}

export async function requestTrayRefresh(): Promise<void> {
  if (!isTauri()) return;
  await invoke('request_tray_refresh');
}

export async function resizeTrayPopover(height: number): Promise<void> {
  if (!isTauri()) return;
  await invoke('resize_tray_popover', { height });
}

export async function hideTrayPopover(): Promise<void> {
  if (!isTauri()) return;
  await invoke('hide_tray_popover');
}

export async function openMainWindowFromTray(action?: TrayMainWindowAction): Promise<void> {
  if (!isTauri()) return;
  await invoke('tray_open_main_window', { action: action ?? { kind: 'show' } });
}

export async function quitAppFromTray(): Promise<void> {
  if (!isTauri()) return;
  await invoke('tray_quit_app');
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

export function subscribeTrayRefreshRequested(listener: () => void): () => void {
  return subscribeTauriEvent<void>(TRAY_REFRESH_REQUESTED_EVENT, () => listener());
}

export function subscribeTrayMainAction(
  listener: (action: TrayMainWindowAction) => void,
): () => void {
  return subscribeTauriEvent<TrayMainWindowAction>(TRAY_MAIN_ACTION_EVENT, listener);
}
