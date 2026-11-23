import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { access, chmod, copyFile, mkdir, readFile, readdir, realpath, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
if (process.platform !== 'darwin' || process.arch !== 'arm64') {
  throw new Error('M0 is verified only for macOS arm64. Prepare an explicit target runtime before building another architecture.');
}
if (process.versions.node !== pkg.engines.node) throw new Error(`Use Node ${pkg.engines.node} to prepare the pinned runtime`);
const triple = 'aarch64-apple-darwin';
const generated = join(root, 'src-tauri/resources/generated');
const binaries = join(root, 'src-tauri/binaries');
await Promise.all([mkdir(generated, { recursive: true }), mkdir(binaries, { recursive: true })]);
const runtimeSource = await realpath(process.execPath);
const runtimeTarget = join(binaries, `picket-node-${triple}`);
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const runtimeHash = sha256(await readFile(runtimeSource));
let existingHash;
try { existingHash = sha256(await readFile(runtimeTarget)); } catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
if (existingHash !== runtimeHash) await copyFile(runtimeSource, runtimeTarget);
await chmod(runtimeTarget, 0o755);
const license = resolve(dirname(runtimeSource), '../LICENSE');
await access(license);
await copyFile(license, join(generated, 'NODE-LICENSE.txt'));
const result = await build({
  absWorkingDir: root, entryPoints: ['agent/src/main.ts'],
  outfile: join(generated, 'pi-sidecar.mjs'), bundle: true, platform: 'node',
  target: 'node24', format: 'esm', legalComments: 'external', metafile: true,
  banner: { js: 'import { createRequire } from "node:module"; const require = createRequire(import.meta.url);' },
});
const packageRoots = new Set();
for (const input of Object.keys(result.metafile.inputs)) {
  const marker = input.lastIndexOf('/node_modules/');
  if (marker < 0) continue;
  const segments = input.slice(marker + '/node_modules/'.length).split('/');
  const name = segments[0].startsWith('@') ? segments.slice(0, 2).join('/') : segments[0];
  packageRoots.add(resolve(root, input.slice(0, marker), 'node_modules', name));
}
const notices = [];
// Pi's npm tarballs omit the monorepo license. This is the v1.6.0 tag's
// immutable source, not a blanket license inference for future dependencies.
const piLicenseSource = 'https://github.com/earendil-works/pi/blob/faac585884ae2c013a6b1400f3e9e77f10b34bac/LICENSE';
const piPackages = new Set(['@earendil-works/pi-agent-core', '@earendil-works/pi-ai', '@earendil-works/pi-schema', '@earendil-works/pi-telemetry']);
const standardWebhooksLicenseSource = 'https://github.com/standard-webhooks/standard-webhooks/blob/daee8284cfe9d4e435ec16e3e68a8f30084d6aaf/libraries/LICENSE';
for (const directory of [...packageRoots].sort()) {
  const dependency = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'));
  const licenses = (await readdir(directory)).filter((name) => /^(license|copying|notice)([._-]|$)/i.test(name));
  let text;
  if (licenses.length > 0) {
    text = (await Promise.all(licenses.sort().map(async (name) =>
      `${name}\n${await readFile(join(directory, name), 'utf8')}`))).join('\n');
  } else if (piPackages.has(dependency.name) && dependency.version === '1.6.0' && dependency.license === 'MIT') {
    text = `${piLicenseSource}\n${await readFile(join(root, 'agent/licenses/pi-1.6.0-LICENSE'), 'utf8')}`;
  } else if (dependency.name === 'standardwebhooks' && dependency.version === '1.3.0' && dependency.license === 'MIT') {
    text = `${standardWebhooksLicenseSource}\n${await readFile(join(root, 'agent/licenses/standardwebhooks-1.3.0-LICENSE'), 'utf8')}`;
  } else throw new Error(`Missing bundled dependency license: ${dependency.name}`);
  notices.push(`${dependency.name}@${dependency.version}\n${text}`);
}
await writeFile(join(generated, 'THIRD-PARTY-NOTICES.txt'), notices.join('\n\n---\n\n') + '\n');
await writeFile(join(generated, 'bundle-inputs.json'), JSON.stringify(Object.keys(result.metafile.inputs), null, 2) + '\n');
await writeFile(join(generated, 'manifest.json'), JSON.stringify({
  protocolVersion: 1, nodeVersion: process.versions.node, target: triple,
  piCoreVersion: pkg.dependencies['@earendil-works/pi-agent-core'],
  runtimeSha256: runtimeHash,
  sidecarSha256: sha256(await readFile(join(generated, 'pi-sidecar.mjs'))),
}, null, 2) + '\n');
console.log(`Prepared Pi Core ${pkg.dependencies['@earendil-works/pi-agent-core']} and Node ${process.versions.node} (${triple})`);
