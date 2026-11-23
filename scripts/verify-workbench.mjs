import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const chromePath = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
if (!existsSync(chromePath)) throw new Error('需要本机 Chrome。可用 CHROME_PATH 指定可执行文件；此检查不安装浏览器。');
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
const stop = async (process) => {
  if (!process || process.exitCode !== null) return;
  const exited = new Promise((resolve) => process.once('exit', resolve));
  process.kill('SIGTERM'); await exited;
};
try {
  let ready = false;
  for (let attempt = 0; attempt < 60; attempt++) {
    if (serverError) throw serverError;
    if (server.exitCode !== null) throw new Error(`开发服务退出：${serverOutput}`);
    try { ready = (await fetch(`${origin}/dev.html`)).ok; } catch { /* Server is still starting. */ }
    if (ready) break;
    await pause();
  }
  if (!ready) throw new Error(`开发服务未就绪：${serverOutput}`);
  chrome = spawn(chromePath, ['--headless', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    `--user-data-dir=${join(temporary, 'chrome')}`, '--remote-debugging-port=0', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
  let chromeOutput = '';
  chrome.stderr.on('data', (chunk) => { chromeOutput += chunk; });
  chrome.on('error', (error) => { chromeOutput += error.message; });
  const portFile = join(temporary, 'chrome/DevToolsActivePort');
  for (let attempt = 0; attempt < 100 && !existsSync(portFile); attempt++) {
    if (chrome.exitCode !== null) throw new Error(`Chrome 退出：${chromeOutput}`);
    await pause();
  }
  if (!existsSync(portFile)) throw new Error(`Chrome 未就绪：${chromeOutput}`);
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
    const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`Chrome 命令超时：${method}`)); }, 30_000);
    pending.set(id, { resolve, reject, timeout }); socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.text);
    return result.result.value;
  };
  const navigate = async (query) => {
    const loaded = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { events.delete('Page.loadEventFired'); reject(new Error('开发页面加载超时')); }, 20_000);
      events.set('Page.loadEventFired', () => { clearTimeout(timeout); resolve(); });
    });
    await send('Page.navigate', { url: `${origin}/dev.html?${query}` }); await loaded;
  };
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 400, height: 700, deviceScaleFactor: 1, mobile: false });
  await navigate('verify=1');
  const result = await evaluate(`new Promise(resolve => {
    const inspect = () => { const result = document.querySelector('[data-workbench-result]'); if (result) { resolve({ status: result.dataset.workbenchResult, text: result.textContent }); return true; } };
    if (!inspect()) { const observer = new MutationObserver(() => { if (inspect()) observer.disconnect(); }); observer.observe(document.body, { childList: true, subtree: true }); }
  })`);
  if (result.status !== 'passed') throw new Error(result.text);
  if (browserErrors.length) throw new Error(`浏览器控制台错误：${browserErrors.join('\n')}`);
  console.log(result.text);
  const { build } = await import('vite');
  const production = await build({ root, logLevel: 'warn', build: { write: false } });
  const builds = Array.isArray(production) ? production : [production];
  const output = builds.flatMap((result) => result.output);
  const modules = output.filter((entry) => entry.type === 'chunk').flatMap((chunk) => chunk.moduleIds);
  assert(!modules.some((path) => /\/testing\/|\.test\.|\/Foundation\.tsx/.test(path)), '开发样例或诊断页进入正式构建');
  assert(!output.some((entry) => entry.fileName === 'dev.html'), '开发入口进入正式构建');
  console.log(JSON.stringify({ productionModules: modules.length, developmentFixturesExcluded: true, productionBuildWritten: false }));
  const screenshotOption = process.argv.find((argument) => argument.startsWith('--screenshots='));
  if (screenshotOption) {
    const destination = resolve(screenshotOption.slice('--screenshots='.length)); mkdirSync(destination, { recursive: true });
    for (const scenario of ['empty', 'running', 'question', 'typing', 'done', 'review-during-question', 'push-failed', 'conflict', 'manual', 'resolved', 'no-model', 'single', 'long-paths']) {
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
    await navigate('scenario=no-model');
    await evaluate(`document.querySelector('button[aria-label="设置"]').click(); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
    detail = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(destination, 'settings.png'), Buffer.from(detail.data, 'base64'));
    await evaluate(`[...document.querySelectorAll('button')].find(button => button.textContent === '添加第一个模型').click(); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
    detail = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(destination, 'add-model.png'), Buffer.from(detail.data, 'base64'));
    if (browserErrors.length) throw new Error(`截图页面控制台错误：${browserErrors.join('\n')}`);
    console.log(`400 × 700 开发样例截图：${destination}`);
  }
} finally {
  socket?.close(); await stop(chrome); await stop(server);
  rmSync(temporary, { recursive: true, force: true });
}
