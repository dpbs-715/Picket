import { decodeGitReadReply, GIT_READ_PROTOCOL_VERSION } from './git-read-protocol';
import type { GitReadTarget, GitRepositorySnapshot } from './git-read-protocol';
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

export interface StartCPushRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  selections: ModelFileSelection[];
  draft: string;
}

export interface CommitAndPushRequest {
  requestId: string;
  target: GitReadTarget;
  selections: ModelFileSelection[];
  message: string;
  modelId?: string;
}

export interface PullRebaseRequest {
  requestId: string;
  target: GitReadTarget;
  modelId?: string;
}

export interface ReconcileCPushRequest {
  requestId: string;
  target: GitReadTarget;
}

export interface RespondCPushRequest {
  requestId: string;
  target: GitReadTarget;
  runId: string;
  messageId: string;
  optionId?: string;
  text?: string;
  modelId?: string;
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
    throw new Error('桌面层返回了无效的 C-Push 会话消息结构。');
  }
  const files = value.files === undefined || value.files === null ? undefined : (() => {
    if (!Array.isArray(value.files)) throw new Error('无效的 C-Push 文件引用列表。');
    return value.files.map((file) => {
      if (!isRecord(file)
        || typeof file.id !== 'string' || !file.id
        || typeof file.label !== 'string'
        || !(file.badge === undefined || file.badge === null || typeof file.badge === 'string')) {
        throw new Error('无效的 C-Push 文件引用项。');
      }
      return {
        id: file.id,
        label: file.label,
        ...(typeof file.badge === 'string' && file.badge ? { badge: file.badge } : {}),
      };
    });
  })();
  const commits = value.commits === undefined || value.commits === null ? undefined : (() => {
    if (!Array.isArray(value.commits)) throw new Error('无效的 C-Push 提交摘要列表。');
    return value.commits.map((item) => {
      if (!isRecord(item) || typeof item.hash !== 'string' || typeof item.title !== 'string') {
        throw new Error('无效的 C-Push 提交摘要项。');
      }
      return { hash: item.hash, title: item.title };
    });
  })();
  const options = value.options === undefined || value.options === null ? undefined : (() => {
    if (!Array.isArray(value.options)) throw new Error('无效的 C-Push 选项列表。');
    return value.options.map((opt) => {
      if (!isRecord(opt)
        || typeof opt.id !== 'string' || !opt.id
        || typeof opt.label !== 'string' || !opt.label
        || !(opt.recommended === undefined || typeof opt.recommended === 'boolean')) {
        throw new Error('无效的 C-Push 选项项。');
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
      throw new Error('无效的 C-Push 进度字段。');
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
    throw new Error('桌面层返回了不合法或包含未预期字段的 C-Push 会话快照。');
  }
  const message = value.message === null || value.message === undefined ? null : decodeUiMessage(value.message);
  const timeline: CPushTimelineItem[] = value.timeline.map((entry) => {
    if (!isRecord(entry)
      || (entry.state !== 'done' && entry.state !== 'active' && entry.state !== 'failed')
      || typeof entry.label !== 'string') {
      throw new Error('无效的 C-Push 时间线条目。');
    }
    return { state: entry.state, label: entry.label };
  });
  const qaLog: CPushQaItem[] = value.qaLog.map((entry) => {
    if (!isRecord(entry)
      || typeof entry.questionId !== 'string'
      || typeof entry.question !== 'string'
      || typeof entry.answer !== 'string') {
      throw new Error('无效的 C-Push 问答记录条目。');
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
      throw new Error('无效的 C-Push 已完成提交记录。');
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
  if (!decoded.snapshot) throw new Error('无效的 C-Push 仓库快照回执。');
  return decoded.snapshot;
}

export function decodeCPushProgressEvent(value: unknown): CPushProgressEvent {
  if (!isRecord(value) || value.protocolVersion !== CPUSH_PROTOCOL_VERSION) {
    throw new Error('C-Push 进度事件协议版本不匹配。');
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
    throw new Error('C-Push 命令回执协议不匹配。');
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
