import { isModelProviderId, type ModelProviderId } from './model-protocol';

export const PI_PROTOCOL_VERSION = 1;
export const PI_CORE_VERSION = '1.6.0';
export const FOUNDATION_SCOPE_VERSION = 'foundation-v1';
export const MAX_PROTOCOL_LINE_BYTES = 256 * 1024;
export const MAX_COMMAND_LINE_BYTES = 512 * 1024;

export interface RequestContext {
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  scopeVersion: string;
}

export interface PiProviderSpec {
  provider: ModelProviderId;
  modelId: string;
  baseUrl: string;
  apiKey: string;
}

export interface PiDiffFile {
  fileId: string;
  path: string;
  previousPath?: string;
  status: string;
  rangeKind: 'partial' | 'staged' | 'unstaged';
  added: number;
  removed: number;
  patch: string;
}

export interface PiPlanFileEntry {
  fileId: string;
  path: string;
  previousPath?: string;
  status: string;
  rangeKind: 'partial' | 'staged' | 'unstaged';
  added: number;
  removed: number;
  ignoredForReview?: boolean;
  binary?: boolean;
  patch?: string;
  skipReason?: string;
}

export interface PiCPushQaTurn {
  questionId: string;
  question: string;
  answer: string;
  optionId?: string;
}

export interface PiConflictFile {
  fileId: string;
  path: string;
  baseContent?: string;
  upstreamContent: string;
  localContent: string;
  conflictedContent: string;
}

export interface PiResolvedConflictFile {
  fileId: string;
  path: string;
  content: string;
}

export interface PiConflictResolution {
  summary: string;
  resolvedFiles: PiResolvedConflictFile[];
}

export interface PiCPushCommitGroup {
  id: string;
  message: string;
  fileIds: string[];
}

export interface PiCPushQuestionOption {
  id: string;
  label: string;
  recommended?: boolean;
}

export type PiCPushPlan =
  | {
      kind: 'ready';
      summary: string;
      groups: PiCPushCommitGroup[];
      skippedFileIds: string[];
    }
  | {
      kind: 'question';
      title: string;
      scope: string;
      description: string;
      affectedFileIds: string[];
      options: PiCPushQuestionOption[];
    };

export interface ProbeRequest {
  type: 'probe';
  requestId: string;
  context: RequestContext;
}

export interface TestConnectionRequest {
  type: 'testConnection';
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
}

export interface GenerateCommitCommand {
  type: 'generateCommit';
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
  branch: string;
  recentCommits: string[];
  draft: string;
  files: PiDiffFile[];
  skippedPaths: string[];
}

export interface ReviewScopeCommand {
  type: 'reviewScope';
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
  branch: string;
  draft: string;
  files: PiDiffFile[];
  skippedPaths: string[];
}

export interface PlanCPushCommand {
  type: 'planCPush';
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
  branch: string;
  recentCommits: string[];
  draft: string;
  files: PiPlanFileEntry[];
  unselectedPaths: string[];
  qaHistory: PiCPushQaTurn[];
}

export interface ResolveConflictCommand {
  type: 'resolveConflict';
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
  branch: string;
  upstream: string;
  userInstruction: string;
  commits: string[];
  files: PiConflictFile[];
}

export type PiTaskRequest =
  | ProbeRequest
  | TestConnectionRequest
  | GenerateCommitCommand
  | ReviewScopeCommand
  | PlanCPushCommand
  | ResolveConflictCommand;

export type PiCommand = PiTaskRequest
  | { type: 'cancel'; requestId: string }
  | { type: 'shutdown' };

export interface PiReviewFinding {
  fileId: string;
  path: string;
  line: number;
  severity: 'high' | 'suggestion';
  title: string;
  reason: string;
}

export type ProbeEvent = {
  requestId: string;
  context: RequestContext;
} & (
  | { type: 'started' }
  | { type: 'textDelta'; delta: string }
  | { type: 'completed'; text: string }
  | { type: 'reviewCompleted'; findings: PiReviewFinding[] }
  | { type: 'cPushPlanCompleted'; plan: PiCPushPlan }
  | { type: 'conflictResolved'; resolution: PiConflictResolution }
  | { type: 'cancelled' }
  | { type: 'failed'; message: string }
);

export type PiEvent = ProbeEvent
  | { type: 'ready'; protocolVersion: number; piCoreVersion: string; nodeVersion: string; pid: number }
  | { type: 'protocolError'; message: string };

function decodeRequestContext(value: unknown): RequestContext {
  if (!isRecord(value)
    || typeof value.repositoryId !== 'string' || !value.repositoryId
    || typeof value.worktreeId !== 'string' || !value.worktreeId
    || typeof value.sessionId !== 'string' || !value.sessionId
    || typeof value.scopeVersion !== 'string' || !value.scopeVersion) {
    throw new Error('Invalid request context');
  }
  return {
    repositoryId: value.repositoryId,
    worktreeId: value.worktreeId,
    sessionId: value.sessionId,
    scopeVersion: value.scopeVersion,
  };
}

function decodeProviderSpec(value: unknown): PiProviderSpec {
  if (!isRecord(value)
    || !isModelProviderId(value.provider)
    || typeof value.modelId !== 'string' || !value.modelId.trim()
    || typeof value.baseUrl !== 'string'
    || typeof value.apiKey !== 'string') {
    throw new Error('Invalid model specification');
  }
  return {
    provider: value.provider,
    modelId: value.modelId.trim(),
    baseUrl: value.baseUrl.trim(),
    apiKey: value.apiKey,
  };
}

function decodeDiffFiles(value: unknown): PiDiffFile[] {
  if (!Array.isArray(value)) throw new Error('Expected files array');
  return value.map((item) => {
    if (!isRecord(item)
      || typeof item.fileId !== 'string' || !item.fileId
      || typeof item.path !== 'string' || !item.path
      || !(item.previousPath === undefined || item.previousPath === null || typeof item.previousPath === 'string')
      || typeof item.status !== 'string' || !item.status
      || (item.rangeKind !== 'partial' && item.rangeKind !== 'staged' && item.rangeKind !== 'unstaged')
      || typeof item.added !== 'number' || typeof item.removed !== 'number'
      || typeof item.patch !== 'string') {
      throw new Error('Invalid diff file entry');
    }
    return {
      fileId: item.fileId,
      path: item.path,
      ...(typeof item.previousPath === 'string' && item.previousPath ? { previousPath: item.previousPath } : {}),
      status: item.status,
      rangeKind: item.rangeKind,
      added: item.added,
      removed: item.removed,
      patch: item.patch,
    };
  });
}

function decodeStringList(value: unknown, field: string): string[] {
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === 'string')) {
    throw new Error(`Invalid ${field}`);
  }
  return value as string[];
}

function decodePlanFiles(value: unknown): PiPlanFileEntry[] {
  if (!Array.isArray(value) || value.length === 0) throw new Error('Expected non-empty plan files array');
  return value.map((item) => {
    if (!isRecord(item)
      || typeof item.fileId !== 'string' || !item.fileId
      || typeof item.path !== 'string' || !item.path
      || !(item.previousPath === undefined || item.previousPath === null || typeof item.previousPath === 'string')
      || typeof item.status !== 'string' || !item.status
      || (item.rangeKind !== 'partial' && item.rangeKind !== 'staged' && item.rangeKind !== 'unstaged')
      || typeof item.added !== 'number' || typeof item.removed !== 'number'
      || !(item.ignoredForReview === undefined || typeof item.ignoredForReview === 'boolean')
      || !(item.binary === undefined || typeof item.binary === 'boolean')
      || !(item.patch === undefined || item.patch === null || typeof item.patch === 'string')
      || !(item.skipReason === undefined || item.skipReason === null || typeof item.skipReason === 'string')) {
      throw new Error('Invalid plan file entry');
    }
    return {
      fileId: item.fileId,
      path: item.path,
      ...(typeof item.previousPath === 'string' && item.previousPath ? { previousPath: item.previousPath } : {}),
      status: item.status,
      rangeKind: item.rangeKind,
      added: item.added,
      removed: item.removed,
      ...(typeof item.ignoredForReview === 'boolean' ? { ignoredForReview: item.ignoredForReview } : {}),
      ...(typeof item.binary === 'boolean' ? { binary: item.binary } : {}),
      ...(typeof item.patch === 'string' ? { patch: item.patch } : {}),
      ...(typeof item.skipReason === 'string' && item.skipReason ? { skipReason: item.skipReason } : {}),
    };
  });
}

function decodeQaHistory(value: unknown): PiCPushQaTurn[] {
  if (!Array.isArray(value)) throw new Error('Invalid qaHistory');
  return value.map((item) => {
    if (!isRecord(item)
      || typeof item.questionId !== 'string'
      || typeof item.question !== 'string'
      || typeof item.answer !== 'string'
      || !(item.optionId === undefined || item.optionId === null || typeof item.optionId === 'string')) {
      throw new Error('Invalid qaHistory entry');
    }
    return {
      questionId: item.questionId,
      question: item.question,
      answer: item.answer,
      ...(typeof item.optionId === 'string' && item.optionId ? { optionId: item.optionId } : {}),
    };
  });
}

function decodeConflictFiles(value: unknown): PiConflictFile[] {
  if (!Array.isArray(value) || value.length === 0) throw new Error('Expected non-empty conflict files array');
  return value.map((item) => {
    if (!isRecord(item)
      || typeof item.fileId !== 'string' || !item.fileId
      || typeof item.path !== 'string' || !item.path
      || !(item.baseContent === undefined || item.baseContent === null || typeof item.baseContent === 'string')
      || typeof item.upstreamContent !== 'string'
      || typeof item.localContent !== 'string'
      || typeof item.conflictedContent !== 'string') {
      throw new Error('Invalid conflict file entry');
    }
    return {
      fileId: item.fileId,
      path: item.path,
      ...(typeof item.baseContent === 'string' ? { baseContent: item.baseContent } : {}),
      upstreamContent: item.upstreamContent,
      localContent: item.localContent,
      conflictedContent: item.conflictedContent,
    };
  });
}

export function decodeCommand(line: string): PiCommand {
  if (Buffer.byteLength(line) > MAX_COMMAND_LINE_BYTES) throw new Error('Protocol line is too large');
  const command: unknown = JSON.parse(line);
  if (!isRecord(command)) throw new Error('Expected a command object');
  if (command.type === 'shutdown') return { type: 'shutdown' };
  if (typeof command.requestId !== 'string' || !/^[a-zA-Z0-9-]{1,128}$/.test(command.requestId)) {
    throw new Error('Invalid requestId');
  }
  if (command.type === 'cancel') return { type: 'cancel', requestId: command.requestId };
  if (command.type === 'probe') {
    if (!isRecord(command.context)) throw new Error('Unknown command');
    const context = command.context;
    if (context.repositoryId !== 'foundation' || context.worktreeId !== 'temporary'
      || context.sessionId !== command.requestId || context.scopeVersion !== FOUNDATION_SCOPE_VERSION) {
      throw new Error('M0 accepts only the isolated foundation context');
    }
    return {
      type: 'probe',
      requestId: command.requestId,
      context: {
        repositoryId: 'foundation',
        worktreeId: 'temporary',
        sessionId: command.requestId,
        scopeVersion: FOUNDATION_SCOPE_VERSION,
      },
    };
  }
  if (command.type === 'testConnection') {
    return {
      type: 'testConnection',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
    };
  }
  if (command.type === 'generateCommit') {
    if (typeof command.branch !== 'string' || typeof command.draft !== 'string') {
      throw new Error('Invalid generateCommit fields');
    }
    return {
      type: 'generateCommit',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      branch: command.branch,
      recentCommits: decodeStringList(command.recentCommits, 'recentCommits'),
      draft: command.draft,
      files: decodeDiffFiles(command.files),
      skippedPaths: decodeStringList(command.skippedPaths, 'skippedPaths'),
    };
  }
  if (command.type === 'reviewScope') {
    if (typeof command.branch !== 'string' || typeof command.draft !== 'string') {
      throw new Error('Invalid reviewScope fields');
    }
    return {
      type: 'reviewScope',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      branch: command.branch,
      draft: command.draft,
      files: decodeDiffFiles(command.files),
      skippedPaths: decodeStringList(command.skippedPaths, 'skippedPaths'),
    };
  }
  if (command.type === 'planCPush') {
    if (typeof command.branch !== 'string' || typeof command.draft !== 'string') {
      throw new Error('Invalid planCPush fields');
    }
    return {
      type: 'planCPush',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      branch: command.branch,
      recentCommits: decodeStringList(command.recentCommits, 'recentCommits'),
      draft: command.draft,
      files: decodePlanFiles(command.files),
      unselectedPaths: decodeStringList(command.unselectedPaths, 'unselectedPaths'),
      qaHistory: decodeQaHistory(command.qaHistory),
    };
  }
  if (command.type === 'resolveConflict') {
    if (typeof command.branch !== 'string'
      || typeof command.upstream !== 'string'
      || typeof command.userInstruction !== 'string') {
      throw new Error('Invalid resolveConflict fields');
    }
    return {
      type: 'resolveConflict',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      branch: command.branch,
      upstream: command.upstream,
      userInstruction: command.userInstruction,
      commits: decodeStringList(command.commits, 'commits'),
      files: decodeConflictFiles(command.files),
    };
  }
  throw new Error('Unknown command');
}

function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === 'object' && input !== null && !Array.isArray(input);
}
