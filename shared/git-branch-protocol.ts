import { decodeGitReadReply } from './git-read-protocol';
import type { GitReadReply, GitReadTarget } from './git-read-protocol';

export type GitBranchAction = 'switchBranch' | 'checkoutRemoteBranch';
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
  outcome: 'completed';
  snapshot: NonNullable<GitReadReply['snapshot']>;
}
export function decodeGitBranchReply(value: unknown): GitBranchReply {
  const reply = decodeGitReadReply(value) as GitBranchReply;
  if (reply.kind !== 'branch' || !reply.target
    || (reply.action !== 'switchBranch' && reply.action !== 'checkoutRemoteBranch')
    || reply.outcome !== 'completed' || reply.snapshot === null) {
    throw new Error('桌面分支操作协议不匹配，请刷新 Git 状态后重试。');
  }
  return reply;
}
