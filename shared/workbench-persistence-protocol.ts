import { decodeCPushSessionSnapshot } from './cpush-protocol';
import type { CPushSessionSnapshot } from './cpush-protocol';
import { decodeGitReadReply, GIT_READ_PROTOCOL_VERSION } from './git-read-protocol';
import type { GitRepositorySnapshot } from './git-read-protocol';
import { isGitEditorId } from './ide-protocol';
import type { GitEditorId } from './ide-protocol';
import { decodeSavedModelConfiguration } from './model-protocol';
import type { SavedModelConfiguration } from './model-protocol';

export const WORKBENCH_PERSISTENCE_PROTOCOL_VERSION = 1;

export interface PersistedWorktreeState {
  repositoryId: string;
  worktreeId: string;
  draftText: string;
  draftEditing: boolean;
  draftOutdated: boolean;
  selectedIds: string[];
  scopeVersion: string;
  tab: 'commit' | 'review';
}

export interface WorkbenchSaveRequest {
  revision: number;
  editor: GitEditorId;
  activeWorktreeId: string | null;
  emptyDraft: string;
  emptyEditing: boolean;
  worktrees: PersistedWorktreeState[];
}

export interface WorkbenchRestoreReply {
  protocolVersion: typeof WORKBENCH_PERSISTENCE_PROTOCOL_VERSION;
  editor: GitEditorId;
  installedEditors?: GitEditorId[];
  activeWorktreeId: string | null;
  emptyDraft: string;
  emptyEditing: boolean;
  worktrees: PersistedWorktreeState[];
  snapshots: GitRepositorySnapshot[];
  models?: SavedModelConfiguration[];
  activeModelId?: string | null;
  cPushSessions?: CPushSessionSnapshot[];
  warning: string | null;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

export function decodeWorkbenchRestoreReply(value: unknown): WorkbenchRestoreReply {
  const fail = (): never => {
    throw new Error('桌面工作区持久化协议不匹配，已忽略损坏的恢复响应。');
  };
  if (!record(value) || value.protocolVersion !== WORKBENCH_PERSISTENCE_PROTOCOL_VERSION
    || !isGitEditorId(value.editor)
    || !(value.activeWorktreeId === null || typeof value.activeWorktreeId === 'string')
    || typeof value.emptyDraft !== 'string' || typeof value.emptyEditing !== 'boolean'
    || !(value.warning === null || typeof value.warning === 'string')
    || !Array.isArray(value.worktrees) || !Array.isArray(value.snapshots)) {
    return fail();
  }
  for (const item of value.worktrees) {
    if (!record(item) || typeof item.repositoryId !== 'string' || !item.repositoryId
      || typeof item.worktreeId !== 'string' || !item.worktreeId
      || typeof item.draftText !== 'string' || typeof item.draftEditing !== 'boolean'
      || typeof item.draftOutdated !== 'boolean' || typeof item.scopeVersion !== 'string'
      || !['commit', 'review'].includes(item.tab as string)
      || !Array.isArray(item.selectedIds) || !item.selectedIds.every((id) => typeof id === 'string')) {
      return fail();
    }
  }
  const snapshots: GitRepositorySnapshot[] = [];
  for (const rawSnapshot of value.snapshots) {
    const decoded = decodeGitReadReply({
      protocolVersion: GIT_READ_PROTOCOL_VERSION,
      requestId: 'restore',
      snapshot: rawSnapshot,
    });
    if (!decoded.snapshot) return fail();
    snapshots.push(decoded.snapshot);
  }
  const models = value.models === undefined ? undefined : (() => {
    if (!Array.isArray(value.models)) return fail();
    return value.models.map(decodeSavedModelConfiguration);
  })();
  if (!(value.activeModelId === undefined || value.activeModelId === null || typeof value.activeModelId === 'string')) {
    return fail();
  }
  const cPushSessions = value.cPushSessions === undefined ? undefined : (() => {
    if (!Array.isArray(value.cPushSessions)) return fail();
    return value.cPushSessions.map(decodeCPushSessionSnapshot);
  })();
  const installedEditors = value.installedEditors === undefined ? undefined : (() => {
    if (!Array.isArray(value.installedEditors) || !value.installedEditors.every(isGitEditorId)) return fail();
    return [...new Set(value.installedEditors)];
  })();
  return {
    protocolVersion: WORKBENCH_PERSISTENCE_PROTOCOL_VERSION,
    editor: value.editor,
    ...(installedEditors !== undefined ? { installedEditors } : {}),
    activeWorktreeId: value.activeWorktreeId,
    emptyDraft: value.emptyDraft,
    emptyEditing: value.emptyEditing,
    worktrees: value.worktrees as PersistedWorktreeState[],
    snapshots,
    ...(models !== undefined ? { models } : {}),
    ...(value.activeModelId !== undefined ? { activeModelId: value.activeModelId } : {}),
    ...(cPushSessions !== undefined ? { cPushSessions } : {}),
    warning: value.warning,
  };
}
