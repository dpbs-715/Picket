import {
  CUSTOM_ACTION_LIMITS,
  computeCustomActionPromptHash,
  sanitizeCustomActionConfig,
  type CustomActionConfig,
} from '../../../../shared/custom-action-protocol';
import { getMessages } from '../../../i18n/messages';
import { finishCustomActionCommand } from './repositories';
import {
  computeSyncCustomActionAcrossRepos,
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  isActiveSession,
  localKeepPathsFor,
} from './selectors';
import type { Message, WorkbenchAction, WorkbenchState } from './types';

export type CustomActionWorkbenchAction = Extract<
  WorkbenchAction,
  {
    type:
      | 'customActionStarted'
      | 'customActionSettled'
      | 'customActionFailed'
      | 'selectActiveAction'
      | 'saveCustomAction'
      | 'deleteCustomAction'
      | 'syncCustomActionToAllRepos'
      | 'trustCustomActionWrites'
      | 'resetCustomActionWriteTrust';
  }
>;

export function isCustomActionWorkbenchAction(
  action: WorkbenchAction,
): action is CustomActionWorkbenchAction {
  return (
    action.type === 'customActionStarted'
    || action.type === 'customActionSettled'
    || action.type === 'customActionFailed'
    || action.type === 'selectActiveAction'
    || action.type === 'saveCustomAction'
    || action.type === 'deleteCustomAction'
    || action.type === 'syncCustomActionToAllRepos'
    || action.type === 'trustCustomActionWrites'
    || action.type === 'resetCustomActionWriteTrust'
  );
}

export function reduceCustomAction(state: WorkbenchState, action: CustomActionWorkbenchAction): WorkbenchState {
  if (action.type === 'customActionStarted') {
    const t = getMessages(state.locale).workbench;
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || isActiveSession(tree)) return tree;
        const progressMsg: Message = {
          id: `progress-${action.runId}`,
          kind: 'progress',
          title: action.actionLabel,
          description: t.cPushInitialDescription,
          progress: { completed: 1, total: 3, label: t.cPushInitialProgressLabel },
        };
        return {
          ...tree,
          sessionStatus: 'running',
          dockExpanded: action.expandDock ? true : tree.dockExpanded,
          messages: [progressMsg],
          activeRun: {
            kind: 'customAction',
            runId: action.runId,
            actionId: action.actionId,
            actionLabel: action.actionLabel,
            timeline: [{ state: 'active', label: action.actionLabel }],
            commits: [],
            links: [],
          },
        };
      }),
    };
  }
  if (action.type === 'customActionSettled') {
    return finishCustomActionCommand(state, action.reply, action.expandDock);
  }
  if (action.type === 'customActionFailed') {
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (
          tree.worktreeId !== action.worktreeId
          || tree.activeRun?.kind !== 'customAction'
          || tree.activeRun.runId !== action.runId
        ) {
          return tree;
        }
        const errorMsg: Message | undefined = action.message
          ? {
              id: `error-${action.runId}`,
              kind: 'error',
              title: tree.activeRun.actionLabel,
              description: action.message,
            }
          : undefined;
        return {
          ...tree,
          sessionStatus: 'idle',
          dockExpanded: errorMsg ? tree.dockExpanded : false,
          messages: errorMsg ? [errorMsg] : [],
          activeRun: errorMsg
            ? {
                ...tree.activeRun,
                pendingCallId: undefined,
              }
            : undefined,
        };
      }),
    };
  }
  if (action.type === 'selectActiveAction') {
    const prev = state.repositorySettings[action.repositoryId];
    const actions = prev?.customActions ?? [];
    const nextActiveId = action.actionId && actions.some((item) => item.id === action.actionId)
      ? action.actionId
      : undefined;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    return {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          ...(actions.length ? { customActions: actions } : {}),
          ...(nextActiveId ? { activeActionId: nextActiveId } : {}),
        },
      },
      settingsModified: true,
    };
  }
  if (action.type === 'saveCustomAction') {
    const sanitized = sanitizeCustomActionConfig(action.action);
    if (!sanitized.id || !sanitized.label || !sanitized.prompt) return state;
    const prev = state.repositorySettings[action.repositoryId];
    const existing = prev?.customActions ?? [];
    const idx = existing.findIndex((item) => item.id === sanitized.id);
    let nextActions: CustomActionConfig[];
    let savedAction: CustomActionConfig;
    if (idx >= 0) {
      const prior = existing[idx];
      const promptUnchanged = prior.prompt === sanitized.prompt;
      savedAction = sanitizeCustomActionConfig({
        ...sanitized,
        templateKey: sanitized.templateKey ?? prior.templateKey,
        ...(promptUnchanged && prior.skipWriteConfirm && prior.trustedPromptHash
          ? { skipWriteConfirm: prior.skipWriteConfirm, trustedPromptHash: prior.trustedPromptHash }
          : {}),
      });
      nextActions = existing.map((item, i) => (i === idx ? savedAction : item));
    } else {
      if (existing.length >= CUSTOM_ACTION_LIMITS.maxPerRepo) return state;
      savedAction = sanitized;
      nextActions = [...existing, savedAction];
    }
    const nextActiveId = action.selectAsActive
      ? savedAction.id
      : prev?.activeActionId && nextActions.some((item) => item.id === prev.activeActionId)
        ? prev.activeActionId
        : undefined;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    const savedState: WorkbenchState = {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          customActions: nextActions,
          ...(nextActiveId ? { activeActionId: nextActiveId } : {}),
        },
      },
      settingsModified: true,
    };
    const syncTargets = action.syncTargetRepositoryIds;
    if (!syncTargets?.length) return savedState;
    const { repositorySettings, result } = computeSyncCustomActionAcrossRepos(
      savedState,
      action.repositoryId,
      savedAction,
      action.syncOverwrite ?? true,
      syncTargets,
    );
    if (result.added === 0 && result.updated === 0) return savedState;
    return {
      ...savedState,
      repositorySettings,
    };
  }
  if (action.type === 'deleteCustomAction') {
    const prev = state.repositorySettings[action.repositoryId];
    const existing = prev?.customActions ?? [];
    const nextActions = existing.filter((item) => item.id !== action.actionId);
    if (nextActions.length === existing.length) return state;
    const nextActiveId = prev?.activeActionId === action.actionId ? undefined : prev?.activeActionId ?? undefined;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    return {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          ...(nextActions.length ? { customActions: nextActions } : {}),
          ...(nextActiveId ? { activeActionId: nextActiveId } : {}),
        },
      },
      settingsModified: true,
    };
  }
  if (action.type === 'syncCustomActionToAllRepos') {
    const { repositorySettings, result } = computeSyncCustomActionAcrossRepos(
      state,
      action.sourceRepositoryId,
      action.action,
      Boolean(action.overwrite),
      action.targetRepositoryIds,
    );
    if (result.added === 0 && result.updated === 0) return state;
    return {
      ...state,
      repositorySettings,
      settingsModified: true,
    };
  }
  const prev = state.repositorySettings[action.repositoryId];
  const existing = prev?.customActions ?? [];
  let changed = false;
  const nextActions = existing.map((item) => {
    if (item.id !== action.actionId) return item;
    if (action.type === 'resetCustomActionWriteTrust') {
      if (!item.skipWriteConfirm && !item.trustedPromptHash) return item;
      changed = true;
      return sanitizeCustomActionConfig({
        id: item.id,
        ...(item.templateKey ? { templateKey: item.templateKey } : {}),
        label: item.label,
        prompt: item.prompt,
      });
    }
    const expected = computeCustomActionPromptHash(item.prompt);
    if (expected !== action.trustedPromptHash) return item;
    changed = true;
    return {
      ...item,
      skipWriteConfirm: true,
      trustedPromptHash: expected,
    };
  });
  if (!changed) return state;
  const keepPaths = localKeepPathsFor(state, action.repositoryId);
  return {
    ...state,
    repositorySettings: {
      ...state.repositorySettings,
      [action.repositoryId]: {
        editor: effectiveEditorFor(state, action.repositoryId),
        commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
        ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
        customActions: nextActions,
        ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
      },
    },
    settingsModified: true,
  };
}
