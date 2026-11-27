import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { decodeCPushCommandReply, type CPushCommandReply } from '../../shared/cpush-protocol';
import { decodeGitBranchReply } from '../../shared/git-branch-protocol';
import type { GitBranchReply } from '../../shared/git-branch-protocol';
import { decodeGitOpenFileReply } from '../../shared/ide-protocol';
import type { GitOpenFileReply } from '../../shared/ide-protocol';
import { decodeGitLocalKeepReply, decodeGitReadReply } from '../../shared/git-read-protocol';
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
      modelId: 'm1',
      frozenFileIds: ['order'],
      committedFileIds: ['order'],
      skippedFileIds: [],
      sessionStatus: 'done',
      message: { id: `done-${requestId}`, kind: 'done', title: '1 commit pushed to origin/feat/pay', description: 'Completed' },
      timeline: [{ state: 'done', label: '1 commit pushed to origin/feat/pay' }],
      qaLog: [],
      commits: [{ oid: 'commit-oid-2', shortOid: 'commit-', title: 'feat: push commit', branch: 'feat/pay', fileIds: ['order'] }],
      canRetryRemaining: false,
    },
  };
}

describe('Desktop Git adapter and shared protocols', () => {
  it('openRepository sends only request identity and optional locale, never paths or shell strings', async () => {
    const observed: unknown[] = [];
    mockIPC((command, args) => { observed.push([command, args]); return repositoryReply(); });
    const result = await desktopWorkbenchBackend.execute({ kind: 'openRepository', requestId: 'open-1' });
    expect(result && 'snapshot' in result ? result.snapshot?.repository.id : undefined).toBe('orders');
    expect(observed).toEqual([['open_repository', { request: { requestId: 'open-1' } }]]);
  });

  it('cancelling directory picker returns null without fabricating an empty repository', async () => {
    mockIPC(() => ({ protocolVersion: 1, requestId: 'cancel', snapshot: null }));
    expect(await desktopWorkbenchBackend.execute({ kind: 'openRepository', requestId: 'cancel' })).toMatchObject({ snapshot: null });
  });

  it('refresh binds full target regardless of JSON property order', async () => {
    mockIPC((command, args) => {
      expect(command).toBe('refresh_repository');
      expect(args).toEqual({ request: { requestId: 'refresh', target, locale: 'zh' } });
      return {
        ...repositoryReply('refresh'),
        target: { scopeVersion: target.scopeVersion, sessionId: target.sessionId, worktreeId: target.worktreeId, repositoryId: target.repositoryId },
      };
    });
    await expect(desktopWorkbenchBackend.execute({ kind: 'refresh', requestId: 'refresh', target, locale: 'zh' })).resolves.toMatchObject({ requestId: 'refresh' });
  });

  it.each(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const)('rejects refresh reply with mismatched %s', async (key) => {
    mockIPC(() => ({ ...repositoryReply('refresh'), target: { ...target, [key]: 'wrong' } }));
    await expect(desktopWorkbenchBackend.execute({ kind: 'refresh', requestId: 'refresh', target })).rejects.toThrow('identity mismatch');
  });

  it('rejects wrong requestId and cross-repository snapshot', async () => {
    mockIPC(() => repositoryReply('wrong'));
    await expect(desktopWorkbenchBackend.execute({ kind: 'openRepository', requestId: 'open' })).rejects.toThrow('identity mismatch');
    mockIPC(() => ({ ...repositoryReply('refresh'), target: { ...target, repositoryId: 'other' } }));
    await expect(desktopWorkbenchBackend.execute({ kind: 'refresh', requestId: 'refresh', target: { ...target, repositoryId: 'other' } })).rejects.toThrow('identity mismatch');
  });

  it('switchBranch and checkoutRemoteBranch send full identity, action, and Rust branchToken', async () => {
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
    })).rejects.toThrow('identity mismatch');
  });

  it.each(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const)('branch reply rejects mismatched %s', async (key) => {
    mockIPC(() => ({ ...branchReply(), target: { ...target, [key]: 'wrong' } }));
    await expect(desktopWorkbenchBackend.execute({ kind: 'switchBranch', requestId: 'branch-1', target, branch: 'feat/new', branchToken: 'rust-token' })).rejects.toThrow('identity mismatch');
  });

  it.each(['request', 'branch', 'occupiedWrongBranch', 'repository'] as const)('branch reply rejects wrong %s', async (field) => {
    const reply = branchReply();
    if (field === 'request') reply.requestId = 'wrong';
    if (field === 'branch') reply.snapshot.worktrees[0].branch = 'other';
    if (field === 'occupiedWrongBranch') reply.snapshot.activeWorktreeId = 'fix-tree';
    if (field === 'repository') {
      reply.snapshot.repository.id = 'other'; reply.snapshot.worktrees.forEach((tree) => { tree.repositoryId = 'other'; });
    }
    mockIPC(() => reply);
    await expect(desktopWorkbenchBackend.execute({ kind: 'switchBranch', requestId: 'branch-1', target, branch: 'feat/new', branchToken: 'rust-token' })).rejects.toThrow('identity mismatch');
  });

  it('branch and read protocols reject removed actions, null snapshots, missing branchToken, or missing commitToken', () => {
    expect(() => decodeGitBranchReply({ ...branchReply(), action: 'createBranch' })).toThrow('protocol mismatch');
    expect(() => decodeGitBranchReply({ ...branchReply(), action: 'createWorktree' })).toThrow('protocol mismatch');
    expect(() => decodeGitBranchReply({ ...branchReply(), snapshot: null })).toThrow('protocol mismatch');
    expect(() => decodeGitBranchReply({ ...branchReply(), outcome: 'cancelled', snapshot: null })).toThrow('protocol mismatch');
    const reply = repositoryReply(); Reflect.deleteProperty(reply.snapshot!.worktrees[0], 'branchToken');
    expect(() => decodeGitReadReply(reply)).toThrow('protocol mismatch');
    const readMissingCommit = repositoryReply(); Reflect.deleteProperty(readMissingCommit.snapshot!.worktrees[0].files[0], 'commitToken');
    expect(() => decodeGitReadReply(readMissingCommit)).toThrow('protocol mismatch');
  });

  it('start_cpush, pull_rebase, and session reconciliation bind controlled IPC commands', async () => {
    const selections = [{ fileId: 'order', commitToken: 'commit-order' }];
    mockIPC((command, args) => {
      if (command === 'start_cpush') {
        expect((args as { request: unknown }).request).toEqual({
          requestId: 'cpush-direct-1',
          target,
          modelId: 'm1',
          selections,
          prompt: 'keep concise',
        });
        return cPushReply('cpush-direct-1');
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
      kind: 'cPush',
      requestId: 'cpush-direct-1',
      target,
      fileIds: ['order'],
      selections,
      modelId: 'm1',
      prompt: 'keep concise',
    })).resolves.toMatchObject({ kind: 'cPush', requestId: 'cpush-direct-1' });

    await expect(desktopWorkbenchBackend.execute({
      kind: 'pull', requestId: 'pull-1', target, modelId: 'm1',
    })).resolves.toMatchObject({ kind: 'cPush', requestId: 'pull-1' });

    const observedEvents: unknown[] = [];
    const unsubscribe = desktopWorkbenchBackend.subscribe!((event) => observedEvents.push(event));
    await desktopWorkbenchBackend.reconcileSession!(target, 'en');
    unsubscribe();
    expect(observedEvents).toHaveLength(1);

    expect(decodeCPushCommandReply(cPushReply('ok-plan')).requestId).toBe('ok-plan');
  });

  it('rejects missing commitTokens or C-Push runId before invoking desktop IPC', async () => {
    const called = vi.fn(); mockIPC(called);
    await expect(desktopWorkbenchBackend.execute({ kind: 'cPush', requestId: 'cpush-1', target, fileIds: ['order'], modelId: 'm1', prompt: '' })).rejects.toThrow('scope tokens');
    await expect(desktopWorkbenchBackend.execute({ kind: 'respond', requestId: 'resp-1', target, messageId: 'q-1', optionId: 'split' })).rejects.toThrow('session identifier');
    expect(called).not.toHaveBeenCalled();
  });

  it('external editor launch binds worktree target, editor, optional line number, and direct repo path', async () => {
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

    mockIPC((command, args) => {
      expect(command).toBe('open_in_editor');
      expect(args).toEqual({ request: { requestId: 'ide-path', target, path: 'src/api/pay.ts', editor: 'vscode', line: 46 } });
      return { ...openReply, requestId: 'ide-path' };
    });
    await expect(desktopWorkbenchBackend.execute({
      kind: 'openFile', requestId: 'ide-path', target, path: 'src/api/pay.ts', editor: 'vscode', line: 46,
    })).resolves.toMatchObject({ requestId: 'ide-path', filePath: 'src/api/pay.ts', line: 46 });

    await expect(desktopWorkbenchBackend.execute({
      kind: 'openFile', requestId: 'ide-zero', target, fileId: 'pay', editor: 'vscode', line: 0,
    })).rejects.toThrow('line number');

    mockIPC(() => ({ ...openReply, editor: 'cursor' }));
    await expect(desktopWorkbenchBackend.execute({
      kind: 'openFile', requestId: 'ide-1', target, fileId: 'pay', editor: 'vscode', line: 46,
    })).rejects.toThrow('identity mismatch');
    expect(() => decodeGitOpenFileReply({ ...openReply, action: 'unknown' })).toThrow('protocol mismatch');
  });

  it('workbench persistence v2 load and save validate full snapshot structure without passing local paths or v1 draft fields', async () => {
    const snapshot = repositoryReply('restore', 1).snapshot!;
    mockIPC((command) => {
      if (command === 'load_workbench_state') {
        return {
          protocolVersion: 2, locale: 'zh', editor: 'cursor', defaultCommitLanguage: 'zh',
          repositorySettings: [{ repositoryId: 'orders', editor: 'rustrover', commitLanguage: 'zh' }],
          activeWorktreeId: 'pay-tree', emptyPrompt: '',
          worktrees: [{ repositoryId: 'orders', worktreeId: 'pay-tree', promptText: 'saved extra instruction', selectedIds: ['order'], scopeVersion: 'scope-1', tab: 'commit' }],
          snapshots: [snapshot], warning: null,
        };
      }
      expect(command).toBe('save_workbench_state');
      return null;
    });
    const restored = await desktopWorkbenchBackend.loadWorkbenchState!();
    expect(restored.locale).toBe('zh');
    expect(restored.editor).toBe('cursor');
    expect(restored.repositorySettings).toEqual([{ repositoryId: 'orders', editor: 'rustrover', commitLanguage: 'zh' }]);
    expect(restored.snapshots).toHaveLength(1);
    await expect(desktopWorkbenchBackend.saveWorkbenchState!({
      revision: 1, locale: 'en', editor: 'cursor', defaultCommitLanguage: 'en', repositorySettings: restored.repositorySettings,
      activeWorktreeId: 'pay-tree', emptyPrompt: '', worktrees: restored.worktrees,
    })).resolves.toBeUndefined();
    expect(() => decodeWorkbenchRestoreReply({ ...restored, editor: 'vim' })).toThrow('protocol mismatch');
    expect(() => decodeWorkbenchRestoreReply({ ...restored, protocolVersion: 1 })).toThrow('protocol mismatch');
  });

  it('protocol preserves partial staging, previousPath, binary flag, and localOnly tracking ref', () => {
    const reply = repositoryReply(); reply.snapshot!.worktrees[0].files[0].previousPath = 'old.ts'; reply.snapshot!.worktrees[0].files[1].binary = true;
    expect(decodeGitReadReply(reply)).toEqual(reply);
  });

  it.each(['protocol', 'negativeCount', 'duplicateWorktree', 'duplicateFile', 'unknownActive', 'missingBinary'])('rejects malformed Git read reply: %s', (kind) => {
    const reply = repositoryReply();
    switch (kind) {
      case 'protocol': Object.assign(reply, { protocolVersion: 99 }); break;
      case 'negativeCount': reply.snapshot!.worktrees[0].files[0].range.added = -1; break;
      case 'duplicateWorktree': reply.snapshot!.worktrees.push(reply.snapshot!.worktrees[0]); break;
      case 'duplicateFile': reply.snapshot!.worktrees[0].files.push(reply.snapshot!.worktrees[0].files[0]); break;
      case 'unknownActive': reply.snapshot!.activeWorktreeId = 'unknown'; break;
      case 'missingBinary': Reflect.deleteProperty(reply.snapshot!.worktrees[0].files[0], 'binary'); break;
    }
    expect(() => decodeGitReadReply(reply)).toThrow('protocol mismatch');
  });

  it('setLocalKeep invokes set_repository_local_keep and validates relative paths and target binding', async () => {
    const observed: unknown[] = [];
    mockIPC((command, args) => {
      observed.push([command, args]);
      const base = repositoryReply('keep-1', 3);
      base.snapshot!.repository.localKeepPaths = ['RELEASE_NOTES.md'];
      base.snapshot!.worktrees[0].files = base.snapshot!.worktrees[0].files.map((f) =>
        f.path === 'RELEASE_NOTES.md' ? { ...f, localKeep: true, commitToken: 'tok-notes-kept' } : f,
      );
      return {
        ...base,
        kind: 'localKeep',
        target,
        snapshot: base.snapshot!,
      };
    });

    const result = await desktopWorkbenchBackend.execute({
      kind: 'setLocalKeep',
      requestId: 'keep-1',
      target,
      paths: ['RELEASE_NOTES.md'],
      keep: true,
      locale: 'en',
    });
    expect(observed).toEqual([[
      'set_repository_local_keep',
      { request: { requestId: 'keep-1', target, paths: ['RELEASE_NOTES.md'], keep: true, locale: 'en' } },
    ]]);
    expect(result && 'kind' in result && result.kind === 'localKeep' ? result.snapshot.repository.localKeepPaths : []).toEqual(['RELEASE_NOTES.md']);

    const unsafeReply = repositoryReply('keep-bad', 4);
    unsafeReply.snapshot!.repository.localKeepPaths = ['../outside.env'];
    expect(() => decodeGitLocalKeepReply({ ...unsafeReply, kind: 'localKeep', target, snapshot: unsafeReply.snapshot })).toThrow('protocol mismatch');

    const gitDirReply = repositoryReply('keep-git', 5);
    gitDirReply.snapshot!.repository.localKeepPaths = ['.git/info/exclude'];
    expect(() => decodeGitLocalKeepReply({ ...gitDirReply, kind: 'localKeep', target, snapshot: gitDirReply.snapshot })).toThrow('protocol mismatch');
  });
});
