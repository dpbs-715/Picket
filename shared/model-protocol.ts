import type {
  ApiFormatKind,
  ProviderKind,
  SavedModelConfiguration,
  SavedProviderConnection,
} from './generated';
import type { AppLocale } from './language-protocol';

export const MODEL_PROTOCOL_VERSION = 2;

export type ModelProviderId = ProviderKind;
export const MODEL_PROVIDER_IDS = ['anthropic', 'openai', 'deepseek', 'ollama', 'custom'] as const satisfies readonly ModelProviderId[];

export type ModelApiFormat = ApiFormatKind;
export const MODEL_API_FORMATS = ['openai-completions', 'openai-responses', 'anthropic-messages'] as const satisfies readonly ModelApiFormat[];

export type { SavedModelConfiguration, SavedProviderConnection };

export interface CreateProviderConnectionInput {
  provider: ModelProviderId;
  connectionName: string;
  baseUrl: string;
  apiKey: string;
  modelName: string;
  modelId: string;
  apiFormat?: ModelApiFormat;
  contextWindow?: number;
  maxOutputTokens?: number;
  keepActiveModel?: boolean;
  locale?: AppLocale;
}

export interface UpdateProviderConnectionInput {
  connectionId: string;
  name: string;
  apiKey: string;
  locale?: AppLocale;
}

export interface SaveModelInput {
  id?: string;
  connectionId: string;
  name: string;
  modelId: string;
  apiFormat?: ModelApiFormat;
  contextWindow?: number;
  maxOutputTokens?: number;
  locale?: AppLocale;
}

export type TestConnectionInput =
  | {
      kind: 'newConnection';
      provider: ModelProviderId;
      baseUrl: string;
      apiKey: string;
      modelId: string;
      apiFormat?: ModelApiFormat;
      contextWindow?: number;
      maxOutputTokens?: number;
      locale?: AppLocale;
    }
  | {
      kind: 'connectionModel';
      connectionId: string;
      modelId: string;
      apiFormat?: ModelApiFormat;
      contextWindow?: number;
      maxOutputTokens?: number;
      locale?: AppLocale;
    }
  | {
      kind: 'existingConnection';
      connectionId: string;
      apiKey: string;
      modelId?: string;
      locale?: AppLocale;
    };

export interface ModelTestReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  provider: ModelProviderId;
  modelId: string;
  summary: string;
  latencyMs: number;
}

export interface RemoteModelOption {
  id: string;
  displayName?: string;
  contextWindow?: number;
  maxOutputTokens?: number;
}

export type ListModelsInput =
  | {
      kind: 'newConnection';
      provider: ModelProviderId;
      baseUrl: string;
      apiKey: string;
      apiFormat?: ModelApiFormat;
      locale?: AppLocale;
    }
  | {
      kind: 'existingConnection';
      connectionId: string;
      apiFormat?: ModelApiFormat;
      locale?: AppLocale;
    };

export interface ModelListReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  provider: ModelProviderId;
  models: RemoteModelOption[];
}

export interface ProviderSaveReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  connection: SavedProviderConnection;
  model: SavedModelConfiguration;
  connections: SavedProviderConnection[];
  models: SavedModelConfiguration[];
  activeModelId: string;
}

export interface ProviderUpdateReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  connection: SavedProviderConnection;
  connections: SavedProviderConnection[];
  models: SavedModelConfiguration[];
  activeModelId: string | null;
}

export interface ModelSaveReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  model: SavedModelConfiguration;
  connections: SavedProviderConnection[];
  models: SavedModelConfiguration[];
  activeModelId: string;
}

export interface ModelMutateReply {
  protocolVersion: typeof MODEL_PROTOCOL_VERSION;
  connections: SavedProviderConnection[];
  models: SavedModelConfiguration[];
  activeModelId: string | null;
}

export type ProviderRevealKeyReply =
  | {
      protocolVersion: typeof MODEL_PROTOCOL_VERSION;
      connectionId: string;
      outcome: 'authenticated';
      apiKey: string;
      keyHint: string;
    }
  | {
      protocolVersion: typeof MODEL_PROTOCOL_VERSION;
      connectionId: string;
      outcome: 'cancelled' | 'failed';
      message: string;
    };

export interface ModelFileSelection {
  fileId: string;
  commitToken: string;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function hasControlChars(text: string): boolean {
  for (let index = 0; index < text.length; index++) {
    const code = text.charCodeAt(index);
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

export function isModelProviderId(value: unknown): value is ModelProviderId {
  return typeof value === 'string' && (MODEL_PROVIDER_IDS as readonly string[]).includes(value);
}

export function isModelApiFormat(value: unknown): value is ModelApiFormat {
  return typeof value === 'string' && (MODEL_API_FORMATS as readonly string[]).includes(value);
}

export function decodeSavedProviderConnection(value: unknown): SavedProviderConnection {
  if (!record(value)
    || 'apiKey' in value
    || 'credentialRef' in value
    || typeof value.id !== 'string' || !value.id
    || typeof value.name !== 'string' || !value.name
    || !isModelProviderId(value.provider)
    || !(value.baseUrl === undefined || value.baseUrl === null || typeof value.baseUrl === 'string')
    || !(value.keyHint === undefined || value.keyHint === null || (typeof value.keyHint === 'string' && value.keyHint.length > 0 && value.keyHint.length <= 32 && !hasControlChars(value.keyHint)))) {
    throw new Error('Provider connection protocol mismatch or unexpected fields.');
  }
  return {
    id: value.id,
    name: value.name,
    provider: value.provider,
    ...(typeof value.baseUrl === 'string' && value.baseUrl ? { baseUrl: value.baseUrl } : {}),
    ...(typeof value.keyHint === 'string' ? { keyHint: value.keyHint } : {}),
  };
}

export function decodeSavedModelConfiguration(value: unknown): SavedModelConfiguration {
  if (!record(value)
    || 'apiKey' in value
    || 'credentialRef' in value
    || typeof value.id !== 'string' || !value.id
    || typeof value.connectionId !== 'string' || !value.connectionId
    || typeof value.name !== 'string' || !value.name
    || !isModelProviderId(value.provider)
    || !(value.apiFormat === undefined || value.apiFormat === null || isModelApiFormat(value.apiFormat))
    || typeof value.modelId !== 'string' || !value.modelId
    || !(value.baseUrl === undefined || value.baseUrl === null || typeof value.baseUrl === 'string')
    || !(value.contextWindow === undefined || value.contextWindow === null || (typeof value.contextWindow === 'number' && Number.isSafeInteger(value.contextWindow) && value.contextWindow > 0))
    || !(value.maxOutputTokens === undefined || value.maxOutputTokens === null || (typeof value.maxOutputTokens === 'number' && Number.isSafeInteger(value.maxOutputTokens) && value.maxOutputTokens > 0))) {
    throw new Error('Model configuration protocol mismatch or unexpected fields.');
  }
  return {
    id: value.id,
    connectionId: value.connectionId,
    name: value.name,
    provider: value.provider,
    ...(isModelApiFormat(value.apiFormat) ? { apiFormat: value.apiFormat } : {}),
    modelId: value.modelId,
    ...(typeof value.baseUrl === 'string' && value.baseUrl ? { baseUrl: value.baseUrl } : {}),
    ...(typeof value.contextWindow === 'number' ? { contextWindow: value.contextWindow } : {}),
    ...(typeof value.maxOutputTokens === 'number' ? { maxOutputTokens: value.maxOutputTokens } : {}),
  };
}

export function decodeModelTestReply(value: unknown): ModelTestReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || 'credentialRef' in value
    || !isModelProviderId(value.provider)
    || typeof value.modelId !== 'string' || !value.modelId
    || typeof value.summary !== 'string' || !value.summary
    || typeof value.latencyMs !== 'number' || !Number.isFinite(value.latencyMs) || value.latencyMs < 0) {
    throw new Error('Model connection test reply mismatch.');
  }
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    provider: value.provider,
    modelId: value.modelId,
    summary: value.summary,
    latencyMs: value.latencyMs,
  };
}

export function decodeProviderSaveReply(value: unknown): ProviderSaveReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || 'credentialRef' in value
    || !Array.isArray(value.connections)
    || !Array.isArray(value.models)
    || typeof value.activeModelId !== 'string' || !value.activeModelId) {
    throw new Error('Provider connection save reply mismatch.');
  }
  const connection = decodeSavedProviderConnection(value.connection);
  const model = decodeSavedModelConfiguration(value.model);
  const connections = value.connections.map(decodeSavedProviderConnection);
  const models = value.models.map(decodeSavedModelConfiguration);
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    connection,
    model,
    connections,
    models,
    activeModelId: value.activeModelId,
  };
}

export function decodeProviderUpdateReply(value: unknown): ProviderUpdateReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || 'credentialRef' in value
    || !Array.isArray(value.connections)
    || !Array.isArray(value.models)
    || !(value.activeModelId === null || typeof value.activeModelId === 'string')) {
    throw new Error('Provider connection update reply mismatch.');
  }
  const connection = decodeSavedProviderConnection(value.connection);
  const connections = value.connections.map(decodeSavedProviderConnection);
  const models = value.models.map(decodeSavedModelConfiguration);
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    connection,
    connections,
    models,
    activeModelId: value.activeModelId,
  };
}

export function decodeModelSaveReply(value: unknown): ModelSaveReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || 'credentialRef' in value
    || !Array.isArray(value.connections)
    || !Array.isArray(value.models)
    || typeof value.activeModelId !== 'string' || !value.activeModelId) {
    throw new Error('Model save reply mismatch.');
  }
  const model = decodeSavedModelConfiguration(value.model);
  const connections = value.connections.map(decodeSavedProviderConnection);
  const models = value.models.map(decodeSavedModelConfiguration);
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    model,
    connections,
    models,
    activeModelId: value.activeModelId,
  };
}

export function decodeModelMutateReply(value: unknown): ModelMutateReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || 'credentialRef' in value
    || !Array.isArray(value.connections)
    || !Array.isArray(value.models)
    || !(value.activeModelId === null || typeof value.activeModelId === 'string')) {
    throw new Error('Model mutation reply mismatch.');
  }
  const connections = value.connections.map(decodeSavedProviderConnection);
  const models = value.models.map(decodeSavedModelConfiguration);
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    connections,
    models,
    activeModelId: value.activeModelId,
  };
}

export function decodeProviderRevealKeyReply(value: unknown): ProviderRevealKeyReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'credentialRef' in value
    || typeof value.connectionId !== 'string' || !value.connectionId) {
    throw new Error('Provider reveal key reply mismatch.');
  }
  if (value.outcome === 'authenticated') {
    if (typeof value.apiKey !== 'string' || !value.apiKey
      || typeof value.keyHint !== 'string' || !value.keyHint || hasControlChars(value.keyHint)) {
      throw new Error('Authenticated provider reveal reply is missing valid key fields.');
    }
    return {
      protocolVersion: MODEL_PROTOCOL_VERSION,
      connectionId: value.connectionId,
      outcome: 'authenticated',
      apiKey: value.apiKey,
      keyHint: value.keyHint,
    };
  }
  if (value.outcome === 'cancelled' || value.outcome === 'failed') {
    if ('apiKey' in value || typeof value.message !== 'string' || !value.message) {
      throw new Error('Non-authenticated provider reveal reply must not include apiKey.');
    }
    return {
      protocolVersion: MODEL_PROTOCOL_VERSION,
      connectionId: value.connectionId,
      outcome: value.outcome,
      message: value.message,
    };
  }
  throw new Error('Unknown provider reveal outcome.');
}

export function decodeModelListReply(value: unknown): ModelListReply {
  if (!record(value)
    || value.protocolVersion !== MODEL_PROTOCOL_VERSION
    || 'apiKey' in value
    || 'credentialRef' in value
    || !isModelProviderId(value.provider)
    || !Array.isArray(value.models)) {
    throw new Error('Model list reply mismatch.');
  }
  const models: RemoteModelOption[] = value.models.map((raw) => {
    if (!record(raw)
      || typeof raw.id !== 'string' || !raw.id.trim()
      || !(raw.displayName === undefined || raw.displayName === null || typeof raw.displayName === 'string')
      || !(raw.contextWindow === undefined || raw.contextWindow === null || (typeof raw.contextWindow === 'number' && Number.isSafeInteger(raw.contextWindow) && raw.contextWindow > 0))
      || !(raw.maxOutputTokens === undefined || raw.maxOutputTokens === null || (typeof raw.maxOutputTokens === 'number' && Number.isSafeInteger(raw.maxOutputTokens) && raw.maxOutputTokens > 0))) {
      throw new Error('Invalid remote model entry.');
    }
    return {
      id: raw.id.trim(),
      ...(typeof raw.displayName === 'string' && raw.displayName.trim() ? { displayName: raw.displayName.trim() } : {}),
      ...(typeof raw.contextWindow === 'number' ? { contextWindow: raw.contextWindow } : {}),
      ...(typeof raw.maxOutputTokens === 'number' ? { maxOutputTokens: raw.maxOutputTokens } : {}),
    };
  });
  return {
    protocolVersion: MODEL_PROTOCOL_VERSION,
    provider: value.provider,
    models,
  };
}
