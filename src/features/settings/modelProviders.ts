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
import type {
  CreateProviderConnectionInput,
  ModelApiFormat,
  ModelProviderId,
  SavedModelConfiguration,
  SavedProviderConnection,
  SaveModelInput,
  UpdateProviderConnectionInput,
} from '../../../shared/model-protocol';
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

export type ProviderConnection = SavedProviderConnection;
export type ModelConfiguration = SavedModelConfiguration;

export function providerFor(id: ProviderId) {
  return providers.find((provider) => provider.id === id)!;
}

export function apiFormatFor(id: ApiFormatId) {
  return apiFormats.find((format) => format.id === id)!;
}

export function formatModelDisplayLabel(
  model: ModelConfiguration,
  connections: readonly ProviderConnection[],
  allModels: readonly ModelConfiguration[],
): string {
  const hasDuplicateName = allModels.some(
    (other) => other.id !== model.id && other.name.trim() === model.name.trim(),
  );
  if (!hasDuplicateName) return model.name;
  const conn = connections.find((item) => item.id === model.connectionId);
  return conn ? `${conn.name} / ${model.name}` : model.name;
}

export function newProviderConnectionInput(
  provider: ProviderId = 'anthropic',
  locale: AppLocale = DEFAULT_APP_LOCALE,
): CreateProviderConnectionInput {
  const defaults = providerFor(provider);
  const limits = resolveDefaultTokenLimits(provider, defaults.defaultApiFormat);
  return {
    provider,
    connectionName: getMessages(locale).providers[provider],
    baseUrl: defaults.url,
    apiKey: '',
    modelName: defaults.name,
    modelId: defaults.model,
    apiFormat: defaults.defaultApiFormat,
    contextWindow: limits.contextWindow,
    maxOutputTokens: limits.maxOutputTokens,
  };
}

export function editProviderConnectionInput(
  connection: ProviderConnection,
): UpdateProviderConnectionInput {
  return {
    connectionId: connection.id,
    name: connection.name,
    apiKey: '',
  };
}

export function newModelEntryInput(connection: ProviderConnection): SaveModelInput {
  const defaults = providerFor(connection.provider);
  const limits = resolveDefaultTokenLimits(connection.provider, defaults.defaultApiFormat);
  return {
    connectionId: connection.id,
    name: '',
    modelId: '',
    apiFormat: defaults.defaultApiFormat,
    contextWindow: limits.contextWindow,
    maxOutputTokens: limits.maxOutputTokens,
  };
}

export function editModelEntryInput(model: ModelConfiguration): SaveModelInput {
  const defaults = providerFor(model.provider);
  const apiFormat = model.apiFormat ?? defaults.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(model.provider, apiFormat);
  return {
    id: model.id,
    connectionId: model.connectionId,
    name: model.name,
    modelId: model.modelId,
    apiFormat,
    contextWindow: model.contextWindow ?? fallbackLimits.contextWindow,
    maxOutputTokens: model.maxOutputTokens ?? fallbackLimits.maxOutputTokens,
  };
}

export function validateBaseUrl(raw: string, locale: AppLocale = DEFAULT_APP_LOCALE): string | undefined {
  const validation = getMessages(locale).modelForm.validation;
  const trimmed = raw.trim();
  if (!trimmed) return validation.missingBaseUrl;
  try {
    const url = new URL(trimmed);
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
      return validation.invalidUrlCredentialsOrProtocol;
    }
  } catch {
    return validation.invalidUrlFormat;
  }
  return undefined;
}

export function validateCreateProviderConnection(
  input: CreateProviderConnectionInput,
  locale: AppLocale = input.locale ?? DEFAULT_APP_LOCALE,
): string | undefined {
  const validation = getMessages(locale).modelForm.validation;
  const provider = providerFor(input.provider);
  if (provider.needsUrl) {
    const urlErr = validateBaseUrl(input.baseUrl, locale);
    if (urlErr) return urlErr;
  }
  if (provider.needsKey && !input.apiKey.trim()) {
    return validation.missingApiKey;
  }
  if (!input.modelId.trim()) return validation.missingModelId;
  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(input.provider, effectiveFormat);
  const contextWindow = input.contextWindow ?? fallbackLimits.contextWindow;
  const maxOutputTokens = input.maxOutputTokens ?? fallbackLimits.maxOutputTokens;
  const code = validateTokenLimitsCode(contextWindow, maxOutputTokens);
  return code ? validation.tokenLimit(code) : undefined;
}

export function validateModelEntry(
  providerId: ProviderId,
  input: SaveModelInput,
  locale: AppLocale = input.locale ?? DEFAULT_APP_LOCALE,
): string | undefined {
  const validation = getMessages(locale).modelForm.validation;
  const provider = providerFor(providerId);
  if (!input.modelId.trim()) return validation.missingModelId;
  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(providerId, effectiveFormat);
  const contextWindow = input.contextWindow ?? fallbackLimits.contextWindow;
  const maxOutputTokens = input.maxOutputTokens ?? fallbackLimits.maxOutputTokens;
  const code = validateTokenLimitsCode(contextWindow, maxOutputTokens);
  return code ? validation.tokenLimit(code) : undefined;
}

export function normalizeCreateProviderConnection(
  input: CreateProviderConnectionInput,
  locale: AppLocale = input.locale ?? DEFAULT_APP_LOCALE,
): CreateProviderConnectionInput {
  const provider = providerFor(input.provider);
  const apiFormat = input.apiFormat ?? provider.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(input.provider, apiFormat);
  return {
    provider: input.provider,
    connectionName: input.connectionName.trim() || getMessages(locale).providers[input.provider],
    baseUrl: provider.needsUrl ? input.baseUrl.trim() : '',
    apiKey: provider.needsKey ? input.apiKey.trim() : '',
    modelName: input.modelName.trim() || input.modelId.trim(),
    modelId: input.modelId.trim(),
    apiFormat,
    contextWindow: input.contextWindow ?? fallbackLimits.contextWindow,
    maxOutputTokens: input.maxOutputTokens ?? fallbackLimits.maxOutputTokens,
    ...(input.keepActiveModel ? { keepActiveModel: true } : {}),
    ...(input.locale ? { locale: input.locale } : {}),
  };
}

export function normalizeModelEntry(
  providerId: ProviderId,
  input: SaveModelInput,
): SaveModelInput {
  const provider = providerFor(providerId);
  const apiFormat = input.apiFormat ?? provider.defaultApiFormat;
  const fallbackLimits = resolveDefaultTokenLimits(providerId, apiFormat);
  return {
    ...(input.id ? { id: input.id } : {}),
    connectionId: input.connectionId,
    name: input.name.trim() || input.modelId.trim(),
    modelId: input.modelId.trim(),
    apiFormat,
    contextWindow: input.contextWindow ?? fallbackLimits.contextWindow,
    maxOutputTokens: input.maxOutputTokens ?? fallbackLimits.maxOutputTokens,
    ...(input.locale ? { locale: input.locale } : {}),
  };
}
