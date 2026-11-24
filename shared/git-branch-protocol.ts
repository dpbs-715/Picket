import { decodeGitReadReply } from './git-read-protocol';
import type { GitReadReply, GitReadTarget } from './git-read-protocol';
import type { AppLocale } from './language-protocol';

export type GitBranchAction = 'switchBranch' | 'checkoutRemoteBranch';
export interface GitBranchRequest {
  requestId: string;
  target: GitReadTarget;
  action: GitBranchAction;
  branch: string;
  // Issued by Rust for the full Git state, independent of the UI selection.
  branchToken: string;
  locale?: AppLocale;
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
    throw new Error('Desktop branch protocol mismatch. Refresh Git status and try again.');
  }
  return reply;
}
