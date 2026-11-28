import {
  CUSTOM_ACTION_LIMITS,
  DEFAULT_ACTION_APPROVAL_MODE,
  sanitizeCustomActionConfig,
  type CustomActionConfig,
  type CustomActionSyncResult,
} from '../../../../shared/custom-action-protocol';
import { DEFAULT_APP_LOCALE, DEFAULT_COMMIT_LANGUAGE, type AppLocale, type CommitLanguage } from '../../../../shared/language-protocol';
import type { RunCommandConfig, RunEnvEntry } from '../../../../shared/run-protocol';
import { DEFAULT_APP_THEME } from '../../../../shared/theme-protocol';
import {
  WINDOW_DIMENSIONS,
  type PersistedWindowPreferences,
} from '../../../../shared/workbench-persistence-protocol';
import { getMessages } from '../../../i18n/messages';
import type {
  Change,
  CompactStatusSummary,
  EditorId,
  Message,
  RepositorySettings,
  SettingsReturnTarget,
  Target,
  WorkbenchPresentation,
  WorkbenchState,
  Worktree,
} from './types';

export function resolveWorkbenchPresentation(
  compactMode: boolean,
  settingsReturnTo: SettingsReturnTarget | null,
): WorkbenchPresentation {
  if (settingsReturnTo === 'compact') {
    return {
      surface: 'standaloneSettings',
      windowSizeMode: 'full',
      mountWorkbenchContent: false,
      showSettings: true,
    };
  }
  if (compactMode) {
    return {
      surface: 'compact',
      windowSizeMode: 'compact',
      mountWorkbenchContent: false,
      showSettings: false,
    };
  }
  return {
    surface: 'workbench',
    windowSizeMode: 'full',
    mountWorkbenchContent: true,
    showSettings: settingsReturnTo === 'workbench',
  };
}

export function defaultWindowPreferences(): PersistedWindowPreferences {
  return {
    pinned: false,
    compactMode: false,
    pinnedExpanded: false,
    fullSize: {
      width: WINDOW_DIMENSIONS.defaultFullWidth,
      height: WINDOW_DIMENSIONS.defaultFullHeight,
    },
  };
}

export function computeNextPinPreferences(
  current: PersistedWindowPreferences,
  nextPinned: boolean,
): PersistedWindowPreferences {
  return {
    ...current,
    pinned: nextPinned,
  };
}

export function computeNextCompactPreferences(
  current: PersistedWindowPreferences,
  nextCompactMode: boolean,
): PersistedWindowPreferences {
  return {
    ...current,
    compactMode: nextCompactMode,
  };
}

export function emptyWorkbench(): WorkbenchState {
  return {
    repositories: [],
    worktrees: [],
    activeWorktreeId: null,
    emptyPrompt: '',
    connections: [],
    models: [],
    activeModelId: null,
    locale: DEFAULT_APP_LOCALE,
    theme: DEFAULT_APP_THEME,
    actionApprovalMode: DEFAULT_ACTION_APPROVAL_MODE,
    editor: 'idea',
    defaultCommitLanguage: DEFAULT_COMMIT_LANGUAGE,
    repositorySettings: {},
    windowPrefs: defaultWindowPreferences(),
    navigationRevision: 0,
    repositoryReads: {},
    repositoryBranches: {},
  };
}

export function effectiveEditorFor(state: WorkbenchState, repositoryId?: string | null): EditorId {
  const desired = repositoryId
    ? state.repositorySettings[repositoryId]?.editor ?? state.editor
    : state.editor;
  if (state.installedEditors?.length && !state.installedEditors.includes(desired)) {
    return state.installedEditors[0];
  }
  return desired;
}

export function effectiveCommitLanguageFor(state: WorkbenchState, repositoryId?: string | null): CommitLanguage {
  return repositoryId
    ? state.repositorySettings[repositoryId]?.commitLanguage ?? state.defaultCommitLanguage
    : state.defaultCommitLanguage;
}

export function localKeepPathsFor(state: WorkbenchState, repositoryId?: string | null): string[] {
  if (!repositoryId) return [];
  const fromSettings = state.repositorySettings[repositoryId]?.localKeepPaths;
  if (fromSettings) return fromSettings;
  return state.repositories.find((repository) => repository.id === repositoryId)?.localKeepPaths ?? [];
}

export function customActionsFor(state: WorkbenchState, repositoryId?: string | null): CustomActionConfig[] {
  if (!repositoryId) return [];
  return state.repositorySettings[repositoryId]?.customActions ?? [];
}

export function runCommandsFor(state: WorkbenchState, repositoryId?: string | null): RunCommandConfig[] {
  if (!repositoryId) return [];
  return state.repositorySettings[repositoryId]?.runCommands ?? [];
}

export function runSharedEnvFor(state: WorkbenchState, repositoryId?: string | null): RunEnvEntry[] {
  if (!repositoryId) return [];
  return state.repositorySettings[repositoryId]?.runSharedEnv ?? [];
}

export function activeCustomActionFor(state: WorkbenchState, repositoryId?: string | null): CustomActionConfig | undefined {
  if (!repositoryId) return undefined;
  const settings = state.repositorySettings[repositoryId];
  const activeId = settings?.activeActionId;
  if (!activeId) return undefined;
  return settings.customActions?.find((item) => item.id === activeId);
}

export type CustomActionSyncRepoStatus = 'new' | 'synced' | 'sameLabel' | 'full';

export interface CustomActionSyncRepoTarget {
  id: string;
  name: string;
  existingActions: CustomActionConfig[];
}

export function classifyCustomActionSyncTarget(
  existingActions: readonly CustomActionConfig[],
  candidate: { id?: string; templateKey?: string; label: string },
): CustomActionSyncRepoStatus {
  const templateKey = candidate.templateKey ?? candidate.id;
  const normalizedLabel = candidate.label.trim().toLowerCase();
  if (templateKey && existingActions.some((item) => item.templateKey === templateKey)) {
    return 'synced';
  }
  if (normalizedLabel && existingActions.some((item) => item.label.trim().toLowerCase() === normalizedLabel)) {
    return 'sameLabel';
  }
  if (existingActions.length >= CUSTOM_ACTION_LIMITS.maxPerRepo) {
    return 'full';
  }
  return 'new';
}

export function computeSyncCustomActionAcrossRepos(
  state: WorkbenchState,
  sourceRepositoryId: string,
  action: CustomActionConfig,
  overwrite = false,
  targetRepositoryIds?: readonly string[],
): { repositorySettings: Record<string, RepositorySettings>; result: CustomActionSyncResult } {
  const sanitized = sanitizeCustomActionConfig(action);
  const templateKey = sanitized.templateKey ?? sanitized.id;
  const normalizedLabel = sanitized.label.toLowerCase();
  const nextSettings: Record<string, RepositorySettings> = { ...state.repositorySettings };
  const allowedTargets = targetRepositoryIds ? new Set(targetRepositoryIds) : null;
  let added = 0;
  let updated = 0;
  let skipped = 0;

  // Ensure the source repository's copy records the same templateKey so future syncs recognize lineage.
  const sourcePrev = nextSettings[sourceRepositoryId];
  if (sourcePrev?.customActions?.some((item) => item.id === sanitized.id && !item.templateKey)) {
    nextSettings[sourceRepositoryId] = {
      ...sourcePrev,
      customActions: sourcePrev.customActions.map((item) =>
        item.id === sanitized.id ? { ...item, templateKey } : item,
      ),
    };
  }

  for (const repo of state.repositories) {
    if (repo.id === sourceRepositoryId) continue;
    if (allowedTargets && !allowedTargets.has(repo.id)) continue;
    const prev = nextSettings[repo.id];
    const existingList = prev?.customActions ?? [];
    const byTemplateIndex = existingList.findIndex((item) => item.templateKey && item.templateKey === templateKey);
    const byLabelIndex = existingList.findIndex((item) => item.label.trim().toLowerCase() === normalizedLabel);
    const matchIndex = byTemplateIndex >= 0 ? byTemplateIndex : byLabelIndex;
    if (matchIndex >= 0) {
      if (!overwrite) {
        skipped += 1;
        continue;
      }
      if (byTemplateIndex >= 0 && byLabelIndex >= 0 && byTemplateIndex !== byLabelIndex) {
        skipped += 1;
        continue;
      }
      const existing = existingList[matchIndex];
      const replaced = sanitizeCustomActionConfig({
        id: existing.id,
        templateKey,
        label: sanitized.label,
        prompt: sanitized.prompt,
      });
      const updatedList = existingList.map((item, idx) => (idx === matchIndex ? replaced : item));
      nextSettings[repo.id] = {
        editor: prev?.editor ?? effectiveEditorFor(state, repo.id),
        commitLanguage: prev?.commitLanguage ?? effectiveCommitLanguageFor(state, repo.id),
        ...(prev?.localKeepPaths?.length ? { localKeepPaths: prev.localKeepPaths } : {}),
        customActions: updatedList,
        ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
        ...(prev?.runCommands?.length ? { runCommands: prev.runCommands } : {}),
        ...(prev?.runSharedEnv?.length ? { runSharedEnv: prev.runSharedEnv } : {}),
      };
      updated += 1;
      continue;
    }
    if (existingList.length >= CUSTOM_ACTION_LIMITS.maxPerRepo) {
      skipped += 1;
      continue;
    }
    const copy: CustomActionConfig = sanitizeCustomActionConfig({
      id: `act-${crypto.randomUUID().slice(0, 12)}`,
      templateKey,
      label: sanitized.label,
      prompt: sanitized.prompt,
    });
    nextSettings[repo.id] = {
      editor: prev?.editor ?? effectiveEditorFor(state, repo.id),
      commitLanguage: prev?.commitLanguage ?? effectiveCommitLanguageFor(state, repo.id),
      ...(prev?.localKeepPaths?.length ? { localKeepPaths: prev.localKeepPaths } : {}),
      customActions: [...existingList, copy],
      ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
      ...(prev?.runCommands?.length ? { runCommands: prev.runCommands } : {}),
      ...(prev?.runSharedEnv?.length ? { runSharedEnv: prev.runSharedEnv } : {}),
    };
    added += 1;
  }

  return {
    repositorySettings: nextSettings,
    result: { added, updated, skipped },
  };
}

export function normalChanges(worktree: Pick<Worktree, 'files'>): Change[] {
  return worktree.files.filter((file) => !file.localKeep);
}

export function localKeepChanges(worktree: Pick<Worktree, 'files'>): Change[] {
  return worktree.files.filter((file) => !!file.localKeep);
}

export function selectedLocalKeepChanges(worktree: Pick<Worktree, 'files' | 'selectedIds'>): Change[] {
  return worktree.files.filter((file) => !!file.localKeep && worktree.selectedIds.includes(file.id));
}

export function computeNormalUnselectedIds(files: readonly Change[], selectedIds: readonly string[]): string[] {
  const selectedSet = new Set(selectedIds);
  return files.filter((file) => !file.localKeep && !selectedSet.has(file.id)).map((file) => file.id);
}

export function targetOf(worktree: Worktree): Target {
  return { repositoryId: worktree.repositoryId, worktreeId: worktree.worktreeId, sessionId: worktree.sessionId, scopeVersion: worktree.scopeVersion };
}

export function selectedFiles(worktree: Worktree): Change[] {
  return worktree.files.filter((file) => worktree.selectedIds.includes(file.id));
}

export function scopeSummary(worktree?: Worktree, locale: AppLocale = DEFAULT_APP_LOCALE): string {
  const t = getMessages(locale).changes;
  if (worktree?.readError) return t.statusUnavailable;
  if (!worktree || !worktree.files.length) return t.noChanges;
  const selected = selectedFiles(worktree);
  const normalCount = normalChanges(worktree).length;
  const selectedKeptCount = selectedLocalKeepChanges(worktree).length;
  const conflicts = worktree.files.filter((file) => file.status === 'U').length;
  if (!selected.length) {
    return conflicts ? `${t.noneSelected} · ${t.localKeepConflicts(conflicts)}` : t.noneSelected;
  }
  return t.selectedSummary(
    selected.length,
    normalCount + selectedKeptCount,
    selected.some((file) => file.range.kind === 'partial'),
    conflicts,
  );
}

export function splitPath(path: string) {
  const separator = path.lastIndexOf('/');
  return { directory: path.slice(0, Math.max(separator, 0)), name: path.slice(separator + 1) };
}

export function groupFiles(files: Change[], rootDirectoryLabel = getMessages().changes.rootDirectory) {
  const groups = new Map<string, Change[]>();
  for (const file of files) {
    const directory = splitPath(file.path).directory || rootDirectoryLabel;
    groups.set(directory, [...(groups.get(directory) ?? []), file]);
  }
  return [...groups].map(([directory, groupedFiles]) => ({ directory, files: groupedFiles }));
}

export const messageLabels: Record<Message['kind'], string> = getMessages().messageLabels;

const priorities: Record<Message['kind'], number> = {
  conflict: 100, question: 95, waitingManual: 90, resolved: 85, pushFailed: 80, error: 75, progress: 60, done: 30,
};

export function orderedMessages(messages: Message[]) {
  return messages.toSorted((a, b) => priorities[b.kind] - priorities[a.kind]);
}

export function isProtected(branch: string) {
  return branch === 'main' || branch === 'master';
}

export function isActiveSession(worktree: Worktree) {
  return worktree.sessionStatus !== 'idle' && worktree.sessionStatus !== 'done';
}

function extractCommitStepProgress(message?: Message): { completed: number; total: number } | undefined {
  if (!message) return undefined;
  if (message.id.includes('-step-') || /commit|\u63d0\u4ea4/i.test(`${message.title} ${message.progress?.label ?? ''}`)) {
    const match = /(\d+)\s*\/\s*(\d+)/.exec(`${message.progress?.label ?? ''} ${message.title}`);
    if (match) {
      const completed = Number(match[1]);
      const total = Number(match[2]);
      if (completed > 0 && total >= completed) return { completed, total };
    }
  }
  return undefined;
}

export function compactStatusSummary(
  worktree: Worktree | undefined,
  hasModel: boolean,
  locale: AppLocale = DEFAULT_APP_LOCALE,
): CompactStatusSummary {
  const t = getMessages(locale).compact.status;
  if (!worktree) {
    return { tone: 'muted', label: t.noRepo, expandTarget: 'repo' };
  }
  if (isActiveSession(worktree)) {
    switch (worktree.sessionStatus) {
      case 'conflict':
        return { tone: 'danger', label: t.conflict, expandTarget: 'dock' };
      case 'pushFailed':
        return { tone: 'danger', label: t.pushFailed, expandTarget: 'dock' };
      case 'needsDecision':
        return { tone: 'warning', label: t.needsDecision, expandTarget: 'dock' };
      case 'waitingManual':
        return { tone: 'warning', label: t.waitingManual, expandTarget: 'dock' };
      case 'running': {
        const progressMsg = orderedMessages(worktree.messages).find((item) => item.kind === 'progress');
        const step = extractCommitStepProgress(progressMsg);
        return {
          tone: 'running',
          label: step ? t.committingStep(step.completed, step.total) : t.running,
        };
      }
    }
  }
  if (worktree.readError) {
    return { tone: 'danger', label: t.readError, expandTarget: 'changes' };
  }
  if (worktree.detached) {
    return { tone: 'warning', label: t.detachedHead, expandTarget: 'changes' };
  }
  if (worktree.files.length === 0) {
    const doneMsg = worktree.sessionStatus === 'done'
      ? orderedMessages(worktree.messages).find((item) => item.kind === 'done')
      : undefined;
    if (doneMsg) {
      return { tone: 'success', label: `${doneMsg.title} ▸`, expandTarget: 'dock' };
    }
    return { tone: 'muted', label: t.clean };
  }
  const selectedCount = selectedFiles(worktree).length;
  const total = normalChanges(worktree).length + selectedLocalKeepChanges(worktree).length;
  if (selectedCount === 0) {
    return { tone: 'muted', label: t.noneSelected(worktree.files.length), expandTarget: 'changes' };
  }
  if (!hasModel) {
    return { tone: 'warning', label: t.selectedNoModel(selectedCount, total), expandTarget: 'changes' };
  }
  return { tone: 'ready', label: t.selectedReady(selectedCount, total), expandTarget: 'changes' };
}

export function primaryAction(
  worktree: Worktree | undefined,
  hasModel: boolean,
  locale: AppLocale = DEFAULT_APP_LOCALE,
  activeCustomAction?: CustomActionConfig,
) {
  const labels = getMessages(locale).primaryActions;
  if (worktree && isActiveSession(worktree)) {
    return {
      label: labels[worktree.sessionStatus as 'running' | 'needsDecision' | 'conflict' | 'waitingManual' | 'pushFailed'],
      configureModel: false,
      resume: true,
      disabled: false,
    };
  }
  if (!hasModel) {
    return {
      label: labels.configureModel,
      configureModel: true,
      resume: false,
      disabled: false,
    };
  }
  if (activeCustomAction) {
    return {
      label: activeCustomAction.label,
      configureModel: false,
      resume: false,
      disabled: !worktree || !!worktree.readError,
    };
  }
  return {
    label: labels.cPush,
    configureModel: false,
    resume: false,
    disabled: !worktree || !!worktree.readError || selectedFiles(worktree).length === 0,
  };
}
