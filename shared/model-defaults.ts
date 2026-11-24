import rawDefaults from './model-defaults.json';
import type { ModelApiFormat, ModelProviderId } from './model-protocol';

export interface ProviderDefaultConfig {
  label: string;
  defaultApiFormat: ModelApiFormat;
  needsKey: boolean;
  needsUrl: boolean;
  defaultBaseUrl: string;
  placeholderUrl: string;
  exampleModelId: string;
  exampleName: string;
  defaultContextWindow: number;
  defaultMaxOutputTokens: number;
}

export interface ApiFormatDefaultConfig {
  label: string;
  short: string;
  defaultContextWindow: number;
  defaultMaxOutputTokens: number;
}

export const MODEL_TOKEN_LIMITS = rawDefaults.limits;
export const MODEL_GLOBAL_DEFAULTS = rawDefaults.defaults;
export const MODEL_API_FORMAT_DEFAULTS = rawDefaults.apiFormats as Record<ModelApiFormat, ApiFormatDefaultConfig>;
export const MODEL_PROVIDER_DEFAULTS = rawDefaults.providers as Record<ModelProviderId, ProviderDefaultConfig>;

export function resolveDefaultApiFormat(provider: ModelProviderId): ModelApiFormat {
  return MODEL_PROVIDER_DEFAULTS[provider].defaultApiFormat;
}

export function resolveDefaultBaseUrl(provider: ModelProviderId): string {
  return MODEL_PROVIDER_DEFAULTS[provider].defaultBaseUrl;
}

export function resolveDefaultTokenLimits(
  provider: ModelProviderId,
  apiFormat?: ModelApiFormat,
): { contextWindow: number; maxOutputTokens: number } {
  const providerCfg = MODEL_PROVIDER_DEFAULTS[provider];
  const effectiveFormat = apiFormat ?? providerCfg.defaultApiFormat;
  if (effectiveFormat === providerCfg.defaultApiFormat) {
    return {
      contextWindow: providerCfg.defaultContextWindow,
      maxOutputTokens: providerCfg.defaultMaxOutputTokens,
    };
  }
  const formatCfg = MODEL_API_FORMAT_DEFAULTS[effectiveFormat];
  return {
    contextWindow: formatCfg.defaultContextWindow,
    maxOutputTokens: formatCfg.defaultMaxOutputTokens,
  };
}

export type TokenLimitValidationCode = 'context_range' | 'output_range' | 'output_exceeds_context';

export function validateTokenLimitsCode(
  contextWindow: number,
  maxOutputTokens: number,
): TokenLimitValidationCode | undefined {
  const { minContextWindow, maxContextWindow, minMaxOutputTokens, maxMaxOutputTokens } = MODEL_TOKEN_LIMITS;
  if (!Number.isSafeInteger(contextWindow) || contextWindow < minContextWindow || contextWindow > maxContextWindow) {
    return 'context_range';
  }
  if (!Number.isSafeInteger(maxOutputTokens) || maxOutputTokens < minMaxOutputTokens || maxOutputTokens > maxMaxOutputTokens) {
    return 'output_range';
  }
  if (maxOutputTokens > contextWindow) {
    return 'output_exceeds_context';
  }
  return undefined;
}

export function validateTokenLimits(contextWindow: number, maxOutputTokens: number): string | undefined {
  const { minContextWindow, maxContextWindow, minMaxOutputTokens, maxMaxOutputTokens } = MODEL_TOKEN_LIMITS;
  const code = validateTokenLimitsCode(contextWindow, maxOutputTokens);
  if (code === 'context_range') {
    return `Context window must be an integer between ${minContextWindow} and ${maxContextWindow}`;
  }
  if (code === 'output_range') {
    return `Max output tokens must be an integer between ${minMaxOutputTokens} and ${maxMaxOutputTokens}`;
  }
  if (code === 'output_exceeds_context') {
    return 'Max output tokens cannot exceed context window';
  }
  return undefined;
}

export function formatTokenCount(tokens: number): string {
  if (tokens >= 1_000_000 && tokens % 1_000_000 === 0) {
    return `${tokens / 1_000_000}M`;
  }
  if (tokens >= 1_048_576 && tokens % 1_048_576 === 0) {
    return `${tokens / 1_048_576}M`;
  }
  if (tokens >= 1_000 && tokens % 1_000 === 0) {
    return `${tokens / 1_000}K`;
  }
  if (tokens >= 1_024 && tokens % 1_024 === 0) {
    return `${tokens / 1_024}K`;
  }
  return String(tokens);
}
