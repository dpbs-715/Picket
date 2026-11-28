import {
  computeCustomActionPromptHash,
  type ActionCommandReply,
  type ActionProgressEvent,
  type ActionSessionSnapshot,
  type ActionSessionStatus,
  type TrustedActionUpdate,
} from '../../../../shared/custom-action-protocol';
import type { GitRepositorySnapshot } from '../../../../shared/git-read-protocol';
import { mergeSnapshot } from './snapshotMerge';
import type { ActiveActionRun, Message, WorkbenchState, Worktree } from './types';

export function toWorktreeSessionStatus(status: ActionSessionStatus): Worktree['sessionStatus'] {
  switch (status) {
    case 'running':
    case 'needsDecision':
    case 'conflict':
    case 'waitingManual':
    case 'pushFailed':
    case 'done':
      return status;
    case 'idle':
    case 'interrupted':
    case 'cancelled':
    case 'failed':
      return 'idle';
  }
}

export function toActiveActionRun(
  session: ActionSessionSnapshot,
  fallbackSessionId: string,
  fallbackScopeVersion: string,
  promptRevisionAtStart: number,
): ActiveActionRun {
  return {
    profile: session.profile,
    runId: session.runId,
    actionId: session.actionId,
    actionLabel: session.actionLabel,
    frozenTarget: {
      repositoryId: session.repositoryId,
      worktreeId: session.worktreeId,
      sessionId: session.sessionId || fallbackSessionId,
      scopeVersion: session.frozenScopeVersion || fallbackScopeVersion,
    },
    frozenFileIds: session.frozenFileIds,
    promptRevisionAtStart,
    modelId: session.modelId,
    ...(session.pendingCallId ? { pendingCallId: session.pendingCallId } : {}),
    timeline: session.timeline,
    qaLog: session.qaLog,
    commits: session.commits,
    links: session.links,
    canRetryRemaining: session.canRetryRemaining,
  };
}

function applyTrustedUpdate(state: WorkbenchState, update?: TrustedActionUpdate): WorkbenchState {
  if (!update) return state;
  const prev = state.repositorySettings[update.repositoryId];
  const existing = prev?.customActions;
  if (!existing?.length) return state;
  let changed = false;
  const nextActions = existing.map((item) => {
    if (item.id !== update.actionId) return item;
    if (computeCustomActionPromptHash(item.prompt) !== update.trustedPromptHash) return item;
    changed = true;
    return {
      ...item,
      skipWriteConfirm: true,
      trustedPromptHash: update.trustedPromptHash,
    };
  });
  if (!changed) return state;
  return {
    ...state,
    repositorySettings: {
      ...state.repositorySettings,
      [update.repositoryId]: {
        ...prev,
        customActions: nextActions,
      },
    },
    settingsModified: true,
  };
}

export interface ApplyActionSnapshotOptions {
  repositorySnapshot?: GitRepositorySnapshot;
  trustedActionUpdate?: TrustedActionUpdate;
  expandDockOnSettle?: boolean;
  suppressExpand?: boolean;
  isSettledReply?: boolean;
  expectedRunId?: string;
}

export function applyActionSnapshot(
  state: WorkbenchState,
  session: ActionSessionSnapshot,
  options: ApplyActionSnapshotOptions = {},
): WorkbenchState {
  const {
    repositorySnapshot,
    trustedActionUpdate,
    expandDockOnSettle,
    suppressExpand = false,
    isSettledReply = false,
    expectedRunId,
  } = options;
  const previousTree = state.worktrees.find(
    (tree) => tree.repositoryId === session.repositoryId && tree.worktreeId === session.worktreeId,
  );
  if (!previousTree) return state;
  const previousRun = previousTree.activeRun;
  if (expectedRunId && previousRun?.runId !== expectedRunId) {
    return state;
  }
  if (
    previousRun
    && previousRun.runId !== session.runId
    && previousTree.sessionStatus !== 'idle'
    && previousTree.sessionStatus !== 'done'
  ) {
    return state;
  }

  const promptRevAtStart = previousRun?.promptRevisionAtStart ?? previousTree.prompt.revision;
  const isCPushCompleted = session.profile === 'cpush'
    && session.status === 'done'
    && !session.message?.id.startsWith('done-local-')
    && !session.message?.id.startsWith('done-merge-pull-')
    && !session.message?.id.startsWith('done-manual-rebase-')
    && (session.commits.length > 0 || session.skippedFileIds.length > 0);

  let nextState = repositorySnapshot
    ? mergeSnapshot(
        state,
        repositorySnapshot,
        false,
        session.committedFileIds.length > 0 || isCPushCompleted
          ? {
              worktreeId: session.worktreeId,
              promptRevision: promptRevAtStart,
              clearPrompt: isCPushCompleted,
              committedFileIds: session.committedFileIds,
            }
          : undefined,
      )
    : state;

  nextState = applyTrustedUpdate(nextState, trustedActionUpdate);
  const nextSessionStatus = toWorktreeSessionStatus(session.status);

  return {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== session.repositoryId || tree.worktreeId !== session.worktreeId) {
        return tree;
      }
      const nextMessages: Message[] = session.message ? [session.message] : [];
      const nextPrompt = isCPushCompleted && !repositorySnapshot && tree.prompt.revision === promptRevAtStart
        ? { text: '', revision: tree.prompt.revision + 1 }
        : tree.prompt;
      const shouldAutoExpandLiveQuestion = session.status === 'needsDecision'
        && Boolean(session.pendingCallId)
        && !suppressExpand
        && (tree.worktreeId === nextState.activeWorktreeId || session.profile === 'customAction');
      const shouldExpandSettledFailure = isSettledReply
        && session.status === 'failed'
        && Boolean(session.message)
        && !suppressExpand;
      const nextDockExpanded = session.autoCollapse || session.status === 'idle' || session.status === 'cancelled'
        ? false
        : shouldAutoExpandLiveQuestion || shouldExpandSettledFailure
          ? true
          : expandDockOnSettle
            ? true
            : tree.dockExpanded;

      const keepActiveRun = session.status !== 'idle'
        && session.status !== 'cancelled'
        && !(session.status === 'failed' && !session.message);
      const existingPromptRev = tree.activeRun?.promptRevisionAtStart ?? promptRevAtStart;
      return {
        ...tree,
        sessionStatus: nextSessionStatus,
        dockExpanded: nextDockExpanded,
        messages: nextMessages,
        prompt: nextPrompt,
        activeRun: keepActiveRun
          ? toActiveActionRun(session, tree.sessionId, tree.scopeVersion, existingPromptRev)
          : undefined,
      };
    }),
  };
}

export function finishActionCommand(
  state: WorkbenchState,
  reply: ActionCommandReply,
  expandDockOnSettle?: boolean,
  suppressExpand = false,
): WorkbenchState {
  return applyActionSnapshot(state, reply.session, {
    repositorySnapshot: reply.repositorySnapshot,
    trustedActionUpdate: reply.trustedActionUpdate,
    expandDockOnSettle,
    suppressExpand,
    isSettledReply: true,
  });
}

export function receiveActionProgress(
  state: WorkbenchState,
  progress: ActionProgressEvent,
  suppressExpand = false,
  expectedRunId?: string,
): WorkbenchState {
  return applyActionSnapshot(state, progress.session, {
    repositorySnapshot: progress.repositorySnapshot,
    trustedActionUpdate: progress.trustedActionUpdate,
    suppressExpand,
    expectedRunId,
  });
}
