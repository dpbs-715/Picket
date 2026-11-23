import type { GitReadReply, GitRepositorySnapshot } from '../../../../shared/git-read-protocol';
import { sampleFiles } from './fixtures';

export function repositorySnapshot(sequence = 1): GitRepositorySnapshot {
  return {
    repository: {
      id: 'orders',
      name: 'real-contract-fixture',
      branches: [{ name: 'feat/pay', worktreeId: 'pay-tree' }, { name: 'fix/npe', worktreeId: 'fix-tree' }, { name: 'feat/invoice' }],
      remoteBranches: [
        { name: 'mirror/feat/pay', remote: 'mirror', localName: 'feat/pay' },
        { name: 'mirror/feat/reconcile', remote: 'mirror', localName: 'feat/reconcile' },
      ],
    },
    activeWorktreeId: 'pay-tree', readSequence: sequence,
    worktrees: [
      { repositoryId: 'orders', worktreeId: 'pay-tree', label: 'pay', path: '/test-only/pay', branch: 'feat/pay', head: 'head-1', detached: false,
        statusVersion: `status-${sequence}`, branchToken: `branch-${sequence}`, readError: null, remote: { kind: 'tracked', branch: 'mirror/feat/pay', ahead: 2, behind: 1, localOnly: true },
        files: sampleFiles().map((file) => ({ ...file, gitStatus: file.range.kind === 'partial' ? 'MM' : `.${file.status}`, ignoredForReview: !!file.ignoredForReview, binary: false, contentVersion: `content-${file.id}`, commitToken: `commit-${file.id}` })) },
      { repositoryId: 'orders', worktreeId: 'fix-tree', label: 'fix', path: '/test-only/fix', branch: 'fix/npe', head: 'head-1', detached: false,
        statusVersion: `fix-${sequence}`, branchToken: `fix-branch-${sequence}`, readError: null, remote: { kind: 'missing', reason: '没有跟踪分支' }, files: [] },
    ],
  };
}
export function repositoryReply(requestId = 'open-1', sequence = 1): GitReadReply {
  return { protocolVersion: 1, requestId, snapshot: repositorySnapshot(sequence) };
}
