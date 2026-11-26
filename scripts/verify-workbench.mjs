import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const chromePath = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
if (!existsSync(chromePath)) throw new Error('Local Chrome is required. Set CHROME_PATH to specify the binary; this check never installs a browser.');
const temporary = mkdtempSync(join(tmpdir(), 'picket-workbench-'));
const origin = 'http://127.0.0.1:1421';
const pause = () => new Promise((resolve) => setTimeout(resolve, 100));
const server = spawn(process.execPath, [join(root, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', '1421', '--strictPort'], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
let serverOutput = '';
server.stdout.on('data', (chunk) => { serverOutput += chunk; });
server.stderr.on('data', (chunk) => { serverOutput += chunk; });
let serverError;
server.on('error', (error) => { serverError = error; });
let chrome, socket;
const stop = async (child) => {
  if (!child || child.exitCode !== null) return;
  const exited = new Promise((resolve) => child.once('exit', resolve));
  child.kill('SIGTERM');
  const timer = setTimeout(() => {
    if (child.exitCode === null) child.kill('SIGKILL');
  }, 5_000);
  await exited;
  clearTimeout(timer);
};
try {
  let ready = false;
  for (let attempt = 0; attempt < 60; attempt++) {
    if (serverError) throw serverError;
    if (server.exitCode !== null) throw new Error(`Dev server exited early: ${serverOutput}`);
    try { ready = (await fetch(`${origin}/dev.html`)).ok; } catch { /* Server is still starting. */ }
    if (ready) break;
    await pause();
  }
  if (!ready) throw new Error(`Dev server did not become ready: ${serverOutput}`);
  chrome = spawn(chromePath, ['--headless', '--disable-gpu', '--disable-crash-reporter', '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    `--user-data-dir=${join(temporary, 'chrome')}`, '--remote-debugging-port=0', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
  let chromeOutput = '';
  chrome.stderr.on('data', (chunk) => { chromeOutput += chunk; });
  chrome.on('error', (error) => { chromeOutput += error.message; });
  const portFile = join(temporary, 'chrome/DevToolsActivePort');
  for (let attempt = 0; attempt < 100 && !existsSync(portFile); attempt++) {
    if (chrome.exitCode !== null) throw new Error(`Chrome exited early: ${chromeOutput}`);
    await pause();
  }
  if (!existsSync(portFile)) throw new Error(`Chrome did not become ready: ${chromeOutput}`);
  const port = readFileSync(portFile, 'utf8').split('\n')[0];
  const page = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
  let sequence = 0;
  const pending = new Map();
  const events = new Map();
  const browserErrors = [];
  socket.addEventListener('message', ({ data }) => {
    const message = JSON.parse(String(data));
    if (message.method === 'Runtime.exceptionThrown') browserErrors.push(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text);
    if (message.method === 'Runtime.consoleAPICalled' && message.params.type === 'error') browserErrors.push(message.params.args.map((argument) => argument.value ?? argument.description ?? '').join(' '));
    if (message.id) {
      const request = pending.get(message.id);
      if (request) { pending.delete(message.id); clearTimeout(request.timeout); message.error ? request.reject(new Error(JSON.stringify(message.error))) : request.resolve(message.result); }
    } else if (events.has(message.method)) { events.get(message.method)(); events.delete(message.method); }
  });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`Chrome CDP command timed out: ${method}`)); }, 30_000);
    pending.set(id, { resolve, reject, timeout }); socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.text);
    return result.result.value;
  };
  const navigate = async (query) => {
    const loaded = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { events.delete('Page.loadEventFired'); reject(new Error('Dev page load timed out')); }, 20_000);
      events.set('Page.loadEventFired', () => { clearTimeout(timeout); resolve(); });
    });
    await send('Page.navigate', { url: `${origin}/dev.html?${query}` }); await loaded;
  };
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Page.bringToFront');
  await send('Emulation.setEmulatedMedia', {
    features: [{ name: 'prefers-reduced-motion', value: 'no-preference' }],
  });
  await send('Emulation.setDeviceMetricsOverride', { width: 400, height: 700, deviceScaleFactor: 1, mobile: false });
  await navigate('verify=1');
  const result = await evaluate(`new Promise(resolve => {
    const inspect = () => { const result = document.querySelector('[data-workbench-result]'); if (result) { resolve({ status: result.dataset.workbenchResult, text: result.textContent }); return true; } };
    if (!inspect()) { const observer = new MutationObserver(() => { if (inspect()) observer.disconnect(); }); observer.observe(document.body, { childList: true, subtree: true }); }
  })`);
  if (result.status !== 'passed') {
    const errSuffix = browserErrors.length ? `\nBrowser console errors:\n${browserErrors.join('\n')}` : '';
    throw new Error(`${result.text}${errSuffix}`);
  }
  if (browserErrors.length) throw new Error(`Browser console errors:\n${browserErrors.join('\n')}`);
  console.log(result.text);

  // Verify 400 x 480 full-mode minimum height and 400 x 140 compact bar viewport geometry:
  await send('Emulation.setDeviceMetricsOverride', { width: 400, height: 480, deviceScaleFactor: 1, mobile: false });
  await navigate('scenario=changed');
  const minHeightCheck = await evaluate(`new Promise(resolve => {
    const inspect = () => {
      const header = document.querySelector('.context-header');
      const changes = document.querySelector('.changes-panel');
      const lower = document.querySelector('.lower-workspace');
      const footer = document.querySelector('.action-footer');
      if (!header || !changes || !lower || !footer) return false;
      requestAnimationFrame(() => {
        const h = header.getBoundingClientRect();
        const c = changes.getBoundingClientRect();
        const l = lower.getBoundingClientRect();
        const f = footer.getBoundingClientRect();
        resolve({
          innerHeight,
          changesHeight: c.height,
          lowerHeight: l.height,
          footerBottom: f.bottom,
          ordered: h.bottom <= c.top + 1 && c.bottom <= l.top + 1 && l.bottom <= f.top + 1 && f.bottom <= 480.5,
        });
      });
      return true;
    };
    if (!inspect()) {
      const observer = new MutationObserver(() => { if (inspect()) observer.disconnect(); });
      observer.observe(document.body, { childList: true, subtree: true });
    }
  })`);
  assert(
    minHeightCheck.innerHeight === 480
      && minHeightCheck.changesHeight === 152
      && minHeightCheck.lowerHeight >= 200
      && minHeightCheck.ordered,
    `400x480 full-mode minimum height layout overlapped or clipped: ${JSON.stringify(minHeightCheck)}`,
  );

  await send('Emulation.setDeviceMetricsOverride', { width: 400, height: 86, deviceScaleFactor: 1, mobile: false });
  await navigate('scenario=compact');
  const compactViewportCheck = await evaluate(`new Promise(resolve => {
    const inspect = () => {
      const bar = document.querySelector('.compact-bar');
      const top = document.querySelector('.compact-top');
      const bottom = document.querySelector('.compact-bottom');
      if (!bar || !top || !bottom) return false;
      requestAnimationFrame(() => {
        const b = bar.getBoundingClientRect();
        const t = top.getBoundingClientRect();
        const f = bottom.getBoundingClientRect();
        resolve({
          barHeight: b.height,
          topHeight: t.height,
          bottomBottom: f.bottom,
        });
      });
      return true;
    };
    if (!inspect()) {
      const observer = new MutationObserver(() => { if (inspect()) observer.disconnect(); });
      observer.observe(document.body, { childList: true, subtree: true });
    }
  })`);
  assert(
    compactViewportCheck.topHeight === 38
      && compactViewportCheck.barHeight >= 82
      && compactViewportCheck.barHeight <= 92
      && compactViewportCheck.bottomBottom <= 92,
    `400x86 compact viewport layout check failed: ${JSON.stringify(compactViewportCheck)}`,
  );
  await send('Emulation.setDeviceMetricsOverride', { width: 400, height: 700, deviceScaleFactor: 1, mobile: false });
  if (browserErrors.length) throw new Error(`Viewport geometry browser console errors:\n${browserErrors.join('\n')}`);
  console.log(JSON.stringify({ minFullViewport400x480Verified: true, compactViewport400x86Verified: true }));
  const { build } = await import('vite');
  const production = await build({ root, logLevel: 'warn', build: { write: false } });
  const builds = Array.isArray(production) ? production : [production];
  const output = builds.flatMap((entry) => entry.output);
  const modules = output.filter((entry) => entry.type === 'chunk').flatMap((chunk) => chunk.moduleIds);
  assert(!modules.some((path) => /\/testing\/|\.test\.|\/Foundation\.tsx/.test(path)), 'Development fixtures or Foundation diagnostic page leaked into production build');
  assert(!output.some((entry) => entry.fileName === 'dev.html'), 'dev.html leaked into production build');
  console.log(JSON.stringify({ productionModules: modules.length, developmentFixturesExcluded: true, productionBuildWritten: false }));
  const screenshotOption = process.argv.find((argument) => argument.startsWith('--screenshots='));
  if (screenshotOption) {
    const destination = resolve(screenshotOption.slice('--screenshots='.length)); mkdirSync(destination, { recursive: true });
    for (const scenario of ['empty', 'clean', 'running', 'generating', 'question', 'typing', 'done', 'review-during-question', 'push-failed', 'conflict', 'manual', 'resolved', 'no-model', 'single', 'long-paths']) {
      await navigate(`scenario=${scenario}`);
      await evaluate(`new Promise(resolve => {
        const inspect = () => { if (document.querySelector('.workbench')) { requestAnimationFrame(() => requestAnimationFrame(resolve)); return true; } };
        if (!inspect()) { const observer = new MutationObserver(() => { if (inspect()) observer.disconnect(); }); observer.observe(document.body, { childList: true, subtree: true }); }
      })`);
      const screenshot = await send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(join(destination, `${scenario}.png`), Buffer.from(screenshot.data, 'base64'));
    }
    await navigate('scenario=review-during-question');
    await evaluate(`document.getElementById('review-tab').click(); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
    let detail = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(destination, 'review.png'), Buffer.from(detail.data, 'base64'));
    await navigate('scenario=typing');
    await evaluate(`document.querySelector('button[aria-label="Settings"]').click(); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
    detail = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(destination, 'settings-models.png'), Buffer.from(detail.data, 'base64'));
    await navigate('scenario=no-model');
    await evaluate(`document.querySelector('button[aria-label="Settings"]').click(); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
    detail = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(destination, 'settings.png'), Buffer.from(detail.data, 'base64'));
    await evaluate(`[...document.querySelectorAll('button')].find(button => button.textContent === 'Add First Model').click(); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
    detail = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(destination, 'add-model.png'), Buffer.from(detail.data, 'base64'));
    if (browserErrors.length) throw new Error(`Screenshot page console errors:\n${browserErrors.join('\n')}`);
    console.log(`400 x 700 fixture screenshots written to: ${destination}`);
  }
} finally {
  if (socket?.readyState === WebSocket.OPEN) {
    try { socket.send(JSON.stringify({ id: 999_999, method: 'Browser.close', params: {} })); } catch { /* ignore */ }
  }
  socket?.close();
  await stop(chrome);
  await stop(server);
  try {
    rmSync(temporary, { recursive: true, force: true, maxRetries: 10, retryDelay: 150 });
  } catch (cleanupError) {
    console.warn(`Warning: failed to remove temporary Chrome profile ${temporary}: ${cleanupError}`);
  }
}
