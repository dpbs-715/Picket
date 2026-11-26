import { createServer, type Server } from 'node:http';
import { afterEach, describe, expect, it } from 'vitest';
import type { ListModelsRequest, ProbeEvent } from '../../shared/pi-protocol';
import { parseRemoteModelOptions } from '../src/providers';
import { createTaskRunner } from '../src/tasks';

const servers: Server[] = [];

afterEach(async () => {
  while (servers.length > 0) {
    const server = servers.pop();
    if (server) {
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  }
});

describe('Provider remote model list fetching', () => {
  it('parses OpenAI, Anthropic, Ollama, and gateway token metadata while filtering invalid bounds', () => {
    const parsed = parseRemoteModelOptions({
      data: [
        {
          id: 'kimi-k3.1-pro',
          display_name: 'Kimi K3.1 Pro',
          context_length: 1_048_576,
          max_completion_tokens: 131_072,
        },
        {
          id: 'claude-sonnet-4-20250514',
          display_name: 'Claude Sonnet 4',
        },
        {
          id: 'bad-limits-model',
          context_window: 99_999_999,
          max_output_tokens: 10,
        },
        {
          id: 'kimi-k3.1-pro',
          context_length: 128_000,
        },
      ],
    });
    expect(parsed).toEqual([
      { id: 'bad-limits-model' },
      { id: 'claude-sonnet-4-20250514', displayName: 'Claude Sonnet 4' },
      {
        id: 'kimi-k3.1-pro',
        displayName: 'Kimi K3.1 Pro',
        contextWindow: 1_048_576,
        maxOutputTokens: 131_072,
      },
    ]);

    const ollamaTags = parseRemoteModelOptions({
      models: [
        { name: 'qwen2.5-coder:14b' },
        { name: 'deepseek-r1:8b' },
      ],
    });
    expect(ollamaTags.map((m) => m.id)).toEqual(['deepseek-r1:8b', 'qwen2.5-coder:14b']);
  });

  it('fetches OpenAI/Kimi models, Anthropic paginated models, Ollama /api/tags fallback, and redacts 401 secrets', async () => {
    const secret = 'sk-moonshot-super-secret-9999';
    const seenAuthHeaders: string[] = [];

    const server = createServer((req, res) => {
      const url = new URL(req.url ?? '/', 'http://127.0.0.1');
      if (url.pathname === '/openai/v1/models') {
        seenAuthHeaders.push(String(req.headers.authorization ?? ''));
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({
          data: [
            { id: 'kimi-k3.1-pro', context_length: 1_048_576, max_completion_tokens: 131_072 },
            { id: 'kimi-k2.8' },
          ],
        }));
        return;
      }
      if (url.pathname === '/anthropic/v1/models') {
        if (req.headers['x-api-key'] !== secret || !req.headers['anthropic-version']) {
          res.writeHead(401, { 'content-type': 'application/json' });
          res.end(JSON.stringify({ error: `Invalid key ${secret}` }));
          return;
        }
        const afterId = url.searchParams.get('after_id');
        res.writeHead(200, { 'content-type': 'application/json' });
        if (!afterId) {
          res.end(JSON.stringify({
            data: [{ id: 'claude-sonnet-4-20250514', display_name: 'Claude Sonnet 4' }],
            has_more: true,
            last_id: 'claude-sonnet-4-20250514',
          }));
        } else {
          res.end(JSON.stringify({
            data: [{ id: 'claude-opus-4-20250514', display_name: 'Claude Opus 4' }],
            has_more: false,
          }));
        }
        return;
      }
      if (url.pathname === '/api/tags') {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({
          models: [{ name: 'qwen2.5-coder:14b' }],
        }));
        return;
      }
      if (url.pathname === '/fail/v1/models') {
        res.writeHead(401, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ error: `Rejected token ${secret}` }));
        return;
      }
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'not found' }));
    });
    servers.push(server);
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('Expected TCP address');
    const origin = `http://127.0.0.1:${address.port}`;

    const runList = async (request: ListModelsRequest) => {
      const events: ProbeEvent[] = [];
      const runner = createTaskRunner(request, (event) => {
        events.push(event);
      });
      await runner.run();
      return events;
    };

    // 1. Custom OpenAI-compatible endpoint (auto-falls back from /openai/models -> /openai/v1/models):
    const openaiEvents = await runList({
      type: 'listModels',
      requestId: 'req-list-openai',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 's1', scopeVersion: 'v1' },
      model: {
        provider: 'custom',
        apiFormat: 'openai-completions',
        modelId: '',
        baseUrl: `${origin}/openai`,
        apiKey: secret,
      },
      locale: 'en',
    });
    expect(seenAuthHeaders).toEqual([`Bearer ${secret}`]);
    const openaiListed = openaiEvents.find((e) => e.type === 'modelsListed');
    expect(openaiListed?.type === 'modelsListed' && openaiListed.models).toEqual([
      { id: 'kimi-k2.8' },
      { id: 'kimi-k3.1-pro', contextWindow: 1_048_576, maxOutputTokens: 131_072 },
    ]);

    // 2. Anthropic paginated /v1/models:
    const anthropicEvents = await runList({
      type: 'listModels',
      requestId: 'req-list-anthropic',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 's2', scopeVersion: 'v1' },
      model: {
        provider: 'anthropic',
        apiFormat: 'anthropic-messages',
        modelId: '',
        baseUrl: `${origin}/anthropic`,
        apiKey: secret,
      },
      locale: 'en',
    });
    const anthropicListed = anthropicEvents.find((e) => e.type === 'modelsListed');
    expect(anthropicListed?.type === 'modelsListed' && anthropicListed.models).toEqual([
      { id: 'claude-opus-4-20250514', displayName: 'Claude Opus 4' },
      { id: 'claude-sonnet-4-20250514', displayName: 'Claude Sonnet 4' },
    ]);

    // 3. Ollama keyless fallback from /v1/models (404) -> /api/tags:
    const ollamaEvents = await runList({
      type: 'listModels',
      requestId: 'req-list-ollama',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 's3', scopeVersion: 'v1' },
      model: {
        provider: 'ollama',
        apiFormat: 'openai-completions',
        modelId: '',
        baseUrl: `${origin}/v1`,
        apiKey: '',
      },
      locale: 'en',
    });
    const ollamaListed = ollamaEvents.find((e) => e.type === 'modelsListed');
    expect(ollamaListed?.type === 'modelsListed' && ollamaListed.models).toEqual([
      { id: 'qwen2.5-coder:14b' },
    ]);

    // 4. 401 error redacts echoed API key:
    const failedEvents = await runList({
      type: 'listModels',
      requestId: 'req-list-fail',
      context: { repositoryId: 'settings', worktreeId: 'settings', sessionId: 's4', scopeVersion: 'v1' },
      model: {
        provider: 'custom',
        apiFormat: 'openai-completions',
        modelId: '',
        baseUrl: `${origin}/fail/v1`,
        apiKey: secret,
      },
      locale: 'en',
    });
    const failed = failedEvents.find((e) => e.type === 'failed');
    expect(failed?.type === 'failed' && failed.message).toContain('Authentication failed (401/403)');
    expect(JSON.stringify(failedEvents)).not.toContain(secret);
  });
});
