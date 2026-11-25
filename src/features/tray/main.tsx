import { isTauri } from '@tauri-apps/api/core';
import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import type { TrayMainWindowAction, TrayStateSnapshot } from '../../../shared/tray-protocol';
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
    void readTrayState()
      .then((initial) => {
        if (mounted) setSnapshot(initial);
      })
      .catch(() => {});

    const unsubState = subscribeTrayState((next) => {
      if (mounted) setSnapshot(next);
    });
    const unsubShown = subscribeTrayPopoverShown(() => {
      if (mounted) setOpenGeneration((value) => value + 1);
    });
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
      onRefresh={() => void requestTrayRefresh()}
      onOpenSettings={() => void openMainWindowFromTray({ kind: 'openSettings' })}
      onOpenMainWindow={(action?: TrayMainWindowAction) => void openMainWindowFromTray(action)}
      onQuit={() => void quitAppFromTray()}
      onDismiss={() => void hideTrayPopover()}
      onHeightChange={(height) => void resizeTrayPopover(height)}
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
