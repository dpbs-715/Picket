import { useCallback, useEffect, useRef, useState } from 'react';
import {
  isRunProcessActive,
  makeRunProcessId,
  type RunCommandConfig,
  type RunProcessSnapshot,
} from '../../../../shared/run-protocol';
import { getMessages } from '../../../i18n';
import type { WorkbenchBackend } from '../commands';
import { runSharedEnvFor, targetOf, type WorkbenchState, type Worktree } from '../state/workbench';
import type { NotifyFn } from '../types';

function processStatusRank(status: RunProcessSnapshot['status']): number {
  switch (status) {
    case 'starting':
      return 0;
    case 'running':
      return 1;
    case 'stopping':
      return 2;
    case 'exited':
    case 'failed':
      return 3;
  }
}

export function isNewerProcessSnapshot(
  next: RunProcessSnapshot,
  existing: RunProcessSnapshot,
): boolean {
  const nextGen = next.generation ?? 0;
  const prevGen = existing.generation ?? 0;
  if (nextGen !== prevGen) return nextGen > prevGen;
  if (next.startedAtMs !== existing.startedAtMs) return next.startedAtMs > existing.startedAtMs;
  const nextRank = processStatusRank(next.status);
  const prevRank = processStatusRank(existing.status);
  if (nextRank !== prevRank) return nextRank > prevRank;
  return next.logLineCount >= existing.logLineCount;
}

export interface RunProcessesController {
  processes: RunProcessSnapshot[];
  busyIds: string[];
  activeCountForWorktree: number;
  processForCommand: (commandId: string) => RunProcessSnapshot | undefined;
  start: (command: RunCommandConfig) => Promise<void>;
  stop: (processId: string, label?: string) => Promise<void>;
  stopAll: () => Promise<void>;
  stopAllForCommand: (repositoryId: string, commandId: string) => Promise<boolean>;
  restart: (command: RunCommandConfig) => Promise<void>;
  openLogs: (processId?: string) => Promise<void>;
  openUrl: (url: string) => Promise<void>;
  refreshEnv: () => Promise<void>;
}

export function useRunProcesses({
  backend,
  state,
  worktree,
  notify,
}: {
  backend: WorkbenchBackend;
  state: WorkbenchState;
  worktree: Worktree | undefined;
  notify: NotifyFn;
}): RunProcessesController {
  const [processes, setProcesses] = useState<RunProcessSnapshot[]>([]);
  const [busyIds, setBusyIds] = useState<string[]>([]);
  const processesRef = useRef(processes);
  processesRef.current = processes;
  const inFlightRef = useRef(new Set<string>());

  useEffect(() => {
    let active = true;
    let receivedLiveState = false;
    const unsub = backend.subscribeRunState?.((next) => {
      if (!active) return;
      receivedLiveState = true;
      setProcesses(next);
    });
    void backend.listRunProcesses?.()
      .then((initial) => {
        if (active && !receivedLiveState && Array.isArray(initial)) {
          setProcesses(initial);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
      unsub?.();
    };
  }, [backend]);

  const upsertProcess = useCallback((snapshot: RunProcessSnapshot) => {
    setProcesses((prev) => {
      const existing = prev.find((item) => item.processId === snapshot.processId);
      if (existing && !isNewerProcessSnapshot(snapshot, existing)) {
        return prev;
      }
      const rest = prev.filter((item) => item.processId !== snapshot.processId);
      return [snapshot, ...rest];
    });
  }, []);

  const withBusy = useCallback(async (key: string, fn: () => Promise<void>) => {
    if (inFlightRef.current.has(key)) return;
    inFlightRef.current.add(key);
    setBusyIds((prev) => (prev.includes(key) ? prev : [...prev, key]));
    try {
      await fn();
    } finally {
      inFlightRef.current.delete(key);
      setBusyIds((prev) => prev.filter((item) => item !== key));
    }
  }, []);

  const start = useCallback(async (command: RunCommandConfig) => {
    if (!worktree || !backend.startRunCommand) return;
    const processId = makeRunProcessId(worktree.worktreeId, command.id);
    const sharedEnv = runSharedEnvFor(state, worktree.repositoryId);
    const t = getMessages(state.locale).run;
    await withBusy(processId, async () => {
      try {
        const snapshot = await backend.startRunCommand!({
          target: targetOf(worktree),
          command,
          ...(sharedEnv.length > 0 ? { sharedEnv } : {}),
          locale: state.locale,
        });
        upsertProcess(snapshot);
        if (snapshot.status === 'running' || snapshot.status === 'starting') {
          notify(t.startedNotice(command.label), undefined, { tone: 'success' });
        }
      } catch (err) {
        notify(err instanceof Error ? err.message : String(err), undefined, { tone: 'error' });
      }
    });
  }, [backend, notify, state, upsertProcess, withBusy, worktree]);

  const stop = useCallback(async (processId: string, label?: string) => {
    if (!backend.stopRunCommand) return;
    const t = getMessages(state.locale).run;
    await withBusy(`stopping:${processId}`, async () => {
      try {
        const snapshot = await backend.stopRunCommand!({
          processId,
          locale: state.locale,
        });
        upsertProcess(snapshot);
        if (label) {
          notify(t.stoppedNotice(label), undefined, { tone: 'info' });
        }
      } catch (err) {
        notify(err instanceof Error ? err.message : String(err), undefined, { tone: 'error' });
      }
    });
  }, [backend, notify, state.locale, upsertProcess, withBusy]);

  const stopAll = useCallback(async () => {
    if (!worktree || !backend.stopRunCommand) return;
    const activeItems = processesRef.current.filter(
      (item) => item.worktreeId === worktree.worktreeId && isRunProcessActive(item.status),
    );
    if (activeItems.length === 0) return;
    const t = getMessages(state.locale).run;
    await withBusy('global:stopAll', async () => {
      const results = await Promise.allSettled(
        activeItems.map((item) =>
          backend.stopRunCommand!({ processId: item.processId, locale: state.locale }).then((snap) => {
            upsertProcess(snap);
            return snap;
          }),
        ),
      );
      const stoppedCount = results.filter((r) => r.status === 'fulfilled').length;
      if (stoppedCount > 0) {
        notify(t.stoppedAllNotice(stoppedCount), undefined, { tone: 'info' });
      }
      const firstRejected = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
      if (firstRejected) {
        notify(
          firstRejected.reason instanceof Error ? firstRejected.reason.message : String(firstRejected.reason),
          undefined,
          { tone: 'error' },
        );
      }
    });
  }, [backend, notify, state.locale, upsertProcess, withBusy, worktree]);

  const stopAllForCommand = useCallback(async (repositoryId: string, commandId: string): Promise<boolean> => {
    const activeItems = processesRef.current.filter(
      (item) => item.repositoryId === repositoryId
        && item.commandId === commandId
        && isRunProcessActive(item.status),
    );
    if (activeItems.length === 0) return true;
    if (!backend.stopRunCommand) return false;
    let allStopped = true;
    await withBusy(`stopCmd:${repositoryId}:${commandId}`, async () => {
      const results = await Promise.allSettled(
        activeItems.map((item) =>
          backend.stopRunCommand!({ processId: item.processId, locale: state.locale }).then((snap) => {
            upsertProcess(snap);
            return snap;
          }),
        ),
      );
      const firstRejected = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
      if (firstRejected) {
        allStopped = false;
        notify(
          firstRejected.reason instanceof Error ? firstRejected.reason.message : String(firstRejected.reason),
          undefined,
          { tone: 'error' },
        );
      }
    });
    return allStopped;
  }, [backend, notify, state.locale, upsertProcess, withBusy]);

  const restart = useCallback(async (command: RunCommandConfig) => {
    if (!worktree || !backend.restartRunCommand) return;
    const processId = makeRunProcessId(worktree.worktreeId, command.id);
    const sharedEnv = runSharedEnvFor(state, worktree.repositoryId);
    const t = getMessages(state.locale).run;
    await withBusy(`restarting:${processId}`, async () => {
      try {
        const snapshot = await backend.restartRunCommand!({
          target: targetOf(worktree),
          command,
          ...(sharedEnv.length > 0 ? { sharedEnv } : {}),
          locale: state.locale,
        });
        upsertProcess(snapshot);
        if (snapshot.status === 'running' || snapshot.status === 'starting') {
          notify(t.restartedNotice(command.label), undefined, { tone: 'success' });
        }
      } catch (err) {
        notify(err instanceof Error ? err.message : String(err), undefined, { tone: 'error' });
      }
    });
  }, [backend, notify, state, upsertProcess, withBusy, worktree]);

  const openLogs = useCallback(async (processId?: string) => {
    if (!backend.openRunLogsWindow) return;
    try {
      await backend.openRunLogsWindow(processId ? { processId } : {});
    } catch (err) {
      notify(err instanceof Error ? err.message : String(err), undefined, { tone: 'error' });
    }
  }, [backend, notify]);

  const openUrl = useCallback(async (url: string) => {
    if (!backend.openRunLocalUrl) return;
    try {
      await backend.openRunLocalUrl({ url, locale: state.locale });
    } catch (err) {
      notify(err instanceof Error ? err.message : String(err), undefined, { tone: 'error' });
    }
  }, [backend, notify, state.locale]);

  const refreshEnv = useCallback(async () => {
    if (!backend.refreshUserShellEnv) return;
    const t = getMessages(state.locale).run;
    await withBusy('global:refreshEnv', async () => {
      try {
        const summary = await backend.refreshUserShellEnv!(state.locale);
        notify(
          summary.warning ?? t.refreshEnvDone(summary.shell, summary.pathCount),
          undefined,
          { tone: summary.warning ? 'warning' : 'success' },
        );
      } catch (err) {
        notify(err instanceof Error ? err.message : String(err), undefined, { tone: 'error' });
      }
    });
  }, [backend, notify, state.locale, withBusy]);

  const processForCommand = useCallback(
    (commandId: string) => {
      if (!worktree) return undefined;
      const pid = makeRunProcessId(worktree.worktreeId, commandId);
      return processes.find((item) => item.processId === pid);
    },
    [processes, worktree],
  );

  const activeCountForWorktree = worktree
    ? processes.filter((item) => item.worktreeId === worktree.worktreeId && isRunProcessActive(item.status)).length
    : 0;

  return {
    processes,
    busyIds,
    activeCountForWorktree,
    processForCommand,
    start,
    stop,
    stopAll,
    stopAllForCommand,
    restart,
    openLogs,
    openUrl,
    refreshEnv,
  };
}
