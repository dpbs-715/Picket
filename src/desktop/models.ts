import { invoke } from '@tauri-apps/api/core';
import type { AppLocale } from '../../shared/language-protocol';
import {
  decodeModelListReply,
  decodeModelMutateReply,
  decodeModelSaveReply,
  decodeModelTestReply,
  decodeProviderRevealKeyReply,
  decodeProviderSaveReply,
  decodeProviderUpdateReply,
  type CreateProviderConnectionInput,
  type ListModelsInput,
  type ModelMutateReply,
  type ModelSaveReply,
  type ProviderRevealKeyReply,
  type ProviderSaveReply,
  type ProviderUpdateReply,
  type RemoteModelOption,
  type SaveModelInput,
  type TestConnectionInput,
  type UpdateProviderConnectionInput,
} from '../../shared/model-protocol';
import { getMessages } from '../i18n/messages';

export async function testDesktopModelConnection(input: TestConnectionInput): Promise<string> {
  const reply = decodeModelTestReply(await invoke('test_model_connection', { input }));
  if (input.kind === 'newConnection'
    && (reply.provider !== input.provider || reply.modelId !== input.modelId.trim())) {
    throw new Error(getMessages(input.locale).desktop.modelTestMismatch);
  }
  if (input.kind === 'connectionModel' && reply.modelId !== input.modelId.trim()) {
    throw new Error(getMessages(input.locale).desktop.modelTestMismatch);
  }
  return reply.summary;
}

export async function listDesktopProviderModels(input: ListModelsInput): Promise<RemoteModelOption[]> {
  const reply = decodeModelListReply(await invoke('list_provider_models', { input }));
  if (input.kind === 'newConnection' && reply.provider !== input.provider) {
    throw new Error(getMessages(input.locale).desktop.modelTestMismatch);
  }
  return reply.models;
}

export async function createDesktopProviderConnection(
  input: CreateProviderConnectionInput,
): Promise<ProviderSaveReply> {
  const reply = decodeProviderSaveReply(await invoke('create_provider_connection', { input }));
  if (reply.connection.provider !== input.provider
    || reply.model.connectionId !== reply.connection.id
    || reply.model.modelId !== input.modelId.trim()) {
    throw new Error(getMessages(input.locale).desktop.modelSaveMismatch);
  }
  return reply;
}

export async function updateDesktopProviderConnection(
  input: UpdateProviderConnectionInput,
): Promise<ProviderUpdateReply> {
  const reply = decodeProviderUpdateReply(await invoke('update_provider_connection', { input }));
  if (reply.connection.id !== input.connectionId) {
    throw new Error(getMessages(input.locale).desktop.modelSaveMismatch);
  }
  return reply;
}

export async function deleteDesktopProviderConnection(
  connectionId: string,
  locale?: AppLocale,
): Promise<ModelMutateReply> {
  const reply = decodeModelMutateReply(
    await invoke('delete_provider_connection', {
      request: { connectionId, ...(locale ? { locale } : {}) },
    }),
  );
  if (reply.connections.some((conn) => conn.id === connectionId)) {
    throw new Error(getMessages(locale).desktop.modelDeleteFailed);
  }
  return reply;
}

export async function saveDesktopModel(input: SaveModelInput): Promise<ModelSaveReply> {
  const reply = decodeModelSaveReply(await invoke('save_model', { input }));
  if (reply.model.connectionId !== input.connectionId || reply.model.modelId !== input.modelId.trim()) {
    throw new Error(getMessages(input.locale).desktop.modelSaveMismatch);
  }
  return reply;
}

export async function selectDesktopModel(modelId: string, locale?: AppLocale): Promise<ModelMutateReply> {
  const reply = decodeModelMutateReply(await invoke('select_model', { request: { modelId, ...(locale ? { locale } : {}) } }));
  if (reply.activeModelId !== modelId) {
    throw new Error(getMessages(locale).desktop.modelSelectMismatch);
  }
  return reply;
}

export async function deleteDesktopModel(modelId: string, locale?: AppLocale): Promise<ModelMutateReply> {
  const reply = decodeModelMutateReply(await invoke('delete_model', { request: { modelId, ...(locale ? { locale } : {}) } }));
  if (reply.models.some((model) => model.id === modelId)) {
    throw new Error(getMessages(locale).desktop.modelDeleteFailed);
  }
  return reply;
}

export async function revealDesktopProviderApiKey(
  connectionId: string,
  locale?: AppLocale,
): Promise<ProviderRevealKeyReply> {
  const reply = decodeProviderRevealKeyReply(
    await invoke('reveal_provider_api_key', {
      request: {
        connectionId,
        ...(locale ? { locale } : {}),
      },
    }),
  );
  if (reply.connectionId !== connectionId) {
    throw new Error(getMessages(locale).desktop.modelRevealMismatch);
  }
  return reply;
}

export async function cancelDesktopModelRequest(requestId: string): Promise<void> {
  await invoke('cancel_model_request', { requestId });
}
