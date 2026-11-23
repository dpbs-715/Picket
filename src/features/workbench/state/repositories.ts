import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitCommitReply } from '../../../../shared/git-commit-protocol';
import type { GitReadReply, GitRepositorySnapshot, GitWorktree } from '../../../../shared/git-read-protocol';
import type { WorkbenchState, Worktree } from './workbench';

interface CommitCompletion {
  worktreeId: string;
  draftRevision: number;
  committedFileIds: string[];
}

export function readScopeVersion(tree: Pick<Worktree, 'head' | 'branch' | 'files' | 'selectedIds' | 'statusVersion' | 'scopeVersion'>): string {
  if (tree.statusVersion === undefined) return tree.scopeVersion;
  // A UI invalidation token, not a Git write authorization. Rust freezes and
  // re-checks per-file commitTokens before executing any local commit.
  return JSON.stringify([tree.head, tree.branch, tree.files.filter((file) => tree.selectedIds.includes(file.id)).map((file) => [file.id, file.contentVersion, file.range.kind])]);
}

function projectWorktree(
  incoming: GitWorktree,
  previous?: Worktree,
  emptyDraft?: { text: string; editing: boolean },
  commitCompletion?: CommitCompletion,
): Worktree {
  const committedSet = commitCompletion && !incoming.readError ? new Set(commitCompletion.committedFileIds) : null;
  const selectedIds = previous
    ? previous.selectedIds.filter((id) => (!committedSet || !committedSet.has(id)) && incoming.files.some((file) => file.id === id))
    : incoming.files.filter((file) => !file.ignoredForReview).map((file) => file.id);
  const baseChanged = !!previous && (previous.head !== incoming.head || previous.branch !== incoming.branch);
  const changed = previous?.files.filter((file) => {
    const next = incoming.files.find((item) => item.id === file.id);
    return baseChanged || next?.contentVersion !== file.contentVersion || next?.range.kind !== file.range.kind;
  }).map((file) => file.path) ?? [];
  const relevant = baseChanged || !!previous?.files.some((file) => previous.selectedIds.includes(file.id) && changed.includes(file.path));
  const reviewedPaths = previous?.files.filter((file) => previous.review.scopeFileIds?.includes(file.id)).map((file) => file.path) ?? [];
  const draft = !previous
    ? { text: emptyDraft?.text ?? '', editing: emptyDraft?.editing ?? false, outdated: false, revision: 0 }
    : committedSet
      ? previous.draft.revision === commitCompletion!.draftRevision
        ? { text: '', editing: false, outdated: false, revision: previous.draft.revision + 1 }
        : { ...previous.draft, outdated: !!previous.draft.text }
      : { ...previous.draft, outdated: previous.draft.outdated || (relevant && !!previous.draft.text) };
  const review = !previous
    ? { status: 'idle' as const, findings: [], skippedPaths: [], changedPaths: [] }
    : committedSet
      ? (() => {
          const liveIds = new Set(incoming.files.map((file) => file.id));
          const validScopeIds = (previous.review.scopeFileIds ?? []).filter((id) => liveIds.has(id) && !committedSet.has(id));
          const validFindings = previous.review.findings.filter((finding) => liveIds.has(finding.fileId) && !committedSet.has(finding.fileId));
          if (previous.review.status === 'done' && (previous.review.scopeFileIds?.length ?? 0) > 0 && validScopeIds.length === 0) {
            return { status: 'idle' as const, findings: [], skippedPaths: [], changedPaths: [] };
          }
          return {
            ...previous.review,
            status: previous.review.status === 'running' ? previous.requests.review?.previousReviewStatus ?? 'idle' : previous.review.status,
            findings: validFindings,
            scopeFileIds: previous.review.scopeFileIds ? validScopeIds : undefined,
            changedPaths: previous.review.changedPaths.filter((path) => incoming.files.some((file) => file.path === path && validScopeIds.includes(file.id))),
          };
        })()
      : {
          ...previous.review,
          status: relevant && previous.review.status === 'running' ? previous.requests.review?.previousReviewStatus ?? 'idle' : previous.review.status,
          changedPaths: [...new Set([...previous.review.changedPaths, ...changed.filter((path) => reviewedPaths.includes(path))])],
        };
  const projected: Worktree = {
    ...previous, ...incoming,
    sessionId: previous?.sessionId ?? crypto.randomUUID(), scopeVersion: previous?.scopeVersion ?? incoming.statusVersion,
    selectedIds,
    draft,
    review,
    sessionStatus: previous?.sessionStatus ?? 'idle', messages: previous?.messages ?? [],
    tab: previous?.tab ?? 'commit', dockExpanded: previous?.dockExpanded ?? false, replies: previous?.replies ?? {},
    requests: relevant ? {} : previous?.requests ?? {},
  };
  if (incoming.readError && previous) {
    return { ...projected, files: previous.files, selectedIds: previous.selectedIds, scopeVersion: previous.scopeVersion,
      draft: { ...previous.draft, outdated: previous.draft.outdated || !!previous.draft.text },
      review: { ...previous.review, status: previous.review.status === 'running' ? previous.requests.review?.previousReviewStatus ?? 'idle' : previous.review.status }, requests: {} };
  }
  return { ...projected, scopeVersion: readScopeVersion(projected) };
}

function mergeSnapshot(state: WorkbenchState, snapshot: GitRepositorySnapshot, focus: boolean, commitCompletion?: CommitCompletion): WorkbenchState {
  const previousRepository = state.repositories.find((repository) => repository.id === snapshot.repository.id);
  if ((previousRepository?.readSequence ?? -1) >= snapshot.readSequence) return focus && state.worktrees.some((tree) => tree.worktreeId === snapshot.activeWorktreeId) ? { ...state, activeWorktreeId: snapshot.activeWorktreeId } : state;
  const sameRepository = state.worktrees.filter((tree) => tree.repositoryId === snapshot.repository.id);
  const worktrees = snapshot.worktrees.map((incoming) => projectWorktree(
    incoming,
    sameRepository.find((tree) => tree.worktreeId === incoming.worktreeId),
    focus && !state.activeWorktreeId && incoming.worktreeId === snapshot.activeWorktreeId ? { text: state.emptyDraft, editing: state.emptyEditing } : undefined,
    commitCompletion?.worktreeId === incoming.worktreeId ? commitCompletion : undefined,
  ));
  const missing = sameRepository.filter((tree) => !worktrees.some((incoming) => incoming.worktreeId === tree.worktreeId))
    .map((tree) => ({ ...tree, readError: '该工作树已不在 Git 的最新列表中，草稿和选择已保留，请重新打开目录。', requests: {} }));
  return { ...state,
    repositories: previousRepository ? state.repositories.map((repository) => repository.id === snapshot.repository.id ? { ...repository, ...snapshot.repository, readSequence: snapshot.readSequence } : repository)
      : [...state.repositories, { ...snapshot.repository, readSequence: snapshot.readSequence }],
    worktrees: [...state.worktrees.filter((tree) => tree.repositoryId !== snapshot.repository.id), ...worktrees, ...missing],
    activeWorktreeId: focus ? snapshot.activeWorktreeId : state.activeWorktreeId,
  };
}

export function finishRepositoryCommit(state: WorkbenchState, repositoryId: string, reply: GitCommitReply): WorkbenchState {
  const request = state.repositoryCommits?.[repositoryId];
  if (!request || request.requestId !== reply.requestId || request.worktreeId !== reply.target.worktreeId) return state;
  const repositoryCommits = { ...state.repositoryCommits }; delete repositoryCommits[repositoryId];
  const next = { ...state, repositoryCommits };
  return mergeSnapshot(next, reply.snapshot, false, request);
}

export function finishRepositoryBranch(state: WorkbenchState, repositoryId: string, reply: GitBranchReply): WorkbenchState {
  const request = state.repositoryBranches?.[repositoryId];
  if (!request || request.requestId !== reply.requestId) return state;
  const repositoryBranches = { ...state.repositoryBranches }; delete repositoryBranches[repositoryId];
  const next = { ...state, repositoryBranches };
  if (!reply.snapshot) return next;
  return mergeSnapshot(next, reply.snapshot, request.navigationRevision === state.navigationRevision);
}

export function finishRepositoryRead(state: WorkbenchState, key: string, reply: GitReadReply): WorkbenchState {
  const request = state.repositoryReads[key];
  if (!request || request.requestId !== reply.requestId) return state;
  const repositoryReads = { ...state.repositoryReads }; delete repositoryReads[key];
  const next = { ...state, repositoryReads };
  if (!reply.snapshot) return next;
  return mergeSnapshot(next, reply.snapshot, key === 'open' && request.navigationRevision === state.navigationRevision);
}
