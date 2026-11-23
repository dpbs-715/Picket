import { Channel, invoke } from '@tauri-apps/api/core';
import {
  decodeGenerateCommitReply,
  decodeModelMutateReply,
  decodeModelSaveReply,
  decodeModelTestReply,
  decodeReviewScopeReply,
  type GenerateCommitReply,
  type ModelFileSelection,
  type ModelInputPayload,
  type ModelMutateReply,
  type ModelStreamEvent,
  type ReviewScopeReply,
  type SavedModelConfiguration,
} from '../../shared/model-protocol';
import type { GitReadTarget } from '../../shared/git-read-protocol';

function sameTarget(a: GitReadTarget, b: GitReadTarget): boolean {
  return a.repositoryId === b.repositoryId
    && a.worktreeId === b.worktreeId
    && a.sessionId === b.sessionId
    && a.scopeVersion === b.scopeVersion;
}

export async function testDesktopModelConnection(input: ModelInputPayload): Promise<string> {
  const reply = decodeModelTestReply(await invoke('test_model_connection', { input }));
  if (reply.provider !== input.provider || reply.modelId !== input.modelId.trim()) {
    throw new Error('模型连接测试回执与当前请求不匹配，已忽略该结果。');
  }
  return reply.summary;
}

export async function saveDesktopModel(input: ModelInputPayload): Promise<SavedModelConfiguration> {
  const reply = decodeModelSaveReply(await invoke('save_model', { input }));
  if (reply.model.provider !== input.provider || reply.model.modelId !== input.modelId.trim()) {
    throw new Error('保存的模型配置与提交参数不一致，请刷新设置核对。');
  }
  return reply.model;
}

export async function selectDesktopModel(modelId: string): Promise<ModelMutateReply> {
  const reply = decodeModelMutateReply(await invoke('select_model', { request: { modelId } }));
  if (reply.activeModelId !== modelId) {
    throw new Error('模型切换回执不匹配，请重新选择模型。');
  }
  return reply;
}

export async function deleteDesktopModel(modelId: string): Promise<ModelMutateReply> {
  const reply = decodeModelMutateReply(await invoke('delete_model', { request: { modelId } }));
  if (reply.models.some((model) => model.id === modelId)) {
    throw new Error('模型删除未生效，请刷新设置后重试。');
  }
  return reply;
}

export async function cancelDesktopModelRequest(requestId: string): Promise<void> {
  await invoke('cancel_model_request', { requestId });
}

export async function generateDesktopCommitMessage(params: {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  draft: string;
  onStream?: (event: ModelStreamEvent) => void;
}): Promise<GenerateCommitReply> {
  const onEvent = new Channel<ModelStreamEvent>();
  if (params.onStream) {
    onEvent.onmessage = params.onStream;
  }
  const reply = decodeGenerateCommitReply(await invoke('generate_commit_message', {
    request: {
      requestId: params.requestId,
      target: params.target,
      modelId: params.modelId,
      selections: params.selections,
      draft: params.draft,
    },
    onEvent,
  }));
  if (reply.requestId !== params.requestId || reply.modelId !== params.modelId || !sameTarget(reply.target, params.target)) {
    throw new Error('提交说明生成回执的目标或请求身份不匹配，已忽略该结果。');
  }
  return reply;
}

export async function reviewDesktopScope(params: {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  draft: string;
  changedPaths?: string[];
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
      draft: params.draft,
      ...(params.changedPaths ? { changedPaths: params.changedPaths } : {}),
    },
    onEvent,
  }));
  if (reply.requestId !== params.requestId || reply.modelId !== params.modelId || !sameTarget(reply.target, params.target)) {
    throw new Error('代码审查回执的目标或请求身份不匹配，已忽略该结果。');
  }
  return reply;
}
