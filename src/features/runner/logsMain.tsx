import { isTauri } from '@tauri-apps/api/core';
import { StrictMode, useCallback, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import type { AppLocale } from '../../../shared/language-protocol';
import type { RunProcessSnapshot } from '../../../shared/run-protocol';
import { DEFAULT_APP_THEME, type AppTheme } from '../../../shared/theme-protocol';
import {
  clearDesktopRunProcessLogs,
  getDesktopRunProcessLogs,
  getSelectedDesktopRunLogsProcess,
  listDesktopRunProcesses,
  openDesktopRunLocalUrl,
  stopDesktopRunCommand,
  subscribeDesktopRunLogBatch,
  subscribeDesktopRunLogsSelect,
  subscribeDesktopRunState,
} from '../../desktop/runner';
import { readTrayState, subscribeTrayState } from '../../desktop/tray';
import { RunLogsWindow } from './RunLogsWindow';
import './logs.css';

function RunLogsApp() {
  const [locale, setLocale] = useState<AppLocale>('en');
  const [theme, setTheme] = useState<AppTheme>(DEFAULT_APP_THEME);
  const [processes, setProcesses] = useState<RunProcessSnapshot[]>([]);
  const [selectedProcessId, setSelectedProcessId] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    let mounted = true;
    let receivedLiveTray = false;
    let receivedLiveState = false;
    let receivedLiveSelect = false;

    const unsubTray = subscribeTrayState((tray) => {
      if (!mounted) return;
      receivedLiveTray = true;
      setLocale(tray.locale);
      setTheme(tray.theme);
    });
    const unsubState = subscribeDesktopRunState((next) => {
      if (!mounted) return;
      receivedLiveState = true;
      setProcesses(next);
      setSelectedProcessId((prev) => {
        if (prev && next.some((p) => p.processId === prev)) return prev;
        return next[0]?.processId ?? null;
      });
    });
    const unsubSelect = subscribeDesktopRunLogsSelect((pid) => {
      if (!mounted) return;
      receivedLiveSelect = true;
      setSelectedProcessId(pid);
    });

    void readTrayState()
      .then((tray) => {
        if (!mounted || receivedLiveTray) return;
        setLocale(tray.locale);
        setTheme(tray.theme);
      })
      .catch(() => {});
    void Promise.all([
      listDesktopRunProcesses(),
      getSelectedDesktopRunLogsProcess(),
    ])
      .then(([list, initialSelected]) => {
        if (!mounted) return;
        if (!receivedLiveState) {
          setProcesses(list);
        }
        if (!receivedLiveSelect) {
          if (initialSelected) {
            setSelectedProcessId(initialSelected);
          } else if (list.length > 0) {
            setSelectedProcessId((prev) => prev ?? list[0].processId);
          }
        }
      })
      .catch(() => {});

    return () => {
      mounted = false;
      unsubTray();
      unsubState();
      unsubSelect();
    };
  }, []);

  const loadLogs = useCallback(async (processId: string) => {
    const reply = await getDesktopRunProcessLogs({ processId });
    return reply.lines;
  }, []);

  const handleStop = useCallback(async (processId: string) => {
    await stopDesktopRunCommand({ processId, locale });
  }, [locale]);

  const handleClear = useCallback(async (processId: string) => {
    await clearDesktopRunProcessLogs({ processId });
  }, []);

  const handleOpenUrl = useCallback(async (url: string) => {
    await openDesktopRunLocalUrl({ url, locale });
  }, [locale]);

  return (
    <RunLogsWindow
      locale={locale}
      theme={theme}
      processes={processes}
      selectedProcessId={selectedProcessId}
      onSelectProcess={setSelectedProcessId}
      loadLogs={loadLogs}
      subscribeLogs={subscribeDesktopRunLogBatch}
      onStop={handleStop}
      onClear={handleClear}
      onOpenUrl={handleOpenUrl}
    />
  );
}

const root = document.getElementById('run-logs-root');
if (!root) throw new Error('Missing run logs root');
createRoot(root).render(
  <StrictMode>
    <RunLogsApp />
  </StrictMode>,
);
