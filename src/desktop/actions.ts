import { Channel } from '@tauri-apps/api/core';
import { invokeDesktop } from './errors';
import {
  decodeActionCommandReply,
  decodeActionProgressEvent,
  decodeGenerateCustomActionProgressEvent,
  decodeGenerateCustomActionReply,
  isMatchingActionTarget,
  type ActionCommandReply,
  type ActionProgressEvent,
  type ActionSessionSnapshot,
  type DismissSessionDoneRequest,
  type GenerateCustomActionProgressEvent,
  type GenerateCustomActionReply,
  type GenerateCustomActionRequest,
  type ReconcileSessionRequest,
  type RespondActionQuestionRequest,
  type SessionRecoveryRequest,
  type StartActionRequest,
} from '../../shared/custom-action-protocol';
import type { GitReadTarget } from '../../shared/git-read-protocol';
import { getMessages } from '../i18n/messages';

function isMatchingWorktree(
  session: ActionSessionSnapshot,
  target: GitReadTarget,
): boolean {
  return !!target.repositoryId
    && !!target.worktreeId
    && session.repositoryId === target.repositoryId
    && session.worktreeId === target.worktreeId;
}

export async function startDesktopAction(
  request: StartActionRequest,
  onProgress?: (event: ActionProgressEvent) => void,
): Promise<ActionCommandReply> {
  const onEvent = new Channel<unknown>();
  const strictScope = request.profile === 'cpush';
  let settled = false;
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      if (settled) return;
      const decoded = decodeActionProgressEvent(raw);
      const matches = strictScope
        ? isMatchingActionTarget(decoded.session, request.target)
        : isMatchingWorktree(decoded.session, request.target);
      if (matches) {
        onProgress(decoded);
      }
    };
  }
  let reply: ActionCommandReply;
  try {
    reply = decodeActionCommandReply(await invokeDesktop('start_action', { request, onEvent }));
  } finally {
    settled = true;
  }
  const matches = strictScope
    ? reply.session.modelId === request.modelId && isMatchingActionTarget(reply.session, request.target)
    : isMatchingWorktree(reply.session, request.target);
  if (reply.requestId !== request.requestId || !matches) {
    throw new Error(getMessages(request.locale).desktop.cPushStartMismatch);
  }
  return reply;
}

export async function respondDesktopActionQuestion(
  request: RespondActionQuestionRequest,
): Promise<boolean> {
  const result = await invokeDesktop('respond_action_question', { request });
  return Boolean(result);
}

export async function runDesktopSessionRecovery(
  request: SessionRecoveryRequest,
  onProgress?: (event: ActionProgressEvent) => void,
): Promise<ActionCommandReply> {
  const onEvent = new Channel<unknown>();
  let settled = false;
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      if (settled) return;
      const decoded = decodeActionProgressEvent(raw);
      const matchesRun = !request.runId || decoded.session.runId === request.runId;
      if (matchesRun && isMatchingWorktree(decoded.session, request.target)) {
        onProgress(decoded);
      }
    };
  }
  let reply: ActionCommandReply;
  try {
    reply = decodeActionCommandReply(await invokeDesktop('run_session_recovery', { request, onEvent }));
  } finally {
    settled = true;
  }
  const matchesRun = !request.runId || reply.session.runId === request.runId;
  if (reply.requestId !== request.requestId || !matchesRun || !isMatchingWorktree(reply.session, request.target)) {
    throw new Error(
      request.action.kind === 'pullRebase'
        ? getMessages(request.locale).desktop.pullRebaseMismatch
        : getMessages(request.locale).desktop.cPushRespondMismatch,
    );
  }
  return reply;
}

export async function reconcileDesktopActionSession(
  request: ReconcileSessionRequest,
): Promise<ActionProgressEvent | null> {
  const raw = await invokeDesktop('reconcile_action_session', { request });
  if (raw === null || raw === undefined) return null;
  const decoded = decodeActionProgressEvent(raw);
  if (!isMatchingWorktree(decoded.session, request.target)) {
    return null;
  }
  return decoded;
}

export async function generateDesktopCustomAction(
  request: GenerateCustomActionRequest,
  onProgress?: (event: GenerateCustomActionProgressEvent) => void,
): Promise<GenerateCustomActionReply> {
  const onEvent = new Channel<unknown>();
  let settled = false;
  if (onProgress) {
    onEvent.onmessage = (raw) => {
      if (settled) return;
      const decoded = decodeGenerateCustomActionProgressEvent(raw);
      if (decoded.runId === request.requestId) {
        onProgress(decoded);
      }
    };
  }
  let reply: GenerateCustomActionReply;
  try {
    reply = decodeGenerateCustomActionReply(await invokeDesktop('generate_custom_action', { request, onEvent }));
  } finally {
    settled = true;
  }
  if (reply.requestId !== request.requestId) {
    throw new Error('Desktop returned a mismatched custom action draft reply.');
  }
  return reply;
}

export async function cancelDesktopAction(requestId: string): Promise<void> {
  await invokeDesktop('cancel_action', { requestId });
}

export async function dismissDesktopSessionDone(
  request: DismissSessionDoneRequest,
): Promise<boolean> {
  const result = await invokeDesktop('dismiss_session_done', { request });
  return Boolean(result);
}
