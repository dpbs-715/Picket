import * as anthropicMessages from '@earendil-works/pi-ai/api/anthropic-messages';
import * as openaiCompletions from '@earendil-works/pi-ai/api/openai-completions';
import * as openaiResponses from '@earendil-works/pi-ai/api/openai-responses';
import { createModels, createProvider, type MutableModels } from '@earendil-works/pi-ai/models';
import type { Api, Model, ProviderStreams } from '@earendil-works/pi-ai';
import type { AppLocale } from '../../shared/language-protocol';
import {
  MODEL_TOKEN_LIMITS,
  resolveDefaultApiFormat,
  resolveDefaultBaseUrl,
  resolveDefaultTokenLimits,
} from '../../shared/model-defaults';
import type { ModelApiFormat, RemoteModelOption } from '../../shared/model-protocol';
import type { PiProviderSpec } from '../../shared/pi-protocol';
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

export function sanitizeProviderError(error: unknown, spec: PiProviderSpec, locale: AppLocale = 'en'): string {
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
    return msg.requestFailed(compact);
  }

  const lower = compact.toLowerCase();
  if (lower.includes('401') || lower.includes('403') || lower.includes('invalid_api_key') || lower.includes('authentication') || lower.includes('unauthorized')) {
    return msg.authFailed(compact);
  }
  if (lower.includes('404') || lower.includes('model_not_found') || lower.includes('not found')) {
    return msg.endpointOrModelNotFound(compact);
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
    return msg.rateLimitedOrQuota(compact);
  }
  if (/(?<![:/\d])\b(?:500|502|503|504)\b/.test(lower) || lower.includes('internal server error') || lower.includes('bad gateway') || lower.includes('service unavailable') || lower.includes('server_error') || lower.includes('overloaded')) {
    return msg.providerServiceError(compact);
  }
  if (lower.includes('econnrefused') || lower.includes('enotfound') || lower.includes('ehostunreach') || lower.includes('econnreset') || lower.includes('fetch failed') || lower.includes('connection error')) {
    return msg.connectionFailed(compact);
  }
  if (lower.includes('timed out') || lower.includes('timeout') || lower.includes('etimedout')) {
    return msg.requestTimedOut(compact);
  }
  if (
    lower.includes('does not support tools')
    || lower.includes('tools are not supported')
    || lower.includes('tool use is not supported')
    || lower.includes('tool calling is not supported')
    || lower.includes('function calling is not supported')
    || lower.includes('does not support function calling')
  ) {
    return msg.toolsUnsupported(compact);
  }
  return msg.requestFailed(compact);
}

const MAX_REMOTE_MODELS = 300;
const MAX_REMOTE_MODEL_PAGES = 3;

function readPositiveInt(value: unknown): number | undefined {
  if (typeof value !== 'number' || !Number.isFinite(value)) return undefined;
  const rounded = Math.trunc(value);
  return rounded > 0 && Number.isSafeInteger(rounded) ? rounded : undefined;
}

function asObject(value: unknown): Record<string, unknown> | undefined {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

export function parseRemoteModelOptions(payload: unknown): RemoteModelOption[] {
  const root = asObject(payload);
  const rawList = Array.isArray(payload)
    ? payload
    : Array.isArray(root?.data)
      ? root.data
      : Array.isArray(root?.models)
        ? root.models
        : null;
  if (!rawList) {
    throw new Error('Unexpected model list response format (expected `data` or `models` array)');
  }

  const byId = new Map<string, RemoteModelOption>();
  for (const rawItem of rawList) {
    const item = asObject(rawItem);
    if (!item) continue;
    const idRaw = typeof item.id === 'string' && item.id.trim()
      ? item.id.trim()
      : typeof item.name === 'string' && item.name.trim()
        ? item.name.trim()
        : typeof item.model === 'string' && item.model.trim()
          ? item.model.trim()
          : '';
    if (!idRaw || idRaw.length > 256 || /[\x00-\x1f\x7f]/.test(idRaw)) continue;

    const displayCandidate = typeof item.display_name === 'string' && item.display_name.trim()
      ? item.display_name.trim()
      : typeof item.displayName === 'string' && item.displayName.trim()
        ? item.displayName.trim()
        : typeof item.id === 'string' && typeof item.name === 'string' && item.name.trim() && item.name.trim() !== idRaw
          ? item.name.trim()
          : undefined;
    const displayName = displayCandidate && displayCandidate !== idRaw && !/[\x00-\x1f\x7f]/.test(displayCandidate)
      ? displayCandidate.slice(0, 128)
      : undefined;

    const topProvider = asObject(item.top_provider);
    const rawContext = readPositiveInt(item.context_window)
      ?? readPositiveInt(item.contextWindow)
      ?? readPositiveInt(item.context_length)
      ?? readPositiveInt(item.contextLength)
      ?? readPositiveInt(item.max_context_length)
      ?? readPositiveInt(item.input_token_limit)
      ?? readPositiveInt(topProvider?.context_length);
    const contextWindow = rawContext !== undefined
      && rawContext >= MODEL_TOKEN_LIMITS.minContextWindow
      && rawContext <= MODEL_TOKEN_LIMITS.maxContextWindow
      ? rawContext
      : undefined;

    const rawMaxOut = readPositiveInt(item.max_output_tokens)
      ?? readPositiveInt(item.maxOutputTokens)
      ?? readPositiveInt(item.max_completion_tokens)
      ?? readPositiveInt(item.maxCompletionTokens)
      ?? readPositiveInt(item.output_token_limit)
      ?? readPositiveInt(topProvider?.max_completion_tokens);
    const maxOutputTokens = rawMaxOut !== undefined
      && rawMaxOut >= MODEL_TOKEN_LIMITS.minMaxOutputTokens
      && rawMaxOut <= MODEL_TOKEN_LIMITS.maxMaxOutputTokens
      && (contextWindow === undefined || rawMaxOut <= contextWindow)
      ? rawMaxOut
      : undefined;

    const existing = byId.get(idRaw);
    if (!existing) {
      byId.set(idRaw, {
        id: idRaw,
        ...(displayName ? { displayName } : {}),
        ...(contextWindow !== undefined ? { contextWindow } : {}),
        ...(maxOutputTokens !== undefined ? { maxOutputTokens } : {}),
      });
    } else {
      byId.set(idRaw, {
        id: idRaw,
        ...(existing.displayName || displayName ? { displayName: existing.displayName ?? displayName } : {}),
        ...(existing.contextWindow !== undefined || contextWindow !== undefined
          ? { contextWindow: existing.contextWindow ?? contextWindow }
          : {}),
        ...(existing.maxOutputTokens !== undefined || maxOutputTokens !== undefined
          ? { maxOutputTokens: existing.maxOutputTokens ?? maxOutputTokens }
          : {}),
      });
    }
    if (byId.size >= MAX_REMOTE_MODELS) break;
  }

  return [...byId.values()].sort((a, b) => a.id.localeCompare(b.id));
}

interface ModelListAttempt {
  url: string;
  headers: Record<string, string>;
  anthropicPaginated?: boolean;
}

function buildModelListAttempts(spec: PiProviderSpec, baseUrl: string, parsedUrl: URL): ModelListAttempt[] {
  const cleanBase = baseUrl.replace(/\/+$/, '');
  const cleanPath = parsedUrl.pathname.replace(/\/+$/, '');
  const endsWithVersion = /\/v\d+(?:beta\d*)?$/i.test(cleanPath);
  const effectiveFormat = spec.apiFormat ?? resolveDefaultApiFormat(spec.provider);
  const key = spec.apiKey.trim();

  if (spec.provider === 'ollama') {
    const origin = parsedUrl.origin.replace(/\/+$/, '');
    const attempts: ModelListAttempt[] = [
      { url: `${cleanBase}/models`, headers: { accept: 'application/json' } },
    ];
    const tagsUrl = `${origin}/api/tags`;
    if (tagsUrl !== `${cleanBase}/models`) {
      attempts.push({ url: tagsUrl, headers: { accept: 'application/json' } });
    }
    return attempts;
  }

  if (spec.provider === 'anthropic') {
    const modelsUrl = endsWithVersion ? `${cleanBase}/models` : `${cleanBase}/v1/models`;
    return [{
      url: modelsUrl,
      headers: {
        accept: 'application/json',
        'x-api-key': key,
        'anthropic-version': '2023-06-01',
      },
      anthropicPaginated: true,
    }];
  }

  if (spec.provider === 'custom' && effectiveFormat === 'anthropic-messages') {
    const primaryUrl = endsWithVersion ? `${cleanBase}/models` : `${cleanBase}/v1/models`;
    const headers: Record<string, string> = {
      accept: 'application/json',
      'x-api-key': key,
      'anthropic-version': '2023-06-01',
      authorization: `Bearer ${key}`,
    };
    const attempts: ModelListAttempt[] = [{ url: primaryUrl, headers, anthropicPaginated: true }];
    if (!endsWithVersion) {
      attempts.push({ url: `${cleanBase}/models`, headers });
    }
    return attempts;
  }

  const bearerHeaders: Record<string, string> = {
    accept: 'application/json',
    authorization: `Bearer ${key}`,
  };
  const attempts: ModelListAttempt[] = [{ url: `${cleanBase}/models`, headers: bearerHeaders }];
  if (!endsWithVersion) {
    attempts.push({ url: `${cleanBase}/v1/models`, headers: bearerHeaders });
  }
  return attempts;
}

export async function fetchProviderModels(
  spec: PiProviderSpec,
  signal: AbortSignal,
  locale: AppLocale = 'en',
): Promise<RemoteModelOption[]> {
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
  if (spec.provider !== 'ollama' && !spec.apiKey.trim()) {
    throw new LocalizedSidecarError(msg.missingApiKey);
  }

  const attempts = buildModelListAttempts(spec, baseUrl, parsedUrl);
  let lastError: Error | undefined;

  for (let i = 0; i < attempts.length; i += 1) {
    const attempt = attempts[i];
    const hasFallback = i + 1 < attempts.length;
    try {
      const collected: RemoteModelOption[] = [];
      let afterId: string | undefined;

      for (let page = 0; page < MAX_REMOTE_MODEL_PAGES; page += 1) {
        const targetUrl = new URL(attempt.url);
        if (attempt.anthropicPaginated) {
          targetUrl.searchParams.set('limit', '100');
          if (afterId) targetUrl.searchParams.set('after_id', afterId);
        }
        const response = await fetch(targetUrl, {
          method: 'GET',
          headers: attempt.headers,
          signal,
        });
        const bodyText = await response.text();
        if (!response.ok) {
          const detail = bodyText.slice(0, 200).trim();
          const err = new Error(`HTTP ${response.status} ${response.statusText}${detail ? `: ${detail}` : ''}`);
          if (hasFallback && (response.status === 404 || response.status === 405)) {
            lastError = err;
            collected.length = 0;
            break;
          }
          throw err;
        }
        let payload: unknown;
        try {
          payload = JSON.parse(bodyText);
        } catch {
          const err = new Error('Provider returned non-JSON response for model list');
          if (hasFallback) {
            lastError = err;
            collected.length = 0;
            break;
          }
          throw err;
        }
        const pageModels = parseRemoteModelOptions(payload);
        collected.push(...pageModels);

        const root = asObject(payload);
        const hasMore = attempt.anthropicPaginated
          && root?.has_more === true
          && typeof root.last_id === 'string'
          && root.last_id.trim().length > 0
          && collected.length < MAX_REMOTE_MODELS;
        if (!hasMore) {
          return parseRemoteModelOptions(collected);
        }
        afterId = (root.last_id as string).trim();
      }
      if (collected.length > 0 || !hasFallback) {
        return parseRemoteModelOptions(collected);
      }
    } catch (error) {
      if (signal.aborted) throw error;
      lastError = error instanceof Error ? error : new Error(String(error));
      if (!hasFallback) throw lastError;
    }
  }

  throw lastError ?? new Error('Unable to fetch model list from provider');
}
