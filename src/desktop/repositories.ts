import { invokeDesktop } from './errors';
import { decodeGitBranchReply } from '../../shared/git-branch-protocol';
import { decodeGitOpenFileReply } from '../../shared/ide-protocol';
import { decodeGitLocalKeepReply, decodeGitReadReply } from '../../shared/git-read-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import type { WorkbenchBackend } from '../features/workbench/commands';
import { disconnectedBackend } from '../features/workbench/commands';
import type { WorkbenchEvent } from '../features/workbench/state/workbench';
import { getMessages } from '../i18n/messages';
import {
  cancelDesktopAction,
  dismissDesktopSessionDone,
  generateDesktopCustomAction,
  reconcileDesktopActionSession,
  respondDesktopActionQuestion,
  runDesktopSessionRecovery,
  startDesktopAction,
} from './actions';
import {
  cancelDesktopModelRequest,
  createDesktopProviderConnection,
  deleteDesktopModel,
  deleteDesktopProviderConnection,
  listDesktopProviderModels,
  revealDesktopProviderApiKey,
  saveDesktopModel,
  selectDesktopModel,
  testDesktopModelConnection,
  updateDesktopProviderConnection,
} from './models';
import {
  cancelDesktopDiscoverRunCommands,
  discoverDesktopRunCommands,
  listDesktopRunProcesses,
  openDesktopRunLocalUrl,
  openDesktopRunLogsWindow,
  refreshDesktopUserShellEnv,
  restartDesktopRunCommand,
  startDesktopRunCommand,
  stopDesktopRunCommand,
  subscribeDesktopRunState,
} from './runner';
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
  listRunProcesses: listDesktopRunProcesses,
  startRunCommand: startDesktopRunCommand,
  stopRunCommand: stopDesktopRunCommand,
  restartRunCommand: restartDesktopRunCommand,
  openRunLogsWindow: openDesktopRunLogsWindow,
  openRunLocalUrl: openDesktopRunLocalUrl,
  refreshUserShellEnv: refreshDesktopUserShellEnv,
  subscribeRunState: subscribeDesktopRunState,
  discoverRunCommands: discoverDesktopRunCommands,
  cancelDiscoverRunCommands: cancelDesktopDiscoverRunCommands,
  cancelRequest: cancelDesktopModelRequest,
  respondActionQuestion: respondDesktopActionQuestion,
  generateCustomAction: generateDesktopCustomAction,
  cancelAction: cancelDesktopAction,
  subscribe(receive) {
    eventSubscribers.add(receive);
    return () => { eventSubscribers.delete(receive); };
  },
  async reconcileSession(target, locale, expectedRunId) {
    const progress = await reconcileDesktopActionSession({
      requestId: crypto.randomUUID(),
      target,
      ...(locale ? { locale } : {}),
    });
    if (progress && (!expectedRunId || progress.session.runId === expectedRunId)) {
      emitWorkbenchEvent({
        type: 'actionProgress',
        progress,
        ...(expectedRunId ? { expectedRunId } : {}),
      });
    }
  },
  async dismissSessionDone(target, runId, messageId, locale) {
    await dismissDesktopSessionDone({
      requestId: crypto.randomUUID(),
      target,
      ...(runId ? { runId } : {}),
      messageId,
      ...(locale ? { locale } : {}),
    });
  },
  async loadWorkbenchState() {
    return decodeWorkbenchRestoreReply(await invokeDesktop('load_workbench_state'));
  },
  async saveWorkbenchState(request) {
    await invokeDesktop('save_workbench_state', { request });
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
    if (command.kind === 'startAction') {
      if (command.profile === 'cpush') {
        if (!command.selections?.length || command.selections.length !== command.fileIds.length
          || command.selections.some((item, index) => item.fileId !== command.fileIds[index] || !item.commitToken)) {
          throw new Error(t.missingCPushScopeTokens);
        }
      }
      return startDesktopAction(
        {
          requestId: command.requestId,
          target: command.target,
          modelId: command.modelId,
          profile: command.profile,
          ...(command.action ? { action: command.action } : {}),
          selections: command.selections ?? [],
          extraPrompt: command.extraPrompt,
          ...(command.push !== undefined ? { push: command.push } : {}),
          ...(command.followUpPrompt ? { followUpPrompt: command.followUpPrompt } : {}),
          ...(command.resumeFromRunId ? { resumeFromRunId: command.resumeFromRunId } : {}),
          ...(command.locale ? { locale: command.locale } : {}),
          ...(command.commitLanguage ? { commitLanguage: command.commitLanguage } : {}),
          ...(command.approvalMode ? { approvalMode: command.approvalMode } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'actionProgress', progress }),
      );
    }
    if (command.kind === 'runRecovery') {
      if (command.action.kind !== 'pullRebase' && !command.runId) {
        throw new Error(t.missingCPushRunId);
      }
      return runDesktopSessionRecovery(
        {
          requestId: command.requestId,
          target: command.target,
          ...(command.runId ? { runId: command.runId } : {}),
          ...(command.messageId ? { messageId: command.messageId } : {}),
          action: command.action,
          ...(command.modelId !== undefined ? { modelId: command.modelId } : {}),
          ...(command.locale ? { locale: command.locale } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'actionProgress', progress }),
      );
    }
    if (command.kind === 'openFile') {
      if (!command.fileId && !command.path) {
        throw new Error(t.editorReplyMismatch);
      }
      if (command.line !== undefined && (!Number.isSafeInteger(command.line) || command.line < 1)) {
        throw new Error(t.invalidLineNumber);
      }
      const request = {
        requestId: command.requestId,
        target: command.target,
        ...(command.fileId ? { fileId: command.fileId } : {}),
        ...(command.path ? { path: command.path } : {}),
        editor: command.editor,
        ...(command.line !== undefined ? { line: command.line } : {}),
        ...(command.locale ? { locale: command.locale } : {}),
      };
      const reply = decodeGitOpenFileReply(await invokeDesktop('open_in_editor', { request }));
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
      const reply = decodeGitBranchReply(await invokeDesktop('change_repository_branch', { request }));
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
      const reply = decodeGitLocalKeepReply(await invokeDesktop('set_repository_local_keep', { request }));
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
    const reply = decodeGitReadReply(await invokeDesktop(command.kind === 'openRepository' ? 'open_repository' : 'refresh_repository', { request }));
    const expectedTarget = command.kind === 'refresh' ? command.target : undefined;
    if (reply.requestId !== command.requestId || (expectedTarget && (!reply.target || !reply.snapshot
      || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target![key] !== expectedTarget[key])
      || reply.snapshot.repository.id !== expectedTarget.repositoryId || reply.snapshot.activeWorktreeId !== expectedTarget.worktreeId))) {
      throw new Error(t.gitReadReplyMismatch);
    }
    return reply;
  },
};
