import type {
  BranchAction as GitBranchAction,
  BranchReply as GitBranchReply,
  BranchRequest as GitBranchRequest,
} from './generated';
import { decodeGitReadReply } from './git-read-protocol';

export type { GitBranchAction, GitBranchReply, GitBranchRequest };
export function decodeGitBranchReply(value: unknown): GitBranchReply {
  const reply = decodeGitReadReply(value) as GitBranchReply;
  if (reply.kind !== 'branch' || !reply.target
    || (reply.action !== 'switchBranch' && reply.action !== 'checkoutRemoteBranch')
    || reply.outcome !== 'completed' || reply.snapshot === null) {
    throw new Error('Desktop branch protocol mismatch. Refresh Git status and try again.');
  }
  return reply;
}
