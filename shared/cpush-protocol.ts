import { decodeGitReadReply, GIT_READ_PROTOCOL_VERSION } from './git-read-protocol';
import type { GitReadTarget, GitRepositorySnapshot } from './git-read-protocol';
import type { AppLocale, CommitLanguage } from './language-protocol';
import type { ModelFileSelection } from './model-protocol';

export const CPUSH_PROTOCOL_VERSION = 1;

export type CPushSessionStatus =
  | 'idle'
  | 'running'
  | 'needsDecision'
  | 'conflict'
  | 'waitingManual'
  | 'pushFailed'
  | 'done';

export type CPushMessageKind =
  | 'progress'
  | 'question'
  | 'conflict'
  | 'waitingManual'
  | 'resolved'
  | 'pushFailed'
  | 'done'
  | 'review'
  | 'error';

export interface CPushTimelineItem {
  state: 'done' | 'active' | 'failed';
  label: string;
}

export interface CPushQaItem {
  questionId: string;
  question: string;
  answer: string;
}

export interface CPushLandedCommit {
  oid: string;
  shortOid: string;
  title: string;
  branch: string;
  fileIds: string[];
}

export interface CPushUiMessage {
  id: string;
  kind: CPushMessageKind;
  title: string;
  description: string;
  scope?: string;
  files?: { id: string; label: string; badge?: string }[];
  commits?: { hash: string; title: string }[];
  options?: { id: string; label: string; recommended?: boolean }[];
  progress?: { completed: number; total: number; label: string };
}

export interface CPushSessionSnapshot {
  runId: string;
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  frozenScopeVersion: string;
  modelId: string;
  frozenFileIds: string[];
  committedFileIds: string[];
  skippedFileIds: string[];
  sessionStatus: CPushSessionStatus;
  message: CPushUiMessage | null;
  timeline: CPushTimelineItem[];
  qaLog: CPushQaItem[];
  commits: CPushLandedCommit[];
  canRetryRemaining: boolean;
  autoCollapse?: boolean;
}

export interface CPushPlanGroup {
  id: string;
  fileIds: string[];
  message: string;
}

export interface CPushConfirmedPlan {
  groups: CPushPlanGroup[];
  skippedFileIds: string[];
  skipPush?: boolean;
}

export interface CPushPreparedPlan {
  summary: string;
  groups: CPushPlanGroup[];
  skippedFileIds: string[];
  skipPush?: boolean;
}

export interface StartCPushRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  prompt: string;
  confirmedPlan?: CPushConfirmedPlan;
  push?: boolean;
  locale?: AppLocale;
  commitLanguage?: CommitLanguage;
}

export interface PlanCPushRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  prompt: string;
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

export interface DismissCPushDoneRequest {
  requestId: string;
  target: GitReadTarget;
  runId?: string;
  messageId: string;
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
  preparedPlan?: CPushPreparedPlan;
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
const VALID_MESSAGE_KINDS: readonly CPushMessageKind[] = [
  'progress',
  'question',
  'conflict',
  'waitingManual',
  'resolved',
  'pushFailed',
  'done',
  'review',
  'error',
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

function decodeUiMessage(value: unknown): CPushUiMessage {
  if (!isRecord(value)
    || typeof value.id !== 'string' || !value.id
    || !VALID_MESSAGE_KINDS.includes(value.kind as CPushMessageKind)
    || typeof value.title !== 'string'
    || typeof value.description !== 'string'
    || !(value.scope === undefined || value.scope === null || typeof value.scope === 'string')) {
    throw new Error('Desktop returned an invalid C-Push session message structure.');
  }
  const files = value.files === undefined || value.files === null ? undefined : (() => {
    if (!Array.isArray(value.files)) throw new Error('Invalid C-Push file reference list.');
    return value.files.map((file) => {
      if (!isRecord(file)
        || typeof file.id !== 'string' || !file.id
        || typeof file.label !== 'string'
        || !(file.badge === undefined || file.badge === null || typeof file.badge === 'string')) {
        throw new Error('Invalid C-Push file reference item.');
      }
      return {
        id: file.id,
        label: file.label,
        ...(typeof file.badge === 'string' && file.badge ? { badge: file.badge } : {}),
      };
    });
  })();
  const commits = value.commits === undefined || value.commits === null ? undefined : (() => {
    if (!Array.isArray(value.commits)) throw new Error('Invalid C-Push commit summary list.');
    return value.commits.map((item) => {
      if (!isRecord(item) || typeof item.hash !== 'string' || typeof item.title !== 'string') {
        throw new Error('Invalid C-Push commit summary item.');
      }
      return { hash: item.hash, title: item.title };
    });
  })();
  const options = value.options === undefined || value.options === null ? undefined : (() => {
    if (!Array.isArray(value.options)) throw new Error('Invalid C-Push option list.');
    return value.options.map((opt) => {
      if (!isRecord(opt)
        || typeof opt.id !== 'string' || !opt.id
        || typeof opt.label !== 'string' || !opt.label
        || !(opt.recommended === undefined || typeof opt.recommended === 'boolean')) {
        throw new Error('Invalid C-Push option item.');
      }
      return {
        id: opt.id,
        label: opt.label,
        ...(typeof opt.recommended === 'boolean' ? { recommended: opt.recommended } : {}),
      };
    });
  })();
  const progress = value.progress === undefined || value.progress === null ? undefined : (() => {
    if (!isRecord(value.progress)
      || typeof value.progress.completed !== 'number'
      || typeof value.progress.total !== 'number'
      || typeof value.progress.label !== 'string') {
      throw new Error('Invalid C-Push progress field.');
    }
    return {
      completed: value.progress.completed,
      total: value.progress.total,
      label: value.progress.label,
    };
  })();
  return {
    id: value.id,
    kind: value.kind as CPushMessageKind,
    title: value.title,
    description: value.description,
    ...(typeof value.scope === 'string' && value.scope ? { scope: value.scope } : {}),
    ...(files ? { files } : {}),
    ...(commits ? { commits } : {}),
    ...(options ? { options } : {}),
    ...(progress ? { progress } : {}),
  };
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
    || typeof value.canRetryRemaining !== 'boolean'
    || !(value.autoCollapse === undefined || value.autoCollapse === null || typeof value.autoCollapse === 'boolean')
    || !Array.isArray(value.timeline)
    || !Array.isArray(value.qaLog)
    || !Array.isArray(value.commits)) {
    throw new Error('Desktop returned an invalid or unexpected C-Push session snapshot.');
  }
  const message = value.message === null || value.message === undefined ? null : decodeUiMessage(value.message);
  const timeline: CPushTimelineItem[] = value.timeline.map((entry) => {
    if (!isRecord(entry)
      || (entry.state !== 'done' && entry.state !== 'active' && entry.state !== 'failed')
      || typeof entry.label !== 'string') {
      throw new Error('Invalid C-Push timeline entry.');
    }
    return { state: entry.state, label: entry.label };
  });
  const qaLog: CPushQaItem[] = value.qaLog.map((entry) => {
    if (!isRecord(entry)
      || typeof entry.questionId !== 'string'
      || typeof entry.question !== 'string'
      || typeof entry.answer !== 'string') {
      throw new Error('Invalid C-Push Q&A log entry.');
    }
    return { questionId: entry.questionId, question: entry.question, answer: entry.answer };
  });
  const commits: CPushLandedCommit[] = value.commits.map((entry) => {
    if (!isRecord(entry)
      || typeof entry.oid !== 'string' || !entry.oid
      || typeof entry.shortOid !== 'string' || !entry.shortOid
      || typeof entry.title !== 'string'
      || typeof entry.branch !== 'string'
      || !isStringArray(entry.fileIds)) {
      throw new Error('Invalid C-Push landed commit entry.');
    }
    return {
      oid: entry.oid,
      shortOid: entry.shortOid,
      title: entry.title,
      branch: entry.branch,
      fileIds: entry.fileIds,
    };
  });
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

function decodePreparedPlan(raw: unknown): CPushPreparedPlan | undefined {
  if (raw === undefined || raw === null) return undefined;
  if (!isRecord(raw)
    || typeof raw.summary !== 'string'
    || !Array.isArray(raw.groups)
    || !isStringArray(raw.skippedFileIds)) {
    throw new Error('Invalid C-Push prepared plan reply.');
  }
  const groups: CPushPlanGroup[] = raw.groups.map((item) => {
    if (!isRecord(item)
      || typeof item.id !== 'string' || !item.id.trim()
      || !isStringArray(item.fileIds) || item.fileIds.length === 0
      || typeof item.message !== 'string' || !item.message.trim()) {
      throw new Error('Invalid C-Push prepared plan group entry.');
    }
    return {
      id: item.id,
      fileIds: item.fileIds,
      message: item.message,
    };
  });
  if (groups.length === 0 && raw.skippedFileIds.length === 0) {
    throw new Error('C-Push prepared plan cannot be completely empty.');
  }
  if (!(raw.skipPush === undefined || raw.skipPush === null || typeof raw.skipPush === 'boolean')) {
    throw new Error('Invalid C-Push prepared plan skipPush flag.');
  }
  return {
    summary: raw.summary,
    groups,
    skippedFileIds: raw.skippedFileIds,
    ...(raw.skipPush ? { skipPush: true } : {}),
  };
}

export function decodeCPushCommandReply(value: unknown): CPushCommandReply {
  if (!isRecord(value)
    || value.protocolVersion !== CPUSH_PROTOCOL_VERSION
    || value.kind !== 'cPush'
    || typeof value.requestId !== 'string' || !value.requestId) {
    throw new Error('C-Push command reply protocol mismatch.');
  }
  const preparedPlan = decodePreparedPlan(value.preparedPlan);
  const repositorySnapshot = decodeOptionalSnapshot(value.repositorySnapshot);
  return {
    protocolVersion: CPUSH_PROTOCOL_VERSION,
    kind: 'cPush',
    requestId: value.requestId,
    session: decodeCPushSessionSnapshot(value.session),
    ...(preparedPlan ? { preparedPlan } : {}),
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
