import { normalizeAppLocale, type AppLocale } from '../../shared/language-protocol';

export class LocalizedSidecarError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'LocalizedSidecarError';
  }
}

export interface SidecarMessages {
  missingBaseUrl: string;
  invalidBaseUrl: string;
  unsupportedBaseUrlProtocol: string;
  missingApiKey: string;
  unknownModelError: string;
  authFailed: (detail: string) => string;
  endpointOrModelNotFound: (detail: string) => string;
  rateLimitedOrQuota: (detail: string) => string;
  providerServiceError: (detail: string) => string;
  connectionFailed: (detail: string) => string;
  requestTimedOut: (detail: string) => string;
  toolsUnsupported: (detail: string) => string;
  requestFailed: (detail: string) => string;
  emptyCommitMessage: string;
  noResponseMessage: string;
  providerReturnedError: string;
  lengthTruncatedJson: string;
  thinkingTokensExhausted: string;
  defaultCustomActionSummary: string;
  invalidCustomActionDraftJson: string;
  missingCustomActionDraftFields: string;
  defaultCustomActionDraftSummary: string;
}

const SIDECAR_MESSAGES: Record<AppLocale, SidecarMessages> = {
  en: {
    missingBaseUrl: 'Missing model service Base URL.',
    invalidBaseUrl: 'Model service Base URL must be a valid HTTP or HTTPS URL.',
    unsupportedBaseUrlProtocol: 'Model service Base URL only supports HTTP or HTTPS without embedded credentials.',
    missingApiKey: 'Missing model API Key.',
    unknownModelError: 'Unknown model error',
    authFailed: (detail) => `Authentication failed (401/403). Verify your API key and permissions (${detail})`,
    endpointOrModelNotFound: (detail) => `Endpoint or model not found (404). Verify the model ID and Base URL (${detail})`,
    rateLimitedOrQuota: (detail) => `Rate limit or quota exceeded (402/429). Check your account quota or retry shortly (${detail})`,
    providerServiceError: (detail) => `Model provider service error (5xx). Please retry shortly (${detail})`,
    connectionFailed: (detail) => `Unable to connect to model service. Check your network or local server (${detail})`,
    requestTimedOut: (detail) => `Model request timed out. Please retry shortly (${detail})`,
    toolsUnsupported: (detail) => `The selected model does not support tool calling; switch to a tool-capable model in Model Settings (${detail})`,
    requestFailed: (detail) => `Model request failed: ${detail}`,
    emptyCommitMessage: 'Model returned an empty commit message.',
    noResponseMessage: 'Model returned no response message.',
    providerReturnedError: 'Model service returned an error.',
    lengthTruncatedJson: 'Model output was truncated due to max output tokens before completing structured JSON; narrow the selected file scope and retry.',
    thinkingTokensExhausted: 'Model thinking tokens exhausted max output tokens before generating final output; narrow the selected scope or switch to a non-reasoning model.',
    defaultCustomActionSummary: 'Custom action completed.',
    invalidCustomActionDraftJson: 'Picket did not return a valid button draft JSON object.',
    missingCustomActionDraftFields: 'Generated button draft is missing a label or prompt.',
    defaultCustomActionDraftSummary: 'Generated from repository context; review or edit before saving.',
  },
  zh: {
    missingBaseUrl: '缺少模型服务接口地址（Base URL）',
    invalidBaseUrl: '模型服务接口地址不是有效的 HTTP / HTTPS URL',
    unsupportedBaseUrlProtocol: '模型服务接口地址仅支持不含账号密码的 HTTP / HTTPS 协议',
    missingApiKey: '缺少模型 API Key',
    unknownModelError: '未知模型错误',
    authFailed: (detail) => `鉴权失败，请检查 API Key 或服务权限（${detail}）`,
    endpointOrModelNotFound: (detail) => `未找到对应接口或模型，请检查模型 ID 与接口地址（${detail}）`,
    rateLimitedOrQuota: (detail) => `请求过于频繁或账户额度不足，请稍后重试（${detail}）`,
    providerServiceError: (detail) => `模型服务异常（5xx），请稍后重试（${detail}）`,
    connectionFailed: (detail) => `无法连接到模型服务，请确认网络或本地服务已启动（${detail}）`,
    requestTimedOut: (detail) => `连接模型服务超时，请稍后重试（${detail}）`,
    toolsUnsupported: (detail) => `当前选中的模型不支持工具调用（Tool Calling），请在模型设置中切换支持工具调用的模型（${detail}）`,
    requestFailed: (detail) => `模型请求未完成：${detail}`,
    emptyCommitMessage: '模型返回了空的提交说明',
    noResponseMessage: '模型未返回响应消息',
    providerReturnedError: '模型服务返回错误',
    lengthTruncatedJson: '模型输出达到长度上限被截断，未能生成完整的结构化结果；请缩小单次勾选文件范围后重试',
    thinkingTokensExhausted: '模型思考内容耗尽了输出长度上限，未生成最终正文；请缩小所选变更范围或改用非深度思考模型重试',
    defaultCustomActionSummary: '自定义动作已执行完成。',
    invalidCustomActionDraftJson: 'Picket 未返回有效的按钮草稿 JSON。',
    missingCustomActionDraftFields: '生成的按钮草稿缺少名称或指令内容。',
    defaultCustomActionDraftSummary: '已根据仓库上下文生成按钮草稿，你可以微调后保存。',
  },
};

export function resolveSidecarLocale(locale?: unknown): AppLocale {
  return normalizeAppLocale(locale, 'en');
}

export function getSidecarMessages(locale?: unknown): SidecarMessages {
  return SIDECAR_MESSAGES[resolveSidecarLocale(locale)];
}
