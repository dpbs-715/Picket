import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const defaultAppPath = join(root, 'src-tauri/target/release/bundle/macos/Picket.app');
const defaultOutDir = join(root, 'src-tauri/target/release/bundle/dmg');
const nodeEntitlementsPath = join(root, 'src-tauri/entitlements/picket-node.plist');

function parseArgs(argv) {
  const options = {
    checkVersionOnly: false,
    tag: null,
    appPath: defaultAppPath,
    outDir: defaultOutDir,
    repo: process.env.GITHUB_REPOSITORY || 'dpbs-715/Picket',
    notes: '',
    notesFile: null,
    requireUpdaterSignature: false,
  };
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index];
    if (arg === '--check-version-only') {
      options.checkVersionOnly = true;
    } else if (arg === '--tag') {
      options.tag = argv[++index] ?? '';
      if (!options.tag) throw new Error('--tag requires a value (e.g. v0.1.0)');
    } else if (arg === '--app') {
      options.appPath = resolve(argv[++index] ?? '');
    } else if (arg === '--out-dir') {
      options.outDir = resolve(argv[++index] ?? '');
    } else if (arg === '--repo') {
      options.repo = argv[++index] ?? '';
      if (!options.repo) throw new Error('--repo requires a value (e.g. owner/repo)');
    } else if (arg === '--notes') {
      options.notes = argv[++index] ?? '';
    } else if (arg === '--notes-file') {
      options.notesFile = resolve(argv[++index] ?? '');
    } else if (arg === '--require-updater-signature') {
      options.requireUpdaterSignature = true;
    } else if (arg === '--') {
      continue;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  return options;
}

function runCommand(command, args, opts = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    ...opts,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const details = [result.stderr, result.stdout].filter(Boolean).join('\n').trim();
    throw new Error(`Command failed (${command} ${args.join(' ')}):\n${details}`);
  }
  return result;
}

function readCargoTomlVersion(tomlPath) {
  const raw = readFileSync(tomlPath, 'utf8');
  const packageSection = raw.match(/\[package\][\s\S]*?(?=\n\[|$)/);
  if (!packageSection) throw new Error(`Missing [package] section in ${tomlPath}`);
  const match = packageSection[0].match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error(`Missing package version in ${tomlPath}`);
  return match[1];
}

function readCargoLockVersion(lockPath, packageName) {
  const raw = readFileSync(lockPath, 'utf8');
  const blocks = raw.split(/\n\[\[package\]\]\n/);
  for (const block of blocks.slice(1)) {
    const nameMatch = block.match(/^name\s*=\s*"([^"]+)"/m);
    if (nameMatch?.[1] !== packageName) continue;
    const versionMatch = block.match(/^version\s*=\s*"([^"]+)"/m);
    if (!versionMatch) throw new Error(`Missing version for ${packageName} in ${lockPath}`);
    return versionMatch[1];
  }
  throw new Error(`Package ${packageName} not found in ${lockPath}`);
}

function verifyVersions(tag) {
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const tauriConf = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  const cargoTomlVersion = readCargoTomlVersion(join(root, 'src-tauri/Cargo.toml'));
  const cargoLockVersion = readCargoLockVersion(join(root, 'src-tauri/Cargo.lock'), 'picket');

  const versions = {
    'package.json': pkg.version,
    'src-tauri/tauri.conf.json': tauriConf.version,
    'src-tauri/Cargo.toml': cargoTomlVersion,
    'src-tauri/Cargo.lock': cargoLockVersion,
  };

  const unique = new Set(Object.values(versions));
  if (unique.size !== 1 || !pkg.version) {
    throw new Error(`Version mismatch across manifests:\n${JSON.stringify(versions, null, 2)}`);
  }

  if (tag !== null) {
    if (!tag.startsWith('v')) {
      throw new Error(`Release tag must start with "v" (got "${tag}")`);
    }
    const tagVersion = tag.slice(1);
    if (tagVersion !== pkg.version) {
      throw new Error(`Release tag "${tag}" does not match repository version "${pkg.version}"`);
    }
  }

  const minimumSystemVersion = tauriConf.bundle?.macOS?.minimumSystemVersion ?? '14.0';
  return { version: pkg.version, minimumSystemVersion, versions };
}

function compareSemverPair(left, right) {
  const a = left.split('.').map((n) => Number.parseInt(n, 10));
  const b = right.split('.').map((n) => Number.parseInt(n, 10));
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

function inspectMachOBinary(binaryPath, maxMinOs) {
  const fileInfo = runCommand('/usr/bin/file', [binaryPath]).stdout.trim();
  if (!fileInfo.includes('arm64')) {
    throw new Error(`Binary is not arm64 Mach-O (${binaryPath}): ${fileInfo}`);
  }

  const depsOutput = runCommand('/usr/bin/otool', ['-L', binaryPath]).stdout;
  const linkedLibs = depsOutput
    .split('\n')
    .slice(1)
    .map((line) => line.trim().split(' (')[0])
    .filter(Boolean);

  const nonSystemLibs = linkedLibs.filter(
    (lib) => !lib.startsWith('/System/Library/') && !lib.startsWith('/usr/lib/'),
  );
  if (nonSystemLibs.length > 0) {
    throw new Error(
      `Binary links non-system dynamic libraries (${binaryPath}). Ensure official arm64 Node and clean toolchain are used:\n${nonSystemLibs.join('\n')}`,
    );
  }

  const loadOutput = runCommand('/usr/bin/otool', ['-l', binaryPath]).stdout;
  const minOsMatch = loadOutput.match(/cmd LC_BUILD_VERSION[\s\S]*?minos\s+([0-9.]+)/);
  if (!minOsMatch) {
    throw new Error(`Unable to determine LC_BUILD_VERSION minos for ${binaryPath}`);
  }
  const minOs = minOsMatch[1];
  if (compareSemverPair(minOs, maxMinOs) > 0) {
    throw new Error(
      `Binary minos ${minOs} exceeds configured macOS minimumSystemVersion ${maxMinOs} (${binaryPath})`,
    );
  }

  return { minOs, linkedCount: linkedLibs.length };
}

function smokeTestStagedApp(stagedAppPath, scratchRoot) {
  const mainBinary = existsSync(join(stagedAppPath, 'Contents/MacOS/Picket'))
    ? join(stagedAppPath, 'Contents/MacOS/Picket')
    : join(stagedAppPath, 'Contents/MacOS/picket');
  const emptyHome = join(scratchRoot, 'smoke-home');
  mkdirSync(emptyHome, { recursive: true });

  const result = runCommand(mainBinary, ['--foundation-smoke'], {
    cwd: scratchRoot,
    env: {
      PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
      HOME: emptyHome,
      TMPDIR: scratchRoot,
    },
    timeout: 45_000,
  });

  const report = JSON.parse(result.stdout);
  for (const [phase, expected] of [
    ['complete', 'completed'],
    ['cancelled', 'cancelled'],
    ['shutdown', 'cancelled'],
  ]) {
    if (report[phase]?.status !== expected || report[phase]?.childExitCode !== 0) {
      throw new Error(`Signed bundle smoke test failed for phase "${phase}": ${result.stdout}`);
    }
  }
  if (!report.git?.temporaryRepositoryRemoved || report.git?.commits?.length !== 2) {
    throw new Error(`Signed bundle Git scope smoke test failed: ${result.stdout}`);
  }
  return {
    nodeVersion: report.complete.nodeVersion,
    piCoreVersion: report.complete.piCoreVersion,
  };
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const { version, minimumSystemVersion, versions } = verifyVersions(options.tag);

  if (options.checkVersionOnly) {
    console.log(JSON.stringify({ versionVerified: true, version, tag: options.tag, versions }, null, 2));
    return;
  }

  if (process.platform !== 'darwin' || process.arch !== 'arm64') {
    throw new Error('DMG packaging requires macOS Apple Silicon (arm64).');
  }
  if (!existsSync(options.appPath)) {
    throw new Error(`App bundle not found at ${options.appPath}. Run "pnpm desktop build" first.`);
  }
  if (!existsSync(nodeEntitlementsPath)) {
    throw new Error(`Missing sidecar entitlements file at ${nodeEntitlementsPath}`);
  }

  const scratchRoot = realpathSync(mkdtempSync(join(tmpdir(), 'picket-dmg-')));
  try {
    const stagingDir = join(scratchRoot, 'staging');
    const stagedApp = join(stagingDir, 'Picket.app');
    mkdirSync(stagingDir, { recursive: true });

    // Preserve permissions, symlinks, and bundle metadata verbatim.
    runCommand('/usr/bin/ditto', [options.appPath, stagedApp]);

    const mainBinary = existsSync(join(stagedApp, 'Contents/MacOS/Picket'))
      ? join(stagedApp, 'Contents/MacOS/Picket')
      : join(stagedApp, 'Contents/MacOS/picket');
    const nodeBinary = join(stagedApp, 'Contents/MacOS/picket-node');
    const sidecarScript = join(stagedApp, 'Contents/Resources/sidecar/pi-sidecar.mjs');
    const infoPlist = join(stagedApp, 'Contents/Info.plist');

    for (const requiredPath of [mainBinary, nodeBinary, sidecarScript, infoPlist]) {
      if (!existsSync(requiredPath)) {
        throw new Error(`Expected packaged file is missing: ${requiredPath}`);
      }
    }

    const mainMachO = inspectMachOBinary(mainBinary, minimumSystemVersion);
    const nodeMachO = inspectMachOBinary(nodeBinary, minimumSystemVersion);

    // Internal test build: apply clean ad-hoc signatures (with V8 JIT entitlements for picket-node,
    // without get-task-allow) so the bundle passes strict macOS signature verification on disk.
    runCommand('/usr/bin/codesign', [
      '--force',
      '--sign',
      '-',
      '--entitlements',
      nodeEntitlementsPath,
      nodeBinary,
    ]);
    runCommand('/usr/bin/codesign', ['--force', '--sign', '-', stagedApp]);
    runCommand('/usr/bin/codesign', ['--verify', '--deep', '--strict', stagedApp]);

    // Execute smoke test directly against the freshly signed staged bundle.
    const smoke = smokeTestStagedApp(stagedApp, scratchRoot);

    mkdirSync(options.outDir, { recursive: true });

    // Package the exact same verified & signed Picket.app into an updater tarball (single top-level `Picket.app/`
    // directory, no AppleDouble `._*` files) so DMG and in-app updates ship byte-identical binaries.
    const tarFilename = `Picket-v${version}-macOS-arm64.app.tar.gz`;
    const tarPath = join(options.outDir, tarFilename);
    const tarSha256Path = `${tarPath}.sha256`;
    runCommand('/usr/bin/tar', ['-czf', tarPath, '-C', stagingDir, 'Picket.app'], {
      env: { ...process.env, COPYFILE_DISABLE: '1' },
    });
    const tarListing = runCommand('/usr/bin/tar', ['-tzf', tarPath]).stdout.split('\n').filter(Boolean);
    if (!tarListing.length || !tarListing.every((entry) => entry === 'Picket.app/' || entry.startsWith('Picket.app/'))) {
      throw new Error(`Updater tarball must contain a single top-level Picket.app/ directory (first entry: ${tarListing[0] ?? 'empty'})`);
    }
    const tarSha256 = createHash('sha256').update(readFileSync(tarPath)).digest('hex');
    writeFileSync(tarSha256Path, `${tarSha256}  ${tarFilename}\n`, 'utf8');

    const hasSigningKey = Boolean(
      process.env.TAURI_SIGNING_PRIVATE_KEY?.trim()
      || process.env.TAURI_SIGNING_PRIVATE_KEY_PATH?.trim(),
    );
    let updaterSigned = false;
    let sigFilename = null;
    let latestJsonPath = null;
    if (hasSigningKey) {
      const tauriCliBin = join(root, 'node_modules/.bin/tauri');
      if (!existsSync(tauriCliBin)) {
        throw new Error(`Tauri CLI binary not found at ${tauriCliBin}; run pnpm install first.`);
      }
      runCommand(tauriCliBin, ['signer', 'sign', '--app-version', version, tarPath], {
        cwd: root,
        env: process.env,
      });
      const sigPath = `${tarPath}.sig`;
      if (!existsSync(sigPath)) {
        throw new Error(`Expected updater signature missing at ${sigPath}`);
      }
      const signature = readFileSync(sigPath, 'utf8').trim();
      const releaseNotes = options.notesFile
        ? readFileSync(options.notesFile, 'utf8').trim()
        : options.notes.trim() || `Picket v${version} (macOS Apple Silicon)`;
      const releaseTag = options.tag ?? `v${version}`;
      const manifest = {
        version,
        notes: releaseNotes,
        pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
        platforms: {
          'darwin-aarch64': {
            signature,
            url: `https://github.com/${options.repo}/releases/download/${releaseTag}/${tarFilename}`,
          },
        },
      };
      latestJsonPath = join(options.outDir, 'latest.json');
      writeFileSync(latestJsonPath, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
      sigFilename = `${tarFilename}.sig`;
      updaterSigned = true;
    } else if (options.requireUpdaterSignature) {
      throw new Error('TAURI_SIGNING_PRIVATE_KEY (or TAURI_SIGNING_PRIVATE_KEY_PATH) is required when --require-updater-signature is set.');
    }

    symlinkSync('/Applications', join(stagingDir, 'Applications'));

    const dmgFilename = `Picket-v${version}-macOS-arm64.dmg`;
    const dmgPath = join(options.outDir, dmgFilename);
    const sha256Path = `${dmgPath}.sha256`;

    runCommand('/usr/bin/hdiutil', [
      'create',
      '-volname',
      `Picket v${version}`,
      '-srcfolder',
      stagingDir,
      '-ov',
      '-format',
      'UDZO',
      '-imagekey',
      'zlib-level=9',
      dmgPath,
    ]);
    runCommand('/usr/bin/hdiutil', ['verify', dmgPath]);

    const dmgBytes = readFileSync(dmgPath);
    const sha256 = createHash('sha256').update(dmgBytes).digest('hex');
    writeFileSync(sha256Path, `${sha256}  ${dmgFilename}\n`, 'utf8');

    console.log(
      JSON.stringify(
        {
          version,
          signingMode: 'ad-hoc-internal',
          dmgPath,
          dmgFilename,
          sha256Path,
          sha256,
          sizeBytes: statSync(dmgPath).size,
          updaterTarballPath: tarPath,
          updaterTarballFilename: tarFilename,
          updaterTarballSha256: tarSha256,
          updaterSigned,
          updaterSignatureFilename: sigFilename,
          latestJsonPath,
          minimumSystemVersion,
          mainBinaryMinOs: mainMachO.minOs,
          nodeBinaryMinOs: nodeMachO.minOs,
          bundledNodeVersion: smoke.nodeVersion,
          bundledPiCoreVersion: smoke.piCoreVersion,
        },
        null,
        2,
      ),
    );
  } finally {
    rmSync(scratchRoot, { recursive: true, force: true, maxRetries: 10, retryDelay: 150 });
  }
}

main();
