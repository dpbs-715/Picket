import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const generatedDir = join(root, 'shared/generated');
const mode = process.argv.includes('--sync') ? 'sync' : 'check';

const cargoBin = join(homedir(), '.cargo/bin');
const env = {
  ...process.env,
  PATH: process.env.PATH ? `${cargoBin}:${process.env.PATH}` : cargoBin,
};

function runExport(targetDir) {
  const result = spawnSync(
    'cargo',
    [
      'test',
      '--manifest-path',
      join(root, 'src-tauri/Cargo.toml'),
      'protocol_export::export_ts_protocol_bindings',
      '--',
      '--exact',
      '--nocapture',
    ],
    {
      cwd: root,
      env: { ...env, PICKET_TS_EXPORT_DIR: targetDir },
      stdio: 'inherit',
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function readTsFiles(dir) {
  const map = new Map();
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return map;
  }
  for (const name of entries.filter((f) => f.endsWith('.ts')).sort()) {
    map.set(name, readFileSync(join(dir, name), 'utf8'));
  }
  return map;
}

if (mode === 'sync') {
  runExport(generatedDir);
  const count = readTsFiles(generatedDir).size;
  console.log(`Synced ${count} generated protocol TypeScript files to shared/generated/`);
} else {
  const tmpDir = mkdtempSync(join(tmpdir(), 'picket-ts-rs-'));
  try {
    runExport(tmpDir);
    const expected = readTsFiles(tmpDir);
    const actual = readTsFiles(generatedDir);
    const diffs = [];
    for (const [name, content] of expected) {
      if (!actual.has(name)) {
        diffs.push(`missing: shared/generated/${name}`);
      } else if (actual.get(name) !== content) {
        diffs.push(`outdated: shared/generated/${name}`);
      }
    }
    for (const name of actual.keys()) {
      if (!expected.has(name)) {
        diffs.push(`extra stale file: shared/generated/${name}`);
      }
    }
    if (diffs.length > 0) {
      console.error('Protocol TypeScript bindings are out of sync with Rust types:');
      for (const item of diffs) console.error(`  - ${item}`);
      console.error('Run `pnpm protocol:sync` to regenerate shared/generated/.');
      process.exit(1);
    }
    console.log(`Verified ${expected.size} generated protocol TypeScript files in shared/generated/`);
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
}
