import { decodeGitReadReply } from './git-read-protocol';
import type { GitReadReply, GitReadTarget } from './git-read-protocol';

export type GitBranchAction = 'createBranch' | 'switchBranch' | 'createWorktree';
export interface GitBranchRequest {
  requestId: string;
  target: GitReadTarget;
  action: GitBranchAction;
  branch: string;
  // Issued by Rust for the full Git state, independent of the UI selection.
  branchToken: string;
}
export interface GitBranchReply extends GitReadReply {
  kind: 'branch';
  target: GitReadTarget;
  action: GitBranchAction;
  outcome: 'completed' | 'cancelled';
}
export function decodeGitBranchReply(value: unknown): GitBranchReply {
  const reply = decodeGitReadReply(value) as GitBranchReply;
  if (reply.kind !== 'branch' || !reply.target || !['createBranch', 'switchBranch', 'createWorktree'].includes(reply.action)
    || !['completed', 'cancelled'].includes(reply.outcome) || (reply.outcome === 'completed') !== (reply.snapshot !== null)
    || (reply.outcome === 'cancelled' && reply.action !== 'createWorktree')) {
    throw new Error('桌面分支操作协议不匹配，请刷新 Git 状态后重试。');
  }
  return reply;
}
