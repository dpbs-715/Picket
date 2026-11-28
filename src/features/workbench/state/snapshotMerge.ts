import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import { isValidRepoRelativePath } from '../../../../shared/git-read-protocol';
import type { GitLocalKeepReply, GitReadReply, GitRepositorySnapshot, GitWorktree } from '../../../../shared/git-read-protocol';
import { getMessages } from '../../../i18n/messages';
import {
  computeNormalUnselectedIds,
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  localKeepPathsFor,
} from './selectors';
import type { RepositorySettings, WorkbenchState, Worktree } from './types';

export interface CommitCompletion {
  worktreeId: string;
  promptRevision: number;
  clearPrompt: boolean;
  committedFileIds: string[];
}

export function readScopeVersion(tree: Pick<Worktree, 'head' | 'branch' | 'files' | 'selectedIds' | 'statusVersion' | 'scopeVersion'>): string {
  if (tree.statusVersion === undefined) return tree.scopeVersion;
  // A UI invalidation token, not a Git write authorization. Rust freezes and
  // re-checks per-file commitTokens before executing any local commit.
  const keptIds = tree.files.filter((file) => file.localKeep).map((file) => file.id);
  return JSON.stringify([
    tree.head,
    tree.branch,
    tree.files
      .filter((file) => tree.selectedIds.includes(file.id))
      .map((file) => [file.id, file.contentVersion, file.range.kind, ...(file.localKeep ? ['localKeep'] : [])]),
    ...(keptIds.length ? [keptIds] : []),
  ]);
}

export function normalizeWorktreeLocalKeep(incoming: GitWorktree, keepSet: ReadonlySet<string>, hasSnapshotKeepList: boolean): GitWorktree {
  const resolveKeep = (file: GitWorktree['files'][number]) =>
    hasSnapshotKeepList ? keepSet.has(file.path) : Boolean(file.localKeep ?? keepSet.has(file.path));
  const needsUpdate = incoming.files.some((file) => Boolean(file.localKeep) !== resolveKeep(file));
  if (!needsUpdate) return incoming;
  return {
    ...incoming,
    files: incoming.files.map((file) => ({
      ...file,
      localKeep: resolveKeep(file),
    })),
  };
}

export function projectWorktree(
  incoming: GitWorktree,
  previous?: Worktree,
  emptyPromptText?: string,
  commitCompletion?: CommitCompletion,
): Worktree {
  const committedSet = commitCompletion && !incoming.readError ? new Set(commitCompletion.committedFileIds) : null;
  const previousById = previous ? new Map(previous.files.map((file) => [file.id, file])) : null;
  const previousSelectedSet = previous ? new Set(previous.selectedIds) : null;
  const previousUnselectedSet = previous
    ? new Set(previous.unselectedIds ?? computeNormalUnselectedIds(previous.files, previous.selectedIds))
    : null;
  const selectedIds = previous
    ? incoming.files
        .filter((nextFile) => {
          if (committedSet?.has(nextFile.id)) return false;
          if (nextFile.localKeep) {
            const prevFile = previousById?.get(nextFile.id);
            return Boolean(prevFile?.localKeep && previousSelectedSet?.has(nextFile.id));
          }
          return !previousUnselectedSet?.has(nextFile.id);
        })
        .map((file) => file.id)
    : incoming.files.filter((file) => !file.localKeep).map((file) => file.id);
  const unselectedIds = computeNormalUnselectedIds(incoming.files, selectedIds);
  const prompt = !previous
    ? { text: emptyPromptText ?? '', revision: 0 }
    : commitCompletion?.clearPrompt && !incoming.readError && previous.prompt.revision === commitCompletion.promptRevision
      ? { text: '', revision: previous.prompt.revision + 1 }
      : previous.prompt;
  const projected: Worktree = {
    ...previous, ...incoming,
    sessionId: previous?.sessionId ?? crypto.randomUUID(), scopeVersion: previous?.scopeVersion ?? incoming.statusVersion,
    selectedIds,
    unselectedIds,
    prompt,
    sessionStatus: previous?.sessionStatus ?? 'idle', messages: previous?.messages ?? [],
    activeRun: previous?.activeRun,
    dockExpanded: previous?.dockExpanded ?? false, replies: previous?.replies ?? {},
  };
  if (incoming.readError && previous) {
    return {
      ...projected,
      files: previous.files,
      selectedIds: previous.selectedIds,
      unselectedIds: previous.unselectedIds ?? computeNormalUnselectedIds(previous.files, previous.selectedIds),
      scopeVersion: previous.scopeVersion,
      prompt: previous.prompt,
    };
  }
  return { ...projected, scopeVersion: readScopeVersion(projected) };
}

export function mergeSnapshot(state: WorkbenchState, snapshot: GitRepositorySnapshot, focus: boolean, commitCompletion?: CommitCompletion): WorkbenchState {
  const previousRepository = state.repositories.find((repository) => repository.id === snapshot.repository.id);
  const existingSettings = state.repositorySettings[snapshot.repository.id];
  const resolvedKeepPaths = snapshot.repository.localKeepPaths !== undefined
    ? [...new Set(snapshot.repository.localKeepPaths.filter(isValidRepoRelativePath))].sort()
    : localKeepPathsFor(state, snapshot.repository.id);
  const keepSet = new Set(resolvedKeepPaths);
  const nextRepoSetting: RepositorySettings = {
    editor: existingSettings?.editor ?? effectiveEditorFor(state, null),
    commitLanguage: existingSettings?.commitLanguage ?? state.defaultCommitLanguage,
    ...(resolvedKeepPaths.length ? { localKeepPaths: resolvedKeepPaths } : {}),
    ...(existingSettings?.customActions?.length ? { customActions: existingSettings.customActions } : {}),
    ...(existingSettings?.activeActionId ? { activeActionId: existingSettings.activeActionId } : {}),
    ...(existingSettings?.runCommands?.length ? { runCommands: existingSettings.runCommands } : {}),
    ...(existingSettings?.runSharedEnv?.length ? { runSharedEnv: existingSettings.runSharedEnv } : {}),
  };
  const repositorySettings = {
    ...state.repositorySettings,
    [snapshot.repository.id]: nextRepoSetting,
  };
  if ((previousRepository?.readSequence ?? -1) >= snapshot.readSequence) {
    const nextState = { ...state, repositorySettings };
    return focus && nextState.worktrees.some((tree) => tree.worktreeId === snapshot.activeWorktreeId)
      ? { ...nextState, activeWorktreeId: snapshot.activeWorktreeId }
      : nextState;
  }
  const sameRepository = state.worktrees.filter((tree) => tree.repositoryId === snapshot.repository.id);
  const authoritative = snapshot.repository.localKeepPaths !== undefined;
  const worktrees = snapshot.worktrees.map((rawIncoming) => {
    const incoming = normalizeWorktreeLocalKeep(rawIncoming, keepSet, authoritative);
    return projectWorktree(
      incoming,
      sameRepository.find((tree) => tree.worktreeId === incoming.worktreeId),
      focus && !state.activeWorktreeId && incoming.worktreeId === snapshot.activeWorktreeId ? state.emptyPrompt : undefined,
      commitCompletion?.worktreeId === incoming.worktreeId ? commitCompletion : undefined,
    );
  });
  const missingError = getMessages(state.locale).workbench.missingWorktreeError;
  const missing = sameRepository
    .filter((tree) =>
      !worktrees.some((incoming) => incoming.worktreeId === tree.worktreeId)
      && (
        tree.worktreeId === state.activeWorktreeId
        || tree.prompt.text.trim().length > 0
        || tree.sessionStatus !== 'idle'
        || tree.messages.length > 0
      ),
    )
    .map((tree) => ({ ...tree, readError: missingError }));
  const nextRepoPayload = {
    ...snapshot.repository,
    ...(resolvedKeepPaths.length ? { localKeepPaths: resolvedKeepPaths } : { localKeepPaths: undefined }),
    readSequence: snapshot.readSequence,
  };
  return {
    ...state,
    repositorySettings,
    repositories: previousRepository
      ? state.repositories.map((repository) => repository.id === snapshot.repository.id ? { ...repository, ...nextRepoPayload } : repository)
      : [...state.repositories, nextRepoPayload],
    worktrees: [...state.worktrees.filter((tree) => tree.repositoryId !== snapshot.repository.id), ...worktrees, ...missing],
    activeWorktreeId: focus ? snapshot.activeWorktreeId : state.activeWorktreeId,
  };
}

export function applyLocalKeepUpdate(
  state: WorkbenchState,
  repositoryId: string,
  paths: string[],
  keep: boolean,
): WorkbenchState {
  const validPaths = paths.filter(isValidRepoRelativePath);
  if (!repositoryId || !validPaths.length) return state;
  const keepSet = new Set(localKeepPathsFor(state, repositoryId));
  let mutated = false;
  for (const path of validPaths) {
    if (keep && !keepSet.has(path)) {
      keepSet.add(path);
      mutated = true;
    } else if (!keep && keepSet.has(path)) {
      keepSet.delete(path);
      mutated = true;
    }
  }
  const hasFileMismatch = state.worktrees.some(
    (tree) => tree.repositoryId === repositoryId && tree.files.some((file) => Boolean(file.localKeep) !== keepSet.has(file.path)),
  );
  if (!mutated && !hasFileMismatch) return state;
  const nextKeepList = [...keepSet].sort();
  const existingSettings = state.repositorySettings[repositoryId];
  const repositorySettings: Record<string, RepositorySettings> = {
    ...state.repositorySettings,
    [repositoryId]: {
      editor: effectiveEditorFor(state, repositoryId),
      commitLanguage: effectiveCommitLanguageFor(state, repositoryId),
      ...(nextKeepList.length ? { localKeepPaths: nextKeepList } : {}),
      ...(existingSettings?.customActions?.length ? { customActions: existingSettings.customActions } : {}),
      ...(existingSettings?.activeActionId ? { activeActionId: existingSettings.activeActionId } : {}),
      ...(existingSettings?.runCommands?.length ? { runCommands: existingSettings.runCommands } : {}),
      ...(existingSettings?.runSharedEnv?.length ? { runSharedEnv: existingSettings.runSharedEnv } : {}),
    },
  };
  const repositories = state.repositories.map((repo) =>
    repo.id === repositoryId
      ? { ...repo, ...(nextKeepList.length ? { localKeepPaths: nextKeepList } : { localKeepPaths: undefined }) }
      : repo,
  );
  const worktrees = state.worktrees.map((tree) => {
    if (tree.repositoryId !== repositoryId) return tree;
    const newlyKeptIds = new Set<string>();
    const newlyNormalIds = new Set<string>();
    const files = tree.files.map((file) => {
      const isKept = keepSet.has(file.path);
      if (isKept && !file.localKeep) newlyKeptIds.add(file.id);
      if (!isKept && file.localKeep) newlyNormalIds.add(file.id);
      return Boolean(file.localKeep) === isKept ? file : { ...file, localKeep: isKept };
    });
    let selectedIds = newlyKeptIds.size > 0
      ? tree.selectedIds.filter((id) => !newlyKeptIds.has(id))
      : tree.selectedIds;
    if (newlyNormalIds.size > 0) {
      const existing = new Set(selectedIds);
      const added = files.filter((file) => newlyNormalIds.has(file.id) && !existing.has(file.id)).map((file) => file.id);
      if (added.length > 0) {
        selectedIds = [...selectedIds, ...added];
      }
    }
    const unselectedIds = computeNormalUnselectedIds(files, selectedIds);
    const scopeVersion = readScopeVersion({ ...tree, files, selectedIds });
    return {
      ...tree,
      files,
      selectedIds,
      unselectedIds,
      scopeVersion,
    };
  });
  return {
    ...state,
    repositorySettings,
    repositories,
    worktrees,
    settingsModified: true,
  };
}

export function finishRepositoryLocalKeep(
  state: WorkbenchState,
  repositoryId: string,
  reply: GitLocalKeepReply,
): WorkbenchState {
  if (reply.snapshot.repository.id !== repositoryId) return state;
  return mergeSnapshot(state, reply.snapshot, false);
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
