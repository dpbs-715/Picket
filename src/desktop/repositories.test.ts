import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import type { CPushCommandReply } from '../../shared/cpush-protocol';
import { decodeGitBranchReply } from '../../shared/git-branch-protocol';
import type { GitBranchReply } from '../../shared/git-branch-protocol';
import { decodeGitCommitReply } from '../../shared/git-commit-protocol';
import type { GitCommitReply } from '../../shared/git-commit-protocol';
import { decodeGitOpenFileReply } from '../../shared/ide-protocol';
import type { GitOpenFileReply } from '../../shared/ide-protocol';
import { decodeGitReadReply } from '../../shared/git-read-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import { repositoryReply } from '../features/workbench/testing/repository-fixture';
import { desktopWorkbenchBackend } from './repositories';

beforeEach(() => { vi.stubGlobal('window', globalThis); });
afterEach(() => { clearMocks(); vi.unstubAllGlobals(); });
const target = { repositoryId: 'orders', worktreeId: 'pay-tree', sessionId: 'local-session', scopeVersion: 'scope-1' };

function branchReply(): GitBranchReply {
  const reply = repositoryReply('branch-1', 2);
  reply.snapshot!.worktrees[0].branch = 'feat/new';
  return { ...reply, snapshot: reply.snapshot!, kind: 'branch', target, action: 'switchBranch', outcome: 'completed' };
}

function commitReply(): GitCommitReply {
  const reply = repositoryReply('commit-1', 2);
  reply.snapshot!.worktrees[0].head = 'commit-oid-2';
  return {
    protocolVersion: 1,
    kind: 'commit',
    requestId: 'commit-1',
    target,
    snapshot: reply.snapshot!,
    commit: { oid: 'commit-oid-2', shortOid: 'commit-', title: 'feat: local commit', branch: 'feat/pay', parent: 'head-1' },
  };
}

function cPushReply(requestId: string): CPushCommandReply {
  const reply = repositoryReply(requestId, 2);
  return {
    protocolVersion: 1,
    kind: 'cPush',
    requestId,
    repositorySnapshot: reply.snapshot!,
    session: {
      runId: requestId,
      repositoryId: target.repositoryId,
      worktreeId: target.worktreeId,
      sessionId: target.sessionId,
      frozenScopeVersion: target.scopeVersion,
      modelId: '',
      frozenFileIds: ['order'],
      committedFileIds: ['order'],
      skippedFileIds: [],
      sessionStatus: 'done',
      message: { id: `done-${requestId}`, kind: 'done', title: '1 个提交已推送至 origin/feat/pay', description: '已完成' },
      timeline: [{ state: 'done', label: '1 个提交已推送至 origin/feat/pay' }],
      qaLog: [],
      commits: [{ oid: 'commit-oid-2', shortOid: 'commit-', title: 'feat: push commit', branch: 'feat/pay', fileIds: ['order'] }],
      canRetryRemaining: false,
    },
  };
}

describe('桌面 Git 适配器与共享协议', () => {
  it('打开命令不接收路径或 shell，只传请求身份', async () => {
    const observed: unknown[] = [];
    mockIPC((command, args) => { observed.push([command, args]); return repositoryReply(); });
    const result = await desktopWorkbenchBackend.execute({ kind: 'openRepository', requestId: 'open-1' });
    expect(result && 'snapshot' in result ? result.snapshot?.repository.id : undefined).toBe('orders');
    expect(observed).toEqual([['open_repository', { request: { requestId: 'open-1' } }]]);
  });
  it('取消目录选择返回 null，不伪造空仓库或成功提交', async () => {
    mockIPC(() => ({ protocolVersion: 1, requestId: 'cancel', snapshot: null }));
    expect(await desktopWorkbenchBackend.execute({ kind: 'openRepository', requestId: 'cancel' })).toMatchObject({ snapshot: null });
  });
  it('刷新绑定完整目标；JSON 字段顺序不影响身份比较', async () => {
    mockIPC((command, args) => {
      expect(command).toBe('refresh_repository'); expect(args).toEqual({ request: { requestId: 'refresh', target } });
      return { ...repositoryReply('refresh'), target: { scopeVersion: target.scopeVersion, sessionId: target.sessionId, worktreeId: target.worktreeId, repositoryId: target.repositoryId } };
    });
    await expect(desktopWorkbenchBackend.execute({ kind: 'refresh', requestId: 'refresh', target })).resolves.toMatchObject({ requestId: 'refresh' });
  });
  it.each(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const)('拒绝目标字段 %s 不匹配的响应', async (key) => {
    mockIPC(() => ({ ...repositoryReply('refresh'), target: { ...target, [key]: 'wrong' } }));
    await expect(desktopWorkbenchBackend.execute({ kind: 'refresh', requestId: 'refresh', target })).rejects.toThrow('身份不匹配');
  });
  it('拒绝错误 requestId 和跨仓库快照', async () => {
    mockIPC(() => repositoryReply('wrong'));
    await expect(desktopWorkbenchBackend.execute({ kind: 'openRepository', requestId: 'open' })).rejects.toThrow('身份不匹配');
    mockIPC(() => ({ ...repositoryReply('refresh'), target: { ...target, repositoryId: 'other' } }));
    await expect(desktopWorkbenchBackend.execute({ kind: 'refresh', requestId: 'refresh', target: { ...target, repositoryId: 'other' } })).rejects.toThrow('身份不匹配');
  });
  it('switchBranch 只传完整身份、操作和 Rust 签发的版本，并支持跳转到已占用该分支的既有工作树', async () => {
    mockIPC((command, args) => {
      expect(command).toBe('change_repository_branch');
      expect(args).toEqual({ request: { requestId: 'branch-1', target, action: 'switchBranch', branch: 'feat/new', branchToken: 'rust-token' } });
      return branchReply();
    });
    await expect(desktopWorkbenchBackend.execute({ kind: 'switchBranch', requestId: 'branch-1', target, branch: 'feat/new', branchToken: 'rust-token' })).resolves.toMatchObject({ outcome: 'completed' });

    const jumped = branchReply();
    jumped.snapshot.activeWorktreeId = 'fix-tree';
    mockIPC(() => jumped);
    await expect(desktopWorkbenchBackend.execute({ kind: 'switchBranch', requestId: 'branch-1', target, branch: 'fix/npe', branchToken: 'rust-token' })).resolves.toMatchObject({
      outcome: 'completed',
      snapshot: { activeWorktreeId: 'fix-tree' },
    });

    const checkedOutRemote = branchReply();
    checkedOutRemote.action = 'checkoutRemoteBranch';
    checkedOutRemote.snapshot.worktrees[0].branch = 'feat/reconcile';
    mockIPC((command, args) => {
      expect(command).toBe('change_repository_branch');
      expect(args).toEqual({
        request: {
          requestId: 'branch-remote-1',
          target,
          action: 'checkoutRemoteBranch',
          branch: 'mirror/feat/reconcile',
          branchToken: 'rust-token',
        },
      });
      return { ...checkedOutRemote, requestId: 'branch-remote-1' };
    });
    await expect(desktopWorkbenchBackend.execute({
      kind: 'checkoutRemoteBranch',
      requestId: 'branch-remote-1',
      target,
      branch: 'mirror/feat/reconcile',
      branchToken: 'rust-token',
    })).resolves.toMatchObject({
      action: 'checkoutRemoteBranch',
      outcome: 'completed',
    });

    const wrongRemoteResult = branchReply();
    wrongRemoteResult.action = 'checkoutRemoteBranch';
    wrongRemoteResult.snapshot.worktrees[0].branch = 'unrelated/branch';
    mockIPC(() => ({ ...wrongRemoteResult, requestId: 'branch-remote-bad' }));
    await expect(desktopWorkbenchBackend.execute({
      kind: 'checkoutRemoteBranch',
      requestId: 'branch-remote-bad',
      target,
      branch: 'mirror/feat/reconcile',
      branchToken: 'rust-token',
    })).rejects.toThrow('身份不匹配');
  });
  it.each(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const)('分支回执拒绝不匹配的 %s', async (key) => {
    mockIPC(() => ({ ...branchReply(), target: { ...target, [key]: 'wrong' } }));
    await expect(desktopWorkbenchBackend.execute({ kind: 'switchBranch', requestId: 'branch-1', target, branch: 'feat/new', branchToken: 'rust-token' })).rejects.toThrow('身份不匹配');
  });
  it.each(['request', 'branch', 'occupiedWrongBranch', 'repository'] as const)('分支回执拒绝错误的 %s', async (field) => {
    const reply = branchReply();
    if (field === 'request') reply.requestId = 'wrong';
    if (field === 'branch') reply.snapshot.worktrees[0].branch = 'other';
    if (field === 'occupiedWrongBranch') reply.snapshot.activeWorktreeId = 'fix-tree';
    if (field === 'repository') {
      reply.snapshot.repository.id = 'other'; reply.snapshot.worktrees.forEach((tree) => { tree.repositoryId = 'other'; });
    }
    mockIPC(() => reply);
    await expect(desktopWorkbenchBackend.execute({ kind: 'switchBranch', requestId: 'branch-1', target, branch: 'feat/new', branchToken: 'rust-token' })).rejects.toThrow('身份不匹配');
  });
  it('分支协议拒绝已移除的新建分支 / Worktree 动作、空快照与缺失 branchToken', () => {
    expect(() => decodeGitBranchReply({ ...branchReply(), action: 'createBranch' })).toThrow('协议不匹配');
    expect(() => decodeGitBranchReply({ ...branchReply(), action: 'createWorktree' })).toThrow('协议不匹配');
    expect(() => decodeGitBranchReply({ ...branchReply(), snapshot: null })).toThrow('协议不匹配');
    expect(() => decodeGitBranchReply({ ...branchReply(), outcome: 'cancelled', snapshot: null })).toThrow('协议不匹配');
    const reply = repositoryReply(); Reflect.deleteProperty(reply.snapshot!.worktrees[0], 'branchToken');
    expect(() => decodeGitReadReply(reply)).toThrow('协议不匹配');
  });
  it('仅本地提交只传完整目标和 Rust 签发的文件凭证，不接受裸范围、路径或推送', async () => {
    const selections = [{ fileId: 'order', commitToken: 'commit-order' }];
    mockIPC((command, args) => {
      expect(command).toBe('commit_repository');
      expect(args).toEqual({ request: { requestId: 'commit-1', target, selections, message: 'feat: local commit', push: false } });
      return commitReply();
    });
    await expect(desktopWorkbenchBackend.execute({
      kind: 'commit', requestId: 'commit-1', target, draft: 'feat: local commit', fileIds: ['order'], selections, push: false,
    })).resolves.toMatchObject({ kind: 'commit', commit: { oid: 'commit-oid-2' } });
  });
  it('手动提交并推送、拉取变基与重连对账绑定受控 IPC 命令并校验工作树身份（M5）', async () => {
    const selections = [{ fileId: 'order', commitToken: 'commit-order' }];
    mockIPC((command, args) => {
      if (command === 'commit_and_push_repository') {
        expect((args as { request: unknown }).request).toEqual({
          requestId: 'push-1',
          target,
          selections,
          message: 'feat: manual push',
        });
        return cPushReply('push-1');
      }
      if (command === 'pull_rebase_repository') {
        expect((args as { request: unknown }).request).toEqual({
          requestId: 'pull-1',
          target,
          modelId: 'm1',
        });
        return cPushReply('pull-1');
      }
      expect(command).toBe('reconcile_cpush_session');
      return { protocolVersion: 1, session: cPushReply('reconcile-1').session };
    });

    await expect(desktopWorkbenchBackend.execute({
      kind: 'commit', requestId: 'push-1', target, draft: 'feat: manual push', fileIds: ['order'], selections, push: true,
    })).resolves.toMatchObject({ kind: 'cPush', requestId: 'push-1' });

    await expect(desktopWorkbenchBackend.execute({
      kind: 'pull', requestId: 'pull-1', target, modelId: 'm1',
    })).resolves.toMatchObject({ kind: 'cPush', requestId: 'pull-1' });

    const observedEvents: unknown[] = [];
    const unsubscribe = desktopWorkbenchBackend.subscribe!((event) => observedEvents.push(event));
    await desktopWorkbenchBackend.reconcileSession!(target);
    unsubscribe();
    expect(observedEvents).toHaveLength(1);
  });
  it('缺少 Rust commitToken 或会话标识在调用桌面命令前即明确拒绝', async () => {
    const called = vi.fn(); mockIPC(called);
    await expect(desktopWorkbenchBackend.execute({ kind: 'commit', requestId: 'write', target, draft: 'manual', fileIds: ['order'], push: false })).rejects.toThrow('提交范围凭证');
    await expect(desktopWorkbenchBackend.execute({ kind: 'commit', requestId: 'push-write', target, draft: 'manual', fileIds: ['order'], push: true })).rejects.toThrow('提交范围凭证');
    await expect(desktopWorkbenchBackend.execute({ kind: 'generate', requestId: 'gen', target, fileIds: ['order'], modelId: 'm1', draft: '' })).rejects.toThrow('范围凭证');
    await expect(desktopWorkbenchBackend.execute({ kind: 'cPush', requestId: 'cpush-1', target, fileIds: ['order'], modelId: 'm1', draft: '' })).rejects.toThrow('范围凭证');
    await expect(desktopWorkbenchBackend.execute({ kind: 'respond', requestId: 'resp-1', target, messageId: 'q-1', optionId: 'split' })).rejects.toThrow('会话标识');
    expect(called).not.toHaveBeenCalled();
  });
  it.each(['request', 'repositoryId', 'worktreeId', 'sessionId', 'scopeVersion', 'repository', 'worktree', 'branch', 'head', 'detached'] as const)('提交回执拒绝不匹配的 %s', async (field) => {
    const reply = commitReply();
    if (field === 'request') reply.requestId = 'wrong';
    if (field === 'repositoryId' || field === 'worktreeId' || field === 'sessionId' || field === 'scopeVersion') reply.target = { ...target, [field]: 'wrong' };
    if (field === 'repository') { reply.snapshot.repository.id = 'other'; reply.snapshot.worktrees.forEach((tree) => { tree.repositoryId = 'other'; }); }
    if (field === 'worktree') reply.snapshot.activeWorktreeId = 'fix-tree';
    if (field === 'branch') reply.snapshot.worktrees[0].branch = 'other';
    if (field === 'head') reply.snapshot.worktrees[0].head = 'wrong-head';
    if (field === 'detached') reply.snapshot.worktrees[0].detached = true;
    mockIPC(() => reply);
    await expect(desktopWorkbenchBackend.execute({
      kind: 'commit', requestId: 'commit-1', target, draft: 'feat: local commit', fileIds: ['order'], selections: [{ fileId: 'order', commitToken: 'commit-order' }], push: false,
    })).rejects.toThrow('身份不匹配');
  });
  it('提交协议拒绝缺失快照、空哈希或缺失文件 commitToken', () => {
    expect(() => decodeGitCommitReply({ ...commitReply(), snapshot: null })).toThrow('协议不匹配');
    expect(() => decodeGitCommitReply({ ...commitReply(), commit: { ...commitReply().commit, oid: '' } })).toThrow('协议不匹配');
    const read = repositoryReply(); Reflect.deleteProperty(read.snapshot!.worktrees[0].files[0], 'commitToken');
    expect(() => decodeGitReadReply(read)).toThrow('协议不匹配');
  });
  it('外部编辑器命令绑定工作树目标、编辑器与可选行号，并校验回执身份', async () => {
    const openReply: GitOpenFileReply = {
      protocolVersion: 1, kind: 'openFile', requestId: 'ide-1', target, editor: 'vscode', filePath: 'src/api/pay.ts', action: 'goto', line: 46,
    };
    mockIPC((command, args) => {
      expect(command).toBe('open_in_editor');
      expect(args).toEqual({ request: { requestId: 'ide-1', target, fileId: 'pay', editor: 'vscode', line: 46 } });
      return openReply;
    });
    await expect(desktopWorkbenchBackend.execute({
      kind: 'openFile', requestId: 'ide-1', target, fileId: 'pay', editor: 'vscode', line: 46,
    })).resolves.toEqual(openReply);

    await expect(desktopWorkbenchBackend.execute({
      kind: 'openFile', requestId: 'ide-zero', target, fileId: 'pay', editor: 'vscode', line: 0,
    })).rejects.toThrow('行号');

    mockIPC(() => ({ ...openReply, editor: 'cursor' }));
    await expect(desktopWorkbenchBackend.execute({
      kind: 'openFile', requestId: 'ide-1', target, fileId: 'pay', editor: 'vscode', line: 46,
    })).rejects.toThrow('身份不匹配');
    expect(() => decodeGitOpenFileReply({ ...openReply, action: 'unknown' })).toThrow('协议不匹配');
  });
  it('工作台持久化读取与保存校验完整快照结构且不传递本地路径', async () => {
    const snapshot = repositoryReply('restore', 1).snapshot!;
    mockIPC((command) => {
      if (command === 'load_workbench_state') {
        return {
          protocolVersion: 1, editor: 'cursor', activeWorktreeId: 'pay-tree', emptyDraft: '', emptyEditing: false,
          worktrees: [{ repositoryId: 'orders', worktreeId: 'pay-tree', draftText: 'saved draft', draftEditing: true, draftOutdated: false, selectedIds: ['order'], scopeVersion: 'scope-1', tab: 'commit' }],
          snapshots: [snapshot], warning: null,
        };
      }
      expect(command).toBe('save_workbench_state');
      return null;
    });
    const restored = await desktopWorkbenchBackend.loadWorkbenchState!();
    expect(restored.editor).toBe('cursor');
    expect(restored.snapshots).toHaveLength(1);
    await expect(desktopWorkbenchBackend.saveWorkbenchState!({
      revision: 1, editor: 'cursor', activeWorktreeId: 'pay-tree', emptyDraft: '', emptyEditing: false, worktrees: restored.worktrees,
    })).resolves.toBeUndefined();
    expect(() => decodeWorkbenchRestoreReply({ ...restored, editor: 'vim' })).toThrow('协议不匹配');
  });
  it('协议保留部分暂存、原路径、二进制和本地引用来源', () => {
    const reply = repositoryReply(); reply.snapshot!.worktrees[0].files[0].previousPath = 'old.ts'; reply.snapshot!.worktrees[0].files[1].binary = true;
    expect(decodeGitReadReply(reply)).toEqual(reply);
  });
  it.each(['protocol', 'negativeCount', 'duplicateWorktree', 'duplicateFile', 'unknownActive', 'missingBinary'])('拒绝不完整或不一致的协议：%s', (kind) => {
    const reply = repositoryReply();
    switch (kind) {
      case 'protocol': Object.assign(reply, { protocolVersion: 99 }); break;
      case 'negativeCount': reply.snapshot!.worktrees[0].files[0].range.added = -1; break;
      case 'duplicateWorktree': reply.snapshot!.worktrees.push(reply.snapshot!.worktrees[0]); break;
      case 'duplicateFile': reply.snapshot!.worktrees[0].files.push(reply.snapshot!.worktrees[0].files[0]); break;
      case 'unknownActive': reply.snapshot!.activeWorktreeId = 'unknown'; break;
      case 'missingBinary': Reflect.deleteProperty(reply.snapshot!.worktrees[0].files[0], 'binary'); break;
    }
    expect(() => decodeGitReadReply(reply)).toThrow('协议不匹配');
  });
});
