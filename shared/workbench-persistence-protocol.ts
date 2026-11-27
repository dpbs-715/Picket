import { decodeCPushSessionSnapshot } from './cpush-protocol';
import type { CPushSessionSnapshot } from './cpush-protocol';
import {
  CUSTOM_ACTION_LIMITS,
  decodeActionSessionSnapshot,
  decodeCustomActionConfig,
  type ActionSessionSnapshot,
  type CustomActionConfig,
} from './custom-action-protocol';
import { decodeGitReadReply, GIT_READ_PROTOCOL_VERSION, isValidRepoRelativePath } from './git-read-protocol';
import type { GitRepositorySnapshot } from './git-read-protocol';
import { isGitEditorId } from './ide-protocol';
import type { GitEditorId } from './ide-protocol';
import { isAppLocale } from './language-protocol';
import type { AppLocale, CommitLanguage } from './language-protocol';
import { decodeSavedModelConfiguration, decodeSavedProviderConnection } from './model-protocol';
import type { SavedModelConfiguration, SavedProviderConnection } from './model-protocol';

export const WORKBENCH_PERSISTENCE_PROTOCOL_VERSION = 2;

export const WINDOW_DIMENSIONS = {
  defaultFullWidth: 400,
  defaultFullHeight: 700,
  minFullWidth: 400,
  minFullHeight: 480,
  compactWidth: 400,
  compactDefaultHeight: 86,
  compactMinHeight: 82,
  compactMaxHeight: 280,
  maxStoredDimension: 16_384,
  maxStoredCoordinateAbs: 65_536,
} as const;

export interface WindowSize {
  width: number;
  height: number;
}

export interface WindowPosition {
  x: number;
  y: number;
}

export interface PersistedWindowPreferences {
  pinned: boolean;
  compactMode: boolean;
  pinnedExpanded: boolean;
  fullSize?: WindowSize;
  fullPosition?: WindowPosition;
  compactPosition?: WindowPosition;
}

export interface PersistedRepositorySettings {
  repositoryId: string;
  editor: GitEditorId;
  commitLanguage: CommitLanguage;
  localKeepPaths?: string[];
  customActions?: CustomActionConfig[];
  activeActionId?: string | null;
}

export interface PersistedWorktreeState {
  repositoryId: string;
  worktreeId: string;
  promptText: string;
  selectedIds: string[];
  unselectedIds?: string[];
  scopeVersion: string;
  tab?: 'commit' | 'review';
}

export interface WorkbenchSaveRequest {
  revision: number;
  locale?: AppLocale;
  editor: GitEditorId;
  defaultCommitLanguage?: CommitLanguage;
  repositorySettings?: PersistedRepositorySettings[];
  windowPreferences?: PersistedWindowPreferences;
  activeWorktreeId: string | null;
  emptyPrompt: string;
  worktrees: PersistedWorktreeState[];
}

export interface WorkbenchRestoreReply {
  protocolVersion: typeof WORKBENCH_PERSISTENCE_PROTOCOL_VERSION;
  locale?: AppLocale;
  editor: GitEditorId;
  defaultCommitLanguage?: CommitLanguage;
  repositorySettings?: PersistedRepositorySettings[];
  windowPreferences?: PersistedWindowPreferences;
  installedEditors?: GitEditorId[];
  activeWorktreeId: string | null;
  emptyPrompt: string;
  worktrees: PersistedWorktreeState[];
  snapshots: GitRepositorySnapshot[];
  connections?: SavedProviderConnection[];
  models?: SavedModelConfiguration[];
  activeModelId?: string | null;
  cPushSessions?: CPushSessionSnapshot[];
  actionSessions?: ActionSessionSnapshot[];
  warning: string | null;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function isValidStoredDimension(value: unknown, min: number): value is number {
  return typeof value === 'number'
    && Number.isInteger(value)
    && value >= min
    && value <= WINDOW_DIMENSIONS.maxStoredDimension;
}

function isValidStoredCoordinate(value: unknown): value is number {
  return typeof value === 'number'
    && Number.isInteger(value)
    && Math.abs(value) <= WINDOW_DIMENSIONS.maxStoredCoordinateAbs;
}

function decodeWindowSize(value: unknown, fail: () => never): WindowSize {
  if (!record(value)
    || !isValidStoredDimension(value.width, WINDOW_DIMENSIONS.minFullWidth)
    || !isValidStoredDimension(value.height, WINDOW_DIMENSIONS.minFullHeight)) {
    return fail();
  }
  return { width: value.width, height: value.height };
}

function decodeWindowPosition(value: unknown, fail: () => never): WindowPosition {
  if (!record(value) || !isValidStoredCoordinate(value.x) || !isValidStoredCoordinate(value.y)) {
    return fail();
  }
  return { x: value.x, y: value.y };
}

export function decodeWorkbenchRestoreReply(value: unknown): WorkbenchRestoreReply {
  const fail = (): never => {
    throw new Error('Desktop workbench persistence protocol mismatch. Ignored corrupted restore reply.');
  };
  if (!record(value) || value.protocolVersion !== WORKBENCH_PERSISTENCE_PROTOCOL_VERSION
    || !(value.locale === undefined || value.locale === null || isAppLocale(value.locale))
    || !isGitEditorId(value.editor)
    || !(value.defaultCommitLanguage === undefined || value.defaultCommitLanguage === null || isAppLocale(value.defaultCommitLanguage))
    || !(value.activeWorktreeId === null || typeof value.activeWorktreeId === 'string')
    || typeof value.emptyPrompt !== 'string'
    || !(value.warning === null || typeof value.warning === 'string')
    || !Array.isArray(value.worktrees) || !Array.isArray(value.snapshots)) {
    return fail();
  }
  const repositorySettings = value.repositorySettings === undefined ? undefined : (() => {
    if (!Array.isArray(value.repositorySettings)) return fail();
    const seen = new Set<string>();
    return value.repositorySettings.map((entry) => {
      if (!record(entry)
        || typeof entry.repositoryId !== 'string' || !entry.repositoryId
        || seen.has(entry.repositoryId)
        || !isGitEditorId(entry.editor)
        || !isAppLocale(entry.commitLanguage)
        || !(entry.localKeepPaths === undefined || (Array.isArray(entry.localKeepPaths) && entry.localKeepPaths.every(isValidRepoRelativePath)))
        || !(entry.customActions === undefined || Array.isArray(entry.customActions))
        || !(entry.activeActionId === undefined || entry.activeActionId === null || typeof entry.activeActionId === 'string')) {
        return fail();
      }
      seen.add(entry.repositoryId);
      const localKeepPaths = Array.isArray(entry.localKeepPaths)
        ? [...new Set(entry.localKeepPaths)]
        : undefined;
      let customActions: CustomActionConfig[] | undefined;
      if (Array.isArray(entry.customActions)) {
        if (entry.customActions.length > CUSTOM_ACTION_LIMITS.maxPerRepo) return fail();
        const seenActionIds = new Set<string>();
        customActions = entry.customActions.map((rawAction) => {
          const decoded = decodeCustomActionConfig(rawAction);
          if (seenActionIds.has(decoded.id)) return fail();
          seenActionIds.add(decoded.id);
          return decoded;
        });
      }
      const rawActiveActionId = typeof entry.activeActionId === 'string' && entry.activeActionId.trim()
        ? entry.activeActionId.trim()
        : null;
      const activeActionId = rawActiveActionId && customActions?.some((item) => item.id === rawActiveActionId)
        ? rawActiveActionId
        : undefined;
      return {
        repositoryId: entry.repositoryId,
        editor: entry.editor,
        commitLanguage: entry.commitLanguage,
        ...(localKeepPaths !== undefined ? { localKeepPaths } : {}),
        ...(customActions !== undefined && customActions.length > 0 ? { customActions } : {}),
        ...(activeActionId !== undefined ? { activeActionId } : {}),
      };
    });
  })();
  for (const item of value.worktrees) {
    if (!record(item) || typeof item.repositoryId !== 'string' || !item.repositoryId
      || typeof item.worktreeId !== 'string' || !item.worktreeId
      || typeof item.promptText !== 'string'
      || typeof item.scopeVersion !== 'string'
      || !(item.tab === undefined || item.tab === null || ['commit', 'review'].includes(item.tab as string))
      || !Array.isArray(item.selectedIds) || !item.selectedIds.every((id) => typeof id === 'string')
      || !(item.unselectedIds === undefined || (Array.isArray(item.unselectedIds) && item.unselectedIds.every((id) => typeof id === 'string')))) {
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
  const connections = value.connections === undefined ? undefined : (() => {
    if (!Array.isArray(value.connections)) return fail();
    return value.connections.map(decodeSavedProviderConnection);
  })();
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
  const actionSessions = value.actionSessions === undefined ? undefined : (() => {
    if (!Array.isArray(value.actionSessions)) return fail();
    return value.actionSessions.map(decodeActionSessionSnapshot);
  })();
  const installedEditors = value.installedEditors === undefined ? undefined : (() => {
    if (!Array.isArray(value.installedEditors) || !value.installedEditors.every(isGitEditorId)) return fail();
    return [...new Set(value.installedEditors)];
  })();
  const windowPreferences = value.windowPreferences === undefined || value.windowPreferences === null ? undefined : (() => {
    const raw = value.windowPreferences;
    if (!record(raw)
      || typeof raw.pinned !== 'boolean'
      || typeof raw.compactMode !== 'boolean'
      || typeof raw.pinnedExpanded !== 'boolean') {
      return fail();
    }
    const fullSize = raw.fullSize === undefined || raw.fullSize === null ? undefined : decodeWindowSize(raw.fullSize, fail);
    const fullPosition = raw.fullPosition === undefined || raw.fullPosition === null ? undefined : decodeWindowPosition(raw.fullPosition, fail);
    const compactPosition = raw.compactPosition === undefined || raw.compactPosition === null ? undefined : decodeWindowPosition(raw.compactPosition, fail);
    return {
      pinned: raw.pinned,
      compactMode: raw.compactMode,
      pinnedExpanded: raw.pinnedExpanded,
      ...(fullSize ? { fullSize } : {}),
      ...(fullPosition ? { fullPosition } : {}),
      ...(compactPosition ? { compactPosition } : {}),
    };
  })();
  return {
    protocolVersion: WORKBENCH_PERSISTENCE_PROTOCOL_VERSION,
    ...(isAppLocale(value.locale) ? { locale: value.locale } : {}),
    editor: value.editor,
    ...(isAppLocale(value.defaultCommitLanguage) ? { defaultCommitLanguage: value.defaultCommitLanguage } : {}),
    ...(repositorySettings !== undefined ? { repositorySettings } : {}),
    ...(windowPreferences !== undefined ? { windowPreferences } : {}),
    ...(installedEditors !== undefined ? { installedEditors } : {}),
    activeWorktreeId: value.activeWorktreeId,
    emptyPrompt: value.emptyPrompt,
    worktrees: value.worktrees as PersistedWorktreeState[],
    snapshots,
    ...(connections !== undefined ? { connections } : {}),
    ...(models !== undefined ? { models } : {}),
    ...(value.activeModelId !== undefined ? { activeModelId: value.activeModelId } : {}),
    ...(cPushSessions !== undefined ? { cPushSessions } : {}),
    ...(actionSessions !== undefined ? { actionSessions } : {}),
    warning: value.warning,
  };
}
