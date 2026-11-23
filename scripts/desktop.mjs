import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, delimiter, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const env = { ...process.env, PATH: `${join(homedir(), '.cargo/bin')}${delimiter}${process.env.PATH ?? ''}` };
const run = (command, args, cwd = root) => {
  const result = spawnSync(command, args, { cwd, env, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};
const action = process.argv[2];
const extras = process.argv.slice(3);
if (!['dev', 'build', 'test', 'check', 'fmt', 'smoke'].includes(action)) {
  throw new Error('Usage: pnpm desktop <dev|build|test|check|fmt|smoke>');
}
if (action !== 'fmt') run(process.execPath, ['scripts/prepare-sidecar.mjs']);
const native = join(root, 'src-tauri');
if (action === 'fmt') run('cargo', ['fmt', ...extras], native);
else if (action === 'test') run('cargo', ['test', ...extras], native);
else if (action === 'check') {
  run('cargo', ['fmt', '--check'], native);
  run('cargo', ['clippy', '--all-targets', '--', '-D', 'warnings'], native);
  run('cargo', ['test'], native);
} else if (action === 'smoke') run('cargo', ['run', '--', '--foundation-smoke'], native);
else {
  const cli = join(root, 'node_modules/.bin/tauri');
  if (!existsSync(cli)) throw new Error('Install project dependencies with pnpm install first');
  run(cli, [action, ...extras]);
}
