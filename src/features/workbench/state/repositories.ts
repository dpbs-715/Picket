import type { CPushCommandReply, CPushProgressEvent, CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitCommitReply } from '../../../../shared/git-commit-protocol';
import type { GitReadReply, GitRepositorySnapshot, GitWorktree } from '../../../../shared/git-read-protocol';
import type { WorkbenchRestoreReply, WorkbenchSaveRequest } from '../../../../shared/workbench-persistence-protocol';
import type { Message, WorkbenchState, Worktree } from './workbench';

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
    cPush: previous?.cPush,
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

export function serializeWorkbenchPersistence(state: WorkbenchState, revision: number): WorkbenchSaveRequest {
  return {
    revision,
    editor: state.editor,
    activeWorktreeId: state.activeWorktreeId,
    emptyDraft: state.emptyDraft,
    emptyEditing: state.emptyEditing,
    worktrees: state.worktrees.map((tree) => ({
      repositoryId: tree.repositoryId,
      worktreeId: tree.worktreeId,
      draftText: tree.draft.text,
      draftEditing: tree.draft.editing,
      draftOutdated: tree.draft.outdated,
      selectedIds: [...tree.selectedIds],
      scopeVersion: tree.scopeVersion,
      tab: tree.tab,
    })),
  };
}

export function applyCPushSnapshot(
  state: WorkbenchState,
  session: CPushSessionSnapshot,
  repositorySnapshot?: GitRepositorySnapshot,
  expandDockOnSettle?: boolean,
): WorkbenchState {
  const previousTree = state.worktrees.find(
    (tree) => tree.repositoryId === session.repositoryId && tree.worktreeId === session.worktreeId,
  );
  if (!previousTree) return state;
  if (
    previousTree.cPush
    && previousTree.cPush.runId !== session.runId
    && previousTree.sessionStatus !== 'idle'
    && previousTree.sessionStatus !== 'done'
  ) {
    return state;
  }

  const pendingCommitReq = state.repositoryCommits?.[session.repositoryId];
  const draftRevAtStart = previousTree.cPush?.draftRevisionAtStart
    ?? (pendingCommitReq?.worktreeId === session.worktreeId ? pendingCommitReq.draftRevision : undefined)
    ?? -1;
  const shouldConsumeDraft = !session.canRetryRemaining
    && session.commits.length > 0
    && session.committedFileIds.length > 0;

  let baseState = state;
  if (pendingCommitReq?.worktreeId === session.worktreeId) {
    const repositoryCommits = { ...state.repositoryCommits };
    delete repositoryCommits[session.repositoryId];
    baseState = { ...state, repositoryCommits };
  }

  let nextState = repositorySnapshot
    ? mergeSnapshot(
        baseState,
        repositorySnapshot,
        false,
        session.committedFileIds.length > 0
          ? {
              worktreeId: session.worktreeId,
              draftRevision: shouldConsumeDraft ? draftRevAtStart : -999_999,
              committedFileIds: session.committedFileIds,
            }
          : undefined,
      )
    : baseState;

  nextState = {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== session.repositoryId || tree.worktreeId !== session.worktreeId) {
        return tree;
      }
      const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
      const nextMessages: Message[] = session.message
        ? [...reviewMessages, session.message]
        : reviewMessages;
      const nextDraft = shouldConsumeDraft && !repositorySnapshot && tree.draft.revision === draftRevAtStart
        ? { text: '', editing: false, outdated: false, revision: tree.draft.revision + 1 }
        : tree.draft;
      const nextDockExpanded = session.autoCollapse || session.sessionStatus === 'idle'
        ? false
        : expandDockOnSettle
          ? true
          : tree.dockExpanded;
      return {
        ...tree,
        sessionStatus: session.sessionStatus,
        dockExpanded: nextDockExpanded,
        messages: nextMessages,
        draft: nextDraft,
        cPush: session.sessionStatus === 'idle'
          ? undefined
          : {
              runId: session.runId,
              frozenTarget: {
                repositoryId: session.repositoryId,
                worktreeId: session.worktreeId,
                sessionId: session.sessionId,
                scopeVersion: session.frozenScopeVersion,
              },
              frozenFileIds: session.frozenFileIds,
              draftRevisionAtStart: tree.cPush?.draftRevisionAtStart ?? (draftRevAtStart >= 0 ? draftRevAtStart : tree.draft.revision),
              modelId: session.modelId,
              timeline: session.timeline,
              qaLog: session.qaLog,
              commits: session.commits,
              canRetryRemaining: session.canRetryRemaining,
            },
      };
    }),
  };
  return nextState;
}

export function finishCPushCommand(
  state: WorkbenchState,
  reply: CPushCommandReply,
  expandDockOnSettle?: boolean,
): WorkbenchState {
  return applyCPushSnapshot(state, reply.session, reply.repositorySnapshot, expandDockOnSettle);
}

export function receiveCPushProgress(state: WorkbenchState, progress: CPushProgressEvent): WorkbenchState {
  return applyCPushSnapshot(state, progress.session, progress.repositorySnapshot);
}

export function hydrateWorkbench(state: WorkbenchState, restored: WorkbenchRestoreReply): WorkbenchState {
  const persistedByWorktree = new Map(restored.worktrees.map((item) => [item.worktreeId, item]));
  const cPushByWorktree = new Map((restored.cPushSessions ?? []).map((item) => [item.worktreeId, item]));
  const userEditedEmpty = state.emptyEditing || state.emptyDraft.length > 0;
  const hydratedModels = restored.models === undefined
    ? state.models
    : state.models.length > 0
      ? [...restored.models.filter((item) => !state.models.some((existing) => existing.id === item.id)), ...state.models]
      : restored.models;
  const hydratedActiveModelId = restored.models === undefined
    ? state.activeModelId
    : state.activeModelId ?? restored.activeModelId ?? hydratedModels[0]?.id ?? null;
  let next: WorkbenchState = {
    ...state,
    editor: state.editorModified ? state.editor : restored.editor,
    emptyDraft: userEditedEmpty ? state.emptyDraft : restored.emptyDraft,
    emptyEditing: userEditedEmpty ? state.emptyEditing : restored.emptyEditing,
    models: hydratedModels,
    activeModelId: hydratedActiveModelId,
  };
  for (const snapshot of restored.snapshots) {
    if (next.repositories.some((repo) => repo.id === snapshot.repository.id)) {
      next = mergeSnapshot(next, snapshot, false);
      continue;
    }
    const worktrees = snapshot.worktrees.map((incoming) => {
      const base = projectWorktree(incoming);
      const saved = persistedByWorktree.get(incoming.worktreeId);
      const savedCPush = cPushByWorktree.get(incoming.worktreeId);
      const withSavedDraft: Worktree = (!saved || incoming.readError) ? base : (() => {
        const selectedIds = saved.selectedIds.filter((id) => incoming.files.some((file) => file.id === id));
        const scopeVersion = readScopeVersion({ ...base, selectedIds });
        const outdated = saved.draftOutdated || (!!saved.draftText && !!saved.scopeVersion && saved.scopeVersion !== scopeVersion);
        return {
          ...base,
          selectedIds,
          scopeVersion,
          draft: { text: saved.draftText, editing: saved.draftEditing, outdated, revision: 0 },
          tab: saved.tab,
        };
      })();
      if (!savedCPush || savedCPush.sessionStatus === 'idle') return withSavedDraft;
      return {
        ...withSavedDraft,
        sessionId: savedCPush.sessionId,
        sessionStatus: savedCPush.sessionStatus,
        messages: savedCPush.message ? [savedCPush.message] : [],
        dockExpanded: false,
        cPush: {
          runId: savedCPush.runId,
          frozenTarget: {
            repositoryId: savedCPush.repositoryId,
            worktreeId: savedCPush.worktreeId,
            sessionId: savedCPush.sessionId,
            scopeVersion: savedCPush.frozenScopeVersion,
          },
          frozenFileIds: savedCPush.frozenFileIds,
          draftRevisionAtStart: withSavedDraft.draft.revision,
          modelId: savedCPush.modelId,
          timeline: savedCPush.timeline,
          qaLog: savedCPush.qaLog,
          commits: savedCPush.commits,
          canRetryRemaining: savedCPush.canRetryRemaining,
        },
      };
    });
    next = {
      ...next,
      repositories: [...next.repositories, { ...snapshot.repository, readSequence: snapshot.readSequence }],
      worktrees: [...next.worktrees, ...worktrees],
    };
  }
  if (state.navigationRevision === 0 && state.activeWorktreeId === null) {
    next = {
      ...next,
      activeWorktreeId: restored.activeWorktreeId && next.worktrees.some((tree) => tree.worktreeId === restored.activeWorktreeId)
        ? restored.activeWorktreeId
        : next.worktrees[0]?.worktreeId ?? null,
    };
  }
  return next;
}
