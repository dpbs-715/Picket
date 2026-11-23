import type { GitReadTarget } from './git-read-protocol';

export const MODEL_PROTOCOL_VERSION = 1;

export const MODEL_PROVIDER_IDS = ['anthropic', 'openai', 'deepseek', 'ollama', 'custom'] as const;
export type ModelProviderId = typeof MODEL_PROVIDER_IDS[number];

export const MODEL_API_FORMATS = ['openai-completions', 'openai-responses', 'anthropic-messages'] as const;
export type ModelApiFormat = typeof MODEL_API_FORMATS[number];

export interface SavedModelConfiguration {
  id: string;
  name: string;
  provider: ModelProviderId;
  apiFormat?: ModelApiFormat;
  modelId: string;
  baseUrl?: string;
}

export interface ModelInputPayload {
  id?: string;
  provider: ModelProviderId;
  apiFormat?: ModelApiFormat;
  name: string;
  modelId: string;
  baseUrl: string;
  apiKey: string;
}

export interface ModelTestReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  provider: ModelProviderId;
  modelId: string;
  summary: string;
  latencyMs: number;
}

export interface ModelSaveReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  model: SavedModelConfiguration;
  activeModelId: string;
}

export interface ModelMutateReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  models: SavedModelConfiguration[];
  activeModelId: string | null;
}

export interface ModelFileSelection {
  fileId: string;
  commitToken: string;
}

export interface GenerateCommitRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  draft: string;
}

export interface GenerateCommitReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  kind: 'generate';
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  text: string;
  skippedPaths: string[];
}

export interface ReviewFindingWire {
  id: string;
  fileId: string;
  path: string;
  line: number;
  severity: 'high' | 'suggestion';
  title: string;
  reason: string;
}

export interface ReviewScopeRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  draft: string;
  changedPaths?: string[];
}

export interface ReviewScopeReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  kind: 'review';
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  findings: ReviewFindingWire[];
  skippedPaths: string[];
  reviewedFileIds: string[];
}

export type ModelStreamEvent =
  | { type: 'started'; requestId: string; kind: 'generate' | 'review'; target: GitReadTarget }
  | { type: 'delta'; requestId: string; kind: 'generate' | 'review'; target: GitReadTarget; delta: string };

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

export function isModelProviderId(value: unknown): value is ModelProviderId {
  return typeof value === 'string' && (MODEL_PROVIDER_IDS as readonly string[]).includes(value);
}

export function isModelApiFormat(value: unknown): value is ModelApiFormat {
  return typeof value === 'string' && (MODEL_API_FORMATS as readonly string[]).includes(value);
}

export function decodeSavedModelConfiguration(value: unknown): SavedModelConfiguration {
  if (!record(value)
    || 'apiKey' in value
    || 'credentialRef' in value
    || typeof value.id !== 'string' || !value.id
    || typeof value.name !== 'string' || !value.name
    || !isModelProviderId(value.provider)
    || !(value.apiFormat === undefined || value.apiFormat === null || isModelApiFormat(value.apiFormat))
    || typeof value.modelId !== 'string' || !value.modelId
    || !(value.baseUrl === undefined || value.baseUrl === null || typeof value.baseUrl === 'string')) {
    throw new Error('模型配置协议不匹配或包含非法字段。');
  }
  return {
    id: value.id,
    name: value.name,
    provider: value.provider,
    ...(isModelApiFormat(value.apiFormat) ? { apiFormat: value.apiFormat } : {}),
    modelId: value.modelId,
    ...(typeof value.baseUrl === 'string' && value.baseUrl ? { baseUrl: value.baseUrl } : {}),
  };
}

function decodeTarget(value: unknown): GitReadTarget {
  if (!record(value)
    || typeof value.repositoryId !== 'string' || !value.repositoryId
    || typeof value.worktreeId !== 'string' || !value.worktreeId
    || typeof value.sessionId !== 'string' || !value.sessionId
    || typeof value.scopeVersion !== 'string' || !value.scopeVersion) {
    throw new Error('模型响应缺少有效的目标身份。');
  }
  return {
    repositoryId: value.repositoryId,
    worktreeId: value.worktreeId,
    sessionId: value.sessionId,
    scopeVersion: value.scopeVersion,
  };
}

export function decodeModelTestReply(value: unknown): ModelTestReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || !isModelProviderId(value.provider)
    || typeof value.modelId !== 'string' || !value.modelId
    || typeof value.summary !== 'string' || !value.summary
    || typeof value.latencyMs !== 'number' || !Number.isFinite(value.latencyMs) || value.latencyMs < 0) {
    throw new Error('模型连接测试回执不匹配。');
  }
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    provider: value.provider,
    modelId: value.modelId,
    summary: value.summary,
    latencyMs: value.latencyMs,
  };
}

export function decodeModelSaveReply(value: unknown): ModelSaveReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || typeof value.activeModelId !== 'string' || !value.activeModelId) {
    throw new Error('模型保存回执不匹配。');
  }
  const model = decodeSavedModelConfiguration(value.model);
  return { protocolVersion: MODEL_PROTOCOL_VERSION, model, activeModelId: value.activeModelId };
}

export function decodeModelMutateReply(value: unknown): ModelMutateReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || !Array.isArray(value.models)
    || !(value.activeModelId === null || typeof value.activeModelId === 'string')) {
    throw new Error('模型管理回执不匹配。');
  }
  const models = value.models.map(decodeSavedModelConfiguration);
  return { protocolVersion: MODEL_PROTOCOL_VERSION, models, activeModelId: value.activeModelId };
}

export function decodeGenerateCommitReply(value: unknown): GenerateCommitReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || value.kind !== 'generate'
    || 'apiKey' in value
    || typeof value.requestId !== 'string' || !value.requestId
    || typeof value.modelId !== 'string' || !value.modelId
    || typeof value.text !== 'string' || !value.text.trim()
    || !Array.isArray(value.skippedPaths) || !value.skippedPaths.every((item) => typeof item === 'string')) {
    throw new Error('提交说明生成回执不匹配。');
  }
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    kind: 'generate',
    requestId: value.requestId,
    target: decodeTarget(value.target),
    modelId: value.modelId,
    text: value.text,
    skippedPaths: value.skippedPaths as string[],
  };
}

export function decodeReviewScopeReply(value: unknown): ReviewScopeReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || value.kind !== 'review'
    || 'apiKey' in value
    || typeof value.requestId !== 'string' || !value.requestId
    || typeof value.modelId !== 'string' || !value.modelId
    || !Array.isArray(value.findings)
    || !Array.isArray(value.skippedPaths) || !value.skippedPaths.every((item) => typeof item === 'string')
    || !Array.isArray(value.reviewedFileIds) || !value.reviewedFileIds.every((item) => typeof item === 'string')) {
    throw new Error('代码审查回执不匹配。');
  }
  const findings: ReviewFindingWire[] = value.findings.map((raw) => {
    if (!record(raw)
      || typeof raw.id !== 'string' || !raw.id
      || typeof raw.fileId !== 'string' || !raw.fileId
      || typeof raw.path !== 'string' || !raw.path
      || typeof raw.line !== 'number' || !Number.isSafeInteger(raw.line) || raw.line < 1
      || (raw.severity !== 'high' && raw.severity !== 'suggestion')
      || typeof raw.title !== 'string' || !raw.title
      || typeof raw.reason !== 'string' || !raw.reason) {
      throw new Error('代码审查结果项格式无效。');
    }
    return {
      id: raw.id,
      fileId: raw.fileId,
      path: raw.path,
      line: raw.line,
      severity: raw.severity,
      title: raw.title,
      reason: raw.reason,
    };
  });
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    kind: 'review',
    requestId: value.requestId,
    target: decodeTarget(value.target),
    modelId: value.modelId,
    findings,
    skippedPaths: value.skippedPaths as string[],
    reviewedFileIds: value.reviewedFileIds as string[],
  };
}
