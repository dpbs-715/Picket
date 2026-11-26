import { useRef, useState, type MutableRefObject } from 'react';
import type { AskUserQuestionResponse } from '../../../../shared/ask-user-protocol';
import type { CustomActionConfig } from '../../../../shared/custom-action-protocol';
import type { CommitLanguage } from '../../../../shared/language-protocol';
import { getMessages } from '../../../i18n';
import type { ModelConfiguration } from '../../settings/modelProviders';
import type { WorkbenchBackend, WorkbenchCommand } from '../commands';
import {
  activeValidPlan,
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
  canPlan: boolean;
  canUseModel: boolean;
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
  canPlan,
  canUseModel,
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
  const cancelledRequests = useRef(new Set<string>());
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
        cancelledRequests,
        latestLocalKeepRequestByRepo,
      });
    } catch (error) {
      if ('requestId' in command && cancelledRequests.current.delete(command.requestId)) {
        return false;
      }
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

  async function runPlan() {
    if (!worktree || !model || !canPlan || worktree.requests.plan) return;
    if (worktree.detached) {
      notify(t.workbench.detachedPrimaryBlocked, viewKey, { tone: 'warning' });
      return;
    }
    const target = targetOf(worktree);
    const requestId = crypto.randomUUID();
    const fileIds = selected.map((file) => file.id);
    const selections = selected.every((file) => !!file.commitToken)
      ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    dispatch({ type: 'request', target, kind: 'plan', requestId, fileIds });
    await execute({
      kind: 'planCPush',
      target,
      requestId,
      fileIds,
      selections,
      modelId: model.id,
      prompt: promptText,
      locale: state.locale,
      commitLanguage: activeCommitLanguage,
    });
  }

  async function runReview(changedOnly = false) {
    if (!worktree || !model || !canUseModel || worktree.requests.review) return;
    const target = targetOf(worktree);
    const requestId = crypto.randomUUID();
    const targetFiles = selected.filter((file) => !changedOnly || worktree.review.changedPaths.includes(file.path));
    const fileIds = targetFiles.map((file) => file.id);
    if (!fileIds.length) {
      notify(t.workbench.noChangedFilesToReReview, viewKey, { tone: 'info' });
      return;
    }
    const selections = targetFiles.every((file) => !!file.commitToken)
      ? targetFiles.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    dispatch({ type: 'request', target, kind: 'review', requestId, fileIds });
    await execute({
      kind: 'review',
      target,
      requestId,
      fileIds,
      selections,
      modelId: model.id,
      prompt: promptText,
      locale: state.locale,
      ...(changedOnly ? { changedPaths: worktree.review.changedPaths } : {}),
    });
  }

  function cancelModel(kind: 'plan' | 'review') {
    const active = worktree?.requests[kind];
    if (!worktree || !active) return;
    cancelledRequests.current.add(active.id);
    dispatch({ type: 'requestFailed', target: targetOf(worktree), kind, requestId: active.id });
    void backend.cancelRequest?.(active.id).catch(() => {});
    notify(kind === 'plan' ? t.workbench.cancelledPlan : t.workbench.cancelledReview, viewKey, { tone: 'info' });
  }

  function cancelActiveModelRequests(tree: Worktree | undefined) {
    if (!tree) return;
    for (const kind of ['plan', 'review'] as const) {
      const active = tree.requests[kind];
      if (active) {
        cancelledRequests.current.add(active.id);
        void backend.cancelRequest?.(active.id).catch(() => {});
      }
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
    cancelActiveModelRequests(worktree);
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

  function launchCustomAction(
    action: CustomActionConfig,
    options?: { followUpPrompt?: string; resumeFromRunId?: string; forceExpandDock?: boolean },
  ) {
    if (!worktree || !model || worktree.readError) return;
    if (inFlight.current.has(`${worktree.repositoryId}:customAction`) || inFlight.current.has(`${worktree.worktreeId}:cPush`)) return;
    const requestId = crypto.randomUUID();
    const target = targetOf(worktree);
    const fileIds = selected.map((file) => file.id);
    const selections = selected.length > 0 && selected.every((file) => !!file.commitToken)
      ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    const expandDock = options?.forceExpandDock ?? (!isEditingCommitInput() && !compactMode);
    dispatch({
      type: 'customActionStarted',
      worktreeId: worktree.worktreeId,
      runId: requestId,
      actionId: action.id,
      actionLabel: action.label,
      expandDock,
    });
    void execute({
      kind: 'runCustomAction',
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
    });
  }

  function followUpCustomAction(message: Message, text: string) {
    const trimmed = text.trim();
    if (!worktree || !trimmed || !worktree.customAction) return;
    if (!model) {
      if (compactMode) {
        openConfigureModelFromCompact();
      } else {
        openConfigureModel();
      }
      return;
    }
    if (worktree.readError) return;
    const savedAction = state.repositorySettings[worktree.repositoryId]?.customActions?.find(
      (item) => item.id === worktree.customAction?.actionId,
    );
    const targetAction: CustomActionConfig = savedAction ?? {
      id: worktree.customAction.actionId,
      label: worktree.customAction.actionLabel,
      prompt: trimmed,
    };
    dispatch({ type: 'reply', worktreeId: worktree.worktreeId, messageId: message.id, text: '' });
    launchCustomAction(targetAction, {
      followUpPrompt: trimmed,
      resumeFromRunId: worktree.customAction.runId,
      forceExpandDock: !compactMode,
    });
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
    if (inFlight.current.has(`${worktree.worktreeId}:cPush`)) return;
    if (worktree.detached) {
      notify(t.workbench.detachedPrimaryBlocked, viewKey, { tone: 'warning' });
      return;
    }
    if (model && selected.length && !worktree.readError) {
      const validPlan = activeValidPlan(worktree);
      if (validPlan && validPlan.groups.some((group) => !group.message.trim())) {
        notify(t.commit.emptyPlanMessageError, viewKey, { tone: 'warning' });
        return;
      }
      if (worktree.plan && !validPlan) {
        // Outdated plan is discarded before starting a fresh C-Push plan:
        dispatch({ type: 'planCleared', worktreeId: worktree.worktreeId });
      }
      const activePlanReq = worktree.requests.plan;
      if (activePlanReq) {
        cancelledRequests.current.add(activePlanReq.id);
        void backend.cancelRequest?.(activePlanReq.id).catch(() => {});
      }
      const requestId = crypto.randomUUID();
      const target = targetOf(worktree);
      const fileIds = selected.map((file) => file.id);
      const selections = selected.every((file) => !!file.commitToken)
        ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
        : undefined;
      const expandDock = !isEditingCommitInput() && !compactMode;
      dispatch({
        type: 'cPushStarted',
        worktreeId: worktree.worktreeId,
        runId: requestId,
        frozenTarget: target,
        frozenFileIds: fileIds,
        modelId: model.id,
        expandDock,
      });
      void execute({
        kind: 'cPush',
        target,
        requestId,
        fileIds,
        selections,
        modelId: model.id,
        prompt: promptText,
        ...(validPlan
          ? {
              confirmedPlan: {
                groups: validPlan.groups.map((group) => ({ ...group, fileIds: [...group.fileIds] })),
                skippedFileIds: [...validPlan.skippedFileIds],
                ...(validPlan.skipPush ? { skipPush: true } : {}),
              },
            }
          : {}),
        locale: state.locale,
        commitLanguage: activeCommitLanguage,
      });
    }
  }

  async function respond(
    message: Message,
    answer: { optionId?: string; text?: string; questionnaireResponse?: AskUserQuestionResponse },
  ) {
    if (!worktree) return;
    const id = worktree.worktreeId;
    if (worktree.customAction?.pendingCallId && answer.questionnaireResponse && backend.respondCustomAction) {
      const respKey = `${id}:respondCustomAction`;
      if (inFlight.current.has(respKey)) return;
      inFlight.current.add(respKey);
      setPending([...inFlight.current]);
      try {
        await backend.respondCustomAction({
          requestId: crypto.randomUUID(),
          runId: worktree.customAction.runId,
          callId: worktree.customAction.pendingCallId,
          response: answer.questionnaireResponse,
          locale: state.locale,
        });
      } catch (error) {
        const errText = error instanceof Error ? error.message : t.workbench.genericOperationFailed;
        notify(errText, id);
      } finally {
        inFlight.current.delete(respKey);
        setPending([...inFlight.current]);
      }
      return;
    }
    const target = worktree.cPush?.frozenTarget ?? targetOf(worktree);
    const runId = worktree.cPush?.runId;
    const { questionnaireResponse, ...wireAnswer } = answer;
    if (await execute({
      kind: 'respond',
      target,
      requestId: crypto.randomUUID(),
      ...(runId ? { runId } : {}),
      ...(model ? { modelId: model.id } : {}),
      ...(questionnaireResponse ? { questionnaireResponse } : {}),
      messageId: message.id,
      locale: state.locale,
      ...wireAnswer,
    })) {
      dispatch({ type: 'reply', worktreeId: id, messageId: message.id, text: '' });
    }
  }

  function cancelActiveCustomAction() {
    const runId = worktree?.customAction?.runId;
    if (!runId) return;
    void backend.cancelCustomAction?.(runId).catch(() => {});
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
    runPlan,
    runReview,
    cancelModel,
    cancelActiveModelRequests,
    switchWorktree,
    setLocalKeep,
    openFile,
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
