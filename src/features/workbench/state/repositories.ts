import type { CPushCommandReply, CPushProgressEvent, CPushSessionSnapshot } from '../../../../shared/cpush-protocol';
import {
  computeCustomActionPromptHash,
  type CustomActionCommandReply,
  type CustomActionProgressEvent,
  type TrustedActionUpdate,
} from '../../../../shared/custom-action-protocol';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import { isValidRepoRelativePath } from '../../../../shared/git-read-protocol';
import type { GitLocalKeepReply, GitReadReply, GitRepositorySnapshot, GitWorktree } from '../../../../shared/git-read-protocol';
import { DEFAULT_APP_LOCALE, DEFAULT_COMMIT_LANGUAGE } from '../../../../shared/language-protocol';
import {
  WINDOW_DIMENSIONS,
  type PersistedWindowPreferences,
  type WorkbenchRestoreReply,
  type WorkbenchSaveRequest,
} from '../../../../shared/workbench-persistence-protocol';
import { getMessages } from '../../../i18n/messages';
import {
  computeNormalUnselectedIds,
  defaultWindowPreferences,
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  localKeepPathsFor,
} from './workbench';
import type { EditorId, Message, RepositorySettings, WorkbenchState, Worktree } from './workbench';

interface CommitCompletion {
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

function normalizeWorktreeLocalKeep(incoming: GitWorktree, keepSet: ReadonlySet<string>, hasSnapshotKeepList: boolean): GitWorktree {
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

function projectWorktree(
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
  const baseChanged = !!previous && (previous.head !== incoming.head || previous.branch !== incoming.branch);
  const changed = previous?.files.filter((file) => {
    const next = incoming.files.find((item) => item.id === file.id);
    return baseChanged
      || next?.contentVersion !== file.contentVersion
      || next?.range.kind !== file.range.kind
      || Boolean(next?.localKeep) !== Boolean(file.localKeep);
  }).map((file) => file.path) ?? [];
  const nextScopeCandidate = readScopeVersion({
    head: incoming.head,
    branch: incoming.branch,
    files: incoming.files,
    selectedIds,
    statusVersion: incoming.statusVersion,
    scopeVersion: previous?.scopeVersion ?? incoming.statusVersion,
  });
  const relevant = baseChanged
    || (!!previous && nextScopeCandidate !== previous.scopeVersion)
    || !!previous?.files.some((file) => previous.selectedIds.includes(file.id) && changed.includes(file.path));
  const reviewedPaths = previous?.files.filter((file) => previous.review.scopeFileIds?.includes(file.id)).map((file) => file.path) ?? [];
  const prompt = !previous
    ? { text: emptyPromptText ?? '', revision: 0 }
    : commitCompletion?.clearPrompt && !incoming.readError && previous.prompt.revision === commitCompletion.promptRevision
      ? { text: '', revision: previous.prompt.revision + 1 }
      : previous.prompt;
  const review = !previous
    ? { status: 'idle' as const, findings: [], skippedPaths: [], changedPaths: [] }
    : committedSet && committedSet.size > 0
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
  const shouldClearPlan = !!committedSet && committedSet.size > 0;
  const projected: Worktree = {
    ...previous, ...incoming,
    sessionId: previous?.sessionId ?? crypto.randomUUID(), scopeVersion: previous?.scopeVersion ?? incoming.statusVersion,
    selectedIds,
    unselectedIds,
    prompt,
    plan: shouldClearPlan ? undefined : previous?.plan,
    planRevision: shouldClearPlan && previous?.plan ? previous.planRevision + 1 : previous?.planRevision ?? 0,
    review,
    sessionStatus: previous?.sessionStatus ?? 'idle', messages: previous?.messages ?? [],
    cPush: previous?.cPush,
    tab: previous?.tab ?? 'commit', dockExpanded: previous?.dockExpanded ?? false, replies: previous?.replies ?? {},
    requests: relevant ? {} : previous?.requests ?? {},
  };
  if (incoming.readError && previous) {
    return {
      ...projected,
      files: previous.files,
      selectedIds: previous.selectedIds,
      unselectedIds: previous.unselectedIds ?? computeNormalUnselectedIds(previous.files, previous.selectedIds),
      scopeVersion: previous.scopeVersion,
      prompt: previous.prompt, plan: previous.plan, planRevision: previous.planRevision,
      review: { ...previous.review, status: previous.review.status === 'running' ? previous.requests.review?.previousReviewStatus ?? 'idle' : previous.review.status }, requests: {},
    };
  }
  return { ...projected, scopeVersion: readScopeVersion(projected) };
}

function mergeSnapshot(state: WorkbenchState, snapshot: GitRepositorySnapshot, focus: boolean, commitCompletion?: CommitCompletion): WorkbenchState {
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
  const missing = sameRepository.filter((tree) => !worktrees.some((incoming) => incoming.worktreeId === tree.worktreeId))
    .map((tree) => ({ ...tree, readError: missingError, requests: {} }));
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
    const hadSelectedRemoved = newlyKeptIds.size > 0 && tree.selectedIds.some((id) => newlyKeptIds.has(id));
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
    const scopeChanged = scopeVersion !== tree.scopeVersion || hadSelectedRemoved;
    return {
      ...tree,
      files,
      selectedIds,
      unselectedIds,
      scopeVersion,
      requests: scopeChanged ? {} : tree.requests,
      review: scopeChanged && tree.review.status === 'running'
        ? { ...tree.review, status: tree.requests.review?.previousReviewStatus ?? 'idle' }
        : tree.review,
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

export function serializeWorkbenchPersistence(state: WorkbenchState, revision: number): WorkbenchSaveRequest {
  const windowPreferences: PersistedWindowPreferences = {
    pinned: state.windowPrefs.pinned,
    compactMode: state.windowPrefs.compactMode,
    pinnedExpanded: state.windowPrefs.pinnedExpanded,
    ...(state.windowPrefs.fullSize ? { fullSize: { ...state.windowPrefs.fullSize } } : {}),
    ...(state.windowPrefs.fullPosition ? { fullPosition: { ...state.windowPrefs.fullPosition } } : {}),
    ...(state.windowPrefs.compactPosition ? { compactPosition: { ...state.windowPrefs.compactPosition } } : {}),
  };
  return {
    revision,
    locale: state.locale,
    editor: state.editor,
    defaultCommitLanguage: state.defaultCommitLanguage,
    repositorySettings: Object.entries(state.repositorySettings).map(([repositoryId, cfg]) => ({
      repositoryId,
      editor: cfg.editor,
      commitLanguage: cfg.commitLanguage,
      ...(cfg.localKeepPaths?.length ? { localKeepPaths: [...cfg.localKeepPaths] } : {}),
      ...(cfg.customActions?.length ? { customActions: cfg.customActions.map((item) => ({ ...item })) } : {}),
      ...(cfg.activeActionId ? { activeActionId: cfg.activeActionId } : {}),
    })),
    windowPreferences,
    activeWorktreeId: state.activeWorktreeId,
    emptyPrompt: state.emptyPrompt,
    worktrees: state.worktrees.map((tree) => ({
      repositoryId: tree.repositoryId,
      worktreeId: tree.worktreeId,
      promptText: tree.prompt.text,
      selectedIds: [...tree.selectedIds],
      unselectedIds: [...(tree.unselectedIds ?? computeNormalUnselectedIds(tree.files, tree.selectedIds))],
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
  planOnly?: boolean,
): WorkbenchState {
  const previousTree = state.worktrees.find(
    (tree) => tree.repositoryId === session.repositoryId && tree.worktreeId === session.worktreeId,
  );
  if (!previousTree) return state;
  if (
    previousTree.cPush
    && !previousTree.cPush.planOnly
    && previousTree.cPush.runId !== session.runId
    && previousTree.sessionStatus !== 'idle'
    && previousTree.sessionStatus !== 'done'
  ) {
    return state;
  }

  const promptRevAtStart = previousTree.cPush?.promptRevisionAtStart ?? previousTree.prompt.revision;
  const isCPushCompleted = session.sessionStatus === 'done'
    && !session.message?.id.startsWith('done-local-')
    && !session.message?.id.startsWith('done-merge-pull-')
    && !session.message?.id.startsWith('done-manual-rebase-')
    && (session.commits.length > 0 || session.skippedFileIds.length > 0);
  const hasCommittedOrFinished = session.committedFileIds.length > 0 || isCPushCompleted;

  let nextState = repositorySnapshot
    ? mergeSnapshot(
        state,
        repositorySnapshot,
        false,
        session.committedFileIds.length > 0 || isCPushCompleted
          ? {
              worktreeId: session.worktreeId,
              promptRevision: promptRevAtStart,
              clearPrompt: isCPushCompleted,
              committedFileIds: session.committedFileIds,
            }
          : undefined,
      )
    : state;

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
      const nextPrompt = isCPushCompleted && !repositorySnapshot && tree.prompt.revision === promptRevAtStart
        ? { text: '', revision: tree.prompt.revision + 1 }
        : tree.prompt;
      const nextDockExpanded = session.autoCollapse || session.sessionStatus === 'idle'
        ? false
        : expandDockOnSettle
          ? true
          : tree.dockExpanded;
      const nextPlan = hasCommittedOrFinished ? undefined : tree.plan;
      const nextPlanRevision = hasCommittedOrFinished && tree.plan ? tree.planRevision + 1 : tree.planRevision;
      return {
        ...tree,
        sessionStatus: session.sessionStatus,
        dockExpanded: nextDockExpanded,
        messages: nextMessages,
        prompt: nextPrompt,
        plan: nextPlan,
        planRevision: nextPlanRevision,
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
              promptRevisionAtStart: tree.cPush?.promptRevisionAtStart ?? promptRevAtStart,
              modelId: session.modelId,
              timeline: session.timeline,
              qaLog: session.qaLog,
              commits: session.commits,
              canRetryRemaining: session.canRetryRemaining,
              ...(planOnly ?? tree.cPush?.planOnly ? { planOnly: true } : {}),
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
  const settled = applyCPushSnapshot(state, reply.session, reply.repositorySnapshot, expandDockOnSettle);
  if (!reply.preparedPlan) return settled;
  const prepared = reply.preparedPlan;
  return {
    ...settled,
    worktrees: settled.worktrees.map((tree) => {
      if (tree.repositoryId !== reply.session.repositoryId || tree.worktreeId !== reply.session.worktreeId) {
        return tree;
      }
      return {
        ...tree,
        requests: { ...tree.requests, plan: undefined },
        plan: {
          id: reply.session.runId,
          baseScopeVersion: reply.session.frozenScopeVersion,
          modelId: reply.session.modelId,
          summary: prepared.summary,
          groups: prepared.groups.map((group) => ({ ...group, fileIds: [...group.fileIds] })),
          skippedFileIds: [...prepared.skippedFileIds],
        },
        planRevision: tree.planRevision + 1,
      };
    }),
  };
}

export function receiveCPushProgress(state: WorkbenchState, progress: CPushProgressEvent): WorkbenchState {
  return applyCPushSnapshot(state, progress.session, progress.repositorySnapshot);
}

function applyTrustedUpdate(state: WorkbenchState, update?: TrustedActionUpdate): WorkbenchState {
  if (!update) return state;
  const prev = state.repositorySettings[update.repositoryId];
  const existing = prev?.customActions;
  if (!existing?.length) return state;
  let changed = false;
  const nextActions = existing.map((item) => {
    if (item.id !== update.actionId) return item;
    if (computeCustomActionPromptHash(item.prompt) !== update.trustedPromptHash) return item;
    changed = true;
    return {
      ...item,
      skipWriteConfirm: true,
      trustedPromptHash: update.trustedPromptHash,
    };
  });
  if (!changed) return state;
  return {
    ...state,
    repositorySettings: {
      ...state.repositorySettings,
      [update.repositoryId]: {
        ...prev,
        customActions: nextActions,
      },
    },
    settingsModified: true,
  };
}

export function applyCustomActionProgress(
  state: WorkbenchState,
  progress: CustomActionProgressEvent,
): WorkbenchState {
  let nextState = progress.repositorySnapshot
    ? mergeSnapshot(state, progress.repositorySnapshot, false)
    : state;
  nextState = applyTrustedUpdate(nextState, progress.trustedActionUpdate);
  const nextStatus: Worktree['sessionStatus'] =
    progress.status === 'running'
      ? 'running'
      : progress.status === 'needsDecision'
        ? 'needsDecision'
        : progress.status === 'done'
          ? 'done'
          : 'idle';
  return {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== progress.repositoryId || tree.worktreeId !== progress.worktreeId) {
        return tree;
      }
      const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
      const nextMessages: Message[] = progress.message
        ? [...reviewMessages, progress.message]
        : reviewMessages;
      const shouldExpand = progress.status === 'needsDecision'
        ? true
        : progress.status === 'done' || progress.status === 'cancelled'
          ? false
          : tree.dockExpanded;
      return {
        ...tree,
        sessionStatus: nextStatus,
        dockExpanded: shouldExpand,
        messages: nextMessages,
        cPush: undefined,
        customAction: progress.status === 'cancelled' || progress.status === 'failed'
          ? undefined
          : {
              runId: progress.runId,
              actionId: progress.actionId,
              actionLabel: progress.actionLabel,
              ...(progress.pendingCallId ? { pendingCallId: progress.pendingCallId } : {}),
              timeline: progress.timeline,
              commits: progress.commits,
              links: progress.links,
            },
      };
    }),
  };
}

export function finishCustomActionCommand(
  state: WorkbenchState,
  reply: CustomActionCommandReply,
  expandDockOnSettle?: boolean,
): WorkbenchState {
  let nextState = reply.repositorySnapshot
    ? mergeSnapshot(state, reply.repositorySnapshot, false)
    : state;
  nextState = applyTrustedUpdate(nextState, reply.trustedActionUpdate);
  const nextStatus: Worktree['sessionStatus'] = reply.status === 'done' ? 'done' : 'idle';
  return {
    ...nextState,
    worktrees: nextState.worktrees.map((tree) => {
      if (tree.repositoryId !== reply.repositoryId || tree.worktreeId !== reply.worktreeId) {
        return tree;
      }
      const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
      const nextMessages: Message[] = reply.message
        ? [...reviewMessages, reply.message]
        : reviewMessages;
      const nextDockExpanded = reply.status === 'cancelled'
        ? false
        : reply.status === 'done'
          ? Boolean(expandDockOnSettle)
          : Boolean(reply.message);
      return {
        ...tree,
        sessionStatus: nextStatus,
        dockExpanded: nextDockExpanded,
        messages: nextMessages,
        cPush: undefined,
        customAction: reply.status === 'done'
          ? {
              runId: reply.runId,
              actionId: reply.actionId,
              actionLabel: reply.actionLabel,
              timeline: reply.timeline,
              commits: reply.commits,
              links: reply.links,
            }
          : undefined,
      };
    }),
  };
}

export function hydrateWorkbench(state: WorkbenchState, restored: WorkbenchRestoreReply): WorkbenchState {
  const persistedByWorktree = new Map(restored.worktrees.map((item) => [item.worktreeId, item]));
  const cPushByWorktree = new Map((restored.cPushSessions ?? []).map((item) => [item.worktreeId, item]));
  const actionByWorktree = new Map((restored.actionSessions ?? []).map((item) => [item.worktreeId, item]));
  const userEditedEmpty = state.emptyPrompt.length > 0;
  const hydratedConnections = restored.connections === undefined
    ? state.connections
    : state.connections.length > 0
      ? [...restored.connections.filter((item) => !state.connections.some((existing) => existing.id === item.id)), ...state.connections]
      : restored.connections;
  const hydratedModels = restored.models === undefined
    ? state.models
    : state.models.length > 0
      ? [...restored.models.filter((item) => !state.models.some((existing) => existing.id === item.id)), ...state.models]
      : restored.models;
  const hydratedActiveModelId = restored.models === undefined
    ? state.activeModelId
    : state.activeModelId ?? restored.activeModelId ?? hydratedModels[0]?.id ?? null;
  const installedEditors = restored.installedEditors ?? state.installedEditors;
  const resolveEditor = (desired: EditorId): EditorId =>
    installedEditors && installedEditors.length > 0 && !installedEditors.includes(desired)
      ? installedEditors[0]
      : desired;
  const restoredLocale = restored.locale ?? DEFAULT_APP_LOCALE;
  const restoredDefaultCommitLang = restored.defaultCommitLanguage ?? DEFAULT_COMMIT_LANGUAGE;
  const locale = state.settingsModified ? state.locale : restoredLocale;
  const defaultCommitLanguage = state.settingsModified ? state.defaultCommitLanguage : restoredDefaultCommitLang;
  const desiredEditor = state.settingsModified ? state.editor : restored.editor;
  const effectiveEditor = resolveEditor(desiredEditor);
  const repositorySettings: Record<string, RepositorySettings> = {};
  for (const entry of restored.repositorySettings ?? []) {
    const keepPaths = entry.localKeepPaths?.filter(isValidRepoRelativePath) ?? [];
    const customActions = entry.customActions?.length ? entry.customActions : undefined;
    const activeActionId = entry.activeActionId && customActions?.some((item) => item.id === entry.activeActionId)
      ? entry.activeActionId
      : undefined;
    repositorySettings[entry.repositoryId] = {
      editor: resolveEditor(entry.editor),
      commitLanguage: entry.commitLanguage,
      ...(keepPaths.length ? { localKeepPaths: [...new Set(keepPaths)].sort() } : {}),
      ...(customActions ? { customActions } : {}),
      ...(activeActionId ? { activeActionId } : {}),
    };
  }
  for (const snapshot of restored.snapshots) {
    const snapKeep = snapshot.repository.localKeepPaths?.filter(isValidRepoRelativePath);
    const existing = repositorySettings[snapshot.repository.id];
    const mergedKeep = snapKeep !== undefined
      ? [...new Set(snapKeep)].sort()
      : existing?.localKeepPaths;
    repositorySettings[snapshot.repository.id] = {
      editor: existing?.editor ?? resolveEditor(restored.editor),
      commitLanguage: existing?.commitLanguage ?? restoredDefaultCommitLang,
      ...(mergedKeep?.length ? { localKeepPaths: mergedKeep } : {}),
      ...(existing?.customActions?.length ? { customActions: existing.customActions } : {}),
      ...(existing?.activeActionId ? { activeActionId: existing.activeActionId } : {}),
    };
  }
  if (state.settingsModified) {
    for (const [repositoryId, cfg] of Object.entries(state.repositorySettings)) {
      const existing = repositorySettings[repositoryId];
      const keepPaths = cfg.localKeepPaths ?? existing?.localKeepPaths;
      const customActions = cfg.customActions ?? existing?.customActions;
      const activeActionId = cfg.activeActionId !== undefined ? cfg.activeActionId : existing?.activeActionId;
      repositorySettings[repositoryId] = {
        editor: resolveEditor(cfg.editor),
        commitLanguage: cfg.commitLanguage,
        ...(keepPaths?.length ? { localKeepPaths: keepPaths } : {}),
        ...(customActions?.length ? { customActions } : {}),
        ...(activeActionId ? { activeActionId } : {}),
      };
    }
  }
  const restoredWindowPrefs = restored.windowPreferences
    ? {
        ...defaultWindowPreferences(),
        ...restored.windowPreferences,
        fullSize: restored.windowPreferences.fullSize
          ? {
              width: Math.max(WINDOW_DIMENSIONS.minFullWidth, restored.windowPreferences.fullSize.width),
              height: Math.max(WINDOW_DIMENSIONS.minFullHeight, restored.windowPreferences.fullSize.height),
            }
          : defaultWindowPreferences().fullSize,
      }
    : state.windowPrefs;
  let next: WorkbenchState = {
    ...state,
    locale,
    editor: effectiveEditor,
    defaultCommitLanguage,
    repositorySettings,
    windowPrefs: state.settingsModified ? state.windowPrefs : restoredWindowPrefs,
    ...(installedEditors !== undefined ? { installedEditors } : {}),
    emptyPrompt: userEditedEmpty ? state.emptyPrompt : restored.emptyPrompt,
    connections: hydratedConnections,
    models: hydratedModels,
    activeModelId: hydratedActiveModelId,
  };
  for (const snapshot of restored.snapshots) {
    if (next.repositories.some((repo) => repo.id === snapshot.repository.id)) {
      next = mergeSnapshot(next, snapshot, false);
      continue;
    }
    const repoKeepPaths = repositorySettings[snapshot.repository.id]?.localKeepPaths ?? [];
    const keepSet = new Set(repoKeepPaths);
    const hasSnapshotKeepList = snapshot.repository.localKeepPaths !== undefined || repoKeepPaths.length > 0;
    const worktrees = snapshot.worktrees.map((rawIncoming): Worktree => {
      const incoming = normalizeWorktreeLocalKeep(rawIncoming, keepSet, hasSnapshotKeepList);
      const base = projectWorktree(incoming);
      const saved = persistedByWorktree.get(incoming.worktreeId);
      const savedCPush = cPushByWorktree.get(incoming.worktreeId);
      const savedAction = actionByWorktree.get(incoming.worktreeId);
      const withSavedPrompt: Worktree = (!saved || incoming.readError) ? base : (() => {
        const selectedIds = saved.unselectedIds !== undefined
          ? (() => {
              const savedSelectedSet = new Set(saved.selectedIds);
              const savedUnselectedSet = new Set(saved.unselectedIds);
              return incoming.files
                .filter((file) => (file.localKeep ? savedSelectedSet.has(file.id) : !savedUnselectedSet.has(file.id)))
                .map((file) => file.id);
            })()
          : saved.selectedIds.filter((id) => incoming.files.some((file) => file.id === id));
        const unselectedIds = computeNormalUnselectedIds(incoming.files, selectedIds);
        const scopeVersion = readScopeVersion({ ...base, selectedIds });
        return {
          ...base,
          selectedIds,
          unselectedIds,
          scopeVersion,
          prompt: { text: saved.promptText, revision: 0 },
          tab: saved.tab,
        };
      })();
      if (savedCPush && savedCPush.sessionStatus !== 'idle') {
        return {
          ...withSavedPrompt,
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
            promptRevisionAtStart: withSavedPrompt.prompt.revision,
            modelId: savedCPush.modelId,
            timeline: savedCPush.timeline,
            qaLog: savedCPush.qaLog,
            commits: savedCPush.commits,
            canRetryRemaining: savedCPush.canRetryRemaining,
          },
        };
      }
      if (savedAction?.message) {
        return {
          ...withSavedPrompt,
          sessionStatus: savedAction.status === 'done' ? 'done' : 'idle',
          messages: [savedAction.message],
          dockExpanded: false,
          customAction: {
            runId: savedAction.runId,
            actionId: savedAction.actionId,
            actionLabel: savedAction.actionLabel,
            timeline: savedAction.timeline,
            commits: savedAction.commits,
            links: savedAction.links,
          },
        };
      }
      return withSavedPrompt;
    });
    next = {
      ...next,
      repositories: [
        ...next.repositories,
        {
          ...snapshot.repository,
          ...(repoKeepPaths.length ? { localKeepPaths: repoKeepPaths } : { localKeepPaths: undefined }),
          readSequence: snapshot.readSequence,
        },
      ],
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
