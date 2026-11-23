export const GIT_READ_PROTOCOL_VERSION = 1;

export interface GitReadTarget {
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  scopeVersion: string;
}
export interface GitReadRequest { requestId: string; target: GitReadTarget }
export interface GitChange {
  id: string;
  path: string;
  previousPath?: string;
  status: 'A' | 'M' | 'D' | 'R' | 'U' | 'T' | 'C';
  gitStatus: string;
  range: { kind: 'partial' | 'staged' | 'unstaged'; added: number; removed: number };
  binary: boolean;
  ignoredForReview: boolean;
  contentVersion: string;
  commitToken: string;
}
export type GitRemote = { kind: 'tracked'; branch: string; ahead: number; behind: number; localOnly: boolean }
  | { kind: 'missing' | 'error'; reason: string };
export interface GitWorktree {
  repositoryId: string;
  worktreeId: string;
  label: string;
  path: string;
  branch: string;
  detached: boolean;
  head: string | null;
  statusVersion: string;
  branchToken: string | null;
  readError: string | null;
  remote: GitRemote;
  files: GitChange[];
}
export interface GitRemoteBranch {
  name: string;
  remote: string;
  localName: string;
}
export interface GitRepositorySnapshot {
  repository: {
    id: string;
    name: string;
    branches: { name: string; worktreeId?: string }[];
    remoteBranches?: GitRemoteBranch[];
  };
  worktrees: GitWorktree[];
  activeWorktreeId: string;
  readSequence: number;
}
export interface GitReadReply {
  protocolVersion: typeof GIT_READ_PROTOCOL_VERSION;
  requestId: string;
  target?: GitReadTarget;
  snapshot: GitRepositorySnapshot | null;
}

function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === 'object' && !Array.isArray(value); }
function string(value: unknown): value is string { return typeof value === 'string'; }
function count(value: unknown) { return Number.isSafeInteger(value) && (value as number) >= 0; }
export function decodeGitReadReply(value: unknown): GitReadReply {
  const fail = (): never => { throw new Error('桌面 Git 读取协议不匹配，请重启应用后重试。'); };
  if (!record(value) || value.protocolVersion !== GIT_READ_PROTOCOL_VERSION || !string(value.requestId)) return fail();
  const target = value.target;
  if (target !== undefined && (!record(target) || !['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'].every((key) => string(target[key])))) return fail();
  if (value.snapshot === null) return value as unknown as GitReadReply;
  const snapshot = value.snapshot;
  if (!record(snapshot) || !count(snapshot.readSequence) || !string(snapshot.activeWorktreeId) || !record(snapshot.repository)
    || !string(snapshot.repository.id) || !string(snapshot.repository.name) || !Array.isArray(snapshot.repository.branches) || !Array.isArray(snapshot.worktrees)) return fail();
  if (!snapshot.repository.branches.every((branch) => record(branch) && string(branch.name) && (branch.worktreeId === undefined || string(branch.worktreeId)))) return fail();
  if (snapshot.repository.remoteBranches !== undefined && (!Array.isArray(snapshot.repository.remoteBranches)
    || !snapshot.repository.remoteBranches.every((branch) => record(branch) && string(branch.name) && string(branch.remote) && string(branch.localName)))) return fail();
  const ids = new Set<string>();
  for (const tree of snapshot.worktrees) {
    if (!record(tree) || !['repositoryId', 'worktreeId', 'label', 'path', 'branch', 'statusVersion'].every((key) => string(tree[key]))
      || tree.repositoryId !== snapshot.repository.id || ids.has(tree.worktreeId as string) || typeof tree.detached !== 'boolean'
      || !(tree.head === null || string(tree.head)) || !(tree.branchToken === null || string(tree.branchToken)) || !(tree.readError === null || string(tree.readError)) || !record(tree.remote) || !Array.isArray(tree.files)) return fail();
    ids.add(tree.worktreeId as string);
    const remote = tree.remote;
    if (remote.kind === 'tracked' ? !string(remote.branch) || !count(remote.ahead) || !count(remote.behind) || typeof remote.localOnly !== 'boolean'
      : !['missing', 'error'].includes(remote.kind as string) || !string(remote.reason)) return fail();
    const fileIds = new Set<string>();
    for (const file of tree.files) {
      if (!record(file) || !['id', 'path', 'contentVersion', 'commitToken'].every((key) => string(file[key])) || !(file.commitToken as string) || fileIds.has(file.id as string)
        || !['A', 'M', 'D', 'R', 'U', 'T', 'C'].includes(file.status as string) || !string(file.gitStatus) || file.gitStatus.length !== 2 || !record(file.range)
        || !['partial', 'staged', 'unstaged'].includes(file.range.kind as string) || !count(file.range.added) || !count(file.range.removed)
        || typeof file.binary !== 'boolean' || typeof file.ignoredForReview !== 'boolean' || (file.previousPath !== undefined && !string(file.previousPath))) return fail();
      fileIds.add(file.id as string);
    }
  }
  if (!ids.has(snapshot.activeWorktreeId)) return fail();
  return value as unknown as GitReadReply;
}
