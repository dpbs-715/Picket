import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { mkdtempSync, rmSync } from 'node:fs';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { afterEach, describe, expect, it } from 'vitest';
import type { PiEvent } from '../../shared/pi-protocol';

const ROOT = process.cwd();
const DEADLINE_MS = 10_000;

interface SidecarHarness {
  child: ChildProcessWithoutNullStreams;
  events: PiEvent[];
  stderr: () => string;
  send(command: unknown): void;
  waitFor(predicate: (event: PiEvent) => boolean): Promise<PiEvent>;
  close(): Promise<number | null>;
}

const activeSidecars: SidecarHarness[] = [];
const activeServers: Server[] = [];

function spawnSidecar(extraEnv: Record<string, string> = {}): SidecarHarness {
  const directory = mkdtempSync(`${tmpdir()}/picket-m3-sidecar-`);
  const child = spawn(
    resolve(ROOT, 'src-tauri/binaries/picket-node-aarch64-apple-darwin'),
    [resolve(ROOT, 'src-tauri/resources/generated/pi-sidecar.mjs')],
    {
      cwd: directory,
      env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', ...extraEnv },
      stdio: ['pipe', 'pipe', 'pipe'],
    },
  );
  const bus = new EventEmitter();
  const events: PiEvent[] = [];
  let stderr = '';
  child.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
  const reader = createInterface({ input: child.stdout });
  reader.on('line', (line) => {
    const event = JSON.parse(line) as PiEvent;
    events.push(event);
    bus.emit('event', event);
  });
  const exited = new Promise<number | null>((yes, no) => {
    child.once('error', no);
    child.once('exit', yes);
  });
  const harness: SidecarHarness = {
    child,
    events,
    stderr: () => stderr,
    send(command: unknown) { child.stdin.write(`${JSON.stringify(command)}\n`); },
    async waitFor(predicate: (event: PiEvent) => boolean): Promise<PiEvent> {
      const existing = events.find(predicate);
      if (existing) return existing;
      return new Promise((yes, no) => {
        const receive = (event: PiEvent) => {
          if (predicate(event)) {
            clearTimeout(timer);
            bus.off('event', receive);
            yes(event);
          }
        };
        const timer = setTimeout(() => {
          bus.off('event', receive);
          no(new Error(`Sidecar event timed out. Events=${JSON.stringify(events)} Stderr=${stderr}`));
        }, DEADLINE_MS);
        bus.on('event', receive);
      });
    },
    async close() {
      if (child.exitCode === null && child.signalCode === null) child.stdin.end();
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        return await Promise.race([
          exited,
          new Promise<never>((_, no) => {
            timer = setTimeout(() => {
              child.kill('SIGKILL');
              no(new Error('Sidecar did not exit after stdin EOF'));
            }, DEADLINE_MS);
          }),
        ]);
      } finally {
        clearTimeout(timer);
        reader.close();
        rmSync(directory, { recursive: true, force: true });
      }
    },
  };
  activeSidecars.push(harness);
  return harness;
}

async function startServer(
  handler: (req: IncomingMessage, body: string, res: ServerResponse) => void | Promise<void>,
): Promise<{ url: string; close: () => Promise<void> }> {
  const server = createServer((req, res) => {
    let body = '';
    req.setEncoding('utf8');
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => { void handler(req, body, res); });
  });
  activeServers.push(server);
  await new Promise<void>((yes) => server.listen(0, '127.0.0.1', yes));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Expected TCP address');
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () => new Promise<void>((yes) => server.close(() => yes())),
  };
}

function writeAnthropicSse(res: ServerResponse, chunks: string[]) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
  const send = (event: string, data: unknown) => { res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`); };
  send('message_start', {
    type: 'message_start',
    message: {
      id: 'msg_test', type: 'message', role: 'assistant', model: 'claude-sonnet-4-20250514',
      content: [], stop_reason: null, stop_sequence: null,
      usage: { input_tokens: 12, output_tokens: 1 },
    },
  });
  send('content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } });
  for (const chunk of chunks) {
    send('content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: chunk } });
  }
  send('content_block_stop', { type: 'content_block_stop', index: 0 });
  send('message_delta', { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 18 } });
  send('message_stop', { type: 'message_stop' });
  res.end();
}

function writeOpenAiResponsesSse(res: ServerResponse, fullText: string) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
  const send = (data: unknown) => { res.write(`data: ${JSON.stringify(data)}\n\n`); };
  send({ type: 'response.created', response: { id: 'resp_1', status: 'in_progress', output: [] } });
  send({
    type: 'response.output_item.added',
    output_index: 0,
    item: { id: 'msg_1', type: 'message', role: 'assistant', status: 'in_progress', content: [] },
  });
  send({
    type: 'response.content_part.added',
    output_index: 0,
    content_index: 0,
    part: { type: 'output_text', text: '', annotations: [] },
  });
  send({ type: 'response.output_text.delta', output_index: 0, content_index: 0, delta: fullText });
  send({ type: 'response.output_text.done', output_index: 0, content_index: 0, text: fullText });
  send({
    type: 'response.output_item.done',
    output_index: 0,
    item: { id: 'msg_1', type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: fullText, annotations: [] }] },
  });
  send({
    type: 'response.completed',
    response: {
      id: 'resp_1', status: 'completed',
      output: [{ id: 'msg_1', type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: fullText, annotations: [] }] }],
      usage: { input_tokens: 10, output_tokens: 8, total_tokens: 18 },
    },
  });
  res.end();
}

function writeOpenAiCompletionsSse(res: ServerResponse, chunks: string[]) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
  for (const chunk of chunks) {
    res.write(`data: ${JSON.stringify({
      id: 'chatcmpl-1', object: 'chat.completion.chunk', created: 1700000000, model: 'test-model',
      choices: [{ index: 0, delta: { content: chunk }, finish_reason: null }],
    })}\n\n`);
  }
  res.write(`data: ${JSON.stringify({
    id: 'chatcmpl-1', object: 'chat.completion.chunk', created: 1700000000, model: 'test-model',
    choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
    usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 },
  })}\n\n`);
  res.write('data: [DONE]\n\n');
  res.end();
}

afterEach(async () => {
  for (const sidecar of activeSidecars.splice(0)) await sidecar.close();
  for (const server of activeServers.splice(0)) await new Promise<void>((yes) => server.close(() => yes()));
});

describe('M3 Pi Core tasks and provider adapters', () => {
  it('connects and streams across anthropic, openai, deepseek, ollama, and custom wire protocols without ambient env leakage', async () => {
    const requests: { path: string; headers: IncomingMessage['headers']; body: string }[] = [];
    const server = await startServer((req, body, res) => {
      const pathname = new URL(req.url ?? '/', 'http://127.0.0.1').pathname;
      requests.push({ path: pathname, headers: req.headers, body });
      if (pathname === '/v1/messages') {
        writeAnthropicSse(res, [
          '<think>分析已暂存改动范围</think>\n```text\nfeat(order): ',
          '补充支付回调幂等校验\n\n- 沿用 Order.ts 已暂存范围更新状态校验\n- 记录锁文件同步范围\n```',
        ]);
        return;
      }
      if (pathname === '/v1/responses') {
        writeOpenAiResponsesSse(res, 'OK');
        return;
      }
      if (pathname === '/v1/chat/completions' || pathname === '/chat/completions') {
        if (body.includes('pre-commit code reviewer')) {
          writeOpenAiCompletionsSse(res, [
            '```json\n{"findings":[',
            '{"fileId":"file-pay","path":"src/api/pay.ts","line":19,"severity":"high","title":"日志包含完整渠道响应","reason":"可能泄露签名密钥"},',
            '{"fileId":"hallucinated","path":"src/not-selected.ts","line":3,"severity":"high","title":"越界项","reason":"不应出现"}',
            ']}\n```',
          ]);
        } else {
          writeOpenAiCompletionsSse(res, ['OK']);
        }
        return;
      }
      res.writeHead(404).end('not found');
    });

    // 1. Anthropic generateCommit (also verifies ambient ANTHROPIC_API_KEY env is ignored when empty key is passed):
    const ambientSidecar = spawnSidecar({ ANTHROPIC_API_KEY: 'sk-ant-ambient-must-never-be-used' });
    await ambientSidecar.waitFor((e) => e.type === 'ready');
    ambientSidecar.send({
      type: 'testConnection',
      requestId: 'ambient-check',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'ambient', scopeVersion: 'v1' },
      model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: server.url, apiKey: '' },
    });
    const ambientRejected = await ambientSidecar.waitFor((e) => e.type === 'failed');
    expect(ambientRejected.type === 'failed' && ambientRejected.message).toContain('API Key');
    expect(requests).toHaveLength(0);
    await ambientSidecar.close();

    const anthropicSidecar = spawnSidecar({ ANTHROPIC_API_KEY: 'sk-ant-ambient-must-never-be-used' });
    await anthropicSidecar.waitFor((e) => e.type === 'ready');
    anthropicSidecar.send({
      type: 'generateCommit',
      requestId: 'gen-1',
      context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'scope-1' },
      model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: server.url, apiKey: 'sk-ant-explicit-key-01' },
      branch: 'feat/pay',
      recentCommits: ['fix(auth): 处理过期令牌'],
      draft: '',
      files: [{ fileId: 'file-order', path: 'src/Order.ts', status: 'M', rangeKind: 'partial', added: 4, removed: 1, patch: '@@ -1 +1,4 @@\n-old\n+new' }],
      skippedPaths: ['pnpm-lock.yaml（锁文件仅记录文件名）'],
    });
    const genDone = await anthropicSidecar.waitFor((e) => e.type === 'completed');
    expect(genDone).toMatchObject({
      type: 'completed',
      requestId: 'gen-1',
      text: 'feat(order): 补充支付回调幂等校验\n\n- 沿用 Order.ts 已暂存范围更新状态校验\n- 记录锁文件同步范围',
    });
    expect(requests[0].headers['x-api-key']).toBe('sk-ant-explicit-key-01');
    expect(requests[0].body).toContain('ALWAYS include BOTH the first-line title AND a blank line followed by 1 to 4 concise');
    expect(requests[0].body).toContain('you MUST still include the blank line and `- ` bullet body below the title');
    await anthropicSidecar.close();

    // 2. OpenAI testConnection (/v1/responses):
    const openaiSidecar = spawnSidecar();
    await openaiSidecar.waitFor((e) => e.type === 'ready');
    openaiSidecar.send({
      type: 'testConnection',
      requestId: 'test-openai',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'test-openai', scopeVersion: 'v1' },
      model: { provider: 'openai', modelId: 'gpt-4.1', baseUrl: `${server.url}/v1`, apiKey: 'sk-openai-explicit-02' },
    });
    const openaiDone = await openaiSidecar.waitFor((e) => e.type === 'completed');
    expect(openaiDone).toMatchObject({ type: 'completed', text: 'OK' });
    expect(requests[1].headers.authorization).toBe('Bearer sk-openai-explicit-02');
    await openaiSidecar.close();

    // 3. Ollama keyless testConnection (/v1/chat/completions):
    const ollamaSidecar = spawnSidecar();
    await ollamaSidecar.waitFor((e) => e.type === 'ready');
    ollamaSidecar.send({
      type: 'testConnection',
      requestId: 'test-ollama',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'test-ollama', scopeVersion: 'v1' },
      model: { provider: 'ollama', modelId: 'qwen2.5-coder:14b', baseUrl: `${server.url}/v1`, apiKey: '' },
    });
    expect(await ollamaSidecar.waitFor((e) => e.type === 'completed')).toMatchObject({ type: 'completed', text: 'OK' });
    await ollamaSidecar.close();

    // 4. DeepSeek / Custom reviewScope (filters hallucinated out-of-scope file):
    const reviewSidecar = spawnSidecar();
    await reviewSidecar.waitFor((e) => e.type === 'ready');
    reviewSidecar.send({
      type: 'reviewScope',
      requestId: 'rev-1',
      context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'scope-1' },
      model: { provider: 'deepseek', modelId: 'deepseek-chat', baseUrl: server.url, apiKey: 'ds-secret-03' },
      branch: 'feat/pay',
      draft: '',
      files: [{ fileId: 'file-pay', path: 'src/api/pay.ts', status: 'M', rangeKind: 'unstaged', added: 5, removed: 0, patch: '@@ -15,0 +16,5 @@\n+console.log(rawBody);' }],
      skippedPaths: ['pnpm-lock.yaml（锁文件默认忽略审查）'],
    });
    const revDone = await reviewSidecar.waitFor((e) => e.type === 'reviewCompleted');
    expect(revDone).toMatchObject({
      type: 'reviewCompleted',
      requestId: 'rev-1',
      findings: [{ fileId: 'file-pay', path: 'src/api/pay.ts', line: 19, severity: 'high', title: '日志包含完整渠道响应' }],
    });
    await reviewSidecar.close();
  });

  it('redacts echoed API keys on provider HTTP errors, rejects non-JSON review output, and cancels slow streams', async () => {
    const secret = 'custom-secret-token-never-log-998877';
    let holdStream = false;
    let mode: 'authError' | 'badReview' | 'slow' = 'authError';

    const server = await startServer((_req, _body, res) => {
      if (mode === 'authError') {
        res.writeHead(401, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ error: { message: `Invalid key provided: ${secret} (Bearer ${secret})`, code: 'invalid_api_key' } }));
        return;
      }
      if (mode === 'badReview') {
        writeOpenAiCompletionsSse(res, ['Sorry, I cannot output JSON today.']);
        return;
      }
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.write(`data: ${JSON.stringify({
        id: 'slow-1', object: 'chat.completion.chunk', created: 1700000000, model: 'm',
        choices: [{ index: 0, delta: { content: 'feat: partial ' }, finish_reason: null }],
      })}\n\n`);
      holdStream = true;
      const keepAlive = setInterval(() => {}, 1_000);
      res.on('close', () => { clearInterval(keepAlive); });
    });

    // 1. Secret redaction on 401 error:
    const authSidecar = spawnSidecar();
    await authSidecar.waitFor((e) => e.type === 'ready');
    authSidecar.send({
      type: 'testConnection',
      requestId: 'err-auth',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'err-auth', scopeVersion: 'v1' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
    });
    const failed = await authSidecar.waitFor((e) => e.type === 'failed');
    expect(failed.type === 'failed' && failed.message).toContain('鉴权失败');
    expect(JSON.stringify(authSidecar.events)).not.toContain(secret);
    expect(authSidecar.stderr()).not.toContain(secret);
    await authSidecar.close();

    // 2. Non-JSON review output fails explicitly instead of faking 0 findings:
    mode = 'badReview';
    const badReviewSidecar = spawnSidecar();
    await badReviewSidecar.waitFor((e) => e.type === 'ready');
    badReviewSidecar.send({
      type: 'reviewScope',
      requestId: 'bad-rev',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
      branch: 'feat/test',
      draft: '',
      files: [{ fileId: 'f1', path: 'a.ts', status: 'M', rangeKind: 'unstaged', added: 1, removed: 0, patch: '+x' }],
      skippedPaths: [],
    });
    const badRevEvent = await badReviewSidecar.waitFor((e) => e.type === 'failed');
    expect(badRevEvent.type === 'failed' && badRevEvent.message).toContain('结构化审查');
    await badReviewSidecar.close();

    // 3. Cancellation aborts slow HTTP stream cleanly:
    mode = 'slow';
    const slowSidecar = spawnSidecar();
    await slowSidecar.waitFor((e) => e.type === 'ready');
    slowSidecar.send({
      type: 'generateCommit',
      requestId: 'slow-gen',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
      branch: 'feat/test',
      recentCommits: [],
      draft: '',
      files: [{ fileId: 'f1', path: 'a.ts', status: 'M', rangeKind: 'unstaged', added: 1, removed: 0, patch: '+x' }],
      skippedPaths: [],
    });
    await slowSidecar.waitFor((e) => e.type === 'textDelta');
    expect(holdStream).toBe(true);
    slowSidecar.send({ type: 'cancel', requestId: 'slow-gen' });
    expect(await slowSidecar.waitFor((e) => e.type === 'cancelled')).toMatchObject({ type: 'cancelled', requestId: 'slow-gen' });
    expect(await slowSidecar.close()).toBe(0);
  });
});
