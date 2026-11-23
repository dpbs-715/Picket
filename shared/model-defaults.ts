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

export function validateTokenLimits(contextWindow: number, maxOutputTokens: number): string | undefined {
  const { minContextWindow, maxContextWindow, minMaxOutputTokens, maxMaxOutputTokens } = MODEL_TOKEN_LIMITS;
  if (!Number.isSafeInteger(contextWindow) || contextWindow < minContextWindow || contextWindow > maxContextWindow) {
    return `上下文长度须为 ${minContextWindow} 至 ${maxContextWindow} 之间的整数`;
  }
  if (!Number.isSafeInteger(maxOutputTokens) || maxOutputTokens < minMaxOutputTokens || maxOutputTokens > maxMaxOutputTokens) {
    return `最大输出 Token 须为 ${minMaxOutputTokens} 至 ${maxMaxOutputTokens} 之间的整数`;
  }
  if (maxOutputTokens > contextWindow) {
    return '最大输出 Token 不能超过上下文长度';
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
