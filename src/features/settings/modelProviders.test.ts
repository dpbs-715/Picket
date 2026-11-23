import { describe, expect, it } from 'vitest';
import { editModelInput, newModelInput, normalizedModel, providers, validateModel } from './modelProviders';

describe('模型表单字段规则', () => {
  it.each(providers)('$label 没有预填凭据，字段要求符合定稿', (provider) => {
    const input = newModelInput(provider.id);
    expect(input.apiKey).toBe('');
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
  it('空名称回退 ID，隐藏字段不随请求携带，编辑模式允许留空保留原钥匙串密钥并支持自定义接口格式', () => {
    expect(normalizedModel({ ...newModelInput('anthropic'), name: ' ', modelId: ' test-model ', apiKey: ' placeholder ', baseUrl: 'https://irrelevant.example.com' }))
      .toEqual({ provider: 'anthropic', apiFormat: 'anthropic-messages', name: 'test-model', modelId: 'test-model', apiKey: 'placeholder', baseUrl: '' });
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
    expect(validateModel(editing, { canKeepExistingKey: true })).toBeUndefined();
    expect(validateModel(editing, { canKeepExistingKey: false })).toBe('请填写 API Key');
    expect(normalizedModel({ ...editing, modelId: 'kimi-k3.1-pro-highspeed' })).toEqual({
      id: 'model-edit-1',
      provider: 'custom',
      apiFormat: 'anthropic-messages',
      name: 'Kimi Proxy',
      modelId: 'kimi-k3.1-pro-highspeed',
      baseUrl: 'http://127.0.0.1:8317/v1',
      apiKey: '',
    });
  });
});
