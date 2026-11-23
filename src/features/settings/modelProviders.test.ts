import { describe, expect, it } from 'vitest';
import {
  editModelInput,
  formatTokenCount,
  newModelInput,
  normalizedModel,
  providers,
  resolveDefaultTokenLimits,
  validateModel,
} from './modelProviders';

describe('模型表单字段规则', () => {
  it.each(providers)('$label 没有预填凭据，字段要求与默认 Token 配置符合定稿', (provider) => {
    const input = newModelInput(provider.id);
    expect(input.apiKey).toBe('');
    expect(input.contextWindow).toBe(provider.defaultContextWindow);
    expect(input.maxOutputTokens).toBe(provider.defaultMaxOutputTokens);
    expect(validateModel({ ...input, modelId: ' ' })).toBe('请填写模型 ID');
    expect(validateModel(input)).toBe(provider.needsKey ? '请填写 API Key' : undefined);
    if (provider.needsUrl) expect(validateModel({ ...input, baseUrl: '' })).toBe('请填写接口地址');
  });

  it('仅本地 / 自定义需要地址，Ollama 不要求 Key', () => {
    expect(providers.filter((provider) => provider.needsUrl).map((provider) => provider.id)).toEqual(['ollama', 'custom']);
    expect(providers.filter((provider) => !provider.needsKey).map((provider) => provider.id)).toEqual(['ollama']);
  });

  it.each(['file:///tmp/key', 'not-a-url', 'https://user:password@example.com/v1'])('拒绝不安全或无效地址 %s', (baseUrl) => {
    expect(validateModel({ ...newModelInput('ollama'), baseUrl })).toBeTruthy();
  });

  it('空名称回退 ID，隐藏字段不随请求携带，编辑模式允许留空保留原钥匙串密钥并支持自定义接口格式与 Token 配置', () => {
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
    expect(validateModel(editing, { canKeepExistingKey: false })).toBe('请填写 API Key');
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

  it('按提供商与接口格式计算默认上下文与输出上限，并拦截越界配置', () => {
    expect(resolveDefaultTokenLimits('custom', 'openai-responses')).toEqual({
      contextWindow: 200_000,
      maxOutputTokens: 32_768,
    });
    expect(validateModel({ ...newModelInput('ollama'), contextWindow: 1024 })).toContain('上下文长度须为');
    expect(validateModel({ ...newModelInput('ollama'), maxOutputTokens: 64 })).toContain('最大输出 Token 须为');
    expect(validateModel({ ...newModelInput('ollama'), contextWindow: 8_192, maxOutputTokens: 16_384 }))
      .toBe('最大输出 Token 不能超过上下文长度');
    expect(formatTokenCount(200_000)).toBe('200K');
    expect(formatTokenCount(32_768)).toBe('32K');
    expect(formatTokenCount(1_000_000)).toBe('1M');
  });
});
