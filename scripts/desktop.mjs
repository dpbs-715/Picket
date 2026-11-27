import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, delimiter, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const env = { ...process.env, PATH: `${join(homedir(), '.cargo/bin')}${delimiter}${process.env.PATH ?? ''}` };
const run = (command, args, cwd = root, extraEnv = undefined) => {
  const result = spawnSync(command, args, {
    cwd,
    env: extraEnv ? { ...env, ...extraEnv } : env,
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};
const action = process.argv[2];
const extras = process.argv.slice(3);
if (!['dev', 'build', 'build:smoke', 'test', 'check', 'fmt', 'smoke'].includes(action)) {
  throw new Error('Usage: pnpm desktop <dev|build|build:smoke|test|check|fmt|smoke>');
}
if (action !== 'fmt') run(process.execPath, ['scripts/prepare-sidecar.mjs']);
const native = join(root, 'src-tauri');
if (action === 'fmt') run('cargo', ['fmt', ...extras], native);
else if (action === 'test') run('cargo', ['test', ...extras], native);
else if (action === 'check') {
  run('cargo', ['fmt', '--check'], native);
  run('cargo', ['clippy', '--all-targets', '--', '-D', 'warnings'], native);
  run('cargo', ['clippy', '--all-targets', '--features', 'smoke-harness', '--', '-D', 'warnings'], native);
  run('cargo', ['test'], native);
} else if (action === 'smoke') {
  run('cargo', ['run', '--features', 'smoke-harness', '--', '--foundation-smoke'], native);
} else if (action === 'build:smoke') {
  const cli = join(root, 'node_modules/.bin/tauri');
  if (!existsSync(cli)) throw new Error('Install project dependencies with pnpm install first');
  // Build the smoke-harness bundle into isolated dist-smoke and target-smoke directories
  // so release artifacts in dist/ and src-tauri/target/release/ are never contaminated.
  const smokeConfig = JSON.stringify({ build: { frontendDist: '../dist-smoke' } });
  run(
    cli,
    ['build', '--features', 'smoke-harness', '--config', smokeConfig, ...extras],
    root,
    {
      VITE_PICKET_SMOKE: '1',
      PICKET_DIST_DIR: 'dist-smoke',
      CARGO_TARGET_DIR: join(native, 'target-smoke'),
    },
  );
} else {
  const cli = join(root, 'node_modules/.bin/tauri');
  if (!existsSync(cli)) throw new Error('Install project dependencies with pnpm install first');
  run(cli, [action, ...extras]);
}
