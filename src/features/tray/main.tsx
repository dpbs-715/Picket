import { isTauri } from '@tauri-apps/api/core';
import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import type { TrayMainWindowAction, TrayStateSnapshot } from '../../../shared/tray-protocol';
import {
  openDesktopRunLogsWindow,
  stopDesktopRunCommand,
} from '../../desktop/runner';
import {
  hideTrayPopover,
  openMainWindowFromTray,
  quitAppFromTray,
  readTrayState,
  requestTrayRefresh,
  resizeTrayPopover,
  subscribeTrayPopoverShown,
  subscribeTrayState,
} from '../../desktop/tray';
import { TrayPopover } from './TrayPopover';
import './tray.css';

const EMPTY_SNAPSHOT: TrayStateSnapshot = {
  protocolVersion: 1,
  locale: 'en',
  theme: 'system',
  hasUpdate: false,
  refreshing: false,
  repositories: [],
};

function TrayApp() {
  const [snapshot, setSnapshot] = useState<TrayStateSnapshot>(EMPTY_SNAPSHOT);
  const [openGeneration, setOpenGeneration] = useState(0);

  useEffect(() => {
    if (!isTauri()) return;
    let mounted = true;
    let receivedLiveState = false;

    const unsubState = subscribeTrayState((next) => {
      if (!mounted) return;
      receivedLiveState = true;
      setSnapshot(next);
    });
    const unsubShown = subscribeTrayPopoverShown(() => {
      if (mounted) setOpenGeneration((value) => value + 1);
    });

    void readTrayState()
      .then((initial) => {
        if (mounted && !receivedLiveState) setSnapshot(initial);
      })
      .catch(() => {});

    return () => {
      mounted = false;
      unsubState();
      unsubShown();
    };
  }, []);

  return (
    <TrayPopover
      snapshot={snapshot}
      openGeneration={openGeneration}
      onRefresh={() => void requestTrayRefresh().catch(() => {})}
      onOpenSettings={() => void openMainWindowFromTray({ kind: 'openSettings' }).catch(() => {})}
      onOpenMainWindow={(action?: TrayMainWindowAction) => void openMainWindowFromTray(action).catch(() => {})}
      onStopRunService={(processId) => void stopDesktopRunCommand({ processId, locale: snapshot.locale }).catch(() => {})}
      onOpenRunLogs={(processId) => void openDesktopRunLogsWindow({ processId }).catch(() => {})}
      onQuit={() => void quitAppFromTray().catch(() => {})}
      onDismiss={() => void hideTrayPopover().catch(() => {})}
      onHeightChange={(height) => void resizeTrayPopover(height).catch(() => {})}
    />
  );
}

const root = document.getElementById('tray-root');
if (!root) throw new Error('Missing tray root');
createRoot(root).render(
  <StrictMode>
    <TrayApp />
  </StrictMode>,
);
