import { decodeGitReadReply } from './git-read-protocol';
import type { GitReadReply, GitReadTarget } from './git-read-protocol';

export interface GitCommitSelection {
  fileId: string;
  // Issued by Rust per file for the current branch, HEAD, and effective staged/unstaged scope.
  commitToken: string;
}
export interface GitCommitResult {
  oid: string;
  shortOid: string;
  title: string;
  branch: string;
  parent: string | null;
}
export interface GitCommitRequest {
  requestId: string;
  target: GitReadTarget;
  selections: GitCommitSelection[];
  message: string;
  push: boolean;
}
export interface GitCommitReply extends GitReadReply {
  kind: 'commit';
  target: GitReadTarget;
  snapshot: NonNullable<GitReadReply['snapshot']>;
  commit: GitCommitResult;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}
function nonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0;
}

export function decodeGitCommitReply(value: unknown): GitCommitReply {
  const reply = decodeGitReadReply(value) as GitCommitReply;
  const commit = (value as Record<string, unknown>).commit;
  if (reply.kind !== 'commit' || !reply.target || reply.snapshot === null || !record(commit)
    || !nonEmptyString(commit.oid) || !nonEmptyString(commit.shortOid) || typeof commit.title !== 'string'
    || !nonEmptyString(commit.branch) || !(commit.parent === null || nonEmptyString(commit.parent))) {
    throw new Error('桌面提交协议不匹配，请刷新 Git 状态后核对实际历史。');
  }
  return reply;
}
