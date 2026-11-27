import { readScopeVersion } from './repositories';
import { computeNormalUnselectedIds } from './selectors';
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
          ...worktree.messages.filter((message) => message.id !== event.message.id && event.sessionStatus !== 'done'),
          event.message,
        ],
      };
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
      return {
        ...worktree,
        files: incomingFiles,
        scopeVersion: event.nextScopeVersion,
        selectedIds: nextSelectedIds,
        unselectedIds: nextUnselectedIds,
      };
    }
  }
}
