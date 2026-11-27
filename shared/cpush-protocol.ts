import type {
  CPushQaItem,
  CPushSessionSnapshot,
  CPushSessionStatus,
} from './generated';
import { decodeGitReadReply, GIT_READ_PROTOCOL_VERSION } from './git-read-protocol';
import type { GitReadTarget, GitRepositorySnapshot } from './git-read-protocol';
import type { AppLocale, CommitLanguage } from './language-protocol';
import type { ModelFileSelection } from './model-protocol';
import {
  decodeWorkflowCommits,
  decodeWorkflowTimeline,
  decodeWorkflowUiMessage,
  type WorkflowLandedCommit,
  type WorkflowMessageKind,
  type WorkflowTimelineItem,
  type WorkflowUiMessage,
} from './workflow-message-protocol';

export const CPUSH_PROTOCOL_VERSION = 1;

export type { CPushQaItem, CPushSessionSnapshot, CPushSessionStatus };

export type CPushMessageKind = WorkflowMessageKind;
export type CPushTimelineItem = WorkflowTimelineItem;
export type CPushLandedCommit = WorkflowLandedCommit;
export type CPushUiMessage = WorkflowUiMessage;

export interface StartCPushRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  prompt: string;
  push?: boolean;
  locale?: AppLocale;
  commitLanguage?: CommitLanguage;
}

export interface PullRebaseRequest {
  requestId: string;
  target: GitReadTarget;
  modelId?: string;
  locale?: AppLocale;
}

export interface ReconcileCPushRequest {
  requestId: string;
  target: GitReadTarget;
  locale?: AppLocale;
}

export interface RespondCPushRequest {
  requestId: string;
  target: GitReadTarget;
  runId: string;
  messageId: string;
  optionId?: string;
  text?: string;
  modelId?: string;
  locale?: AppLocale;
}

export interface CPushProgressEvent {
  protocolVersion: typeof CPUSH_PROTOCOL_VERSION;
  session: CPushSessionSnapshot;
  repositorySnapshot?: GitRepositorySnapshot;
}

export interface CPushCommandReply {
  protocolVersion: typeof CPUSH_PROTOCOL_VERSION;
  kind: 'cPush';
  requestId: string;
  session: CPushSessionSnapshot;
  repositorySnapshot?: GitRepositorySnapshot;
}

const VALID_SESSION_STATUSES: readonly CPushSessionStatus[] = [
  'idle',
  'running',
  'needsDecision',
  'conflict',
  'waitingManual',
  'pushFailed',
  'done',
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

export function decodeCPushSessionSnapshot(value: unknown): CPushSessionSnapshot {
  if (!isRecord(value)
    || 'apiKey' in value || 'credentialRef' in value
    || typeof value.runId !== 'string' || !value.runId
    || typeof value.repositoryId !== 'string' || !value.repositoryId
    || typeof value.worktreeId !== 'string' || !value.worktreeId
    || typeof value.sessionId !== 'string' || !value.sessionId
    || typeof value.frozenScopeVersion !== 'string'
    || typeof value.modelId !== 'string'
    || !isStringArray(value.frozenFileIds)
    || !isStringArray(value.committedFileIds)
    || !isStringArray(value.skippedFileIds)
    || !VALID_SESSION_STATUSES.includes(value.sessionStatus as CPushSessionStatus)
    || !(value.pendingCallId === undefined || value.pendingCallId === null || typeof value.pendingCallId === 'string')
    || typeof value.canRetryRemaining !== 'boolean'
    || !(value.autoCollapse === undefined || value.autoCollapse === null || typeof value.autoCollapse === 'boolean')
    || !Array.isArray(value.timeline)
    || !Array.isArray(value.qaLog)
    || !Array.isArray(value.commits)) {
    throw new Error('Desktop returned an invalid or unexpected C-Push session snapshot.');
  }
  const message = value.message === null || value.message === undefined
    ? null
    : decodeWorkflowUiMessage(value.message, 'C-Push session');
  const timeline = decodeWorkflowTimeline(value.timeline, 'C-Push');
  const qaLog: CPushQaItem[] = value.qaLog.map((entry) => {
    if (!isRecord(entry)
      || typeof entry.questionId !== 'string'
      || typeof entry.question !== 'string'
      || typeof entry.answer !== 'string') {
      throw new Error('Invalid C-Push Q&A log entry.');
    }
    return { questionId: entry.questionId, question: entry.question, answer: entry.answer };
  });
  const commits = decodeWorkflowCommits(value.commits, 'C-Push');
  return {
    runId: value.runId,
    repositoryId: value.repositoryId,
    worktreeId: value.worktreeId,
    sessionId: value.sessionId,
    frozenScopeVersion: value.frozenScopeVersion,
    modelId: value.modelId,
    frozenFileIds: value.frozenFileIds,
    committedFileIds: value.committedFileIds,
    skippedFileIds: value.skippedFileIds,
    sessionStatus: value.sessionStatus as CPushSessionStatus,
    ...(typeof value.pendingCallId === 'string' && value.pendingCallId ? { pendingCallId: value.pendingCallId } : {}),
    message,
    timeline,
    qaLog,
    commits,
    canRetryRemaining: value.canRetryRemaining,
    ...(typeof value.autoCollapse === 'boolean' ? { autoCollapse: value.autoCollapse } : {}),
  };
}

function decodeOptionalSnapshot(raw: unknown): GitRepositorySnapshot | undefined {
  if (raw === undefined || raw === null) return undefined;
  const decoded = decodeGitReadReply({
    protocolVersion: GIT_READ_PROTOCOL_VERSION,
    requestId: 'cpush-snapshot',
    snapshot: raw,
  });
  if (!decoded.snapshot) throw new Error('Invalid C-Push repository snapshot reply.');
  return decoded.snapshot;
}

export function decodeCPushProgressEvent(value: unknown): CPushProgressEvent {
  if (!isRecord(value) || value.protocolVersion !== CPUSH_PROTOCOL_VERSION) {
    throw new Error('C-Push progress event protocol version mismatch.');
  }
  const repositorySnapshot = decodeOptionalSnapshot(value.repositorySnapshot);
  return {
    protocolVersion: CPUSH_PROTOCOL_VERSION,
    session: decodeCPushSessionSnapshot(value.session),
    ...(repositorySnapshot ? { repositorySnapshot } : {}),
  };
}

export function decodeCPushCommandReply(value: unknown): CPushCommandReply {
  if (!isRecord(value)
    || value.protocolVersion !== CPUSH_PROTOCOL_VERSION
    || value.kind !== 'cPush'
    || typeof value.requestId !== 'string' || !value.requestId) {
    throw new Error('C-Push command reply protocol mismatch.');
  }
  const repositorySnapshot = decodeOptionalSnapshot(value.repositorySnapshot);
  return {
    protocolVersion: CPUSH_PROTOCOL_VERSION,
    kind: 'cPush',
    requestId: value.requestId,
    session: decodeCPushSessionSnapshot(value.session),
    ...(repositorySnapshot ? { repositorySnapshot } : {}),
  };
}

export function isMatchingCPushTarget(session: CPushSessionSnapshot, target: GitReadTarget): boolean {
  return !!target.repositoryId
    && !!target.worktreeId
    && !!target.sessionId
    && session.repositoryId === target.repositoryId
    && session.worktreeId === target.worktreeId
    && session.sessionId === target.sessionId
    && session.frozenScopeVersion === target.scopeVersion;
}
