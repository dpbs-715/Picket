import { WINDOW_DIMENSIONS } from '../../../../shared/workbench-persistence-protocol';
import { getMessages } from '../../../i18n/messages';
import { isCustomActionWorkbenchAction, reduceCustomAction } from './reducerCustomActions';
import { matchesTarget, receiveEvent, updateWorktreeSelection } from './reducerEvents';
import {
  applyCustomActionProgress,
  applyLocalKeepUpdate,
  finishCPushCommand,
  finishRepositoryBranch,
  finishRepositoryLocalKeep,
  finishRepositoryRead,
  hydrateWorkbench,
  receiveCPushProgress,
} from './repositories';
import {
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  isActiveSession,
  localKeepPathsFor,
  normalChanges,
} from './selectors';
import type { Message, WorkbenchAction, WorkbenchState, Worktree } from './types';

function reduceWorktreeLocal(worktree: Worktree, action: WorkbenchAction): Worktree {
  switch (action.type) {
    case 'select': {
      if (!worktree.files.some((file) => file.id === action.fileId)) return worktree;
      const selectedIds = worktree.selectedIds.includes(action.fileId)
        ? worktree.selectedIds.filter((id) => id !== action.fileId)
        : [...worktree.selectedIds, action.fileId];
      return updateWorktreeSelection(worktree, selectedIds);
    }
    case 'selectAll': {
      if (!worktree.files.length) return worktree;
      const normal = normalChanges(worktree);
      if (!normal.length) {
        return worktree.selectedIds.length ? updateWorktreeSelection(worktree, []) : worktree;
      }
      const allNormalSelected = normal.every((file) => worktree.selectedIds.includes(file.id));
      const selectedIds = allNormalSelected
        ? []
        : [...new Set([...worktree.selectedIds, ...normal.map((file) => file.id)])];
      return updateWorktreeSelection(worktree, selectedIds);
    }
    case 'selectFiles': {
      const liveIds = new Set(worktree.files.map((file) => file.id));
      const targetIds = [...new Set(action.fileIds)].filter((id) => liveIds.has(id));
      if (!targetIds.length) return worktree;
      const targetSet = new Set(targetIds);
      const allTargetSelected = targetIds.every((id) => worktree.selectedIds.includes(id));
      const selectedIds = allTargetSelected
        ? worktree.selectedIds.filter((id) => !targetSet.has(id))
        : [...worktree.selectedIds, ...targetIds.filter((id) => !worktree.selectedIds.includes(id))];
      return updateWorktreeSelection(worktree, selectedIds);
    }
    case 'prompt':
      return { ...worktree, prompt: { text: action.text, revision: worktree.prompt.revision + 1 } };
    case 'clearPrompt':
      return { ...worktree, prompt: { text: '', revision: worktree.prompt.revision + 1 } };
    case 'planGroupEdited': {
      if (!worktree.plan || !worktree.plan.groups.some((group) => group.id === action.groupId)) return worktree;
      return {
        ...worktree,
        plan: {
          ...worktree.plan,
          groups: worktree.plan.groups.map((group) => (group.id === action.groupId ? { ...group, message: action.message } : group)),
        },
        planRevision: worktree.planRevision + 1,
      };
    }
    case 'planCleared': {
      if (!worktree.plan && !worktree.requests.plan) return worktree;
      return {
        ...worktree,
        plan: undefined,
        planRevision: worktree.planRevision + 1,
        requests: { ...worktree.requests, plan: undefined },
      };
    }
    case 'tab':
      return { ...worktree, tab: action.tab, dockExpanded: false };
    case 'dock':
      return { ...worktree, dockExpanded: action.expanded };
    case 'dismissMessage': {
      const nextMessages = worktree.messages.filter((message) => message.id !== action.messageId);
      const nextReplies = { ...worktree.replies };
      delete nextReplies[action.messageId];
      const stillHasDone = nextMessages.some((message) => message.kind === 'done');
      const nextSessionStatus = worktree.sessionStatus === 'done' && !stillHasDone
        ? 'idle'
        : worktree.sessionStatus;
      return {
        ...worktree,
        messages: nextMessages,
        replies: nextReplies,
        sessionStatus: nextSessionStatus,
        customAction: nextSessionStatus === 'idle' ? undefined : worktree.customAction,
        dockExpanded: false,
      };
    }
    case 'reply':
      return { ...worktree, replies: { ...worktree.replies, [action.messageId]: action.text } };
    default:
      return worktree;
  }
}

export function workbenchReducer(state: WorkbenchState, action: WorkbenchAction): WorkbenchState {
  if (action.type === 'workbenchHydrated') return hydrateWorkbench(state, action.restored);
  if (action.type === 'event' && action.event.type === 'cPushProgress') {
    return receiveCPushProgress(state, action.event.progress);
  }
  if (action.type === 'event' && action.event.type === 'customActionProgress') {
    return applyCustomActionProgress(state, action.event.progress);
  }
  if (isCustomActionWorkbenchAction(action)) return reduceCustomAction(state, action);

  if (action.type === 'cPushStarted') {
    const t = getMessages(state.locale).workbench;
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || (isActiveSession(tree) && !tree.cPush?.planOnly)) return tree;
        const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
        const progressMsg: Message = {
          id: `progress-${action.runId}`,
          kind: 'progress',
          title: t.cPushInitialTitle,
          description: t.cPushInitialDescription,
          scope: t.cPushInitialScope(action.frozenFileIds.length),
          progress: { completed: 1, total: 3, label: t.cPushInitialProgressLabel },
        };
        return {
          ...tree,
          requests: { ...tree.requests, plan: undefined },
          sessionStatus: 'running',
          dockExpanded: action.expandDock ? true : tree.dockExpanded,
          messages: [...reviewMessages, progressMsg],
          cPush: {
            runId: action.runId,
            frozenTarget: action.frozenTarget,
            frozenFileIds: action.frozenFileIds,
            promptRevisionAtStart: tree.prompt.revision,
            modelId: action.modelId,
            timeline: [
              { state: 'done', label: t.cPushTimelineFrozen(action.frozenFileIds.length) },
              { state: 'active', label: t.cPushTimelinePlanning },
            ],
            qaLog: [],
            commits: [],
            canRetryRemaining: false,
            planOnly: false,
          },
        };
      }),
    };
  }
  if (action.type === 'cPushSettled') return finishCPushCommand(state, action.reply, action.expandDock);
  if (action.type === 'cPushStartFailed') {
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || tree.cPush?.runId !== action.runId || tree.cPush.commits.length > 0) {
          return tree;
        }
        return {
          ...tree,
          sessionStatus: 'idle',
          dockExpanded: false,
          messages: tree.messages.filter((msg) => msg.kind === 'review'),
          cPush: undefined,
        };
      }),
    };
  }
  if (action.type === 'repositoryBranchStarted') {
    return {
      ...state,
      navigationRevision: state.navigationRevision + 1,
      repositoryBranches: {
        ...state.repositoryBranches,
        [action.repositoryId]: { requestId: action.requestId, navigationRevision: state.navigationRevision + 1 },
      },
    };
  }
  if (action.type === 'repositoryBranchFinished') return finishRepositoryBranch(state, action.repositoryId, action.reply);
  if (action.type === 'repositoryBranchFailed') {
    if (state.repositoryBranches?.[action.repositoryId]?.requestId !== action.requestId) return state;
    const repositoryBranches = { ...state.repositoryBranches };
    delete repositoryBranches[action.repositoryId];
    return { ...state, repositoryBranches };
  }
  if (action.type === 'repositoryReadStarted') {
    const navigationRevision = state.navigationRevision + (action.key === 'open' ? 1 : 0);
    return { ...state, navigationRevision, repositoryReads: { ...state.repositoryReads, [action.key]: { requestId: action.requestId, navigationRevision } } };
  }
  if (action.type === 'repositoryReadFinished') return finishRepositoryRead(state, action.key, action.reply);
  if (action.type === 'repositoryReadFailed') {
    if (state.repositoryReads[action.key]?.requestId !== action.requestId) return state;
    const repositoryReads = { ...state.repositoryReads };
    delete repositoryReads[action.key];
    const fallbackError = getMessages(state.locale).workbench.readFailedFallback;
    return {
      ...state,
      repositoryReads,
      worktrees: state.worktrees.map((tree) => tree.worktreeId === action.worktreeId
        ? {
            ...tree,
            readError: action.error ?? fallbackError,
            review: { ...tree.review, status: tree.review.status === 'running' ? tree.requests.review?.previousReviewStatus ?? 'idle' : tree.review.status },
            requests: {},
          }
        : tree),
    };
  }
  if (action.type === 'localKeepUpdated') {
    return applyLocalKeepUpdate(state, action.repositoryId, action.paths, action.keep);
  }
  if (action.type === 'repositoryLocalKeepFinished') {
    return finishRepositoryLocalKeep(state, action.repositoryId, action.reply);
  }
  if (action.type === 'locale') {
    return { ...state, locale: action.locale, settingsModified: true };
  }
  if (action.type === 'windowPreferencesUpdated') {
    return {
      ...state,
      windowPrefs: {
        ...action.preferences,
        ...(action.preferences.fullSize ? { fullSize: { ...action.preferences.fullSize } } : {}),
        ...(action.preferences.fullPosition ? { fullPosition: { ...action.preferences.fullPosition } } : {}),
        ...(action.preferences.compactPosition ? { compactPosition: { ...action.preferences.compactPosition } } : {}),
      },
      settingsModified: true,
    };
  }
  if (action.type === 'windowSnapshotSynced') {
    if (state.settingsModified || state.windowPrefs.compactMode || state.windowPrefs.fullPosition) {
      return state;
    }
    return {
      ...state,
      windowPrefs: {
        ...state.windowPrefs,
        pinned: action.pinned,
        fullPosition: { ...action.position },
        fullSize: { ...action.size },
      },
    };
  }
  if (action.type === 'windowGeometryObserved') {
    if (state.windowPrefs.compactMode) {
      const prev = state.windowPrefs.compactPosition;
      if (prev && prev.x === action.position.x && prev.y === action.position.y) return state;
      return {
        ...state,
        windowPrefs: {
          ...state.windowPrefs,
          compactPosition: { ...action.position },
        },
        settingsModified: true,
      };
    }
    const validFullSize = action.size.width >= WINDOW_DIMENSIONS.minFullWidth
      && action.size.height >= WINDOW_DIMENSIONS.minFullHeight;
    const nextSize = validFullSize ? { ...action.size } : state.windowPrefs.fullSize;
    const prevPos = state.windowPrefs.fullPosition;
    const prevSize = state.windowPrefs.fullSize;
    if (
      prevPos?.x === action.position.x
      && prevPos?.y === action.position.y
      && prevSize?.width === nextSize?.width
      && prevSize?.height === nextSize?.height
    ) {
      return state;
    }
    return {
      ...state,
      windowPrefs: {
        ...state.windowPrefs,
        fullPosition: { ...action.position },
        ...(nextSize ? { fullSize: nextSize } : {}),
      },
      settingsModified: true,
    };
  }
  if (action.type === 'editor') {
    if (action.repositoryId) {
      const prev = state.repositorySettings[action.repositoryId];
      const keepPaths = localKeepPathsFor(state, action.repositoryId);
      return {
        ...state,
        editor: action.editor,
        repositorySettings: {
          ...state.repositorySettings,
          [action.repositoryId]: {
            editor: action.editor,
            commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
            ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
            ...(prev?.customActions?.length ? { customActions: prev.customActions } : {}),
            ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
          },
        },
        settingsModified: true,
      };
    }
    return { ...state, editor: action.editor, settingsModified: true };
  }
  if (action.type === 'commitLanguage') {
    if (action.repositoryId) {
      const prev = state.repositorySettings[action.repositoryId];
      const keepPaths = localKeepPathsFor(state, action.repositoryId);
      return {
        ...state,
        repositorySettings: {
          ...state.repositorySettings,
          [action.repositoryId]: {
            editor: effectiveEditorFor(state, action.repositoryId),
            commitLanguage: action.commitLanguage,
            ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
            ...(prev?.customActions?.length ? { customActions: prev.customActions } : {}),
            ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
          },
        },
        settingsModified: true,
      };
    }
    return { ...state, defaultCommitLanguage: action.commitLanguage, settingsModified: true };
  }
  if (action.type === 'catalogSynced') {
    return {
      ...state,
      connections: action.connections,
      models: action.models,
      activeModelId: action.activeModelId,
    };
  }
  if (action.type === 'providerKeyHintSynced') {
    return {
      ...state,
      connections: state.connections.map((conn) =>
        conn.id === action.connectionId ? { ...conn, keyHint: action.keyHint } : conn,
      ),
    };
  }
  if (action.type === 'modelSelected') {
    return state.models.some((model) => model.id === action.modelId)
      ? { ...state, activeModelId: action.modelId }
      : state;
  }
  if (action.type === 'modelDeleted') {
    const models = state.models.filter((model) => model.id !== action.modelId);
    return { ...state, models, activeModelId: state.activeModelId === action.modelId ? models[0]?.id ?? null : state.activeModelId };
  }
  if (action.type === 'providerDeleted') {
    const connections = state.connections.filter((conn) => conn.id !== action.connectionId);
    const models = state.models.filter((model) => model.connectionId !== action.connectionId);
    const activeModelId = models.some((model) => model.id === state.activeModelId)
      ? state.activeModelId
      : models[0]?.id ?? null;
    return { ...state, connections, models, activeModelId };
  }
  if (action.type === 'switch') {
    if (!state.worktrees.some((worktree) => worktree.worktreeId === action.worktreeId)) return state;
    return {
      ...state,
      activeWorktreeId: action.worktreeId,
      navigationRevision: state.navigationRevision + 1,
      worktrees: state.worktrees.map((worktree) => worktree.worktreeId === action.worktreeId && action.expand
        ? { ...worktree, dockExpanded: true }
        : worktree),
    };
  }
  if ((action.type === 'prompt' || action.type === 'clearPrompt') && action.worktreeId === null) {
    return { ...state, emptyPrompt: action.type === 'prompt' ? action.text : '' };
  }

  return {
    ...state,
    worktrees: state.worktrees.map((worktree) => {
      if (action.type === 'event') {
        return receiveEvent(worktree, action.event, new Set(localKeepPathsFor(state, worktree.repositoryId)));
      }
      if (action.type === 'request' || action.type === 'requestFailed') {
        if (!matchesTarget(worktree, action.target)) return worktree;
        if (action.type === 'requestFailed') {
          if (worktree.requests[action.kind]?.id !== action.requestId) return worktree;
          if (action.kind === 'review') {
            const prevStatus = worktree.requests.review?.previousReviewStatus ?? 'idle';
            return {
              ...worktree,
              requests: { ...worktree.requests, review: undefined },
              review: {
                ...worktree.review,
                status: action.error && prevStatus !== 'done' ? 'failed' : prevStatus,
                error: action.error,
              },
            };
          }
          return { ...worktree, requests: { ...worktree.requests, plan: undefined } };
        }
        if (action.kind === 'plan') {
          return {
            ...worktree,
            requests: {
              ...worktree.requests,
              plan: {
                id: action.requestId,
                promptRevision: worktree.prompt.revision,
                planRevision: worktree.planRevision,
                fileIds: action.fileIds ?? worktree.selectedIds,
              },
            },
          };
        }
        return {
          ...worktree,
          requests: {
            ...worktree.requests,
            review: {
              id: action.requestId,
              promptRevision: worktree.prompt.revision,
              fileIds: action.fileIds ?? worktree.selectedIds,
              previousReviewStatus: worktree.requests.review?.previousReviewStatus ?? worktree.review.status,
            },
          },
          review: { ...worktree.review, status: 'running', error: undefined },
        };
      }
      if (worktree.worktreeId !== action.worktreeId) return worktree;
      return reduceWorktreeLocal(worktree, action);
    }),
  };
}
