export const apiFormats = [
  { id: 'openai-completions', label: 'OpenAI Chat Completions（/chat/completions）', short: 'Chat Completions' },
  { id: 'openai-responses', label: 'OpenAI Responses（/responses）', short: 'Responses' },
  { id: 'anthropic-messages', label: 'Anthropic Messages（/v1/messages）', short: 'Anthropic' },
] as const;
export type ApiFormatId = typeof apiFormats[number]['id'];

export const providers = [
  { id: 'anthropic', label: 'Anthropic', defaultApiFormat: 'anthropic-messages', needsKey: true, needsUrl: false, model: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4', url: '' },
  { id: 'openai', label: 'OpenAI', defaultApiFormat: 'openai-responses', needsKey: true, needsUrl: false, model: 'gpt-4.1', name: 'GPT-4.1', url: '' },
  { id: 'deepseek', label: 'DeepSeek', defaultApiFormat: 'openai-completions', needsKey: true, needsUrl: false, model: 'deepseek-chat', name: 'DeepSeek V3', url: '' },
  { id: 'ollama', label: 'Ollama（本地）', defaultApiFormat: 'openai-completions', needsKey: false, needsUrl: true, model: 'qwen2.5-coder:14b', name: 'Qwen 2.5 Coder', url: 'http://127.0.0.1:11434/v1' },
  { id: 'custom', label: '自定义兼容', defaultApiFormat: 'openai-completions', needsKey: true, needsUrl: true, model: 'custom-code-model', name: '自定义模型', url: 'https://api.example.com/v1' },
] as const;
export type ProviderId = typeof providers[number]['id'];

export interface ModelConfiguration {
  id: string;
  name: string;
  provider: ProviderId;
  apiFormat?: ApiFormatId;
  modelId: string;
  baseUrl?: string;
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
}

export function providerFor(id: ProviderId) { return providers.find((provider) => provider.id === id)!; }
export function apiFormatFor(id: ApiFormatId) { return apiFormats.find((format) => format.id === id)!; }
export function newModelInput(provider: ProviderId = 'anthropic'): ModelInput {
  const defaults = providerFor(provider);
  return { provider, apiFormat: defaults.defaultApiFormat, name: defaults.name, modelId: defaults.model, baseUrl: defaults.url, apiKey: '' };
}
export function editModelInput(model: ModelConfiguration): ModelInput {
  const defaults = providerFor(model.provider);
  return {
    id: model.id,
    provider: model.provider,
    apiFormat: model.apiFormat ?? defaults.defaultApiFormat,
    name: model.name,
    modelId: model.modelId,
    baseUrl: model.baseUrl ?? defaults.url,
    apiKey: '',
  };
}
export function validateModel(input: ModelInput, options?: { canKeepExistingKey?: boolean }): string | undefined {
  const provider = providerFor(input.provider);
  if (!input.modelId.trim()) return '请填写模型 ID';
  if (provider.needsUrl) {
    if (!input.baseUrl.trim()) return '请填写接口地址';
    try {
      const url = new URL(input.baseUrl.trim());
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) return '接口地址须为不含用户名和密码的 HTTP / HTTPS 地址';
    } catch { return '请填写有效的 HTTP / HTTPS 接口地址'; }
  }
  if (provider.needsKey && !input.apiKey.trim() && !options?.canKeepExistingKey) return '请填写 API Key';
}
export function normalizedModel(input: ModelInput): ModelInput {
  const provider = providerFor(input.provider);
  return {
    ...(input.id ? { id: input.id } : {}),
    provider: input.provider,
    apiFormat: input.apiFormat ?? provider.defaultApiFormat,
    name: input.name.trim() || input.modelId.trim(),
    modelId: input.modelId.trim(),
    baseUrl: provider.needsUrl ? input.baseUrl.trim() : '',
    apiKey: provider.needsKey ? input.apiKey.trim() : '',
  };
}
