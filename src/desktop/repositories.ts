import { invoke } from '@tauri-apps/api/core';
import { decodeGitBranchReply } from '../../shared/git-branch-protocol';
import { decodeGitCommitReply } from '../../shared/git-commit-protocol';
import { decodeGitReadReply } from '../../shared/git-read-protocol';
import type { WorkbenchBackend } from '../features/workbench/commands';
import { disconnectedBackend } from '../features/workbench/commands';

export const desktopWorkbenchBackend: WorkbenchBackend = {
  ...disconnectedBackend,
  repositoryReadsAvailable: true,
  async execute(command) {
    if (command.kind === 'commit') {
      if (command.push) {
        throw new Error('远端推送将在 M5 接入；本次未执行提交或推送，草稿与选择已保留。当前请使用「仅本地提交」。');
      }
      if (!command.selections?.length || command.selections.length !== command.fileIds.length
        || command.selections.some((selection, index) => selection.fileId !== command.fileIds[index] || !selection.commitToken)) {
        throw new Error('缺少 Rust 签发的提交范围凭证，请刷新仓库状态后重新确认提交范围。');
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
    if (command.kind === 'createBranch' || command.kind === 'switchBranch' || command.kind === 'createWorktree') {
      const request = { requestId: command.requestId, target: command.target, action: command.kind, branch: command.branch, branchToken: command.branchToken };
      const reply = decodeGitBranchReply(await invoke('change_repository_branch', { request }));
      if (reply.requestId !== command.requestId || reply.action !== command.kind
        || (['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).some((key) => reply.target[key] !== command.target[key])
        || (reply.snapshot && (reply.snapshot.repository.id !== command.target.repositoryId
          || reply.snapshot.worktrees.find((tree) => tree.worktreeId === reply.snapshot!.activeWorktreeId)?.branch !== command.branch
          || (command.kind === 'createBranch' && reply.snapshot.activeWorktreeId !== command.target.worktreeId)))) {
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
