import { Channel, invoke } from '@tauri-apps/api/core';
import type { AppLocale } from '../../shared/language-protocol';
import {
  decodeModelMutateReply,
  decodeModelRevealKeyReply,
  decodeModelSaveReply,
  decodeModelTestReply,
  decodeReviewScopeReply,
  type ModelFileSelection,
  type ModelInputPayload,
  type ModelMutateReply,
  type ModelRevealKeyReply,
  type ModelSaveReply,
  type ModelStreamEvent,
  type ReviewScopeReply,
} from '../../shared/model-protocol';
import type { GitReadTarget } from '../../shared/git-read-protocol';
import { getMessages } from '../i18n/messages';

function sameTarget(a: GitReadTarget, b: GitReadTarget): boolean {
  return a.repositoryId === b.repositoryId
    && a.worktreeId === b.worktreeId
    && a.sessionId === b.sessionId
    && a.scopeVersion === b.scopeVersion;
}

export async function testDesktopModelConnection(input: ModelInputPayload): Promise<string> {
  const reply = decodeModelTestReply(await invoke('test_model_connection', { input }));
  if (reply.provider !== input.provider || reply.modelId !== input.modelId.trim()) {
    throw new Error(getMessages(input.locale).desktop.modelTestMismatch);
  }
  return reply.summary;
}

export async function saveDesktopModel(input: ModelInputPayload): Promise<ModelSaveReply> {
  const reply = decodeModelSaveReply(await invoke('save_model', { input }));
  if (reply.model.provider !== input.provider || reply.model.modelId !== input.modelId.trim()) {
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

export async function revealDesktopModelApiKey(modelId: string, locale?: AppLocale): Promise<ModelRevealKeyReply> {
  const reply = decodeModelRevealKeyReply(
    await invoke('reveal_model_api_key', {
      request: {
        modelId,
        ...(locale ? { locale } : {}),
      },
    }),
  );
  if (reply.modelId !== modelId) {
    throw new Error(getMessages(locale).desktop.modelRevealMismatch);
  }
  return reply;
}

export async function cancelDesktopModelRequest(requestId: string): Promise<void> {
  await invoke('cancel_model_request', { requestId });
}

export async function reviewDesktopScope(params: {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  prompt: string;
  changedPaths?: string[];
  locale?: AppLocale;
  onStream?: (event: ModelStreamEvent) => void;
}): Promise<ReviewScopeReply> {
  const onEvent = new Channel<ModelStreamEvent>();
  if (params.onStream) {
    onEvent.onmessage = params.onStream;
  }
  const reply = decodeReviewScopeReply(await invoke('review_selected_scope', {
    request: {
      requestId: params.requestId,
      target: params.target,
      modelId: params.modelId,
      selections: params.selections,
      prompt: params.prompt,
      ...(params.changedPaths ? { changedPaths: params.changedPaths } : {}),
      ...(params.locale ? { locale: params.locale } : {}),
    },
    onEvent,
  }));
  if (reply.requestId !== params.requestId || reply.modelId !== params.modelId || !sameTarget(reply.target, params.target)) {
    throw new Error(getMessages(params.locale).desktop.reviewMismatch);
  }
  return reply;
}
