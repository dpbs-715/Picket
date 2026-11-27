import * as anthropicMessages from '@earendil-works/pi-ai/api/anthropic-messages';
import * as openaiCompletions from '@earendil-works/pi-ai/api/openai-completions';
import * as openaiResponses from '@earendil-works/pi-ai/api/openai-responses';
import { createModels, createProvider, type MutableModels } from '@earendil-works/pi-ai/models';
import type { Api, Model, ProviderStreams } from '@earendil-works/pi-ai';
import type { AppLocale } from '../../shared/language-protocol';
import {
  resolveDefaultApiFormat,
  resolveDefaultBaseUrl,
  resolveDefaultTokenLimits,
} from '../../shared/model-defaults';
import type { ModelApiFormat } from '../../shared/model-protocol';
import type { PiErrorCode, PiProviderSpec } from '../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from './i18n';

interface ResolvedRuntimeModel {
  models: MutableModels;
  model: Model<Api>;
}

function resolveApiAndStreams(
  provider: PiProviderSpec['provider'],
  explicitFormat?: ModelApiFormat,
): { format: ModelApiFormat; api: Api; streams: ProviderStreams } {
  const format = explicitFormat ?? resolveDefaultApiFormat(provider);
  if (format === 'anthropic-messages') {
    return { format, api: 'anthropic-messages', streams: anthropicMessages as unknown as ProviderStreams };
  }
  if (format === 'openai-responses') {
    return { format, api: 'openai-responses', streams: openaiResponses as unknown as ProviderStreams };
  }
  return { format, api: 'openai-completions', streams: openaiCompletions as unknown as ProviderStreams };
}

export function createIsolatedModelRuntime(spec: PiProviderSpec, locale: AppLocale = 'en'): ResolvedRuntimeModel {
  const msg = getSidecarMessages(locale);
  const baseUrl = (spec.baseUrl || resolveDefaultBaseUrl(spec.provider)).trim();
  if (!baseUrl) {
    throw new LocalizedSidecarError(msg.missingBaseUrl);
  }
  let parsedUrl: URL;
  try {
    parsedUrl = new URL(baseUrl);
  } catch {
    throw new LocalizedSidecarError(msg.invalidBaseUrl);
  }
  if ((parsedUrl.protocol !== 'http:' && parsedUrl.protocol !== 'https:') || parsedUrl.username || parsedUrl.password) {
    throw new LocalizedSidecarError(msg.unsupportedBaseUrlProtocol);
  }

  const requiresKey = spec.provider !== 'ollama';
  const trimmedKey = spec.apiKey.trim();
  if (requiresKey && !trimmedKey) {
    throw new LocalizedSidecarError(msg.missingApiKey);
  }
  // Keyless Ollama local endpoints run through the OpenAI SDK client, which
  // expects a non-empty placeholder string even when the local server ignores auth.
  const effectiveKey = requiresKey ? trimmedKey : 'ollama';
  const { format, api, streams } = resolveApiAndStreams(spec.provider, spec.apiFormat);
  let normalizedBaseUrl = baseUrl.replace(/\/+$/, '');
  if (api === 'anthropic-messages' && /\/v1$/i.test(parsedUrl.pathname.replace(/\/+$/, ''))) {
    // @anthropic-ai/sdk always appends `/v1/messages`; strip a trailing `/v1`
    // when users paste an OpenAI-style `http://host:port/v1` gateway URL.
    normalizedBaseUrl = normalizedBaseUrl.replace(/\/v1$/i, '');
  }
  const tokenDefaults = resolveDefaultTokenLimits(spec.provider, format, spec.modelId);
  const contextWindow = spec.contextWindow ?? tokenDefaults.contextWindow;
  const maxTokens = spec.maxOutputTokens ?? tokenDefaults.maxOutputTokens;

  const model: Model<Api> = {
    id: spec.modelId.trim(),
    name: spec.modelId.trim(),
    api,
    provider: spec.provider,
    baseUrl: normalizedBaseUrl,
    input: ['text'],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    reasoning: false,
    contextWindow,
    maxTokens,
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
          return { auth: { apiKey: effectiveKey }, source: requiresKey ? 'local-credentials' : 'local' };
        },
      },
    },
    models: [model],
    api: streams,
  });
  models.setProvider(provider);

  return { models, model };
}

export function classifyProviderError(
  error: unknown,
  spec: PiProviderSpec,
  locale: AppLocale = 'en',
): { message: string; errorCode: PiErrorCode } {
  const msg = getSidecarMessages(locale);
  let text = error instanceof Error ? error.message : typeof error === 'string' ? error : msg.unknownModelError;
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
  if (error instanceof LocalizedSidecarError) {
    return { message: msg.requestFailed(compact), errorCode: 'modelInvalidConfig' };
  }

  const lower = compact.toLowerCase();
  if (lower.includes('401') || lower.includes('403') || lower.includes('invalid_api_key') || lower.includes('authentication') || lower.includes('unauthorized')) {
    return { message: msg.authFailed(compact), errorCode: 'modelAuthFailed' };
  }
  if (lower.includes('404') || lower.includes('model_not_found') || lower.includes('not found')) {
    return { message: msg.endpointOrModelNotFound(compact), errorCode: 'modelEndpointNotFound' };
  }
  if (
    lower.includes('402')
    || lower.includes('429')
    || lower.includes('payment required')
    || lower.includes('insufficient balance')
    || lower.includes('credit balance')
    || lower.includes('billing')
    || lower.includes('rate_limit')
    || lower.includes('rate limit')
    || lower.includes('too many requests')
    || lower.includes('insufficient_quota')
    || lower.includes('quota')
  ) {
    return { message: msg.rateLimitedOrQuota(compact), errorCode: 'modelRateLimited' };
  }
  if (/(?<![:/\d])\b(?:500|502|503|504)\b/.test(lower) || lower.includes('internal server error') || lower.includes('bad gateway') || lower.includes('service unavailable') || lower.includes('server_error') || lower.includes('overloaded')) {
    return { message: msg.providerServiceError(compact), errorCode: 'modelServiceError' };
  }
  if (lower.includes('econnrefused') || lower.includes('enotfound') || lower.includes('ehostunreach') || lower.includes('econnreset') || lower.includes('fetch failed') || lower.includes('connection error')) {
    return { message: msg.connectionFailed(compact), errorCode: 'modelConnectionFailed' };
  }
  if (lower.includes('timed out') || lower.includes('timeout') || lower.includes('etimedout')) {
    return { message: msg.requestTimedOut(compact), errorCode: 'modelTimeout' };
  }
  if (
    lower.includes('does not support tools')
    || lower.includes('tools are not supported')
    || lower.includes('tool use is not supported')
    || lower.includes('tool calling is not supported')
    || lower.includes('function calling is not supported')
    || lower.includes('does not support function calling')
  ) {
    return { message: msg.toolsUnsupported(compact), errorCode: 'modelToolsUnsupported' };
  }
  return { message: msg.requestFailed(compact), errorCode: 'modelRequestFailed' };
}

export function sanitizeProviderError(error: unknown, spec: PiProviderSpec, locale: AppLocale = 'en'): string {
  return classifyProviderError(error, spec, locale).message;
}
