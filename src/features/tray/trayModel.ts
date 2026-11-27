import type {
  TrayRepositorySummary,
  TraySessionStatus,
  TrayStateSnapshot,
} from '../../../shared/tray-protocol';
import { orderedMessages, type WorkbenchState, type Worktree } from '../workbench/state/workbench';

export const AUTO_FOLD_REPO_THRESHOLD = 5;

const URGENT_RANK: Record<TraySessionStatus, number> = {
  conflict: 0,
  needsDecision: 1,
  pushFailed: 2,
  waitingManual: 3,
  running: 4,
  idle: 5,
  done: 5,
};

export function isTrayRepoUrgent(repo: TrayRepositorySummary): boolean {
  return URGENT_RANK[repo.sessionStatus] < 5;
}

export function isTrayRepoNeedsDock(repo: TrayRepositorySummary): boolean {
  return repo.sessionStatus === 'conflict'
    || repo.sessionStatus === 'needsDecision'
    || repo.sessionStatus === 'pushFailed'
    || repo.sessionStatus === 'waitingManual';
}

export function isTrayRepoCleanIdle(repo: TrayRepositorySummary): boolean {
  return !isTrayRepoUrgent(repo)
    && !repo.readError
    && !repo.detached
    && repo.changeCount === 0
    && repo.ahead === 0
    && repo.behind === 0;
}

function effectiveSessionStatus(tree: Worktree): TraySessionStatus {
  return tree.sessionStatus;
}

function worktreePriorityScore(tree: Worktree, isActive: boolean): number {
  const status = effectiveSessionStatus(tree);
  if (URGENT_RANK[status] < 5) return URGENT_RANK[status];
  if (tree.readError) return 10;
  const normalChanges = tree.files.filter((file) => !file.localKeep).length;
  const syncCount = tree.remote.kind === 'tracked' ? tree.remote.ahead + tree.remote.behind : 0;
  if (normalChanges > 0 || syncCount > 0 || tree.detached) {
    return isActive ? 15 : 20;
  }
  return isActive ? 25 : 30;
}

function summarizeWorktree(
  repoName: string,
  worktreeCount: number,
  tree: Worktree,
  active: boolean,
): TrayRepositorySummary {
  const sessionStatus = effectiveSessionStatus(tree);
  const normalChanges = tree.files.filter((file) => !file.localKeep).length;
  const localKeepCount = tree.files.length - normalChanges;
  const hasUpstream = tree.remote.kind === 'tracked';
  const ahead = tree.remote.kind === 'tracked' ? tree.remote.ahead : 0;
  const behind = tree.remote.kind === 'tracked' ? tree.remote.behind : 0;
  const topMsg = orderedMessages(tree.messages)[0];
  const statusDetail = URGENT_RANK[sessionStatus] < 5 && topMsg?.title
    ? topMsg.title
    : undefined;

  return {
    repositoryId: tree.repositoryId,
    worktreeId: tree.worktreeId,
    name: repoName,
    branch: tree.branch,
    detached: Boolean(tree.detached),
    ahead,
    behind,
    hasUpstream,
    changeCount: normalChanges,
    localKeepCount,
    worktreeCount,
    active,
    ...(tree.readError ? { readError: tree.readError } : {}),
    sessionStatus,
    ...(statusDetail ? { statusDetail } : {}),
  };
}

export function buildTrayStateSnapshot(
  state: WorkbenchState,
  options: { hasUpdate: boolean; refreshing: boolean },
): TrayStateSnapshot {
  const repositories: TrayRepositorySummary[] = [];
  for (const repo of state.repositories) {
    const trees = state.worktrees.filter((tree) => tree.repositoryId === repo.id);
    if (trees.length === 0) continue;
    const activeTree = trees.find((tree) => tree.worktreeId === state.activeWorktreeId);
    let representative = activeTree ?? trees[0];
    let bestScore = worktreePriorityScore(representative, representative === activeTree);
    for (const candidate of trees) {
      const score = worktreePriorityScore(candidate, candidate === activeTree);
      if (score < bestScore) {
        bestScore = score;
        representative = candidate;
      }
    }
    repositories.push(
      summarizeWorktree(
        repo.name,
        trees.length,
        representative,
        representative.worktreeId === state.activeWorktreeId,
      ),
    );
  }
  return {
    protocolVersion: 1,
    locale: state.locale,
    hasUpdate: options.hasUpdate,
    refreshing: options.refreshing,
    repositories,
  };
}

function repoSortTuple(repo: TrayRepositorySummary): [number, number, number] {
  const urgentRank = URGENT_RANK[repo.sessionStatus];
  if (urgentRank < 5) {
    return [0, urgentRank, repo.active ? 0 : 1];
  }
  if (!isTrayRepoCleanIdle(repo)) {
    const subRank = repo.readError ? 0 : repo.detached ? 1 : repo.changeCount > 0 ? 2 : 3;
    return [1, repo.active ? 0 : 1, subRank];
  }
  return [2, repo.active ? 0 : 1, 0];
}

export function sortTrayRepositories(
  repositories: readonly TrayRepositorySummary[],
): TrayRepositorySummary[] {
  return repositories
    .map((repo, index) => ({ repo, index, key: repoSortTuple(repo) }))
    .sort((a, b) => {
      if (a.key[0] !== b.key[0]) return a.key[0] - b.key[0];
      if (a.key[1] !== b.key[1]) return a.key[1] - b.key[1];
      if (a.key[2] !== b.key[2]) return a.key[2] - b.key[2];
      return a.index - b.index;
    })
    .map((entry) => entry.repo);
}

export interface PartitionedTrayRepositories {
  primary: TrayRepositorySummary[];
  foldedClean: TrayRepositorySummary[];
}

/**
 * Sorts repositories by urgency and auto-folds completely clean idle workspaces into a single
 * collapsible row when more than `AUTO_FOLD_REPO_THRESHOLD` (5) repositories are open.
 */
export function partitionTrayRepositories(
  repositories: readonly TrayRepositorySummary[],
): PartitionedTrayRepositories {
  const sorted = sortTrayRepositories(repositories);
  if (sorted.length <= AUTO_FOLD_REPO_THRESHOLD) {
    return { primary: sorted, foldedClean: [] };
  }

  const nonClean: TrayRepositorySummary[] = [];
  const clean: TrayRepositorySummary[] = [];
  for (const repo of sorted) {
    if (isTrayRepoCleanIdle(repo)) {
      clean.push(repo);
    } else {
      nonClean.push(repo);
    }
  }

  // If every single repository is clean, keep the top (active/first) card visible so the popover
  // never collapses down to a lone disclosure bar.
  if (nonClean.length === 0) {
    if (clean.length <= 2) {
      return { primary: sorted, foldedClean: [] };
    }
    return { primary: [clean[0]], foldedClean: clean.slice(1) };
  }

  // Do not replace a single clean card with a disclosure row.
  if (clean.length < 2) {
    return { primary: sorted, foldedClean: [] };
  }

  return { primary: nonClean, foldedClean: clean };
}
