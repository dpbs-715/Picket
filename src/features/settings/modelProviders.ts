import {
  formatTokenCount,
  MODEL_API_FORMAT_DEFAULTS,
  MODEL_PROVIDER_DEFAULTS,
  MODEL_TOKEN_LIMITS,
  resolveDefaultTokenLimits,
  validateTokenLimitsCode,
} from '../../../shared/model-defaults';
import { DEFAULT_APP_LOCALE, type AppLocale } from '../../../shared/language-protocol';
import { MODEL_API_FORMATS, MODEL_PROVIDER_IDS } from '../../../shared/model-protocol';
import type { ModelApiFormat, ModelProviderId } from '../../../shared/model-protocol';
import { getMessages } from '../../i18n/messages';

export { formatTokenCount, MODEL_TOKEN_LIMITS, resolveDefaultTokenLimits };

export const apiFormats = MODEL_API_FORMATS.map((id) => ({
  id,
  label: MODEL_API_FORMAT_DEFAULTS[id].label,
  short: MODEL_API_FORMAT_DEFAULTS[id].short,
}));
export type ApiFormatId = ModelApiFormat;

export const providers = MODEL_PROVIDER_IDS.map((id) => {
  const cfg = MODEL_PROVIDER_DEFAULTS[id];
  return {
    id,
    label: cfg.label,
    defaultApiFormat: cfg.defaultApiFormat,
    needsKey: cfg.needsKey,
    needsUrl: cfg.needsUrl,
    model: cfg.exampleModelId,
    name: cfg.exampleName,
    url: cfg.placeholderUrl,
    defaultContextWindow: cfg.defaultContextWindow,
    defaultMaxOutputTokens: cfg.defaultMaxOutputTokens,
  };
});
export type ProviderId = ModelProviderId;

export interface ModelConfiguration {
  id: string;
  name: string;
  provider: ProviderId;
  apiFormat?: ApiFormatId;
  modelId: string;
  baseUrl?: string;
  contextWindow?: number;
  maxOutputTokens?: number;
}

// Credentials exist only in the open form and the explicit desktop request.
// They must never be included in a workbench snapshot or ordinary settings.
export interface ModelInput {
  id?: string;
  provider: ProviderId;
  apiFormat?: ApiFormatId;
  name: string;
  modelId: string;
  baseUrl: string;
  apiKey: string;
  contextWindow?: number;
  maxOutputTokens?: number;
  locale?: AppLocale;
}

export function providerFor(id: ProviderId) {
  return providers.find((provider) => provider.id === id)!;
}

export function apiFormatFor(id: ApiFormatId) {
  return apiFormats.find((format) => format.id === id)!;
}

export function newModelInput(provider: ProviderId = 'anthropic'): ModelInput {
  const defaults = providerFor(provider);
  const limits = resolveDefaultTokenLimits(provider, defaults.defaultApiFormat);
  return {
    provider,
    apiFormat: defaults.defaultApiFormat,
    name: defaults.name,
    modelId: defaults.model,
    baseUrl: defaults.url,
    apiKey: '',
    contextWindow: limits.contextWindow,
    maxOutputTokens: limits.maxOutputTokens,
  };
}

export function editModelInput(model: ModelConfiguration): ModelInput {
  const defaults = providerFor(model.provider);
  const apiFormat = model.apiFormat ?? defaults.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(model.provider, apiFormat);
  return {
    id: model.id,
    provider: model.provider,
    apiFormat,
    name: model.name,
    modelId: model.modelId,
    baseUrl: model.baseUrl ?? defaults.url,
    apiKey: '',
    contextWindow: model.contextWindow ?? fallbackLimits.contextWindow,
    maxOutputTokens: model.maxOutputTokens ?? fallbackLimits.maxOutputTokens,
  };
}

export function validateModel(
  input: ModelInput,
  options?: { canKeepExistingKey?: boolean; locale?: AppLocale },
): string | undefined {
  const validation = getMessages(options?.locale ?? input.locale ?? DEFAULT_APP_LOCALE).modelForm.validation;
  const provider = providerFor(input.provider);
  if (!input.modelId.trim()) return validation.missingModelId;
  if (provider.needsUrl) {
    if (!input.baseUrl.trim()) return validation.missingBaseUrl;
    try {
      const url = new URL(input.baseUrl.trim());
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
        return validation.invalidUrlCredentialsOrProtocol;
      }
    } catch {
      return validation.invalidUrlFormat;
    }
  }
  if (provider.needsKey && !input.apiKey.trim() && !options?.canKeepExistingKey) {
    return validation.missingApiKey;
  }
  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(input.provider, effectiveFormat);
  const contextWindow = input.contextWindow ?? fallbackLimits.contextWindow;
  const maxOutputTokens = input.maxOutputTokens ?? fallbackLimits.maxOutputTokens;
  const code = validateTokenLimitsCode(contextWindow, maxOutputTokens);
  return code ? validation.tokenLimit(code) : undefined;
}

export function normalizedModel(input: ModelInput): ModelInput {
  const provider = providerFor(input.provider);
  const apiFormat = input.apiFormat ?? provider.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(input.provider, apiFormat);
  return {
    ...(input.id ? { id: input.id } : {}),
    provider: input.provider,
    apiFormat,
    name: input.name.trim() || input.modelId.trim(),
    modelId: input.modelId.trim(),
    baseUrl: provider.needsUrl ? input.baseUrl.trim() : '',
    apiKey: provider.needsKey ? input.apiKey.trim() : '',
    contextWindow: input.contextWindow ?? fallbackLimits.contextWindow,
    maxOutputTokens: input.maxOutputTokens ?? fallbackLimits.maxOutputTokens,
    ...(input.locale ? { locale: input.locale } : {}),
  };
}
