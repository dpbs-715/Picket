import { Channel, invoke } from '@tauri-apps/api/core';
import {
  decodeCPushCommandReply,
  decodeCPushProgressEvent,
  isMatchingCPushTarget,
  type CommitAndPushRequest,
  type CPushCommandReply,
  type CPushProgressEvent,
  type PullRebaseRequest,
  type ReconcileCPushRequest,
  type RespondCPushRequest,
  type StartCPushRequest,
} from '../../shared/cpush-protocol';
import type { GitReadTarget } from '../../shared/git-read-protocol';

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
    throw new Error('C-Push 启动回执的目标工作树或请求身份不匹配，已忽略该回执。');
  }
  return reply;
}

export async function commitAndPushDesktop(
  request: CommitAndPushRequest,
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
  const reply = decodeCPushCommandReply(await invoke('commit_and_push_repository', { request, onEvent }));
  if (reply.requestId !== request.requestId || !isMatchingWorktree(reply.session, request.target)) {
    throw new Error('提交并推送回执的目标工作树或请求身份不匹配，请刷新核对实际 Git 历史。');
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
    throw new Error('拉取并变基回执的目标工作树或请求身份不匹配，请刷新核对实际 Git 状态。');
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
    throw new Error('C-Push 确认回执的目标会话或请求身份不匹配，已忽略该回执。');
  }
  return reply;
}
