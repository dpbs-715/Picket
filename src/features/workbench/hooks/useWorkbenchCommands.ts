import { useRef, useState, type MutableRefObject } from 'react';
import {
  BUILTIN_CPUSH_ACTION_ID,
  type CustomActionConfig,
  type SessionRecoveryAction,
} from '../../../../shared/custom-action-protocol';
import type { CommitLanguage } from '../../../../shared/language-protocol';
import type { MessageUserResponse } from '../../../../shared/message-user-protocol';
import { getMessages } from '../../../i18n';
import type { ModelConfiguration } from '../../settings/modelProviders';
import type { WorkbenchBackend, WorkbenchCommand } from '../commands';
import {
  computeSyncCustomActionAcrossRepos,
  isActiveSession,
  targetOf,
  type Change,
  type EditorId,
  type Message,
  type WorkbenchState,
  type Worktree,
} from '../state/workbench';
import type { NotifyFn, WorkbenchDispatch } from '../types';
import { applyCommandError, applyCommandReply, commandInFlightKey } from './commandHandlers';

interface UseWorkbenchCommandsParams {
  state: WorkbenchState;
  latestState: MutableRefObject<WorkbenchState>;
  worktree: Worktree | undefined;
  model: ModelConfiguration | undefined;
  activeEditor: EditorId;
  activeCommitLanguage: CommitLanguage;
  activeCustomAction: CustomActionConfig | undefined;
  selected: Change[];
  promptText: string;
  compactMode: boolean;
  viewKey: string;
  backend: WorkbenchBackend;
  dispatch: WorkbenchDispatch;
  notify: NotifyFn;
  isEditingCommitInput: () => boolean;
  openConfigureModel: () => void;
  openConfigureModelFromCompact: () => void;
  expandFromCompact: (target?: 'dock') => Promise<void>;
}

export function useWorkbenchCommands({
  state,
  latestState,
  worktree,
  model,
  activeEditor,
  activeCommitLanguage,
  activeCustomAction,
  selected,
  promptText,
  compactMode,
  viewKey,
  backend,
  dispatch,
  notify,
  isEditingCommitInput,
  openConfigureModel,
  openConfigureModelFromCompact,
  expandFromCompact,
}: UseWorkbenchCommandsParams) {
  const [pending, setPending] = useState<string[]>([]);
  const inFlight = useRef(new Set<string>());
  const latestLocalKeepRequestByRepo = useRef<Record<string, string>>({});
  const t = getMessages(state.locale);

  async function execute(command: WorkbenchCommand, preserveFeedback = false): Promise<boolean> {
    const targetKey = 'target' in command ? command.target.worktreeId : viewKey;
    const key = commandInFlightKey(command, targetKey);
    if (inFlight.current.has(key)) return false;
    inFlight.current.add(key);
    setPending([...inFlight.current]);

    if (command.kind === 'openRepository' || command.kind === 'refresh') {
      const readKey = command.kind === 'openRepository' ? 'open' : command.target.repositoryId;
      dispatch({ type: 'repositoryReadStarted', key: readKey, requestId: command.requestId });
    }
    if (command.kind === 'switchBranch' || command.kind === 'checkoutRemoteBranch') {
      dispatch({ type: 'repositoryBranchStarted', repositoryId: command.target.repositoryId, requestId: command.requestId });
    }

    try {
      const reply = await backend.execute(command);
      return applyCommandReply(command, reply, targetKey, preserveFeedback, {
        latestState,
        backend,
        dispatch,
        notify,
        isEditingCommitInput,
        latestLocalKeepRequestByRepo,
      });
    } catch (error) {
      applyCommandError(command, error, targetKey, {
        latestState,
        backend,
        dispatch,
        notify,
        triggerRefresh: (target) => {
          void execute({ kind: 'refresh', target, requestId: crypto.randomUUID(), locale: latestState.current.locale }, true);
        },
      });
      return false;
    } finally {
      inFlight.current.delete(key);
      setPending([...inFlight.current]);
    }
  }

  function switchWorktree(worktreeId: string, expand?: boolean) {
    dispatch({ type: 'switch', worktreeId, expand });
    const next = state.worktrees.find((tree) => tree.worktreeId === worktreeId);
    if (backend.repositoryReadsAvailable && next) {
      void execute({ kind: 'refresh', target: targetOf(next), requestId: crypto.randomUUID(), locale: state.locale });
    }
  }

  function setLocalKeep(paths: string[], keep: boolean) {
    if (!worktree || !paths.length) return;
    const target = targetOf(worktree);
    dispatch({ type: 'localKeepUpdated', repositoryId: worktree.repositoryId, paths, keep });
    if (backend.repositoryReadsAvailable) {
      const requestId = crypto.randomUUID();
      latestLocalKeepRequestByRepo.current[worktree.repositoryId] = requestId;
      void execute(
        {
          kind: 'setLocalKeep',
          target,
          requestId,
          paths,
          keep,
          locale: state.locale,
        },
        true,
      );
    }
  }

  function openFile(fileId: string, line?: number) {
    if (worktree) {
      void execute({
        kind: 'openFile',
        target: targetOf(worktree),
        requestId: crypto.randomUUID(),
        fileId,
        editor: activeEditor,
        line,
        locale: state.locale,
      });
    }
  }

  function openRepoPath(path: string, line?: number) {
    if (!worktree || !path.trim()) return;
    const matchedFile = worktree.files.find((file) => file.path === path);
    void execute({
      kind: 'openFile',
      target: targetOf(worktree),
      requestId: crypto.randomUUID(),
      ...(matchedFile && line === undefined ? { fileId: matchedFile.id } : { path }),
      editor: activeEditor,
      line,
      locale: state.locale,
    });
  }

function resolveSessionRecoveryAction(
  message: Message,
  answer: { optionId?: string; text?: string },
): SessionRecoveryAction {
  const optionId = answer.optionId;
  if (optionId === 'retry-push' || optionId === 'continue-push') {
    return { kind: 'retryPush' };
  }
  if (optionId?.startsWith('push-set-upstream:')) {
    return { kind: 'retryPush', setUpstreamRemote: optionId.slice('push-set-upstream:'.length) };
  }
  if (optionId === 'retry-remaining') {
    return { kind: 'retryRemainingCommits' };
  }
  if (optionId === 'abort-rebase') {
    return { kind: 'abortRebase' };
  }
  if (optionId === 'manual-merge') {
    return { kind: 'manualMerge' };
  }
  if (optionId === 'agent-merge') {
    return { kind: 'agentConflictMerge' };
  }
  if (optionId === 'dismiss-session') {
    return { kind: 'dismissSession' };
  }
  const trimmedText = answer.text?.trim();
  if (message.kind === 'conflict') {
    return { kind: 'agentConflictMerge', ...(trimmedText ? { instruction: trimmedText } : {}) };
  }
  return { kind: 'setUpstreamFreeText', text: trimmedText ?? optionId ?? '' };
}

  function launchCustomAction(
    action: CustomActionConfig,
    options?: { followUpPrompt?: string; resumeFromRunId?: string; forceExpandDock?: boolean },
  ) {
    if (!worktree || !model || worktree.readError) return;
    if (inFlight.current.has(`${worktree.repositoryId}:action`)) return;
    const requestId = crypto.randomUUID();
    const target = targetOf(worktree);
    const fileIds = selected.map((file) => file.id);
    const selections = selected.length > 0 && selected.every((file) => !!file.commitToken)
      ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    const expandDock = options?.forceExpandDock ?? (!isEditingCommitInput() && !compactMode);
    dispatch({
      type: 'actionStarted',
      worktreeId: worktree.worktreeId,
      profile: 'customAction',
      runId: requestId,
      actionId: action.id,
      actionLabel: action.label,
      frozenTarget: target,
      frozenFileIds: fileIds,
      modelId: model.id,
      expandDock,
    });
    void execute({
      kind: 'startAction',
      profile: 'customAction',
      target,
      requestId,
      action,
      fileIds,
      selections,
      modelId: model.id,
      extraPrompt: promptText,
      ...(options?.followUpPrompt ? { followUpPrompt: options.followUpPrompt } : {}),
      ...(options?.resumeFromRunId ? { resumeFromRunId: options.resumeFromRunId } : {}),
      locale: state.locale,
      commitLanguage: activeCommitLanguage,
      approvalMode: state.actionApprovalMode,
    });
  }

  function followUpCustomAction(message: Message, text: string): boolean {
    const trimmed = text.trim();
    const customRun = worktree?.activeRun?.profile === 'customAction' ? worktree.activeRun : undefined;
    if (!worktree || !trimmed || !customRun) return false;
    if (!model) {
      if (compactMode) {
        openConfigureModelFromCompact();
      } else {
        openConfigureModel();
      }
      return false;
    }
    if (worktree.readError) return false;
    const savedAction = state.repositorySettings[worktree.repositoryId]?.customActions?.find(
      (item) => item.id === customRun.actionId,
    );
    const targetAction: CustomActionConfig = savedAction ?? {
      id: customRun.actionId,
      label: customRun.actionLabel,
      prompt: trimmed,
    };
    dispatch({ type: 'reply', worktreeId: worktree.worktreeId, messageId: message.id, text: '' });
    launchCustomAction(targetAction, {
      followUpPrompt: trimmed,
      resumeFromRunId: customRun.runId,
      forceExpandDock: !compactMode,
    });
    return true;
  }

  function primary() {
    if (!model && (!worktree || !isActiveSession(worktree))) {
      if (compactMode) {
        openConfigureModelFromCompact();
      } else {
        openConfigureModel();
      }
      return;
    }
    if (!worktree) return;
    if (isActiveSession(worktree)) {
      if (compactMode) {
        void expandFromCompact('dock');
      } else {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true });
      }
      return;
    }
    if (activeCustomAction) {
      launchCustomAction(activeCustomAction);
      return;
    }
    if (inFlight.current.has(`${worktree.repositoryId}:action`)) return;
    if (worktree.detached) {
      notify(t.workbench.detachedPrimaryBlocked, viewKey, { tone: 'warning' });
      return;
    }
    if (model && selected.length && !worktree.readError) {
      const requestId = crypto.randomUUID();
      const target = targetOf(worktree);
      const fileIds = selected.map((file) => file.id);
      const selections = selected.every((file) => !!file.commitToken)
        ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
        : undefined;
      const expandDock = !isEditingCommitInput() && !compactMode;
      dispatch({
        type: 'actionStarted',
        worktreeId: worktree.worktreeId,
        profile: 'cpush',
        runId: requestId,
        actionId: BUILTIN_CPUSH_ACTION_ID,
        actionLabel: 'C-Push',
        frozenTarget: target,
        frozenFileIds: fileIds,
        modelId: model.id,
        expandDock,
      });
      void execute({
        kind: 'startAction',
        profile: 'cpush',
        target,
        requestId,
        fileIds,
        selections,
        modelId: model.id,
        extraPrompt: promptText,
        locale: state.locale,
        commitLanguage: activeCommitLanguage,
        approvalMode: state.actionApprovalMode,
      });
    }
  }

  async function respond(
    message: Message,
    answer: { optionId?: string; text?: string; questionnaireResponse?: MessageUserResponse },
  ): Promise<boolean> {
    if (!worktree) return false;
    const id = worktree.worktreeId;
    const livePendingRunId = worktree.activeRun?.pendingCallId ? worktree.activeRun.runId : undefined;
    const livePendingCallId = worktree.activeRun?.pendingCallId;
    if (livePendingRunId && livePendingCallId && answer.questionnaireResponse && backend.respondActionQuestion) {
      const respKey = `${id}:respondActionQuestion`;
      if (inFlight.current.has(respKey)) return false;
      inFlight.current.add(respKey);
      setPending([...inFlight.current]);
      try {
        const accepted = await backend.respondActionQuestion({
          requestId: crypto.randomUUID(),
          runId: livePendingRunId,
          callId: livePendingCallId,
          response: answer.questionnaireResponse,
          locale: state.locale,
        });
        return Boolean(accepted);
      } catch (error) {
        const errText = error instanceof Error ? error.message : t.workbench.genericOperationFailed;
        notify(errText, id);
        return false;
      } finally {
        inFlight.current.delete(respKey);
        setPending([...inFlight.current]);
      }
    }
    const activeRun = worktree.activeRun;
    const target = activeRun?.frozenTarget ?? targetOf(worktree);
    const runId = activeRun?.runId;
    const recoveryAction = resolveSessionRecoveryAction(message, answer);
    const accepted = await execute({
      kind: 'runRecovery',
      target,
      requestId: crypto.randomUUID(),
      ...(runId ? { runId } : {}),
      messageId: message.id,
      action: recoveryAction,
      ...(model ? { modelId: model.id } : {}),
      locale: state.locale,
    });
    if (accepted) {
      dispatch({ type: 'reply', worktreeId: id, messageId: message.id, text: '' });
    }
    return accepted;
  }

  async function cancelActiveCustomAction(): Promise<boolean> {
    const runId = worktree?.activeRun?.runId;
    if (!runId || !backend.cancelAction) return false;
    try {
      await backend.cancelAction(runId);
      return true;
    } catch (error) {
      const errText = error instanceof Error ? error.message : t.workbench.genericOperationFailed;
      notify(errText, viewKey, { tone: 'error' });
      return false;
    }
  }

  function syncActionToAllRepos(action: CustomActionConfig, overwrite: boolean) {
    const repoId = worktree?.repositoryId;
    if (!repoId) return { added: 0, updated: 0, skipped: 0 };
    const { result } = computeSyncCustomActionAcrossRepos(latestState.current, repoId, action, overwrite);
    if (result.added > 0 || result.updated > 0) {
      dispatch({
        type: 'syncCustomActionToAllRepos',
        sourceRepositoryId: repoId,
        action,
        overwrite,
      });
    }
    return result;
  }

  async function selectModel(id: string) {
    if (await execute({ kind: 'selectModel', modelId: id, locale: state.locale })) {
      dispatch({ type: 'modelSelected', modelId: id });
    }
  }

  async function deleteModel(id: string) {
    if (await execute({ kind: 'deleteModel', modelId: id, locale: state.locale })) {
      dispatch({ type: 'modelDeleted', modelId: id });
    }
  }

  async function deleteProviderConnection(connectionId: string) {
    if (await execute({ kind: 'deleteProviderConnection', connectionId, locale: state.locale })) {
      dispatch({ type: 'providerDeleted', connectionId });
    }
  }

  return {
    pending,
    inFlight,
    execute,
    switchWorktree,
    setLocalKeep,
    openFile,
    openRepoPath,
    primary,
    followUpCustomAction,
    respond,
    cancelActiveCustomAction,
    syncActionToAllRepos,
    selectModel,
    deleteModel,
    deleteProviderConnection,
  };
}
