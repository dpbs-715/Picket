import { describe, expect, it } from 'vitest';
import { getMessages } from '../../i18n/messages';
import {
  editModelEntryInput,
  formatModelDisplayLabel,
  formatTokenCount,
  newModelEntryInput,
  newProviderConnectionInput,
  normalizeCreateProviderConnection,
  normalizeModelEntry,
  providers,
  resolveDefaultTokenLimits,
  validateCreateProviderConnection,
  validateModelEntry,
} from './modelProviders';

describe('Provider connection and model form validation rules', () => {
  it.each(providers)('$label starts with empty credentials and shared default token limits', (provider) => {
    const input = newProviderConnectionInput(provider.id);
    expect(input.apiKey).toBe('');
    expect(input.contextWindow).toBe(provider.defaultContextWindow);
    expect(input.maxOutputTokens).toBe(provider.defaultMaxOutputTokens);
    expect(validateCreateProviderConnection({ ...input, modelId: ' ' })).toBe(
      provider.needsKey ? 'Enter an API key' : 'Enter a model ID',
    );
    expect(validateCreateProviderConnection(input)).toBe(provider.needsKey ? 'Enter an API key' : undefined);
    if (provider.needsUrl) {
      expect(validateCreateProviderConnection({ ...input, baseUrl: '' })).toBe('Enter a base URL');
    }
  });

  it('returns localized Chinese validation messages when locale is zh', () => {
    const zh = getMessages('zh').modelForm.validation;
    const input = newProviderConnectionInput('custom', 'zh');
    expect(validateCreateProviderConnection({ ...input, baseUrl: '' }, 'zh')).toBe(zh.missingBaseUrl);
    expect(validateCreateProviderConnection({ ...input, baseUrl: 'https://api.example.com/v1' }, 'zh')).toBe(zh.missingApiKey);
    expect(
      validateCreateProviderConnection(
        { ...input, baseUrl: 'https://api.example.com/v1', apiKey: 'sk-test', modelId: '' },
        'zh',
      ),
    ).toBe(zh.missingModelId);
  });

  it('requires baseUrl only for ollama and custom, and omits apiKey for ollama', () => {
    expect(providers.filter((provider) => provider.needsUrl).map((provider) => provider.id)).toEqual(['ollama', 'custom']);
    expect(providers.filter((provider) => !provider.needsKey).map((provider) => provider.id)).toEqual(['ollama']);
  });

  it.each(['file:///tmp/key', 'not-a-url', 'https://user:password@example.com/v1'])('rejects unsafe or invalid baseUrl %s', (baseUrl) => {
    expect(validateCreateProviderConnection({ ...newProviderConnectionInput('ollama'), baseUrl })).toBeTruthy();
  });

  it('normalizes provider creation and child model entry cleanly without leaking secrets or URLs', () => {
    expect(normalizeCreateProviderConnection({
      ...newProviderConnectionInput('anthropic'),
      connectionName: '  ',
      modelName: ' ',
      modelId: ' test-model ',
      apiKey: ' placeholder ',
      baseUrl: 'https://irrelevant.example.com',
    })).toEqual({
      provider: 'anthropic',
      connectionName: 'Anthropic',
      baseUrl: '',
      apiKey: 'placeholder',
      modelName: 'test-model',
      modelId: 'test-model',
      apiFormat: 'anthropic-messages',
      contextWindow: 200_000,
      maxOutputTokens: 16_384,
    });
    expect(normalizeCreateProviderConnection({ ...newProviderConnectionInput('ollama'), apiKey: 'must-not-leave-form' }).apiKey).toBe('');

    const conn = {
      id: 'conn-gw',
      name: 'Company Gateway',
      provider: 'custom' as const,
      baseUrl: 'http://127.0.0.1:8317/v1',
      keyHint: 'sk-a…9001',
    };
    const blankChild = newModelEntryInput(conn);
    expect(blankChild.connectionId).toBe('conn-gw');
    expect(blankChild.modelId).toBe('');
    expect(validateModelEntry(conn.provider, blankChild)).toBe('Enter a model ID');

    const editing = editModelEntryInput({
      id: 'model-edit-1',
      connectionId: 'conn-gw',
      name: 'Kimi Proxy',
      provider: 'custom',
      apiFormat: 'anthropic-messages',
      modelId: 'kimi-k2.8',
      baseUrl: 'http://127.0.0.1:8317/v1',
    });
    expect(validateModelEntry('custom', editing)).toBeUndefined();
    expect(normalizeModelEntry('custom', {
      ...editing,
      modelId: 'kimi-k3.1-pro-highspeed',
      contextWindow: 256_000,
      maxOutputTokens: 32_768,
    })).toEqual({
      id: 'model-edit-1',
      connectionId: 'conn-gw',
      name: 'Kimi Proxy',
      modelId: 'kimi-k3.1-pro-highspeed',
      apiFormat: 'anthropic-messages',
      contextWindow: 256_000,
      maxOutputTokens: 32_768,
    });
  });

  it('resolves default token limits and disambiguates duplicate model names with connection names', () => {
    expect(resolveDefaultTokenLimits('custom', 'openai-responses')).toEqual({
      contextWindow: 200_000,
      maxOutputTokens: 32_768,
    });
    expect(validateCreateProviderConnection({ ...newProviderConnectionInput('ollama'), contextWindow: 1024 }))
      .toContain('Context window must be an integer between');
    expect(validateCreateProviderConnection({ ...newProviderConnectionInput('ollama'), maxOutputTokens: 64 }))
      .toContain('Max output tokens must be an integer between');
    expect(validateCreateProviderConnection({ ...newProviderConnectionInput('ollama'), contextWindow: 8_192, maxOutputTokens: 16_384 }))
      .toBe('Max output tokens cannot exceed context window');
    expect(formatTokenCount(200_000)).toBe('200K');
    expect(formatTokenCount(32_768)).toBe('32K');
    expect(formatTokenCount(1_000_000)).toBe('1M');

    const connections = [
      { id: 'c-work', name: 'Company Gateway', provider: 'openai' as const },
      { id: 'c-personal', name: 'Personal OpenAI', provider: 'openai' as const },
    ];
    const models = [
      { id: 'm1', connectionId: 'c-work', name: 'Fast Model', provider: 'openai' as const, modelId: 'gpt-4.1-mini' },
      { id: 'm2', connectionId: 'c-personal', name: 'Fast Model', provider: 'openai' as const, modelId: 'gpt-4.1-mini' },
      { id: 'm3', connectionId: 'c-work', name: 'Review Model', provider: 'openai' as const, modelId: 'gpt-4.1' },
    ];
    expect(formatModelDisplayLabel(models[0], connections, models)).toBe('Company Gateway / Fast Model');
    expect(formatModelDisplayLabel(models[1], connections, models)).toBe('Personal OpenAI / Fast Model');
    expect(formatModelDisplayLabel(models[2], connections, models)).toBe('Review Model');
  });

  it('auto-matches Kimi model ID presets (including vendor-prefixed IDs) without false prefix hits', () => {
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k3.1-pro')).toEqual({
      contextWindow: 1_048_576,
      maxOutputTokens: 131_072,
    });
    expect(resolveDefaultTokenLimits('custom', 'anthropic-messages', 'moonshotai/kimi-k3.1-pro-highspeed')).toEqual({
      contextWindow: 1_048_576,
      maxOutputTokens: 131_072,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2.8')).toEqual({
      contextWindow: 1_048_576,
      maxOutputTokens: 131_072,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2.7-code')).toEqual({
      contextWindow: 262_144,
      maxOutputTokens: 32_768,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2.6')).toEqual({
      contextWindow: 262_144,
      maxOutputTokens: 32_768,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2.5')).toEqual({
      contextWindow: 262_144,
      maxOutputTokens: 32_768,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2-thinking-turbo')).toEqual({
      contextWindow: 262_144,
      maxOutputTokens: 32_768,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2-0905-preview')).toEqual({
      contextWindow: 262_144,
      maxOutputTokens: 16_384,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-k2-0711-preview')).toEqual({
      contextWindow: 131_072,
      maxOutputTokens: 16_384,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'kimi-for-coding')).toEqual({
      contextWindow: 262_144,
      maxOutputTokens: 32_768,
    });
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'moonshot-v1-8k-vision-preview')).toEqual({
      contextWindow: 8_192,
      maxOutputTokens: 4_096,
    });
    // Non-matching or partial numeric lookalike falls back cleanly to provider/format default:
    expect(resolveDefaultTokenLimits('custom', 'openai-completions', 'moonshot-v1-80k')).toEqual({
      contextWindow: 128_000,
      maxOutputTokens: 16_384,
    });
  });
});
