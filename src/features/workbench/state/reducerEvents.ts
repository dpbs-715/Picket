import { readScopeVersion } from './repositories';
import { computeNormalUnselectedIds, selectedFiles } from './selectors';
import type { Target, WorkbenchEvent, Worktree } from './types';

export function updateWorktreeSelection(worktree: Worktree, selectedIds: string[]): Worktree {
  const nextScopeVersion = worktree.statusVersion !== undefined
    ? readScopeVersion({ ...worktree, selectedIds })
    : `${worktree.scopeVersion.split('#')[0]}#${selectedIds.join(',')}`;
  return {
    ...worktree,
    selectedIds,
    unselectedIds: computeNormalUnselectedIds(worktree.files, selectedIds),
    scopeVersion: nextScopeVersion,
    requests: {},
    review: {
      ...worktree.review,
      status: worktree.review.status === 'running' ? worktree.requests.review?.previousReviewStatus ?? 'idle' : worktree.review.status,
    },
  };
}

export function matchesTarget(worktree: Worktree, target: Target) {
  return worktree.repositoryId === target.repositoryId && worktree.worktreeId === target.worktreeId
    && worktree.sessionId === target.sessionId && worktree.scopeVersion === target.scopeVersion;
}

export function receiveEvent(worktree: Worktree, event: WorkbenchEvent, keepSet: ReadonlySet<string>): Worktree {
  if (event.type === 'cPushProgress' || event.type === 'customActionProgress') return worktree;
  if (!matchesTarget(worktree, event.target)) return worktree;
  switch (event.type) {
    case 'message':
      return {
        ...worktree,
        sessionStatus: event.sessionStatus ?? worktree.sessionStatus,
        dockExpanded: event.sessionStatus === 'done' ? false : worktree.dockExpanded,
        messages: [
          ...worktree.messages.filter((message) => message.id !== event.message.id && (event.sessionStatus !== 'done' || message.kind === 'review')),
          event.message,
        ],
      };
    case 'planPrepared': {
      const request = worktree.requests.plan;
      if (request?.id !== event.requestId) return worktree;
      if (request.promptRevision !== worktree.prompt.revision || request.planRevision !== worktree.planRevision) {
        return { ...worktree, requests: { ...worktree.requests, plan: undefined } };
      }
      return {
        ...worktree,
        requests: { ...worktree.requests, plan: undefined },
        plan: {
          id: event.requestId,
          baseScopeVersion: event.target.scopeVersion,
          modelId: event.modelId,
          summary: event.plan.summary,
          groups: event.plan.groups.map((group) => ({ ...group, fileIds: [...group.fileIds] })),
          skippedFileIds: [...event.plan.skippedFileIds],
          ...(event.plan.skipPush ? { skipPush: true } : {}),
        },
        planRevision: worktree.planRevision + 1,
      };
    }
    case 'reviewed': {
      const request = worktree.requests.review;
      if (request?.id !== event.requestId) return worktree;
      const reviewedIds = new Set(request.fileIds);
      const preserved = worktree.review.findings.filter((finding) => !reviewedIds.has(finding.fileId) && worktree.selectedIds.includes(finding.fileId));
      const reviewedPaths = worktree.files.filter((file) => reviewedIds.has(file.id)).map((file) => file.path);
      const matchesSkipped = (entry: string, path: string) =>
        entry === path || entry.startsWith(`${path} (`) || entry.startsWith(`${path}\uff08`);
      const incomingReviewedIds = event.reviewedFileIds
        ?? request.fileIds.filter((id) => {
          const file = worktree.files.find((item) => item.id === id);
          return !!file && !file.ignoredForReview && !file.binary;
        });
      const preservedReviewedIds = (worktree.review.reviewedFileIds ?? []).filter((id) => !reviewedIds.has(id) && worktree.selectedIds.includes(id));
      return {
        ...worktree,
        requests: { ...worktree.requests, review: undefined },
        review: {
          status: 'done',
          findings: [...preserved, ...event.findings],
          scopeFileIds: [...new Set([...(worktree.review.scopeFileIds ?? []).filter((id) => worktree.selectedIds.includes(id)), ...request.fileIds])],
          reviewedFileIds: [...new Set([...preservedReviewedIds, ...incomingReviewedIds])],
          skippedPaths: [...new Set([...worktree.review.skippedPaths.filter((entry) => !reviewedPaths.some((path) => matchesSkipped(entry, path))), ...event.skippedPaths])],
          changedPaths: worktree.review.changedPaths.filter((path) => !reviewedPaths.includes(path)),
        },
      };
    }
    case 'files': {
      const incomingFiles = event.files.map((file) => ({
        ...file,
        localKeep: file.localKeep ?? keepSet.has(file.path),
      }));
      const prevById = new Map(worktree.files.map((file) => [file.id, file]));
      const prevSelectedSet = new Set(worktree.selectedIds);
      const prevUnselectedSet = new Set(worktree.unselectedIds ?? computeNormalUnselectedIds(worktree.files, worktree.selectedIds));
      const nextSelectedIds = incomingFiles
        .filter((nextFile) => {
          if (nextFile.localKeep) {
            return Boolean(prevById.get(nextFile.id)?.localKeep && prevSelectedSet.has(nextFile.id));
          }
          return !prevUnselectedSet.has(nextFile.id);
        })
        .map((file) => file.id);
      const nextUnselectedIds = computeNormalUnselectedIds(incomingFiles, nextSelectedIds);
      const selectedPaths = selectedFiles(worktree).flatMap((file) => [file.path, file.previousPath].filter((path): path is string => !!path));
      const relevant = event.nextScopeVersion !== worktree.scopeVersion || event.changedPaths.some((path) => selectedPaths.includes(path));
      const reviewedPaths = worktree.files.filter((file) => worktree.review.scopeFileIds?.includes(file.id)).map((file) => file.path);
      return {
        ...worktree,
        files: incomingFiles,
        scopeVersion: event.nextScopeVersion,
        selectedIds: nextSelectedIds,
        unselectedIds: nextUnselectedIds,
        review: {
          ...worktree.review,
          status: relevant && worktree.review.status === 'running' ? worktree.requests.review?.previousReviewStatus ?? 'idle' : worktree.review.status,
          changedPaths: [...new Set([...worktree.review.changedPaths, ...event.changedPaths.filter((path) => reviewedPaths.includes(path))])],
        },
        requests: relevant ? {} : worktree.requests,
      };
    }
  }
}
