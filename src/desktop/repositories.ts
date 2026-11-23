import { invoke } from '@tauri-apps/api/core';
import { decodeGitBranchReply } from '../../shared/git-branch-protocol';
import { decodeGitCommitReply } from '../../shared/git-commit-protocol';
import { decodeGitOpenFileReply } from '../../shared/ide-protocol';
import { decodeGitReadReply } from '../../shared/git-read-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import type { WorkbenchBackend } from '../features/workbench/commands';
import { disconnectedBackend } from '../features/workbench/commands';
import type { WorkbenchEvent } from '../features/workbench/state/workbench';
import {
  commitAndPushDesktop,
  pullRebaseDesktop,
  reconcileDesktopCPush,
  respondDesktopCPush,
  startDesktopCPush,
} from './cpush';
import {
  cancelDesktopModelRequest,
  deleteDesktopModel,
  generateDesktopCommitMessage,
  reviewDesktopScope,
  saveDesktopModel,
  selectDesktopModel,
  testDesktopModelConnection,
} from './models';

const eventSubscribers = new Set<(event: WorkbenchEvent) => void>();
function emitWorkbenchEvent(event: WorkbenchEvent) {
  for (const listener of eventSubscribers) listener(event);
}

export const desktopWorkbenchBackend: WorkbenchBackend = {
  ...disconnectedBackend,
  repositoryReadsAvailable: true,
  testModel: testDesktopModelConnection,
  saveModel: saveDesktopModel,
  cancelRequest: cancelDesktopModelRequest,
  subscribe(receive) {
    eventSubscribers.add(receive);
    return () => { eventSubscribers.delete(receive); };
  },
  async reconcileSession(target) {
    const progress = await reconcileDesktopCPush({
      requestId: crypto.randomUUID(),
      target,
    });
    if (progress) {
      emitWorkbenchEvent({ type: 'cPushProgress', progress });
    }
  },
  async loadWorkbenchState() {
    return decodeWorkbenchRestoreReply(await invoke('load_workbench_state'));
  },
  async saveWorkbenchState(request) {
    await invoke('save_workbench_state', { request });
  },
  async execute(command) {
    if (command.kind === 'selectModel') {
      return selectDesktopModel(command.modelId);
    }
    if (command.kind === 'deleteModel') {
      return deleteDesktopModel(command.modelId);
    }
    if (command.kind === 'generate' || command.kind === 'review') {
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((item, index) => item.fileId !== command.fileIds[index] || !item.commitToken)) {
        throw new Error('缺少 Rust 签发的文件范围凭证，请刷新变更状态后重试模型操作；草稿与选择已保留。');
      }
      if (command.kind === 'generate') {
        return generateDesktopCommitMessage({
          requestId: command.requestId,
          target: command.target,
          modelId: command.modelId,
          selections: command.selections,
          draft: command.draft,
        });
      }
      return reviewDesktopScope({
        requestId: command.requestId,
        target: command.target,
        modelId: command.modelId,
        selections: command.selections,
        draft: command.draft,
        changedPaths: command.changedPaths,
      });
    }
    if (command.kind === 'cPush') {
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((item, index) => item.fileId !== command.fileIds[index] || !item.commitToken)) {
        throw new Error('缺少 Rust 签发的文件范围凭证，请刷新变更状态后再发起 C-Push；草稿与选择已保留。');
      }
      return startDesktopCPush(
        {
          requestId: command.requestId,
          target: command.target,
          modelId: command.modelId,
          selections: command.selections,
          draft: command.draft,
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'pull') {
      return pullRebaseDesktop(
        {
          requestId: command.requestId,
          target: command.target,
          ...(command.modelId ? { modelId: command.modelId } : {}),
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'respond') {
      if (!command.runId) {
        throw new Error('缺少当前 C-Push 会话标识，请刷新后在消息栏重试。');
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
        },
        (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
      );
    }
    if (command.kind === 'openFile') {
      if (command.line !== undefined && (!Number.isSafeInteger(command.line) || command.line < 1)) {
        throw new Error('跳转行号无效，行号必须从 1 开始。');
      }
      const request = {
        requestId: command.requestId,
        target: command.target,
        fileId: command.fileId,
        editor: command.editor,
        ...(command.line !== undefined ? { line: command.line } : {}),
      };
      const reply = decodeGitOpenFileReply(await invoke('open_in_editor', { request }));
      if (reply.requestId !== command.requestId
        || reply.editor !== command.editor
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || reply.line !== command.line) {
        throw new Error('编辑器联动结果的目标或请求身份不匹配，已忽略该回执。');
      }
      return reply;
    }
    if (command.kind === 'commit') {
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((selection, index) => selection.fileId !== command.fileIds[index] || !selection.commitToken)) {
        throw new Error('缺少 Rust 签发的提交范围凭证，请刷新仓库状态后重新确认提交范围。');
      }
      if (command.push) {
        return commitAndPushDesktop(
          {
            requestId: command.requestId,
            target: command.target,
            selections: command.selections,
            message: command.draft,
            ...(command.modelId ? { modelId: command.modelId } : {}),
          },
          (progress) => emitWorkbenchEvent({ type: 'cPushProgress', progress }),
        );
      }
      const request = { requestId: command.requestId, target: command.target, selections: command.selections, message: command.draft, push: false };
      const reply = decodeGitCommitReply(await invoke('commit_repository', { request }));
      const activeTree = reply.snapshot.worktrees.find((tree) => tree.worktreeId === reply.snapshot.activeWorktreeId);
      if (reply.requestId !== command.requestId
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || reply.snapshot.repository.id !== command.target.repositoryId
        || reply.snapshot.activeWorktreeId !== command.target.worktreeId
        || !activeTree || activeTree.detached || activeTree.branch !== reply.commit.branch || activeTree.head !== reply.commit.oid) {
        throw new Error('提交结果的目标或请求身份不匹配，请刷新核对实际 Git 历史。');
      }
      return reply;
    }
    if (command.kind === 'switchBranch') {
      const request = { requestId: command.requestId, target: command.target, action: command.kind, branch: command.branch, branchToken: command.branchToken };
      const reply = decodeGitBranchReply(await invoke('change_repository_branch', { request }));
      if (reply.requestId !== command.requestId || reply.action !== command.kind
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || reply.snapshot.repository.id !== command.target.repositoryId
        || reply.snapshot.worktrees.find((tree) => tree.worktreeId === reply.snapshot.activeWorktreeId)?.branch !== command.branch) {
        throw new Error('分支操作结果的目标或请求身份不匹配，请刷新核对实际 Git 状态。');
      }
      return reply;
    }
    if (command.kind !== 'openRepository' && command.kind !== 'refresh') return disconnectedBackend.execute(command);
    const request = command.kind === 'openRepository' ? { requestId: command.requestId } : { requestId: command.requestId, target: command.target };
    const reply = decodeGitReadReply(await invoke(command.kind === 'openRepository' ? 'open_repository' : 'refresh_repository', { request }));
    const expectedTarget = command.kind === 'refresh' ? command.target : undefined;
    if (reply.requestId !== command.requestId || (expectedTarget && (!reply.target || !reply.snapshot
      || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target![key] !== expectedTarget[key])
      || reply.snapshot.repository.id !== expectedTarget.repositoryId || reply.snapshot.activeWorktreeId !== expectedTarget.worktreeId))) {
      throw new Error('Git 读取结果的目标或请求身份不匹配，已忽略该结果。');
    }
    return reply;
  },
};
