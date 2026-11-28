import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AppLocale } from '../../shared/language-protocol';
import {
  RUN_DISCOVER_PROGRESS_EVENT,
  RUN_LOG_BATCH_EVENT,
  RUN_LOGS_SELECT_EVENT,
  RUN_STATE_EVENT,
  decodeDiscoverRunCommandsReply,
  type ClearRunLogsRequest,
  type DiscoverRunCommandsProgressEvent,
  type DiscoverRunCommandsReply,
  type DiscoverRunCommandsRequest,
  type GetRunLogsRequest,
  type OpenRunLocalUrlRequest,
  type OpenRunLogsWindowRequest,
  type RestartRunCommandRequest,
  type RunLogBatchEvent,
  type RunProcessLogsReply,
  type RunProcessSnapshot,
  type StartRunCommandRequest,
  type StopRunCommandRequest,
  type UserEnvSummary,
} from '../../shared/run-protocol';
import { invokeDesktop } from './errors';

export async function listDesktopRunProcesses(): Promise<RunProcessSnapshot[]> {
  if (!isTauri()) return [];
  const raw = await invokeDesktop<RunProcessSnapshot[]>('list_run_processes');
  return Array.isArray(raw) ? raw : [];
}

export async function getSelectedDesktopRunLogsProcess(): Promise<string | null> {
  if (!isTauri()) return null;
  const raw = await invokeDesktop<string | null>('get_selected_run_logs_process');
  return typeof raw === 'string' && raw ? raw : null;
}

export async function startDesktopRunCommand(
  request: StartRunCommandRequest,
): Promise<RunProcessSnapshot> {
  return invokeDesktop<RunProcessSnapshot>('start_run_command', { request });
}

export async function stopDesktopRunCommand(
  request: StopRunCommandRequest,
): Promise<RunProcessSnapshot> {
  return invokeDesktop<RunProcessSnapshot>('stop_run_command', { request });
}

export async function restartDesktopRunCommand(
  request: RestartRunCommandRequest,
): Promise<RunProcessSnapshot> {
  return invokeDesktop<RunProcessSnapshot>('restart_run_command', { request });
}

export async function getDesktopRunProcessLogs(
  request: GetRunLogsRequest,
): Promise<RunProcessLogsReply> {
  return invokeDesktop<RunProcessLogsReply>('get_run_process_logs', { request });
}

export async function clearDesktopRunProcessLogs(
  request: ClearRunLogsRequest,
): Promise<boolean> {
  return Boolean(await invokeDesktop<boolean>('clear_run_process_logs', { request }));
}

export async function openDesktopRunLogsWindow(
  request: OpenRunLogsWindowRequest = {},
): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('open_run_logs_window', { request });
}

export async function openDesktopRunLocalUrl(
  request: OpenRunLocalUrlRequest,
): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('open_run_local_url', { request });
}

export async function refreshDesktopUserShellEnv(
  locale?: AppLocale,
): Promise<UserEnvSummary> {
  return invokeDesktop<UserEnvSummary>('refresh_user_shell_env', { locale });
}

export async function discoverDesktopRunCommands(
  request: DiscoverRunCommandsRequest,
  onProgress?: (event: DiscoverRunCommandsProgressEvent) => void,
): Promise<DiscoverRunCommandsReply> {
  let unlisten: (() => void) | null = null;
  if (onProgress && isTauri()) {
    unlisten = await listen<DiscoverRunCommandsProgressEvent>(
      RUN_DISCOVER_PROGRESS_EVENT,
      (event) => {
        if (event.payload?.requestId === request.requestId) {
          onProgress(event.payload);
        }
      },
    );
  }
  try {
    const raw = await invokeDesktop('discover_run_commands', { request });
    return decodeDiscoverRunCommandsReply(raw);
  } finally {
    unlisten?.();
  }
}

export async function cancelDesktopDiscoverRunCommands(requestId: string): Promise<void> {
  if (!isTauri()) return;
  await invokeDesktop('cancel_discover_run_commands', { requestId });
}

export function subscribeDesktopRunState(
  receive: (snapshots: RunProcessSnapshot[]) => void,
): () => void {
  if (!isTauri()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | null = null;
  void listen<RunProcessSnapshot[]>(RUN_STATE_EVENT, (event) => {
    if (!disposed && Array.isArray(event.payload)) {
      receive(event.payload);
    }
  }).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  }).catch(() => {});
  return () => {
    disposed = true;
    unlisten?.();
  };
}

export function subscribeDesktopRunLogBatch(
  receive: (batch: RunLogBatchEvent) => void,
): () => void {
  if (!isTauri()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | null = null;
  void listen<RunLogBatchEvent>(RUN_LOG_BATCH_EVENT, (event) => {
    if (!disposed && event.payload) {
      receive(event.payload);
    }
  }).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  }).catch(() => {});
  return () => {
    disposed = true;
    unlisten?.();
  };
}

export function subscribeDesktopRunLogsSelect(
  receive: (processId: string) => void,
): () => void {
  if (!isTauri()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | null = null;
  void listen<string>(RUN_LOGS_SELECT_EVENT, (event) => {
    if (!disposed && typeof event.payload === 'string' && event.payload) {
      receive(event.payload);
    }
  }).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  }).catch(() => {});
  return () => {
    disposed = true;
    unlisten?.();
  };
}
