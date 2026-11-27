import { Channel } from '@tauri-apps/api/core';
import { invokeDesktop } from './errors';
import {
  decodeCustomActionCommandReply,
  decodeCustomActionProgressEvent,
  decodeGenerateCustomActionProgressEvent,
  decodeGenerateCustomActionReply,
  type CustomActionCommandReply,
  type CustomActionProgressEvent,
  type DismissSessionDoneRequest,
  type GenerateCustomActionProgressEvent,
  type GenerateCustomActionReply,
  type GenerateCustomActionRequest,
  type RespondCustomActionRequest,
  type StartCustomActionRequest,
} from '../../shared/custom-action-protocol';

export async function startDesktopCustomAction(
  request: StartCustomActionRequest,
  onProgress?: (event: CustomActionProgressEvent) => void,
): Promise<CustomActionCommandReply> {
  const onEvent = new Channel<unknown>();
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      const decoded = decodeCustomActionProgressEvent(raw);
      if (decoded.repositoryId === request.target.repositoryId && decoded.worktreeId === request.target.worktreeId) {
        onProgress(decoded);
      }
    };
  }
  const reply = decodeCustomActionCommandReply(await invokeDesktop('start_custom_action', { request, onEvent }));
  if (reply.requestId !== request.requestId
    || reply.repositoryId !== request.target.repositoryId
    || reply.worktreeId !== request.target.worktreeId) {
    throw new Error('Desktop returned a mismatched custom action reply.');
  }
  return reply;
}

export async function respondDesktopCustomAction(
  request: RespondCustomActionRequest,
): Promise<boolean> {
  const result = await invokeDesktop('respond_custom_action', { request });
  return Boolean(result);
}

export async function generateDesktopCustomAction(
  request: GenerateCustomActionRequest,
  onProgress?: (event: GenerateCustomActionProgressEvent) => void,
): Promise<GenerateCustomActionReply> {
  const onEvent = new Channel<unknown>();
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      const decoded = decodeGenerateCustomActionProgressEvent(raw);
      if (decoded.runId === request.requestId) {
        onProgress(decoded);
      }
    };
  }
  const reply = decodeGenerateCustomActionReply(await invokeDesktop('generate_custom_action', { request, onEvent }));
  if (reply.requestId !== request.requestId) {
    throw new Error('Desktop returned a mismatched custom action draft reply.');
  }
  return reply;
}

export async function cancelDesktopCustomAction(requestId: string): Promise<void> {
  await invokeDesktop('cancel_custom_action', { requestId });
}

export async function dismissDesktopSessionDone(
  request: DismissSessionDoneRequest,
): Promise<boolean> {
  const result = await invokeDesktop('dismiss_session_done', { request });
  return Boolean(result);
}
