import pkg from '../package.json';
import { isAppLocale, type AppLocale, type CommitLanguage } from './language-protocol';
import {
  isModelApiFormat,
  isModelProviderId,
  type ModelApiFormat,
  type ModelProviderId,
} from './model-protocol';

export const PI_PROTOCOL_VERSION = 1;
export const PI_CORE_VERSION: string = pkg.dependencies['@earendil-works/pi-agent-core'];
export const FOUNDATION_SCOPE_VERSION = 'foundation-v1';
export const MAX_PROTOCOL_LINE_BYTES = 2 * 1024 * 1024;
export const MAX_COMMAND_LINE_BYTES = 2 * 1024 * 1024;

export interface RequestContext {
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  scopeVersion: string;
}

export interface PiProviderSpec {
  provider: ModelProviderId;
  apiFormat?: ModelApiFormat;
  modelId: string;
  baseUrl: string;
  apiKey: string;
  contextWindow?: number;
  maxOutputTokens?: number;
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

export type CustomActionRunMode = 'customAction' | 'cpush' | 'resolveConflict';

export interface PiConflictFileSummary {
  fileId: string;
  path: string;
  lineCount: number;
  hasBase?: boolean;
}

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
  locale?: AppLocale;
}

export interface RuntimeSkillOverrideEntry {
  name: string;
  description: string;
  files: string[];
}

export interface RuntimeSkillOverrideCatalog {
  sourceRepo?: string;
  sourceCommit?: string;
  skills: RuntimeSkillOverrideEntry[];
  disabledSkills?: string[];
}

export interface RunCustomActionCommand {
  type: 'runCustomAction';
  mode?: CustomActionRunMode;
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
  branch: string;
  detached: boolean;
  remoteBranches: string[];
  actionLabel: string;
  actionPrompt: string;
  extraPrompt: string;
  followUpPrompt?: string;
  selectedFiles: PiPlanFileEntry[];
  unselectedPaths: string[];
  localKeepPaths: string[];
  recentCommits?: string[];
  pushAuthorized?: boolean;
  upstream?: string;
  commits?: string[];
  conflictFiles?: PiConflictFileSummary[];
  locale?: AppLocale;
  commitLanguage?: CommitLanguage;
  skillOverrides?: RuntimeSkillOverrideCatalog;
  previousCheckpoint?: string;
}

export interface GenerateCustomActionCommand {
  type: 'generateCustomAction';
  requestId: string;
  context: RequestContext;
  model: PiProviderSpec;
  branch: string;
  existingLabels: string[];
  userRequest: string;
  locale?: AppLocale;
  skillOverrides?: RuntimeSkillOverrideCatalog;
}

export type PiTaskRequest =
  | ProbeRequest
  | TestConnectionRequest
  | RunCustomActionCommand
  | GenerateCustomActionCommand;

export interface PiToolResultCommand {
  type: 'toolResult';
  requestId: string;
  callId: string;
  ok: boolean;
  content: string;
  details?: unknown;
  isError?: boolean;
}

export type PiCommand = PiTaskRequest
  | PiToolResultCommand
  | { type: 'cancel'; requestId: string }
  | { type: 'shutdown' };

export interface PiActionLink {
  label: string;
  url: string;
}

export interface PiCustomActionDraft {
  label: string;
  prompt: string;
  summary: string;
}

export type PiErrorCode =
  | 'modelAuthFailed'
  | 'modelEndpointNotFound'
  | 'modelRateLimited'
  | 'modelServiceError'
  | 'modelConnectionFailed'
  | 'modelTimeout'
  | 'modelToolsUnsupported'
  | 'modelInvalidConfig'
  | 'modelRequestFailed'
  | 'piRuntimeError';

export type ProbeEvent = {
  requestId: string;
  context: RequestContext;
} & (
  | { type: 'started' }
  | { type: 'activity' }
  | { type: 'textDelta'; delta: string }
  | { type: 'toolCall'; callId: string; toolName: string; args: Record<string, unknown> }
  | { type: 'toolProgress'; callId: string; toolName: string; status: string }
  | { type: 'completed'; text: string }
  | { type: 'customActionCompleted'; summary: string; links: PiActionLink[] }
  | { type: 'customActionDrafted'; draft: PiCustomActionDraft }
  | { type: 'cancelled' }
  | { type: 'failed'; message: string; errorCode?: PiErrorCode }
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
    || !(value.apiFormat === undefined || value.apiFormat === null || isModelApiFormat(value.apiFormat))
    || typeof value.modelId !== 'string' || !value.modelId.trim()
    || typeof value.baseUrl !== 'string'
    || typeof value.apiKey !== 'string'
    || !(value.contextWindow === undefined || value.contextWindow === null || (typeof value.contextWindow === 'number' && Number.isSafeInteger(value.contextWindow) && value.contextWindow > 0))
    || !(value.maxOutputTokens === undefined || value.maxOutputTokens === null || (typeof value.maxOutputTokens === 'number' && Number.isSafeInteger(value.maxOutputTokens) && value.maxOutputTokens > 0))) {
    throw new Error('Invalid model specification');
  }
  return {
    provider: value.provider,
    ...(isModelApiFormat(value.apiFormat) ? { apiFormat: value.apiFormat } : {}),
    modelId: value.modelId.trim(),
    baseUrl: value.baseUrl.trim(),
    apiKey: value.apiKey,
    ...(typeof value.contextWindow === 'number' ? { contextWindow: value.contextWindow } : {}),
    ...(typeof value.maxOutputTokens === 'number' ? { maxOutputTokens: value.maxOutputTokens } : {}),
  };
}

function decodeStringList(value: unknown, field: string): string[] {
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === 'string')) {
    throw new Error(`Invalid ${field}`);
  }
  return value as string[];
}

function decodePlanFiles(value: unknown, allowEmpty = false): PiPlanFileEntry[] {
  if (!Array.isArray(value) || (!allowEmpty && value.length === 0)) {
    throw new Error(allowEmpty ? 'Expected plan files array' : 'Expected non-empty plan files array');
  }
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

function decodeConflictFileSummaries(value: unknown): PiConflictFileSummary[] {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error('Expected non-empty conflictFiles array');
  }
  return value.map((item) => {
    if (!isRecord(item)
      || typeof item.fileId !== 'string' || !item.fileId
      || typeof item.path !== 'string' || !item.path
      || typeof item.lineCount !== 'number' || !Number.isSafeInteger(item.lineCount) || item.lineCount < 0
      || !(item.hasBase === undefined || item.hasBase === null || typeof item.hasBase === 'boolean')) {
      throw new Error('Invalid conflictFiles entry');
    }
    return {
      fileId: item.fileId,
      path: item.path,
      lineCount: item.lineCount,
      ...(typeof item.hasBase === 'boolean' ? { hasBase: item.hasBase } : {}),
    };
  });
}

function decodeRunMode(value: unknown): CustomActionRunMode {
  if (value === undefined || value === null || value === 'customAction') return 'customAction';
  if (value === 'cpush' || value === 'resolveConflict') return value;
  throw new Error('Invalid runCustomAction mode');
}

function decodeSkillOverrides(value: unknown): RuntimeSkillOverrideCatalog | undefined {
  if (value === undefined || value === null) return undefined;
  if (!isRecord(value) || !Array.isArray(value.skills)) {
    throw new Error('Invalid skillOverrides catalog');
  }
  const skills: RuntimeSkillOverrideEntry[] = value.skills.map((entry) => {
    if (!isRecord(entry)
      || typeof entry.name !== 'string' || !entry.name.trim()
      || typeof entry.description !== 'string'
      || !Array.isArray(entry.files) || !entry.files.every((file) => typeof file === 'string')) {
      throw new Error('Invalid skillOverrides entry');
    }
    return {
      name: entry.name.trim(),
      description: entry.description,
      files: entry.files as string[],
    };
  });
  const disabledSkills = value.disabledSkills === undefined || value.disabledSkills === null
    ? undefined
    : decodeStringList(value.disabledSkills, 'disabledSkills');
  return {
    ...(typeof value.sourceRepo === 'string' && value.sourceRepo ? { sourceRepo: value.sourceRepo } : {}),
    ...(typeof value.sourceCommit === 'string' && value.sourceCommit ? { sourceCommit: value.sourceCommit } : {}),
    skills,
    ...(disabledSkills && disabledSkills.length > 0 ? { disabledSkills } : {}),
  };
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
  if (command.type === 'toolResult') {
    if (typeof command.callId !== 'string' || !command.callId.trim()
      || typeof command.ok !== 'boolean'
      || typeof command.content !== 'string'
      || !(command.isError === undefined || typeof command.isError === 'boolean')) {
      throw new Error('Invalid toolResult fields');
    }
    return {
      type: 'toolResult',
      requestId: command.requestId,
      callId: command.callId.trim(),
      ok: command.ok,
      content: command.content,
      ...(command.details !== undefined ? { details: command.details } : {}),
      ...(typeof command.isError === 'boolean' ? { isError: command.isError } : {}),
    };
  }
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
    if (!(command.locale === undefined || command.locale === null || isAppLocale(command.locale))) {
      throw new Error('Invalid locale');
    }
    return {
      type: 'testConnection',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      ...(isAppLocale(command.locale) ? { locale: command.locale } : {}),
    };
  }
  if (command.type === 'runCustomAction') {
    const mode = decodeRunMode(command.mode);
    const requiresActionPrompt = mode === 'customAction';
    if (typeof command.branch !== 'string'
      || typeof command.detached !== 'boolean'
      || typeof command.actionLabel !== 'string' || !command.actionLabel.trim()
      || typeof command.actionPrompt !== 'string' || (requiresActionPrompt && !command.actionPrompt.trim())
      || typeof command.extraPrompt !== 'string'
      || !(command.followUpPrompt === undefined || command.followUpPrompt === null || typeof command.followUpPrompt === 'string')
      || !(command.pushAuthorized === undefined || command.pushAuthorized === null || typeof command.pushAuthorized === 'boolean')
      || !(command.upstream === undefined || command.upstream === null || typeof command.upstream === 'string')
      || !(command.locale === undefined || command.locale === null || isAppLocale(command.locale))
      || !(command.commitLanguage === undefined || command.commitLanguage === null || isAppLocale(command.commitLanguage))
      || !(command.previousCheckpoint === undefined || command.previousCheckpoint === null || typeof command.previousCheckpoint === 'string')) {
      throw new Error('Invalid runCustomAction fields');
    }
    const skillOverrides = decodeSkillOverrides(command.skillOverrides);
    const recentCommits = command.recentCommits === undefined || command.recentCommits === null
      ? undefined
      : decodeStringList(command.recentCommits, 'recentCommits');
    const commits = command.commits === undefined || command.commits === null
      ? undefined
      : decodeStringList(command.commits, 'commits');
    const conflictFiles = mode === 'resolveConflict'
      ? decodeConflictFileSummaries(command.conflictFiles)
      : command.conflictFiles === undefined || command.conflictFiles === null
        ? undefined
        : decodeConflictFileSummaries(command.conflictFiles);
    return {
      type: 'runCustomAction',
      ...(mode !== 'customAction' ? { mode } : {}),
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      branch: command.branch,
      detached: command.detached,
      remoteBranches: decodeStringList(command.remoteBranches, 'remoteBranches'),
      actionLabel: command.actionLabel.trim(),
      actionPrompt: command.actionPrompt.trim(),
      extraPrompt: command.extraPrompt,
      ...(typeof command.followUpPrompt === 'string' && command.followUpPrompt.trim()
        ? { followUpPrompt: command.followUpPrompt.trim() }
        : {}),
      selectedFiles: decodePlanFiles(command.selectedFiles, mode !== 'cpush'),
      unselectedPaths: decodeStringList(command.unselectedPaths, 'unselectedPaths'),
      localKeepPaths: decodeStringList(command.localKeepPaths, 'localKeepPaths'),
      ...(recentCommits ? { recentCommits } : {}),
      ...(typeof command.pushAuthorized === 'boolean' ? { pushAuthorized: command.pushAuthorized } : {}),
      ...(typeof command.upstream === 'string' && command.upstream.trim() ? { upstream: command.upstream.trim() } : {}),
      ...(commits ? { commits } : {}),
      ...(conflictFiles ? { conflictFiles } : {}),
      ...(isAppLocale(command.locale) ? { locale: command.locale } : {}),
      ...(isAppLocale(command.commitLanguage) ? { commitLanguage: command.commitLanguage } : {}),
      ...(skillOverrides ? { skillOverrides } : {}),
      ...(typeof command.previousCheckpoint === 'string' && command.previousCheckpoint.trim()
        ? { previousCheckpoint: command.previousCheckpoint }
        : {}),
    };
  }
  if (command.type === 'generateCustomAction') {
    if (typeof command.branch !== 'string'
      || typeof command.userRequest !== 'string' || !command.userRequest.trim()
      || !(command.locale === undefined || command.locale === null || isAppLocale(command.locale))) {
      throw new Error('Invalid generateCustomAction fields');
    }
    const skillOverrides = decodeSkillOverrides(command.skillOverrides);
    return {
      type: 'generateCustomAction',
      requestId: command.requestId,
      context: decodeRequestContext(command.context),
      model: decodeProviderSpec(command.model),
      branch: command.branch,
      existingLabels: decodeStringList(command.existingLabels, 'existingLabels'),
      userRequest: command.userRequest.trim(),
      ...(isAppLocale(command.locale) ? { locale: command.locale } : {}),
      ...(skillOverrides ? { skillOverrides } : {}),
    };
  }
  throw new Error('Unknown command');
}

function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === 'object' && input !== null && !Array.isArray(input);
}
