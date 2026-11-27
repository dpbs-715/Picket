import type { Root } from 'react-dom/client';
import { runCompactModeChecks } from './checks/compactModeChecks';
import { runCoreWorkbenchChecks } from './checks/coreWorkbenchChecks';
import { runCustomActionsChecks } from './checks/customActionsChecks';
import { createBrowserCheckHarness } from './checks/harness';
import { runLocalKeepChecks } from './checks/localKeepChecks';
import { runRepositoryAndWorkflowChecks } from './checks/repositoryAndWorkflowChecks';
import { runSettingsUpdaterMotionChecks } from './checks/settingsUpdaterMotionChecks';
import { runSkillsAndSyncChecks } from './checks/skillsAndSyncChecks';
import { runTrayThemeTransitionsChecks } from './checks/trayThemeTransitionsChecks';

export async function verifyWorkbench(root: Root) {
  const harness = createBrowserCheckHarness(root);
  harness.setInstantMotion(true);
  try {
    await runCoreWorkbenchChecks(harness);
    await runRepositoryAndWorkflowChecks(harness);
    await runLocalKeepChecks(harness);
    await runCompactModeChecks(harness);
    await runSettingsUpdaterMotionChecks(harness);
    await runCustomActionsChecks(harness);
    await runSkillsAndSyncChecks(harness);
    await runTrayThemeTransitionsChecks(harness);

    return {
      environment: 'Chrome headless · development fixtures only (not real Git or live model acceptance)',
      viewport: { width: innerWidth, height: innerHeight },
      checks: harness.checks,
    };
  } finally {
    harness.setInstantMotion(false);
  }
}
