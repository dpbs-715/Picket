import { Channel, invoke } from '@tauri-apps/api/core';
import {
  decodeCPushCommandReply,
  decodeCPushProgressEvent,
  isMatchingCPushTarget,
  type CPushCommandReply,
  type CPushProgressEvent,
  type DismissCPushDoneRequest,
  type PullRebaseRequest,
  type ReconcileCPushRequest,
  type RespondCPushRequest,
  type StartCPushRequest,
} from '../../shared/cpush-protocol';
import type { GitReadTarget } from '../../shared/git-read-protocol';
import { getMessages } from '../i18n/messages';

function isMatchingWorktree(
  session: CPushCommandReply['session'],
  target: GitReadTarget,
): boolean {
  return !!target.repositoryId
    && !!target.worktreeId
    && session.repositoryId === target.repositoryId
    && session.worktreeId === target.worktreeId;
}

export async function startDesktopCPush(
  request: StartCPushRequest,
  onProgress?: (event: CPushProgressEvent) => void,
): Promise<CPushCommandReply> {
  const onEvent = new Channel<unknown>();
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      const decoded = decodeCPushProgressEvent(raw);
      if (isMatchingCPushTarget(decoded.session, request.target)) {
        onProgress(decoded);
      }
    };
  }
  const reply = decodeCPushCommandReply(await invoke('start_cpush', { request, onEvent }));
  if (reply.requestId !== request.requestId
    || reply.session.modelId !== request.modelId
    || !isMatchingCPushTarget(reply.session, request.target)) {
    throw new Error(getMessages(request.locale).desktop.cPushStartMismatch);
  }
  return reply;
}

export async function pullRebaseDesktop(
  request: PullRebaseRequest,
  onProgress?: (event: CPushProgressEvent) => void,
): Promise<CPushCommandReply> {
  const onEvent = new Channel<unknown>();
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      const decoded = decodeCPushProgressEvent(raw);
      if (isMatchingWorktree(decoded.session, request.target)) {
        onProgress(decoded);
      }
    };
  }
  const reply = decodeCPushCommandReply(await invoke('pull_rebase_repository', { request, onEvent }));
  if (reply.requestId !== request.requestId || !isMatchingWorktree(reply.session, request.target)) {
    throw new Error(getMessages(request.locale).desktop.pullRebaseMismatch);
  }
  return reply;
}

export async function reconcileDesktopCPush(
  request: ReconcileCPushRequest,
): Promise<CPushProgressEvent | null> {
  const raw = await invoke('reconcile_cpush_session', { request });
  if (raw === null || raw === undefined) return null;
  const decoded = decodeCPushProgressEvent(raw);
  if (!isMatchingWorktree(decoded.session, request.target)) {
    return null;
  }
  return decoded;
}

export async function dismissDesktopCPushDone(
  request: DismissCPushDoneRequest,
): Promise<boolean> {
  const result = await invoke('dismiss_cpush_done', { request });
  return Boolean(result);
}

export async function respondDesktopCPush(
  request: RespondCPushRequest,
  onProgress?: (event: CPushProgressEvent) => void,
): Promise<CPushCommandReply> {
  const onEvent = new Channel<unknown>();
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      const decoded = decodeCPushProgressEvent(raw);
      if (decoded.session.runId === request.runId && isMatchingWorktree(decoded.session, request.target)) {
        onProgress(decoded);
      }
    };
  }
  const reply = decodeCPushCommandReply(await invoke('respond_cpush', { request, onEvent }));
  if (reply.requestId !== request.requestId
    || reply.session.runId !== request.runId
    || !isMatchingWorktree(reply.session, request.target)) {
    throw new Error(getMessages(request.locale).desktop.cPushRespondMismatch);
  }
  return reply;
}
