import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { mkdtempSync, rmSync } from 'node:fs';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { afterEach, describe, expect, it } from 'vitest';
import type { PiEvent, PiProviderSpec } from '../../shared/pi-protocol';
import { getSidecarMessages } from '../src/i18n';
import { sanitizeProviderError } from '../src/providers';

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
  it('connects and streams across anthropic, openai, deepseek, ollama, and custom wire protocols with explicit language directives and user extra instructions', async () => {
    const requests: { path: string; headers: IncomingMessage['headers']; body: string }[] = [];
    const server = await startServer((req, body, res) => {
      const pathname = new URL(req.url ?? '/', 'http://127.0.0.1').pathname;
      requests.push({ path: pathname, headers: req.headers, body });
      if (pathname === '/v1/messages') {
        const planPayload = JSON.stringify({
          kind: 'ready',
          summary: 'Prepared single commit plan',
          groups: [{
            id: 'g1',
            fileIds: ['file-order'],
            message: 'feat(order): add idempotency check for payment callback\n\n- update staged status check in Order.ts',
          }],
          skippedFileIds: [],
        });
        writeAnthropicSse(res, [
          '<think>Inspecting staged diff scope</think>\n```json\n',
          planPayload,
          '\n```',
        ]);
        return;
      }
      if (pathname === '/v1/responses') {
        writeOpenAiResponsesSse(res, 'OK');
        return;
      }
      if (pathname === '/v1/chat/completions' || pathname === '/chat/completions') {
        writeOpenAiCompletionsSse(res, ['OK']);
        return;
      }
      res.writeHead(404).end('not found');
    });

    // 1. Verify ambient ANTHROPIC_API_KEY env is ignored when empty key is passed:
    const ambientSidecar = spawnSidecar({ ANTHROPIC_API_KEY: 'sk-ant-ambient-must-never-be-used' });
    await ambientSidecar.waitFor((e) => e.type === 'ready');
    ambientSidecar.send({
      type: 'testConnection',
      requestId: 'ambient-check',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'ambient', scopeVersion: 'v1' },
      model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: server.url, apiKey: '' },
    });
    const ambientRejected = await ambientSidecar.waitFor((e) => e.type === 'failed');
    expect(ambientRejected.type === 'failed' && ambientRejected.message).toContain(getSidecarMessages('en').missingApiKey);
    expect(requests).toHaveLength(0);
    await ambientSidecar.close();

    // 2. Anthropic planCPush defaults to commitLanguage = 'en' (-en) and includes user extra instructions:
    const anthropicSidecar = spawnSidecar({ ANTHROPIC_API_KEY: 'sk-ant-ambient-must-never-be-used' });
    await anthropicSidecar.waitFor((e) => e.type === 'ready');
    anthropicSidecar.send({
      type: 'planCPush',
      requestId: 'plan-en-1',
      context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-1', scopeVersion: 'scope-1' },
      model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: server.url, apiKey: 'sk-ant-explicit-key-01' },
      branch: 'feat/pay',
      recentCommits: ['fix(auth): handle expired refresh token'],
      prompt: 'write detailed bullet points',
      files: [{ fileId: 'file-order', path: 'src/Order.ts', status: 'M', rangeKind: 'partial', added: 4, removed: 1, patch: '@@ -1 +1,4 @@\n-old\n+new' }],
      unselectedPaths: ['pnpm-lock.yaml'],
      qaHistory: [],
    });
    const planDone = await anthropicSidecar.waitFor((e) => e.type === 'cPushPlanCompleted');
    expect(planDone).toMatchObject({
      type: 'cPushPlanCompleted',
      requestId: 'plan-en-1',
      plan: {
        kind: 'ready',
        groups: [{
          id: 'g1',
          fileIds: ['file-order'],
          message: 'feat(order): add idempotency check for payment callback\n\n- update staged status check in Order.ts',
        }],
      },
    });
    expect(requests[0].headers['x-api-key']).toBe('sk-ant-explicit-key-01');
    expect(requests[0].body).toContain('<commit_skill_spec>');
    expect(requests[0].body).toContain('1 to 6 concise `- ` bullet points');
    expect(requests[0].body).not.toContain('Use a body only when');
    expect(requests[0].body).toContain('flag `-en`');
    expect(requests[0].body).toContain('User extra instructions (supplemental grouping/message guidance ONLY');
    expect(requests[0].body).toContain('write detailed bullet points');
    await anthropicSidecar.close();

    // 3. Anthropic planCPush with commitLanguage = 'zh' sends -ch directive:
    const anthropicZhSidecar = spawnSidecar();
    await anthropicZhSidecar.waitFor((e) => e.type === 'ready');
    anthropicZhSidecar.send({
      type: 'planCPush',
      requestId: 'plan-zh-1',
      context: { repositoryId: 'repo-1', worktreeId: 'wt-1', sessionId: 'sess-zh', scopeVersion: 'scope-1' },
      model: { provider: 'anthropic', modelId: 'claude-sonnet-4-20250514', baseUrl: server.url, apiKey: 'sk-ant-explicit-key-01' },
      branch: 'feat/pay',
      recentCommits: ['fix(auth): handle expired refresh token'],
      prompt: '',
      files: [{ fileId: 'file-order', path: 'src/Order.ts', status: 'M', rangeKind: 'partial', added: 4, removed: 1, patch: '@@ -1 +1,4 @@\n-old\n+new' }],
      unselectedPaths: [],
      qaHistory: [],
      locale: 'en',
      commitLanguage: 'zh',
    });
    await anthropicZhSidecar.waitFor((e) => e.type === 'cPushPlanCompleted');
    expect(requests[1].body).toContain('flag `-ch`');
    expect(requests[1].body).toContain('write BOTH the subject (after `<type>(<optional scope>):`) AND every `- ` detail bullet of `groups[].message` in concise Chinese');
    await anthropicZhSidecar.close();

    // 4. OpenAI testConnection (/v1/responses):
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
    expect(requests[2].headers.authorization).toBe('Bearer sk-openai-explicit-02');
    await openaiSidecar.close();

    // 5. Ollama keyless testConnection (/v1/chat/completions):
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

    // 6. DeepSeek testConnection (/chat/completions):
    const deepseekSidecar = spawnSidecar();
    await deepseekSidecar.waitFor((e) => e.type === 'ready');
    deepseekSidecar.send({
      type: 'testConnection',
      requestId: 'test-deepseek',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'test-deepseek', scopeVersion: 'v1' },
      model: { provider: 'deepseek', modelId: 'deepseek-chat', baseUrl: server.url, apiKey: 'ds-secret-03' },
    });
    expect(await deepseekSidecar.waitFor((e) => e.type === 'completed')).toMatchObject({ type: 'completed', text: 'OK' });
    await deepseekSidecar.close();
  });

  it('redacts echoed API keys on provider HTTP errors, localizes en and zh messages (including 402/429 quota), rejects non-JSON C-Push plan output, and cancels slow streams', async () => {
    const secret = 'custom-secret-token-never-log-998877';
    let holdStream = false;
    let mode: 'authError' | 'badPlan' | 'slow' = 'authError';

    const server = await startServer((_req, _body, res) => {
      if (mode === 'authError') {
        res.writeHead(401, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ error: { message: `Invalid key provided: ${secret} (Bearer ${secret})`, code: 'invalid_api_key' } }));
        return;
      }
      if (mode === 'badPlan') {
        writeOpenAiCompletionsSse(res, ['Sorry, I cannot output JSON today.']);
        return;
      }
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.write(`data: ${JSON.stringify({
        id: 'slow-1', object: 'chat.completion.chunk', created: 1700000000, model: 'm',
        choices: [{ index: 0, delta: { content: '{"kind":"ready",' }, finish_reason: null }],
      })}\n\n`);
      holdStream = true;
      const keepAlive = setInterval(() => {}, 1_000);
      res.on('close', () => { clearInterval(keepAlive); });
    });

    // 1a. Secret redaction + default English localization on 401 error:
    const authSidecar = spawnSidecar();
    await authSidecar.waitFor((e) => e.type === 'ready');
    authSidecar.send({
      type: 'testConnection',
      requestId: 'err-auth-en',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'err-auth-en', scopeVersion: 'v1' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
    });
    const failedEn = await authSidecar.waitFor((e) => e.type === 'failed');
    expect(failedEn.type === 'failed' && failedEn.message).toContain('Authentication failed (401/403). Verify your API key and permissions');
    expect(JSON.stringify(authSidecar.events)).not.toContain(secret);
    expect(authSidecar.stderr()).not.toContain(secret);
    await authSidecar.close();

    // 1b. Secret redaction + Chinese localization when locale = 'zh':
    const authZhSidecar = spawnSidecar();
    await authZhSidecar.waitFor((e) => e.type === 'ready');
    authZhSidecar.send({
      type: 'testConnection',
      requestId: 'err-auth-zh',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 'err-auth-zh', scopeVersion: 'v1' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
      locale: 'zh',
    });
    const failedZh = await authZhSidecar.waitFor((e) => e.type === 'failed');
    expect(failedZh.type === 'failed' && failedZh.message).toContain('\u9274\u6743\u5931\u8d25');
    expect(JSON.stringify(authZhSidecar.events)).not.toContain(secret);
    await authZhSidecar.close();

    // 1c. Provider error sanitizer covers 402/429 quota, 5xx, network, and timeout in both locales:
    const spec: PiProviderSpec = { provider: 'custom', modelId: 'custom-v1', baseUrl: 'http://127.0.0.1:8080/v1', apiKey: secret };
    expect(sanitizeProviderError(new Error(`429 rate_limit for ${secret}`), spec)).toContain('Rate limit or quota exceeded (402/429)');
    expect(sanitizeProviderError(new Error(`402 Insufficient Balance (${secret})`), spec)).toContain('Rate limit or quota exceeded (402/429)');
    expect(sanitizeProviderError(new Error(`429 rate_limit for ${secret}`), spec, 'zh')).toContain('\u989d\u5ea6\u4e0d\u8db3');
    expect(sanitizeProviderError(new Error(`502 Bad Gateway (${secret})`), spec)).toContain('Model provider service error (5xx)');
    expect(sanitizeProviderError(new Error('fetch failed: ECONNREFUSED'), spec)).toContain('Unable to connect to model service');
    expect(sanitizeProviderError(new Error('fetch failed: ECONNREFUSED'), spec, 'zh')).toContain('\u65e0\u6cd5\u8fde\u63a5\u5230\u6a21\u578b\u670d\u52a1');
    expect(sanitizeProviderError(new Error('Request timed out'), spec)).toContain('Model request timed out');
    expect(sanitizeProviderError(new Error(`502 Bad Gateway (${secret})`), spec)).not.toContain(secret);

    // 2. Non-JSON C-Push plan output fails explicitly:
    mode = 'badPlan';
    const badPlanSidecar = spawnSidecar();
    await badPlanSidecar.waitFor((e) => e.type === 'ready');
    badPlanSidecar.send({
      type: 'planCPush',
      requestId: 'bad-plan',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
      branch: 'feat/test',
      recentCommits: [],
      prompt: '',
      files: [{ fileId: 'f1', path: 'a.ts', status: 'M', rangeKind: 'unstaged', added: 1, removed: 0, patch: '+x' }],
      unselectedPaths: [],
      qaHistory: [],
    });
    const badPlanEvent = await badPlanSidecar.waitFor((e) => e.type === 'failed');
    expect(badPlanEvent.type === 'failed' && badPlanEvent.message).toContain(getSidecarMessages('en').invalidCPushPlanJson);
    await badPlanSidecar.close();

    // 3. Cancellation aborts slow HTTP stream cleanly:
    mode = 'slow';
    const slowSidecar = spawnSidecar();
    await slowSidecar.waitFor((e) => e.type === 'ready');
    slowSidecar.send({
      type: 'planCPush',
      requestId: 'slow-plan',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v' },
      model: { provider: 'custom', modelId: 'custom-v1', baseUrl: `${server.url}/v1`, apiKey: secret },
      branch: 'feat/test',
      recentCommits: [],
      prompt: '',
      files: [{ fileId: 'f1', path: 'a.ts', status: 'M', rangeKind: 'unstaged', added: 1, removed: 0, patch: '+x' }],
      unselectedPaths: [],
      qaHistory: [],
    });
    await slowSidecar.waitFor((e) => e.type === 'textDelta');
    expect(holdStream).toBe(true);
    slowSidecar.send({ type: 'cancel', requestId: 'slow-plan' });
    expect(await slowSidecar.waitFor((e) => e.type === 'cancelled')).toMatchObject({ type: 'cancelled', requestId: 'slow-plan' });
    expect(await slowSidecar.close()).toBe(0);
  });

  it('emits activity heartbeats while reasoning_content streams and separates UI locale from commitLanguage in C-Push prompts', async () => {
    const capturedBodies: string[] = [];
    const server = await startServer(async (_req, body, res) => {
      capturedBodies.push(body);
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
      res.write(`data: ${JSON.stringify({
        id: 'chatcmpl-think-1',
        object: 'chat.completion.chunk',
        choices: [{ index: 0, delta: { role: 'assistant', reasoning_content: 'Thinking step 1...' }, finish_reason: null }],
      })}\n\n`);
      await new Promise((r) => setTimeout(r, 1_050));
      res.write(`data: ${JSON.stringify({
        id: 'chatcmpl-think-1',
        object: 'chat.completion.chunk',
        choices: [{ index: 0, delta: { reasoning_content: 'Thinking step 2...' }, finish_reason: null }],
      })}\n\n`);
      const planJson = JSON.stringify({
        kind: 'ready',
        summary: 'Single commit plan',
        groups: [{ id: 'g1', fileIds: ['f1'], message: 'feat(berth): add berth window validation\n\n- validate cargo schedule boundaries' }],
        skippedFileIds: [],
      });
      res.write(`data: ${JSON.stringify({
        id: 'chatcmpl-think-1',
        object: 'chat.completion.chunk',
        choices: [{ index: 0, delta: { content: planJson }, finish_reason: 'stop' }],
      })}\n\n`);
      res.write('data: [DONE]\n\n');
      res.end();
    });

    const sidecar = spawnSidecar();
    await sidecar.waitFor((e) => e.type === 'ready');
    sidecar.send({
      type: 'planCPush',
      requestId: 'plan-think-1',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v' },
      model: { provider: 'custom', apiFormat: 'openai-completions', modelId: 'thinking-v1', baseUrl: `${server.url}/v1`, apiKey: 'test-key-local' },
      branch: 'v1.9.0',
      recentCommits: ['feat(berth): initialize berth form'],
      prompt: '',
      files: [{ fileId: 'f1', path: 'src/views/work/berth/sofDialog.vue', status: 'M', rangeKind: 'unstaged', added: 12, removed: 3, patch: '+const valid = true;' }],
      unselectedPaths: [],
      qaHistory: [],
      locale: 'zh',
      commitLanguage: 'en',
    });

    const done = await sidecar.waitFor((e) => e.type === 'cPushPlanCompleted');
    expect(done.type === 'cPushPlanCompleted' && done.plan.kind).toBe('ready');
    expect(capturedBodies[0]).toContain('UI interaction language (`locale = \\"zh\\"`)');
    expect(capturedBodies[0]).toContain('commitLanguage = \\"en\\"');
    expect(capturedBodies[0]).toContain('flag `-en`');
    const activities = sidecar.events.filter((e) => e.type === 'activity');
    expect(activities.length).toBeGreaterThanOrEqual(2);
    const leakedReasoning = sidecar.events
      .filter((e): e is Extract<PiEvent, { type: 'textDelta' }> => e.type === 'textDelta')
      .map((e) => e.delta)
      .join('');
    expect(leakedReasoning).not.toContain('Thinking step');
    expect(await sidecar.close()).toBe(0);
  });

  it('reports length truncation clearly in both en and zh when maxTokens cuts off mid-JSON', async () => {
    const server = await startServer((_req, _body, res) => {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
      res.write(`data: ${JSON.stringify({
        id: 'chatcmpl-trunc-1',
        object: 'chat.completion.chunk',
        choices: [{ index: 0, delta: { role: 'assistant', content: '{"kind":"ready","summary":"Cut off mid-stream' }, finish_reason: 'length' }],
      })}\n\n`);
      res.write('data: [DONE]\n\n');
      res.end();
    });

    const sidecarEn = spawnSidecar();
    await sidecarEn.waitFor((e) => e.type === 'ready');
    sidecarEn.send({
      type: 'planCPush',
      requestId: 'plan-trunc-en',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's', scopeVersion: 'v' },
      model: { provider: 'custom', apiFormat: 'openai-completions', modelId: 'thinking-v1', baseUrl: `${server.url}/v1`, apiKey: 'test-key-local' },
      branch: 'v1.9.0',
      recentCommits: [],
      prompt: '',
      files: [{ fileId: 'f1', path: 'a.ts', status: 'M', rangeKind: 'unstaged', added: 1, removed: 0, patch: '+1' }],
      unselectedPaths: [],
      qaHistory: [],
    });
    const failedEn = await sidecarEn.waitFor((e) => e.type === 'failed');
    expect(failedEn.type === 'failed' && failedEn.message).toContain('truncated due to max output tokens');
    expect(await sidecarEn.close()).toBe(0);

    const sidecarZh = spawnSidecar();
    await sidecarZh.waitFor((e) => e.type === 'ready');
    sidecarZh.send({
      type: 'planCPush',
      requestId: 'plan-trunc-zh',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's-zh', scopeVersion: 'v' },
      model: { provider: 'custom', apiFormat: 'openai-completions', modelId: 'thinking-v1', baseUrl: `${server.url}/v1`, apiKey: 'test-key-local' },
      branch: 'v1.9.0',
      recentCommits: [],
      prompt: '',
      files: [{ fileId: 'f1', path: 'a.ts', status: 'M', rangeKind: 'unstaged', added: 1, removed: 0, patch: '+1' }],
      unselectedPaths: [],
      qaHistory: [],
      locale: 'zh',
    });
    const failedZh = await sidecarZh.waitFor((e) => e.type === 'failed');
    expect(failedZh.type === 'failed' && failedZh.message).toContain('\u8fbe\u5230\u957f\u5ea6\u4e0a\u9650\u88ab\u622a\u65ad');
    expect(await sidecarZh.close()).toBe(0);
  });

  it('automatically repairs an omitted deleted file on a 95-file planCPush scope in one follow-up turn and caps retries at 1', async () => {
    const files = Array.from({ length: 95 }, (_, idx) => {
      if (idx === 18) {
        return {
          fileId: 'file-commit-proto',
          path: 'shared/git-commit-protocol.ts',
          status: 'D',
          rangeKind: 'unstaged',
          added: 0,
          removed: 46,
          patch: '-export interface GitCommitRequest {}',
        };
      }
      return {
        fileId: `file-m-${idx + 1}`,
        path: `src/module-${idx + 1}.ts`,
        status: 'M',
        rangeKind: 'unstaged',
        added: 4,
        removed: 1,
        patch: `+export const v${idx + 1} = true;`,
      };
    });

    const bodies: string[] = [];
    let alwaysOmit = false;
    const server = await startServer((_req, body, res) => {
      bodies.push(body);
      const isRepairTurn = body.includes('Validation error: your C-Push plan omitted');
      const group1Aliases = Array.from({ length: 50 }, (_, idx) => `@${idx + 1}`)
        .filter((alias) => (!isRepairTurn || alwaysOmit ? alias !== '@19' : true));
      const group2Aliases = Array.from({ length: 45 }, (_, idx) => `@${idx + 51}`);
      const payload = JSON.stringify({
        kind: 'ready',
        summary: 'Split into 2 logical commits across 95 files',
        groups: [
          {
            id: 'group-1',
            fileIds: group1Aliases,
            message: 'refactor(cpush): remove legacy manual commit protocol and update core modules\n\n- remove shared/git-commit-protocol.ts\n- update module batch 1',
          },
          {
            id: 'group-2',
            fileIds: group2Aliases,
            message: 'feat(workbench): update remaining workbench modules\n\n- update module batch 2',
          },
        ],
        skippedFileIds: [],
      });
      writeOpenAiCompletionsSse(res, [payload]);
    });

    // 1. First request omits @19 (`shared/git-commit-protocol.ts`) -> sidecar sends 1 repair turn -> succeeds with all 95 canonical fileIds:
    const sidecarOk = spawnSidecar();
    await sidecarOk.waitFor((e) => e.type === 'ready');
    sidecarOk.send({
      type: 'planCPush',
      requestId: 'plan-95-repair',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's-95', scopeVersion: 'v' },
      model: { provider: 'custom', apiFormat: 'openai-completions', modelId: 'coder-v1', baseUrl: `${server.url}/v1`, apiKey: 'test-key-local' },
      branch: 'main',
      recentCommits: [],
      prompt: '',
      files,
      unselectedPaths: [],
      qaHistory: [],
    });
    const completed = await sidecarOk.waitFor((e) => e.type === 'cPushPlanCompleted');
    expect(bodies).toHaveLength(2);
    expect(bodies[1]).toContain('@19 | fileId=file-commit-proto | status=D | path=shared/git-commit-protocol.ts');
    expect(completed.type === 'cPushPlanCompleted' && completed.plan.kind === 'ready').toBe(true);
    if (completed.type === 'cPushPlanCompleted' && completed.plan.kind === 'ready') {
      expect(completed.plan.groups[0].fileIds).toContain('file-commit-proto');
      const totalAssigned = completed.plan.groups.reduce((sum, g) => sum + g.fileIds.length, 0);
      expect(totalAssigned).toBe(95);
    }
    expect(await sidecarOk.close()).toBe(0);

    // 2. If the model STILL omits @19 on the repair turn, sidecar fails cleanly after 2 HTTP calls without infinite retry:
    alwaysOmit = true;
    const sidecarFail = spawnSidecar();
    await sidecarFail.waitFor((e) => e.type === 'ready');
    sidecarFail.send({
      type: 'planCPush',
      requestId: 'plan-95-still-missing',
      context: { repositoryId: 'r', worktreeId: 'w', sessionId: 's-95-fail', scopeVersion: 'v' },
      model: { provider: 'custom', apiFormat: 'openai-completions', modelId: 'coder-v1', baseUrl: `${server.url}/v1`, apiKey: 'test-key-local' },
      branch: 'main',
      recentCommits: [],
      prompt: '',
      files,
      unselectedPaths: [],
      qaHistory: [],
    });
    const failed = await sidecarFail.waitFor((e) => e.type === 'failed');
    expect(bodies).toHaveLength(4);
    expect(failed.type === 'failed' && failed.message).toContain('Selected file shared/git-commit-protocol.ts was omitted from the C-Push plan.');
    expect(await sidecarFail.close()).toBe(0);
  });
});
