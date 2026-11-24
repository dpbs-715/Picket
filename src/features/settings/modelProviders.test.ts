import { describe, expect, it } from 'vitest';
import { getMessages } from '../../i18n/messages';
import {
  editModelInput,
  formatTokenCount,
  newModelInput,
  normalizedModel,
  providers,
  resolveDefaultTokenLimits,
  validateModel,
} from './modelProviders';

describe('Model form field rules and localized validation', () => {
  it.each(providers)('$label starts with empty credentials and shared default token limits', (provider) => {
    const input = newModelInput(provider.id);
    expect(input.apiKey).toBe('');
    expect(input.contextWindow).toBe(provider.defaultContextWindow);
    expect(input.maxOutputTokens).toBe(provider.defaultMaxOutputTokens);
    expect(validateModel({ ...input, modelId: ' ' })).toBe('Enter a model ID');
    expect(validateModel(input)).toBe(provider.needsKey ? 'Enter an API key' : undefined);
    if (provider.needsUrl) expect(validateModel({ ...input, baseUrl: '' })).toBe('Enter a base URL');
  });

  it('returns localized Chinese validation messages when locale is zh', () => {
    const zh = getMessages('zh').modelForm.validation;
    const input = newModelInput('custom');
    expect(validateModel({ ...input, modelId: '' }, { locale: 'zh' })).toBe(zh.missingModelId);
    expect(validateModel({ ...input, baseUrl: '' }, { locale: 'zh' })).toBe(zh.missingBaseUrl);
    expect(validateModel({ ...input, baseUrl: 'https://api.example.com/v1' }, { locale: 'zh' })).toBe(zh.missingApiKey);
  });

  it('requires baseUrl only for ollama and custom, and omits apiKey for ollama', () => {
    expect(providers.filter((provider) => provider.needsUrl).map((provider) => provider.id)).toEqual(['ollama', 'custom']);
    expect(providers.filter((provider) => !provider.needsKey).map((provider) => provider.id)).toEqual(['ollama']);
  });

  it.each(['file:///tmp/key', 'not-a-url', 'https://user:password@example.com/v1'])('rejects unsafe or invalid baseUrl %s', (baseUrl) => {
    expect(validateModel({ ...newModelInput('ollama'), baseUrl })).toBeTruthy();
  });

  it('falls back empty name to modelId, strips hidden fields, and supports retaining existing Keychain key on edit', () => {
    expect(normalizedModel({
      ...newModelInput('anthropic'),
      name: ' ',
      modelId: ' test-model ',
      apiKey: ' placeholder ',
      baseUrl: 'https://irrelevant.example.com',
    })).toEqual({
      provider: 'anthropic',
      apiFormat: 'anthropic-messages',
      name: 'test-model',
      modelId: 'test-model',
      apiKey: 'placeholder',
      baseUrl: '',
      contextWindow: 200_000,
      maxOutputTokens: 16_384,
    });
    expect(normalizedModel({ ...newModelInput('ollama'), apiKey: 'must-not-leave-form' }).apiKey).toBe('');

    const editing = editModelInput({
      id: 'model-edit-1',
      name: 'Kimi Proxy',
      provider: 'custom',
      apiFormat: 'anthropic-messages',
      modelId: 'kimi-k2.8',
      baseUrl: 'http://127.0.0.1:8317/v1',
    });
    expect(editing.apiKey).toBe('');
    expect(editing.contextWindow).toBe(200_000);
    expect(editing.maxOutputTokens).toBe(16_384);
    expect(validateModel(editing, { canKeepExistingKey: true })).toBeUndefined();
    expect(validateModel(editing, { canKeepExistingKey: false })).toBe('Enter an API key');
    expect(normalizedModel({
      ...editing,
      modelId: 'kimi-k3.1-pro-highspeed',
      contextWindow: 256_000,
      maxOutputTokens: 32_768,
    })).toEqual({
      id: 'model-edit-1',
      provider: 'custom',
      apiFormat: 'anthropic-messages',
      name: 'Kimi Proxy',
      modelId: 'kimi-k3.1-pro-highspeed',
      baseUrl: 'http://127.0.0.1:8317/v1',
      apiKey: '',
      contextWindow: 256_000,
      maxOutputTokens: 32_768,
    });
  });

  it('resolves default context and output token limits by provider and API format, rejecting out-of-range values', () => {
    expect(resolveDefaultTokenLimits('custom', 'openai-responses')).toEqual({
      contextWindow: 200_000,
      maxOutputTokens: 32_768,
    });
    expect(validateModel({ ...newModelInput('ollama'), contextWindow: 1024 })).toContain('Context window must be an integer between');
    expect(validateModel({ ...newModelInput('ollama'), maxOutputTokens: 64 })).toContain('Max output tokens must be an integer between');
    expect(validateModel({ ...newModelInput('ollama'), contextWindow: 8_192, maxOutputTokens: 16_384 }))
      .toBe('Max output tokens cannot exceed context window');
    expect(formatTokenCount(200_000)).toBe('200K');
    expect(formatTokenCount(32_768)).toBe('32K');
    expect(formatTokenCount(1_000_000)).toBe('1M');
  });
});
