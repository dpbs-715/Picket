import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ResolvedTheme } from '../../../shared/theme-protocol';
import { applyDocumentTheme, readSystemPrefersDark } from '../../ui/theme';
import './splash.css';
import { WhaleSplashStage } from './WhaleSplashStage';
import type { SplashLaunchPlan } from './whaleMath';

function readInitialSplashTheme(): ResolvedTheme {
  if (typeof window !== 'undefined') {
    const queryTheme = new URLSearchParams(window.location.search).get('theme');
    if (queryTheme === 'dark' || queryTheme === 'light') {
      return queryTheme;
    }
  }
  return readSystemPrefersDark() ? 'dark' : 'light';
}

async function bootSplash(): Promise<void> {
  const root = document.getElementById('splash-root');
  if (!root) return;

  const initialTheme = readInitialSplashTheme();
  applyDocumentTheme(initialTheme);

  const placeholderPlan: SplashLaunchPlan = {
    dx: 0,
    dy: 0,
    distance: 0,
    arcHeight: 76,
    leapDurationMs: 440,
    compactMode: false,
    workbenchAlreadyReady: false,
    theme: initialTheme,
  };

  if (!isTauri()) {
    const stage = new WhaleSplashStage({ ...placeholderPlan, workbenchAlreadyReady: true });
    root.append(stage.element);
    stage.start();
    return;
  }

  let unlisten: (() => void) | undefined;
  let readyFromEvent = false;
  const stage = new WhaleSplashStage(placeholderPlan, {
    onLaunchJump: async () => {
      try {
        await invoke('run_splash_jump');
      } catch {
        void invoke('complete_splash').catch(() => {});
      }
    },
    onComplete: () => {
      unlisten?.();
      void invoke('complete_splash').catch(() => {});
    },
  });

  // Mount the transparent SVG before asking Rust to reveal the window so frame 0 is ready.
  root.append(stage.element);

  // Register listener BEFORE `splash_ready` so a concurrent `notify_workbench_ready` can never be missed.
  try {
    unlisten = await listen('picket:splash-ready-to-land', () => {
      readyFromEvent = true;
      stage.markWorkbenchReady();
    });
  } catch {
    readyFromEvent = true;
  }

  let plan: SplashLaunchPlan;
  try {
    plan = await invoke<SplashLaunchPlan>('splash_ready');
  } catch {
    unlisten?.();
    void invoke('complete_splash').catch(() => {});
    return;
  }

  const resolvedTheme: ResolvedTheme =
    plan.theme === 'dark' || plan.theme === 'light' ? plan.theme : initialTheme;
  applyDocumentTheme(resolvedTheme);

  stage.updatePlan({
    ...plan,
    theme: resolvedTheme,
    workbenchAlreadyReady: plan.workbenchAlreadyReady || readyFromEvent,
  });
  stage.start();
}

void bootSplash();
