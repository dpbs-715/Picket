import { invoke } from '@tauri-apps/api/core';
import { decodeGitBranchReply } from '../../shared/git-branch-protocol';
import { decodeGitOpenFileReply } from '../../shared/ide-protocol';
import { decodeGitLocalKeepReply, decodeGitReadReply } from '../../shared/git-read-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import type { WorkbenchBackend } from '../features/workbench/commands';
import { disconnectedBackend } from '../features/workbench/commands';
import type { WorkbenchEvent } from '../features/workbench/state/workbench';
import { getMessages } from '../i18n/messages';
import {
  cancelDesktopCustomAction,
  dismissDesktopCustomActionDone,
  generateDesktopCustomAction,
  respondDesktopCustomAction,
  startDesktopCustomAction,
} from './actions';
import {
  dismissDesktopCPushDone,
  planDesktopCPush,
  pullRebaseDesktop,
  reconcileDesktopCPush,
  respondDesktopCPush,
  startDesktopCPush,
} from './cpush';
import {
  cancelDesktopModelRequest,
  createDesktopProviderConnection,
  deleteDesktopModel,
  deleteDesktopProviderConnection,
  listDesktopProviderModels,
  revealDesktopProviderApiKey,
  reviewDesktopScope,
  saveDesktopModel,
  selectDesktopModel,
  testDesktopModelConnection,
  updateDesktopProviderConnection,
} from './models';
import {
  getDesktopSkillsState,
  resetDesktopSkills,
  setDesktopEnabledSkills,
  syncDesktopSkills,
} from './skills';
import {
  cancelUpdate,
  checkForUpdate,
  getUpdateStatus,
  installUpdate,
  openReleasePage,
  subscribeUpdateState,
} from './updater';

const eventSubscribers = new Set<(event: WorkbenchEvent) => void>();
function emitWorkbenchEvent(event: WorkbenchEvent) {
  for (const listener of eventSubscribers) listener(event);
}

export const desktopWorkbenchBackend: WorkbenchBackend = {
  ...disconnectedBackend,
  repositoryReadsAvailable: true,
  testModel: testDesktopModelConnection,
  listProviderModels: listDesktopProviderModels,
  createProviderConnection: createDesktopProviderConnection,
  updateProviderConnection: updateDesktopProviderConnection,
  saveModel: saveDesktopModel,
  revealProviderApiKey: revealDesktopProviderApiKey,
  getUpdateStatus,
  checkForUpdate,
  installUpdate,
  cancelUpdate,
  openReleasePage,
  subscribeUpdateState,
  getSkillsState: getDesktopSkillsState,
  syncSkills: syncDesktopSkills,
  setEnabledSkills: setDesktopEnabledSkills,
  resetSkills: resetDesktopSkills,
  cancelRequest: cancelDesktopModelRequest,
  respondCustomAction: respondDesktopCustomAction,
  generateCustomAction: generateDesktopCustomAction,
  cancelCustomAction: cancelDesktopCustomAction,
  subscribe(receive) {
    eventSubscribers.add(receive);
    return () => { eventSubscribers.delete(receive); };
  },
  async reconcileSession(target, locale) {
    const progress = await reconcileDesktopCPush({
      requestId: crypto.randomUUID(),
      target,
      ...(locale ? { locale } : {}),
    });
    if (progress) {
      emitWorkbenchEvent({ type: 'cPushProgress', progress });
    }
  },
  async dismissCPushDone(target, runId, messageId, locale) {
    await dismissDesktopCPushDone({
      requestId: crypto.randomUUID(),
      target,
      ...(runId ? { runId } : {}),
      messageId,
      ...(locale ? { locale } : {}),
    });
  },
  async dismissCustomActionDone(target, runId, messageId, locale) {
    await dismissDesktopCustomActionDone({
      requestId: crypto.randomUUID(),
      target,
      ...(runId ? { runId } : {}),
      messageId,
      ...(locale ? { locale } : {}),
    });
  },
  async loadWorkbenchState() {
    return decodeWorkbenchRestoreReply(await invoke('load_workbench_state'));
  },
  async saveWorkbenchState(request) {
    await invoke('save_workbench_state', { request });
  },
  async execute(command) {
    const t = getMessages(command.locale).desktop;
    if (command.kind === 'selectModel') {
      return selectDesktopModel(command.modelId, command.locale);
    }
    if (command.kind === 'deleteModel') {
      return deleteDesktopModel(command.modelId, command.locale);
    }
    if (command.kind === 'deleteProviderConnection') {
      return deleteDesktopProviderConnection(command.connectionId, command.locale);
    }
    if (command.kind === 'review') {
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((item, index) => item.fileId !== command.fileIds[index] || !item.commitToken)) {
        throw new Error(t.missingModelScopeTokens);
      }
      return reviewDesktopScope({
        requestId: command.requestId,
        target: command.target,
        modelId: command.modelId,
        selections: command.selections,
        prompt: command.prompt,
        changedPaths: command.changedPaths,
        locale: command.locale,
      });
    }
    if (command.kind === 'planCPush') {
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((item, index) => item.fileId !== command.fileIds[index] || !item.commitToken)) {
        throw new Error(t.missingCPushScopeTokens);
      }
      return planDesktopCPush(
        {
          requestId: command.requestId,
          target: command.target,
          modelId: command.modelId,
          selections: command.selections,
          prompt: command.prompt,
          ...(command.locale ? { locale: command.locale } : {}),
          ...(command.commitLanguage ? { commitLanguage: command.commitLanguage } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'cPush') {
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((item, index) => item.fileId !== command.fileIds[index] || !item.commitToken)) {
        throw new Error(t.missingCPushScopeTokens);
      }
      return startDesktopCPush(
        {
          requestId: command.requestId,
          target: command.target,
          modelId: command.modelId,
          selections: command.selections,
          prompt: command.prompt,
          ...(command.confirmedPlan ? { confirmedPlan: command.confirmedPlan } : {}),
          ...(command.locale ? { locale: command.locale } : {}),
          ...(command.commitLanguage ? { commitLanguage: command.commitLanguage } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'runCustomAction') {
      return startDesktopCustomAction(
        {
          requestId: command.requestId,
          target: command.target,
          modelId: command.modelId,
          action: command.action,
          selections: command.selections ?? [],
          extraPrompt: command.extraPrompt,
          ...(command.locale ? { locale: command.locale } : {}),
          ...(command.commitLanguage ? { commitLanguage: command.commitLanguage } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'customActionProgress', progress }),
      );
    }
    if (command.kind === 'pull') {
      return pullRebaseDesktop(
        {
          requestId: command.requestId,
          target: command.target,
          ...(command.modelId ? { modelId: command.modelId } : {}),
          ...(command.locale ? { locale: command.locale } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'respond') {
      if (!command.runId) {
        throw new Error(t.missingCPushRunId);
      }
      return respondDesktopCPush(
        {
          requestId: command.requestId,
          target: command.target,
          runId: command.runId,
          messageId: command.messageId,
          ...(command.optionId !== undefined ? { optionId: command.optionId } : {}),
          ...(command.text !== undefined ? { text: command.text } : {}),
          ...(command.modelId !== undefined ? { modelId: command.modelId } : {}),
          ...(command.locale ? { locale: command.locale } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'openFile') {
      if (command.line !== undefined && (!Number.isSafeInteger(command.line) || command.line < 1)) {
        throw new Error(t.invalidLineNumber);
      }
      const request = {
        requestId: command.requestId,
        target: command.target,
        fileId: command.fileId,
        editor: command.editor,
        ...(command.line !== undefined ? { line: command.line } : {}),
        ...(command.locale ? { locale: command.locale } : {}),
      };
      const reply = decodeGitOpenFileReply(await invoke('open_in_editor', { request }));
      if (reply.requestId !== command.requestId
        || reply.editor !== command.editor
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || reply.line !== command.line) {
        throw new Error(t.editorReplyMismatch);
      }
      return reply;
    }
    if (command.kind === 'switchBranch' || command.kind === 'checkoutRemoteBranch') {
      const request = {
        requestId: command.requestId,
        target: command.target,
        action: command.kind,
        branch: command.branch,
        branchToken: command.branchToken,
        ...(command.locale ? { locale: command.locale } : {}),
      };
      const reply = decodeGitBranchReply(await invoke('change_repository_branch', { request }));
      const activeBranch = reply.snapshot.worktrees.find((tree) => tree.worktreeId === reply.snapshot.activeWorktreeId)?.branch;
      const expectedMatches = command.kind === 'switchBranch'
        ? activeBranch === command.branch
        : !!activeBranch && (command.branch === activeBranch || command.branch.endsWith(`/${activeBranch}`));
      if (reply.requestId !== command.requestId || reply.action !== command.kind
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || reply.snapshot.repository.id !== command.target.repositoryId
        || !expectedMatches) {
        throw new Error(t.branchReplyMismatch);
      }
      return reply;
    }
    if (command.kind === 'setLocalKeep') {
      const request = {
        requestId: command.requestId,
        target: command.target,
        paths: command.paths,
        keep: command.keep,
        ...(command.locale ? { locale: command.locale } : {}),
      };
      const reply = decodeGitLocalKeepReply(await invoke('set_repository_local_keep', { request }));
      if (reply.requestId !== command.requestId
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || reply.snapshot.repository.id !== command.target.repositoryId
        || reply.snapshot.activeWorktreeId !== command.target.worktreeId) {
        throw new Error(t.localKeepReplyMismatch);
      }
      return reply;
    }
    if (command.kind !== 'openRepository' && command.kind !== 'refresh') return disconnectedBackend.execute(command);
    const request = command.kind === 'openRepository'
      ? { requestId: command.requestId, ...(command.locale ? { locale: command.locale } : {}) }
      : { requestId: command.requestId, target: command.target, ...(command.locale ? { locale: command.locale } : {}) };
    const reply = decodeGitReadReply(await invoke(command.kind === 'openRepository' ? 'open_repository' : 'refresh_repository', { request }));
    const expectedTarget = command.kind === 'refresh' ? command.target : undefined;
    if (reply.requestId !== command.requestId || (expectedTarget && (!reply.target || !reply.snapshot
      || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target![key] !== expectedTarget[key])
      || reply.snapshot.repository.id !== expectedTarget.repositoryId || reply.snapshot.activeWorktreeId !== expectedTarget.worktreeId))) {
      throw new Error(t.gitReadReplyMismatch);
    }
    return reply;
  },
};
