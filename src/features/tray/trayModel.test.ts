import { describe, expect, it } from 'vitest';
import type { TrayRepositorySummary } from '../../../shared/tray-protocol';
import { emptyWorkbench, type WorkbenchState } from '../workbench/state/workbench';
import {
  buildTrayStateSnapshot,
  partitionTrayRepositories,
  sortTrayRepositories,
} from './trayModel';

function makeRepo(
  name: string,
  overrides: Partial<TrayRepositorySummary> = {},
): TrayRepositorySummary {
  return {
    repositoryId: `repo:${name}`,
    worktreeId: `wt:${name}`,
    name,
    branch: 'main',
    detached: false,
    ahead: 0,
    behind: 0,
    hasUpstream: true,
    changeCount: 0,
    localKeepCount: 0,
    worktreeCount: 1,
    active: false,
    sessionStatus: 'idle',
    ...overrides,
  };
}

describe('tray menu bar state sorting and clean-workspace auto-folding', () => {
  it('orders urgent sessions before dirty/syncing repos and clean idle repos last', () => {
    const items = [
      makeRepo('clean-a'),
      makeRepo('dirty-b', { changeCount: 3 }),
      makeRepo('running-c', { sessionStatus: 'running', changeCount: 1 }),
      makeRepo('decision-d', { sessionStatus: 'needsDecision', changeCount: 2 }),
      makeRepo('active-dirty-e', { changeCount: 1, active: true }),
      makeRepo('conflict-f', { sessionStatus: 'conflict', changeCount: 1 }),
      makeRepo('ahead-g', { ahead: 2 }),
    ];

    const sorted = sortTrayRepositories(items).map((item) => item.name);
    expect(sorted).toEqual([
      'conflict-f',
      'decision-d',
      'running-c',
      'active-dirty-e',
      'dirty-b',
      'ahead-g',
      'clean-a',
    ]);
  });

  it('shows all repositories directly when 5 or fewer are open', () => {
    const items = [
      makeRepo('dirty-1', { changeCount: 2 }),
      makeRepo('clean-1'),
      makeRepo('clean-2'),
      makeRepo('clean-3'),
      makeRepo('clean-4'),
    ];
    const result = partitionTrayRepositories(items);
    expect(result.primary).toHaveLength(5);
    expect(result.foldedClean).toHaveLength(0);
  });

  it('auto-folds clean idle repositories into one collapsible group when more than 5 are open', () => {
    const items = [
      makeRepo('clean-1'),
      makeRepo('dirty-1', { changeCount: 4 }),
      makeRepo('clean-2'),
      makeRepo('decision-1', { sessionStatus: 'needsDecision', changeCount: 1 }),
      makeRepo('clean-3'),
      makeRepo('clean-4'),
    ];
    const result = partitionTrayRepositories(items);
    expect(result.primary.map((r) => r.name)).toEqual(['decision-1', 'dirty-1']);
    expect(result.foldedClean.map((r) => r.name)).toEqual([
      'clean-1',
      'clean-2',
      'clean-3',
      'clean-4',
    ]);
  });

  it('keeps the active repository visible and folds the rest when all 6+ repositories are clean', () => {
    const items = [
      makeRepo('clean-1'),
      makeRepo('clean-2', { active: true }),
      makeRepo('clean-3'),
      makeRepo('clean-4'),
      makeRepo('clean-5'),
      makeRepo('clean-6'),
    ];
    const result = partitionTrayRepositories(items);
    expect(result.primary.map((r) => r.name)).toEqual(['clean-2']);
    expect(result.foldedClean.map((r) => r.name)).toEqual([
      'clean-1',
      'clean-3',
      'clean-4',
      'clean-5',
      'clean-6',
    ]);
  });

  it('never replaces a single clean repository with a fold toggle', () => {
    const items = [
      makeRepo('dirty-1', { changeCount: 1 }),
      makeRepo('dirty-2', { changeCount: 2 }),
      makeRepo('dirty-3', { ahead: 1 }),
      makeRepo('dirty-4', { changeCount: 1 }),
      makeRepo('dirty-5', { behind: 2 }),
      makeRepo('clean-only'),
    ];
    const result = partitionTrayRepositories(items);
    expect(result.primary).toHaveLength(6);
    expect(result.foldedClean).toHaveLength(0);
  });

  it('builds a snapshot prioritizing urgent worktrees per repository and excluding local keep from normal change count', () => {
    const base: WorkbenchState = {
      ...emptyWorkbench(),
      locale: 'zh',
      activeWorktreeId: 'wt-main',
      repositories: [
        { id: 'repo-1', name: 'Picket', branches: [{ name: 'main', worktreeId: 'wt-main' }] },
      ],
      worktrees: [
        {
          repositoryId: 'repo-1',
          worktreeId: 'wt-main',
          sessionId: 's-main',
          scopeVersion: 'v1',
          label: 'main',
          path: '/tmp/picket',
          branch: 'main',
          remote: { kind: 'tracked', branch: 'origin/main', ahead: 1, behind: 0 },
          files: [
            { id: 'a.ts', path: 'a.ts', status: 'M', range: { kind: 'unstaged', added: 2, removed: 0 } },
            { id: 'local.env', path: 'local.env', status: 'M', localKeep: true, range: { kind: 'unstaged', added: 1, removed: 0 } },
          ],
          selectedIds: ['a.ts'],
          prompt: { text: '', revision: 0 },
          planRevision: 0,
          review: { status: 'idle', findings: [], skippedPaths: [], changedPaths: [] },
          sessionStatus: 'idle',
          messages: [],
          tab: 'commit',
          dockExpanded: false,
          replies: {},
          requests: {},
        },
        {
          repositoryId: 'repo-1',
          worktreeId: 'wt-feat',
          sessionId: 's-feat',
          scopeVersion: 'v2',
          label: 'feat/tray',
          path: '/tmp/picket-wt',
          branch: 'feat/tray',
          remote: { kind: 'missing', reason: 'none' },
          files: [
            { id: 'b.ts', path: 'b.ts', status: 'M', range: { kind: 'unstaged', added: 5, removed: 1 } },
          ],
          selectedIds: ['b.ts'],
          prompt: { text: '', revision: 0 },
          planRevision: 0,
          review: { status: 'idle', findings: [], skippedPaths: [], changedPaths: [] },
          sessionStatus: 'needsDecision',
          messages: [
            { id: 'm1', kind: 'question', title: 'Confirm .env exclusion', description: 'Check file' },
          ],
          tab: 'commit',
          dockExpanded: false,
          replies: {},
          requests: {},
        },
      ],
    };

    const snap = buildTrayStateSnapshot(base, { hasUpdate: true, refreshing: false });
    expect(snap.locale).toBe('zh');
    expect(snap.hasUpdate).toBe(true);
    expect(snap.repositories).toHaveLength(1);
    expect(snap.repositories[0]).toMatchObject({
      repositoryId: 'repo-1',
      worktreeId: 'wt-feat',
      branch: 'feat/tray',
      worktreeCount: 2,
      changeCount: 1,
      sessionStatus: 'needsDecision',
      statusDetail: 'Confirm .env exclusion',
    });
  });
});
