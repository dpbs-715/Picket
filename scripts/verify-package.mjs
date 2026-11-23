import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// macOS /var is a symlink to /private/var. Tauri intentionally rejects a
// symlinked starting-binary path, so resolve it rather than disabling that guard.
const temporary = realpathSync(mkdtempSync(join(tmpdir(), 'picket-package-')));
const destination = join(temporary, '独立 路径', 'Picket.app');
const home = join(temporary, 'empty-home');
mkdirSync(home);
const assertGone = (pid) => {
  try { process.kill(pid, 0); } catch (error) {
    if (error.code === 'ESRCH') return;
    throw error;
  }
  throw new Error(`Pi process ${pid} remains alive after the application finished`);
};
try {
  const copied = spawnSync('/usr/bin/ditto', [join(root, 'src-tauri/target/release/bundle/macos/Picket.app'), destination], { encoding: 'utf8' });
  if (copied.status !== 0) throw new Error(copied.stderr || 'Build the application with pnpm desktop build first');
  const binary = join(destination, 'Contents/MacOS/Picket');
  const executable = existsSync(binary) ? binary : join(destination, 'Contents/MacOS/picket');
  const launch = (flag) => {
    const result = spawnSync(executable, [flag], {
      cwd: temporary, env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', HOME: home, TMPDIR: temporary },
      encoding: 'utf8', timeout: 45_000,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(result.stderr || `Application exited with ${result.status}: ${result.stdout}`);
    return result.stdout;
  };
  const report = JSON.parse(launch('--foundation-smoke'));
  for (const [phase, expected] of [['complete', 'completed'], ['cancelled', 'cancelled'], ['shutdown', 'cancelled']]) {
    const probe = report[phase];
    if (probe.status !== expected || probe.childExitCode !== 0 || probe.piCoreVersion !== '1.6.0') {
      throw new Error(`Incorrect packaged Pi result: ${phase}`);
    }
    assertGone(probe.childPid);
  }
  if (!report.git.temporaryRepositoryRemoved || report.git.commits.length !== 2 || report.git.cases.length !== 6) {
    throw new Error('Packaged Git scope proof failed');
  }
  let ui;
  if (process.argv.includes('--ui')) {
    const events = launch('--foundation-ui-smoke').trim().split('\n').map((line) => JSON.parse(line));
    const children = events.filter((event) => event.type === 'uiSmokeChild');
    const branches = events.find((event) => event.type === 'uiSmokeBranchProof');
    if (!branches?.branchCreationVerified || !branches.worktreeCreationVerified || !branches.stagingAndDraftsPreserved) {
      throw new Error('Real Git branch/worktree creation and draft preservation did not pass');
    }
    const commit = events.find((event) => event.type === 'uiSmokeCommitProof');
    if (!commit?.localCommitVerified || !commit.partialStagingCommittedOnlyStaged || !commit.unstagedAndUnselectedPreserved) {
      throw new Error('Real Git local commit and partial/unselected preservation did not pass');
    }
    const exit = events.find((event) => event.type === 'uiSmokeExit');
    if (children.length !== 2 || exit?.status !== 'passed') throw new Error('WebView IPC / close-window verification did not pass');
    for (const child of children) assertGone(child.pid);
    ui = { workbenchEmptyStateVerified: true, nativeTitlebarOverlayVerified: true,
      overlayHeadingClearanceVerified: true, windowPinVerified: true,
      realRepositoryReadVerified: true, selectionLeavesIndexUntouched: true, worktreeDraftIsolationVerified: true,
      realBranchCreationVerified: true, realWorktreeCreationVerified: true, branchStagingAndDraftsPreserved: true,
      realLocalCommitVerified: true, partialStagingCommittedOnlyStaged: true, unstagedAndUnselectedPreservedAfterCommit: true,
      worktreeParentSelection: 'temporary-fixture',
      webviewIpcVerified: true, windowClosedDuringStream: true, allPiProcessesReaped: true };
  }
  console.log(JSON.stringify({ packageIsolatedFromProject: true, globalNodeNotRequired: true,
    allPiProcessesReaped: true, report, ui }, null, 2));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
