import type { MutableRefObject } from 'react';
import { getMessages } from '../../../i18n';
import type { WorkbenchBackend, WorkbenchCommand, WorkbenchCommandReply } from '../commands';
import {
  editors,
  isModelErrorText,
  splitPath,
  type WorkbenchState,
} from '../state/workbench';
import type { NotifyFn, WorkbenchDispatch } from '../types';

export interface CommandReplyContext {
  latestState: MutableRefObject<WorkbenchState>;
  backend: WorkbenchBackend;
  dispatch: WorkbenchDispatch;
  notify: NotifyFn;
  isEditingCommitInput: () => boolean;
  latestLocalKeepRequestByRepo: MutableRefObject<Record<string, string>>;
}

export interface CommandErrorContext {
  latestState: MutableRefObject<WorkbenchState>;
  backend: WorkbenchBackend;
  dispatch: WorkbenchDispatch;
  notify: NotifyFn;
  triggerRefresh: (target: Extract<WorkbenchCommand, { target: unknown }>['target']) => void;
}

export function commandInFlightKey(command: WorkbenchCommand, targetKey: string): string {
  if (command.kind === 'switchBranch' || command.kind === 'checkoutRemoteBranch') {
    return `${command.target.repositoryId}:write`;
  }
  if (command.kind === 'pull') {
    return `${command.target.repositoryId}:pull`;
  }
  if (command.kind === 'runCustomAction') {
    return `${command.target.repositoryId}:customAction`;
  }
  if (command.kind === 'setLocalKeep') {
    return `${command.target.repositoryId}:localKeep:${command.requestId}`;
  }
  if (command.kind === 'openRepository') {
    return 'global:openRepository';
  }
  if (command.kind === 'refresh') {
    return `${command.target.repositoryId}:refresh`;
  }
  return `${targetKey}:${command.kind}`;
}

export function applyCommandReply(
  command: WorkbenchCommand,
  reply: WorkbenchCommandReply,
  targetKey: string,
  preserveFeedback: boolean,
  ctx: CommandReplyContext,
): boolean {
  const {
    latestState,
    backend,
    dispatch,
    notify,
    isEditingCommitInput,
    latestLocalKeepRequestByRepo,
  } = ctx;
  const commandLocale = command.locale ?? latestState.current.locale;
  const wtMessages = getMessages(commandLocale).workbench;

  if (command.kind === 'selectModel' || command.kind === 'deleteModel' || command.kind === 'deleteProviderConnection') {
    if (reply && typeof reply === 'object' && 'models' in reply && 'connections' in reply) {
      dispatch({
        type: 'catalogSynced',
        connections: reply.connections,
        models: reply.models,
        activeModelId: reply.activeModelId,
      });
    }
    return true;
  }

  if (command.kind === 'runCustomAction') {
    if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'customAction') {
      const expandDock = !isEditingCommitInput()
        && !latestState.current.windowPrefs.compactMode
        && reply.status === 'done'
        && Boolean(reply.message);
      dispatch({ type: 'customActionSettled', reply, expandDock });
      if (reply.status === 'cancelled') {
        notify(getMessages(latestState.current.locale).actions.actionCancelledNotice, targetKey, { tone: 'info' });
      } else {
        notify('', targetKey);
      }
    }
    return true;
  }

  if (command.kind === 'cPush' || command.kind === 'respond' || command.kind === 'pull') {
    if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'cPush') {
      const expandDock = command.kind === 'pull'
        && !isEditingCommitInput()
        && !latestState.current.windowPrefs.compactMode
        && reply.session.sessionStatus !== 'idle'
        && !reply.session.autoCollapse;
      dispatch({ type: 'cPushSettled', reply, expandDock });
      if (command.kind === 'pull') {
        if (reply.session.sessionStatus === 'idle') {
          const syncedRemote = reply.repositorySnapshot?.worktrees.find((item) => item.worktreeId === command.target.worktreeId)?.remote;
          const currentMessages = getMessages(latestState.current.locale).workbench;
          notify(
            syncedRemote?.kind === 'tracked'
              ? currentMessages.pullRebaseTrackedDone(syncedRemote.branch)
              : currentMessages.pullRebaseDone,
            targetKey,
            { tone: 'success' },
          );
        } else {
          notify('', targetKey);
        }
      }
    }
    return true;
  }

  if (command.kind === 'openFile') {
    if (!reply || !('kind' in reply) || reply.kind !== 'openFile' || reply.requestId !== command.requestId) {
      throw new Error(wtMessages.openFileMismatchError);
    }
    const shortName = editors.find((item) => item.id === reply.editor)?.short ?? reply.editor;
    const fileName = splitPath(reply.filePath).name;
    notify(
      getMessages(latestState.current.locale).workbench.openedInEditor(shortName, reply.action, fileName, reply.line),
      targetKey,
      { tone: 'success' },
    );
    return true;
  }

  if (command.kind === 'setLocalKeep') {
    if (!reply || !('kind' in reply) || reply.kind !== 'localKeep' || reply.requestId !== command.requestId) {
      throw new Error(getMessages(commandLocale).desktop.localKeepReplyMismatch);
    }
    if (latestLocalKeepRequestByRepo.current[command.target.repositoryId] === command.requestId) {
      dispatch({ type: 'repositoryLocalKeepFinished', repositoryId: command.target.repositoryId, reply });
    }
    return true;
  }

  if (command.kind === 'switchBranch' || command.kind === 'checkoutRemoteBranch') {
    if (!reply || !('kind' in reply) || reply.kind !== 'branch' || reply.requestId !== command.requestId) {
      throw new Error(wtMessages.branchMismatchError);
    }
    dispatch({ type: 'repositoryBranchFinished', repositoryId: command.target.repositoryId, reply });
    const landedBranch = reply.snapshot.worktrees.find((tree) => tree.worktreeId === reply.snapshot.activeWorktreeId)?.branch ?? command.branch;
    const currentMessages = getMessages(latestState.current.locale).workbench;
    notify(
      command.kind === 'checkoutRemoteBranch' && landedBranch !== command.branch
        ? currentMessages.remoteBranchCheckedOut(landedBranch, command.branch)
        : currentMessages.branchSwitched(landedBranch),
      reply.snapshot.activeWorktreeId,
      { tone: 'success' },
    );
    return true;
  }

  if (command.kind === 'openRepository' || command.kind === 'refresh') {
    const readKey = command.kind === 'openRepository' ? 'open' : command.target.repositoryId;
    if (!reply || ('kind' in reply && reply.kind !== undefined) || !('requestId' in reply) || reply.requestId !== command.requestId) {
      throw new Error(wtMessages.readMismatchError);
    }
    const previousTree = latestState.current.worktrees.find((item) => item.worktreeId === targetKey);
    const previousReadError = previousTree?.readError;
    dispatch({ type: 'repositoryReadFinished', key: readKey, reply });
    if (command.kind === 'openRepository') {
      if (reply.snapshot && !preserveFeedback) notify('', targetKey);
    } else {
      let recoveredFromReadError = false;
      if (previousReadError) {
        const refreshedTree = reply.snapshot?.worktrees.find((item) => item.worktreeId === targetKey);
        recoveredFromReadError = !!refreshedTree && !refreshedTree.readError;
      }
      if (!preserveFeedback || recoveredFromReadError) notify('', targetKey);
      const sessionStatus = previousTree?.sessionStatus;
      if (sessionStatus && ['waitingManual', 'conflict', 'pushFailed'].includes(sessionStatus)) {
        void backend.reconcileSession?.(command.target, latestState.current.locale).catch(() => {});
      }
    }
  }

  return true;
}

export function applyCommandError(
  command: WorkbenchCommand,
  error: unknown,
  targetKey: string,
  ctx: CommandErrorContext,
) {
  const { latestState, backend, dispatch, notify, triggerRefresh } = ctx;
  const message = error instanceof Error
    ? error.message
    : typeof error === 'string'
      ? error
      : getMessages(latestState.current.locale).workbench.genericOperationFailed;

  if (command.kind === 'cPush') {
    dispatch({ type: 'cPushStartFailed', worktreeId: command.target.worktreeId, runId: command.requestId });
  }
  if (command.kind === 'runCustomAction') {
    dispatch({ type: 'customActionFailed', worktreeId: command.target.worktreeId, runId: command.requestId, message });
  }
  if (command.kind === 'pull' && backend.repositoryReadsAvailable) {
    triggerRefresh(command.target);
  }
  if (command.kind === 'switchBranch' || command.kind === 'checkoutRemoteBranch') {
    dispatch({ type: 'repositoryBranchFailed', repositoryId: command.target.repositoryId, requestId: command.requestId });
    if (backend.repositoryReadsAvailable) {
      triggerRefresh(command.target);
    }
  }
  if (command.kind === 'setLocalKeep' && backend.repositoryReadsAvailable) {
    triggerRefresh(command.target);
  }
  if (command.kind === 'openRepository' || command.kind === 'refresh') {
    const readKey = command.kind === 'openRepository' ? 'open' : command.target.repositoryId;
    dispatch({
      type: 'repositoryReadFailed',
      key: readKey,
      requestId: command.requestId,
      ...(command.kind === 'refresh' ? { worktreeId: command.target.worktreeId, error: message } : {}),
    });
  }
  const modelRelatedCommand = [
    'cPush',
    'runCustomAction',
    'respond',
    'selectModel',
    'deleteModel',
    'deleteProviderConnection',
  ].includes(command.kind);
  notify(message, targetKey, modelRelatedCommand && isModelErrorText(message));
}
