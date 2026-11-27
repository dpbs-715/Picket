import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const expectedPiCoreVersion = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).dependencies['@earendil-works/pi-agent-core'];
// macOS /var is a symlink to /private/var. Tauri intentionally rejects a
// symlinked starting-binary path, so resolve it rather than disabling that guard.
const temporary = realpathSync(mkdtempSync(join(tmpdir(), 'picket-package-')));
const releaseDestination = join(temporary, '\u72ec\u7acb \u53d1\u5e03', 'Picket.app');
const destination = join(temporary, '\u72ec\u7acb \u8def\u5f84', 'Picket.app');
const home = join(temporary, 'empty-home');
mkdirSync(home);

const assertGone = (pid) => {
  try { process.kill(pid, 0); } catch (error) {
    if (error.code === 'ESRCH') return;
    throw error;
  }
  throw new Error(`Pi process ${pid} remains alive after the application finished`);
};

function collectFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...collectFiles(full));
    else if (entry.isFile()) out.push(full);
  }
  return out;
}

function verifyBundleNoDemoOrSecretLeaks(bundleDir, distDir) {
  const forbidden = [
    'my-order-service',
    '\u6f14\u7ec3\u63a7\u5236\u53f0',
    '\u6a21\u62df\u4fee\u590d',
    'Simulation Console',
    'Simulate Fix',
    'sk-ui-smoke',
    'sk-ant-api03-',
    'sk-proj-',
  ];
  const targets = [
    ...collectFiles(distDir),
    ...collectFiles(join(bundleDir, 'Contents', 'Resources')),
    join(bundleDir, 'Contents', 'Info.plist'),
  ];
  for (const file of targets) {
    if (file.endsWith('.icns') || file.endsWith('.png') || file.endsWith('picket-node')) continue;
    const content = readFileSync(file, 'utf8');
    for (const pattern of forbidden) {
      if (content.includes(pattern)) {
        throw new Error(`Packaged artifact ${file} leaked forbidden demo/secret marker: ${pattern}`);
      }
    }
  }
}

function verifyReleaseBundleNoSmokeBackdoors(bundleDir, distDir) {
  const forbiddenSmokeMarkers = [
    '--foundation-smoke',
    '--foundation-ui-smoke',
    '--foundation-restart-smoke',
    'set_smoke_local_auth',
    'verify_model_m3_smoke',
    'verify_cpush_m4_smoke',
    'verify_m5_conflict_smoke',
    'verify_tray_smoke',
    'picket:foundation-smoke',
    '__PICKET_UI_SMOKE__',
  ];
  const binary = existsSync(join(bundleDir, 'Contents/MacOS/Picket'))
    ? join(bundleDir, 'Contents/MacOS/Picket')
    : join(bundleDir, 'Contents/MacOS/picket');
  const binaryBytes = readFileSync(binary);
  for (const marker of forbiddenSmokeMarkers) {
    if (binaryBytes.includes(Buffer.from(marker, 'utf8'))) {
      throw new Error(`Production release binary ${binary} leaked smoke-harness marker: ${marker}`);
    }
  }
  for (const file of collectFiles(distDir)) {
    if (file.endsWith('.icns') || file.endsWith('.png')) continue;
    const content = readFileSync(file, 'utf8');
    for (const marker of forbiddenSmokeMarkers) {
      if (content.includes(marker)) {
        throw new Error(`Production frontend bundle ${file} leaked smoke-harness marker: ${marker}`);
      }
    }
  }
  const nodeBinary = join(bundleDir, 'Contents/MacOS/picket-node');
  const sidecarScript = join(bundleDir, 'Contents/Resources/sidecar/pi-sidecar.mjs');
  const sidecarHome = join(temporary, 'release-sidecar-home');
  mkdirSync(sidecarHome, { recursive: true });
  const handshakeRun = spawnSync(nodeBinary, [sidecarScript], {
    cwd: temporary,
    env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', HOME: sidecarHome, TMPDIR: temporary },
    input: `${JSON.stringify({ type: 'shutdown' })}\n`,
    encoding: 'utf8',
    timeout: 15_000,
  });
  if (handshakeRun.error) throw handshakeRun.error;
  if (handshakeRun.status !== 0) {
    throw new Error(`Production bundled sidecar failed: ${handshakeRun.stderr || handshakeRun.stdout}`);
  }
  const firstLine = handshakeRun.stdout.trim().split('\n')[0] ?? '';
  const handshake = JSON.parse(firstLine);
  if (handshake?.type !== 'ready' || handshake.protocolVersion !== 1 || handshake.piCoreVersion !== expectedPiCoreVersion) {
    throw new Error(`Unexpected production sidecar handshake: ${firstLine}`);
  }
}

function verifyAppDataSecurity(appDataDir) {
  if (!existsSync(appDataDir)) throw new Error(`Expected Picket data directory missing after UI smoke: ${appDataDir}`);
  const dirMode = statSync(appDataDir).mode & 0o777;
  if (dirMode !== 0o700) throw new Error(`Picket data directory does not have strict 0700 mode (got ${dirMode.toString(8)})`);

  for (const filename of ['workbench-state.json', 'models.json', 'action-sessions.json']) {
    const full = join(appDataDir, filename);
    if (!existsSync(full)) throw new Error(`Expected persisted file missing after UI smoke: ${filename}`);
    const mode = statSync(full).mode & 0o777;
    if (mode !== 0o600) throw new Error(`Persisted file ${filename} does not have strict 0600 mode (got ${mode.toString(8)})`);
    const raw = readFileSync(full, 'utf8');
    for (const forbidden of ['apiKey', 'smoke-token-', 'commitToken', 'branchToken', 'ghp_', 'glpat-']) {
      if (raw.includes(forbidden)) {
        throw new Error(`Persisted file ${filename} leaked sensitive token/secret marker: ${forbidden}`);
      }
    }
  }

  const legacyCPushSessionsFile = join(appDataDir, 'cpush-sessions.json');
  if (existsSync(legacyCPushSessionsFile)) {
    throw new Error('Legacy cpush-sessions.json should not be created on fresh runs (unified into action-sessions.json)');
  }

  const skillsManifestFile = join(appDataDir, 'skills', 'skills-manifest.json');
  if (existsSync(skillsManifestFile)) {
    const mode = statSync(skillsManifestFile).mode & 0o777;
    if (mode !== 0o600) throw new Error(`Persisted file skills/skills-manifest.json does not have strict 0600 mode (got ${mode.toString(8)})`);
    const raw = readFileSync(skillsManifestFile, 'utf8');
    for (const forbidden of ['apiKey', 'smoke-token-', 'commitToken', 'branchToken', 'ghp_', 'glpat-']) {
      if (raw.includes(forbidden)) {
        throw new Error(`Persisted file skills/skills-manifest.json leaked sensitive token/secret marker: ${forbidden}`);
      }
    }
  }

  const credentialsFile = join(appDataDir, 'credentials.json');
  if (!existsSync(credentialsFile)) throw new Error('Expected persisted credentials.json missing after UI smoke');
  const credMode = statSync(credentialsFile).mode & 0o777;
  if (credMode !== 0o600) throw new Error(`Persisted file credentials.json does not have strict 0600 mode (got ${credMode.toString(8)})`);
  const credsRaw = readFileSync(credentialsFile, 'utf8');
  for (const forbidden of ['commitToken', 'branchToken']) {
    if (credsRaw.includes(forbidden)) {
      throw new Error(`Persisted file credentials.json leaked sensitive token marker: ${forbidden}`);
    }
  }
}

try {
  // 1. Verify the production release bundle has zero demo/secret leaks and zero smoke backdoors.
  const releaseBundle = join(root, 'src-tauri/target/release/bundle/macos/Picket.app');
  const copiedRelease = spawnSync('/usr/bin/ditto', [releaseBundle, releaseDestination], { encoding: 'utf8' });
  if (copiedRelease.status !== 0) {
    throw new Error(copiedRelease.stderr || 'Build the release application with pnpm desktop build first');
  }
  verifyBundleNoDemoOrSecretLeaks(releaseDestination, join(root, 'dist'));
  verifyReleaseBundleNoSmokeBackdoors(releaseDestination, join(root, 'dist'));

  // 2. Build (unless --skip-smoke-build) and verify the isolated smoke-harness bundle.
  if (!process.argv.includes('--skip-smoke-build')) {
    const smokeBuild = spawnSync(process.execPath, [join(root, 'scripts/desktop.mjs'), 'build:smoke'], {
      cwd: root,
      stdio: 'inherit',
    });
    if (smokeBuild.error) throw smokeBuild.error;
    if (smokeBuild.status !== 0) {
      throw new Error(`Isolated smoke-harness build failed with status ${smokeBuild.status}`);
    }
  }
  const smokeBundle = join(root, 'src-tauri/target-smoke/release/bundle/macos/Picket.app');
  const copied = spawnSync('/usr/bin/ditto', [smokeBundle, destination], { encoding: 'utf8' });
  if (copied.status !== 0) {
    throw new Error(copied.stderr || 'Build the isolated smoke bundle with pnpm desktop build:smoke first');
  }

  const binary = join(destination, 'Contents/MacOS/Picket');
  const executable = existsSync(binary) ? binary : join(destination, 'Contents/MacOS/picket');
  const launch = (...flags) => {
    const result = spawnSync(executable, flags, {
      cwd: temporary, env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', HOME: home, TMPDIR: temporary },
      encoding: 'utf8', timeout: 90_000,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(result.stderr ? `${result.stderr}\n${result.stdout}` : `Application exited with ${result.status}: ${result.stdout}`);
    return result;
  };
  const report = JSON.parse(launch('--foundation-smoke').stdout);
  for (const [phase, expected] of [['complete', 'completed'], ['cancelled', 'cancelled'], ['shutdown', 'cancelled']]) {
    const probe = report[phase];
    if (probe.status !== expected || probe.childExitCode !== 0 || probe.piCoreVersion !== expectedPiCoreVersion) {
      throw new Error(`Incorrect packaged Pi result: ${phase}`);
    }
    assertGone(probe.childPid);
  }
  if (!report.git.temporaryRepositoryRemoved || report.git.commits.length !== 2 || report.git.cases.length !== 6) {
    throw new Error('Packaged Git scope proof failed');
  }
  let ui;
  if (process.argv.includes('--ui')) {
    const uiRun = launch('--foundation-ui-smoke', '--keep-smoke-repo-once');
    const events = uiRun.stdout.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line));
    const exit = events.find((event) => event.type === 'uiSmokeExit');
    if (exit?.status !== 'passed') {
      throw new Error(`UI smoke failed: ${uiRun.stderr || JSON.stringify(events)}`);
    }
    const children = events.filter((event) => event.type === 'uiSmokeChild');
    const branches = events.find((event) => event.type === 'uiSmokeBranchProof');
    if (!branches?.branchSwitchVerified || !branches.worktreeSwitchVerified || !branches.stagingAndDraftsPreserved) {
      throw new Error('Real Git branch/worktree switching and draft preservation did not pass');
    }
    const commit = events.find((event) => event.type === 'uiSmokeCommitProof');
    if (!commit?.localCommitVerified || !commit.partialStagingCommittedOnlyStaged || !commit.unstagedAndUnselectedPreserved) {
      throw new Error('Real Git local commit and partial/unselected preservation did not pass');
    }
    const ideAndPersistence = events.find((event) => event.type === 'uiSmokeIdeAndPersistenceProof');
    if (!ideAndPersistence?.ideStagedDiffVerified || !ideAndPersistence.persistenceRestoreVerified) {
      throw new Error('IDE staged diff export or workbench persistence cold restore did not pass');
    }
    const modelM3 = events.find((event) => event.type === 'uiSmokeModelProof');
    if (!modelM3?.credentialsPersistedAndRestored || !modelM3.partialScopeCPushPlanVerified) {
      throw new Error('M3 local credentials persistence or scoped C-Push planning did not pass');
    }
    const cpushM4 = events.find((event) => event.type === 'uiSmokeCPushProof');
    if (!cpushM4?.splitCommitsVerified || !cpushM4.unselectedStagedPreserved || !cpushM4.sessionPersistedAndRestored) {
      throw new Error('M4 C-Push split commits, unselected staged preservation, or session persistence did not pass');
    }
    const syncM5 = events.find((event) => event.type === 'uiSmokeM5SyncProof');
    if (!syncM5?.remotePushVerified || !syncM5.conflictAgentMergeVerified || !syncM5.disjointDirtyPreservedAcrossRebase) {
      throw new Error('M5 remote push, Agent conflict merge, or disjoint dirty-worktree preservation did not pass');
    }
    if (children.length !== 2) throw new Error('WebView IPC / close-window verification did not pass');
    for (const child of children) assertGone(child.pid);

    verifyAppDataSecurity(join(home, '.picket'));

    // Launch 2: Brand-new OS process against the same HOME verifying real cross-process cold restart:
    const restartRun = launch('--foundation-restart-smoke');
    const restartEvents = restartRun.stdout.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line));
    const restartExit = restartEvents.find((event) => event.type === 'uiSmokeExit');
    const restartProof = restartEvents.find((event) => event.type === 'uiSmokeRestartProof');
    if (restartExit?.status !== 'passed'
      || !restartProof?.coldProcessRepositoryHydrated
      || !restartProof.coldProcessModelCatalogHydrated
      || !restartProof.coldProcessSessionHistoryHydrated
      || !restartProof.coldProcessCompactWindowHydrated) {
      throw new Error(`Cold OS process restart verification failed: ${restartRun.stderr || JSON.stringify(restartEvents)}`);
    }

    ui = {
      workbenchEmptyStateVerified: true,
      nativeTitlebarOverlayVerified: true,
      overlayHeadingClearanceVerified: true,
      windowPinVerified: true,
      realRepositoryReadVerified: true,
      selectionLeavesIndexUntouched: true,
      worktreeDraftIsolationVerified: true,
      realBranchSwitchVerified: true,
      realWorktreeSwitchVerified: true,
      branchStagingAndDraftsPreserved: true,
      realLocalCommitVerified: true,
      partialStagingCommittedOnlyStaged: true,
      unstagedAndUnselectedPreservedAfterCommit: true,
      ideStagedDiffExportVerified: true,
      workbenchPersistenceColdRestoreVerified: true,
      localCredentialsPersistedAndRestoredVerified: true,
      piScopedCPushPlanVerified: true,
      cPushSplitAndSessionVerified: true,
      m5RemotePushAndConflictMergeVerified: true,
      coldProcessRestartAndLeakScanVerified: true,
      webviewIpcVerified: true,
      windowClosedDuringStream: true,
      allPiProcessesReaped: true,
    };
  }
  console.log(JSON.stringify({
    packageIsolatedFromProject: true,
    bundleFreeOfDemoAndSecrets: true,
    releaseBundleFreeOfSmokeHarness: true,
    globalNodeNotRequired: true,
    allPiProcessesReaped: true,
    report,
    ui,
  }, null, 2));
} finally {
  rmSync(temporary, { recursive: true, force: true, maxRetries: 10, retryDelay: 150 });
}
