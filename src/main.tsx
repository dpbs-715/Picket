import { isTauri } from '@tauri-apps/api/core';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Workbench } from './features/workbench/Workbench';
import { desktopWorkbenchBackend } from './desktop/repositories';
import { disconnectedBackend } from './features/workbench/commands';

// The shipping desktop target is macOS; browser previews have no native controls.
document.documentElement.classList.toggle('native-titlebar', isTauri());
if (import.meta.env.VITE_PICKET_SMOKE === '1') {
  void import('./features/foundation/ui-smoke').then(({ registerUiSmoke }) => {
    registerUiSmoke();
  });
}
const root = document.getElementById('root');
if (!root) throw new Error('Missing application root');
createRoot(root).render(<StrictMode><Workbench backend={isTauri() ? desktopWorkbenchBackend : disconnectedBackend} /></StrictMode>);
