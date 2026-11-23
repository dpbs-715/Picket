import * as anthropicMessages from '@earendil-works/pi-ai/api/anthropic-messages';
import * as openaiCompletions from '@earendil-works/pi-ai/api/openai-completions';
import * as openaiResponses from '@earendil-works/pi-ai/api/openai-responses';
import { createModels, createProvider, type MutableModels } from '@earendil-works/pi-ai/models';
import type { Api, Model, ProviderStreams } from '@earendil-works/pi-ai';
import type { ModelApiFormat } from '../../shared/model-protocol';
import type { PiProviderSpec } from '../../shared/pi-protocol';

interface ResolvedRuntimeModel {
  models: MutableModels;
  model: Model<Api>;
}

const DEFAULT_BASE_URLS: Record<PiProviderSpec['provider'], string> = {
  anthropic: 'https://api.anthropic.com',
  openai: 'https://api.openai.com/v1',
  deepseek: 'https://api.deepseek.com',
  ollama: 'http://127.0.0.1:11434/v1',
  custom: '',
};

function defaultApiFormat(provider: PiProviderSpec['provider']): ModelApiFormat {
  if (provider === 'anthropic') return 'anthropic-messages';
  if (provider === 'openai') return 'openai-responses';
  return 'openai-completions';
}

function resolveApiAndStreams(
  provider: PiProviderSpec['provider'],
  explicitFormat?: ModelApiFormat,
): { api: Api; streams: ProviderStreams } {
  const format = explicitFormat ?? defaultApiFormat(provider);
  if (format === 'anthropic-messages') {
    return { api: 'anthropic-messages', streams: anthropicMessages as unknown as ProviderStreams };
  }
  if (format === 'openai-responses') {
    return { api: 'openai-responses', streams: openaiResponses as unknown as ProviderStreams };
  }
  return { api: 'openai-completions', streams: openaiCompletions as unknown as ProviderStreams };
}

export function createIsolatedModelRuntime(spec: PiProviderSpec): ResolvedRuntimeModel {
  const baseUrl = (spec.baseUrl || DEFAULT_BASE_URLS[spec.provider]).trim();
  if (!baseUrl) {
    throw new Error('缺少模型服务接口地址（Base URL）');
  }
  let parsedUrl: URL;
  try {
    parsedUrl = new URL(baseUrl);
  } catch {
    throw new Error('模型服务接口地址不是有效的 HTTP / HTTPS URL');
  }
  if ((parsedUrl.protocol !== 'http:' && parsedUrl.protocol !== 'https:') || parsedUrl.username || parsedUrl.password) {
    throw new Error('模型服务接口地址仅支持不含账号密码的 HTTP / HTTPS 协议');
  }

  const requiresKey = spec.provider !== 'ollama';
  const trimmedKey = spec.apiKey.trim();
  if (requiresKey && !trimmedKey) {
    throw new Error('缺少模型 API Key');
  }
  // Keyless Ollama local endpoints run through the OpenAI SDK client, which
  // expects a non-empty placeholder string even when the local server ignores auth.
  const effectiveKey = requiresKey ? trimmedKey : 'ollama';
  const { api, streams } = resolveApiAndStreams(spec.provider, spec.apiFormat);
  let normalizedBaseUrl = baseUrl.replace(/\/+$/, '');
  if (api === 'anthropic-messages' && /\/v1$/i.test(parsedUrl.pathname.replace(/\/+$/, ''))) {
    // @anthropic-ai/sdk always appends `/v1/messages`; strip a trailing `/v1`
    // when users paste an OpenAI-style `http://host:port/v1` gateway URL.
    normalizedBaseUrl = normalizedBaseUrl.replace(/\/v1$/i, '');
  }

  const model: Model<Api> = {
    id: spec.modelId.trim(),
    name: spec.modelId.trim(),
    api,
    provider: spec.provider,
    baseUrl: normalizedBaseUrl,
    input: ['text'],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    reasoning: false,
    contextWindow: 128_000,
    maxTokens: 32_768,
    ...(api === 'openai-completions' ? {
      compat: {
        supportsStore: false,
        supportsDeveloperRole: false,
        supportsReasoningEffort: false,
        maxTokensField: 'max_tokens' as const,
      },
    } : {}),
  };

  // Strictly isolate Pi Core from ambient environment variables, home files, or OAuth logins.
  const models = createModels({
    authContext: {
      env: async () => undefined,
      fileExists: async () => false,
    },
    identity: { attribution: null },
  });

  const provider = createProvider({
    id: spec.provider,
    name: spec.provider,
    baseUrl: model.baseUrl,
    auth: {
      apiKey: {
        name: `${spec.provider} key`,
        resolve: async ({ signal }) => {
          signal.throwIfAborted();
          return { auth: { apiKey: effectiveKey }, source: requiresKey ? 'keychain' : 'local' };
        },
      },
    },
    models: [model],
    api: streams,
  });
  models.setProvider(provider);

  return { models, model };
}

export function sanitizeProviderError(raw: unknown, spec: PiProviderSpec): string {
  let text = raw instanceof Error ? raw.message : typeof raw === 'string' ? raw : '未知模型错误';
  const candidates = [spec.apiKey, spec.apiKey.trim()].filter((item) => item.length > 0);
  for (const secret of candidates) {
    text = text.split(secret).join('[REDACTED]');
    if (secret.length >= 4) {
      const encoded = encodeURIComponent(secret);
      if (encoded !== secret) text = text.split(encoded).join('[REDACTED]');
    }
  }
  text = text
    .replace(/Bearer\s+[^\s"',;)\]]+/gi, 'Bearer [REDACTED]')
    .replace(/sk-ant-[A-Za-z0-9_-]{6,}/g, '[REDACTED]')
    .replace(/sk-[A-Za-z0-9_-]{8,}/g, '[REDACTED]')
    .replace(/(https?:\/\/)[^/\s@]+@/gi, '$1[REDACTED]@')
    .replace(/\s+/g, ' ')
    .trim();

  const compact = text.length > 220 ? `${text.slice(0, 220)}…` : text;
  const lower = compact.toLowerCase();
  if (lower.includes('401') || lower.includes('403') || lower.includes('invalid_api_key') || lower.includes('authentication') || lower.includes('unauthorized')) {
    return `鉴权失败，请检查 API Key 或服务权限（${compact}）`;
  }
  if (lower.includes('404') || lower.includes('model_not_found') || lower.includes('not found')) {
    return `未找到对应接口或模型，请检查模型 ID 与接口地址（${compact}）`;
  }
  if (lower.includes('429') || lower.includes('rate_limit') || lower.includes('insufficient_quota') || lower.includes('quota')) {
    return `请求受限或账户额度不足（${compact}）`;
  }
  if (lower.includes('econnrefused') || lower.includes('enotfound') || lower.includes('ehostunreach') || lower.includes('fetch failed') || lower.includes('connection error')) {
    return `无法连接到模型服务，请确认网络或本地服务已启动（${compact}）`;
  }
  if (lower.includes('timed out') || lower.includes('timeout') || lower.includes('etimedout')) {
    return `连接模型服务超时，请稍后重试（${compact}）`;
  }
  return `模型请求未完成：${compact}`;
}
