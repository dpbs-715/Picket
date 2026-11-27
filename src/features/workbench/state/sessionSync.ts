import type { CPushCommandReply, CPushProgressEvent, CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import {
  computeCustomActionPromptHash,
  type CustomActionCommandReply,
  type CustomActionProgressEvent,
  type TrustedActionUpdate,
} from '../../../../shared/custom-action-protocol';
import type { GitRepositorySnapshot } from '../../../../shared/git-read-protocol';
import { mergeSnapshot } from './snapshotMerge';
import type { Message, WorkbenchState, Worktree } from './types';

export function applyCPushSnapshot(
  state: WorkbenchState,
  session: CPushSessionSnapshot,
  repositorySnapshot?: GitRepositorySnapshot,
  expandDockOnSettle?: boolean,
  suppressExpand = false,
): WorkbenchState {
  const previousTree = state.worktrees.find(
    (tree) => tree.repositoryId === session.repositoryId && tree.worktreeId === session.worktreeId,
  );
  if (!previousTree) return state;
  if (
    previousTree.cPush
    && previousTree.cPush.runId !== session.runId
    && previousTree.sessionStatus !== 'idle'
    && previousTree.sessionStatus !== 'done'
  ) {
    return state;
  }

  const promptRevAtStart = previousTree.cPush?.promptRevisionAtStart ?? previousTree.prompt.revision;
  const isCPushCompleted = session.sessionStatus === 'done'
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

  nextState = {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== session.repositoryId || tree.worktreeId !== session.worktreeId) {
        return tree;
      }
      const nextMessages: Message[] = session.message ? [session.message] : [];
      const nextPrompt = isCPushCompleted && !repositorySnapshot && tree.prompt.revision === promptRevAtStart
        ? { text: '', revision: tree.prompt.revision + 1 }
        : tree.prompt;
      const shouldAutoExpandLiveQuestion = session.sessionStatus === 'needsDecision'
        && Boolean(session.pendingCallId)
        && tree.worktreeId === nextState.activeWorktreeId
        && !suppressExpand;
      const nextDockExpanded = session.autoCollapse || session.sessionStatus === 'idle'
        ? false
        : shouldAutoExpandLiveQuestion
          ? true
          : expandDockOnSettle
            ? true
            : tree.dockExpanded;
      return {
        ...tree,
        sessionStatus: session.sessionStatus,
        dockExpanded: nextDockExpanded,
        messages: nextMessages,
        prompt: nextPrompt,
        customAction: undefined,
        cPush: session.sessionStatus === 'idle'
          ? undefined
          : {
              runId: session.runId,
              frozenTarget: {
                repositoryId: session.repositoryId,
                worktreeId: session.worktreeId,
                sessionId: session.sessionId,
                scopeVersion: session.frozenScopeVersion,
              },
              frozenFileIds: session.frozenFileIds,
              promptRevisionAtStart: tree.cPush?.promptRevisionAtStart ?? promptRevAtStart,
              modelId: session.modelId,
              ...(session.pendingCallId ? { pendingCallId: session.pendingCallId } : {}),
              timeline: session.timeline,
              qaLog: session.qaLog,
              commits: session.commits,
              canRetryRemaining: session.canRetryRemaining,
            },
      };
    }),
  };
  return nextState;
}

export function finishCPushCommand(
  state: WorkbenchState,
  reply: CPushCommandReply,
  expandDockOnSettle?: boolean,
): WorkbenchState {
  return applyCPushSnapshot(state, reply.session, reply.repositorySnapshot, expandDockOnSettle);
}

export function receiveCPushProgress(
  state: WorkbenchState,
  progress: CPushProgressEvent,
  suppressExpand = false,
): WorkbenchState {
  return applyCPushSnapshot(state, progress.session, progress.repositorySnapshot, undefined, suppressExpand);
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

export function applyCustomActionProgress(
  state: WorkbenchState,
  progress: CustomActionProgressEvent,
  suppressExpand = false,
): WorkbenchState {
  let nextState = progress.repositorySnapshot
    ? mergeSnapshot(state, progress.repositorySnapshot, false)
    : state;
  nextState = applyTrustedUpdate(nextState, progress.trustedActionUpdate);
  const nextStatus: Worktree['sessionStatus'] =
    progress.status === 'running'
      ? 'running'
      : progress.status === 'needsDecision'
        ? 'needsDecision'
        : progress.status === 'done'
          ? 'done'
          : 'idle';
  return {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== progress.repositoryId || tree.worktreeId !== progress.worktreeId) {
        return tree;
      }
      const nextMessages: Message[] = progress.message ? [progress.message] : [];
      const shouldExpand = progress.status === 'needsDecision'
        ? (suppressExpand ? tree.dockExpanded : true)
        : progress.status === 'cancelled'
          ? false
          : tree.dockExpanded;
      return {
        ...tree,
        sessionStatus: nextStatus,
        dockExpanded: shouldExpand,
        messages: nextMessages,
        cPush: undefined,
        customAction: progress.status === 'cancelled' || progress.status === 'failed'
          ? undefined
          : {
              runId: progress.runId,
              actionId: progress.actionId,
              actionLabel: progress.actionLabel,
              ...(progress.pendingCallId ? { pendingCallId: progress.pendingCallId } : {}),
              timeline: progress.timeline,
              commits: progress.commits,
              links: progress.links,
            },
      };
    }),
  };
}

export function finishCustomActionCommand(
  state: WorkbenchState,
  reply: CustomActionCommandReply,
  expandDockOnSettle?: boolean,
): WorkbenchState {
  let nextState = reply.repositorySnapshot
    ? mergeSnapshot(state, reply.repositorySnapshot, false)
    : state;
  nextState = applyTrustedUpdate(nextState, reply.trustedActionUpdate);
  const nextStatus: Worktree['sessionStatus'] = reply.status === 'done' ? 'done' : 'idle';
  return {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== reply.repositoryId || tree.worktreeId !== reply.worktreeId) {
        return tree;
      }
      const nextMessages: Message[] = reply.message ? [reply.message] : [];
      const nextDockExpanded = reply.status === 'cancelled'
        ? false
        : reply.status === 'done'
          ? Boolean(expandDockOnSettle)
          : Boolean(reply.message);
      return {
        ...tree,
        sessionStatus: nextStatus,
        dockExpanded: nextDockExpanded,
        messages: nextMessages,
        cPush: undefined,
        customAction: reply.status === 'done' || (reply.status === 'failed' && Boolean(reply.message))
          ? {
              runId: reply.runId,
              actionId: reply.actionId,
              actionLabel: reply.actionLabel,
              timeline: reply.timeline,
              commits: reply.commits,
              links: reply.links,
            }
          : undefined,
      };
    }),
  };
}
