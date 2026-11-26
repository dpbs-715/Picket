import type { GitBranchAction } from '../../../../shared/git-branch-protocol';
import { useI18n } from '../../../i18n';
import { Icon, RotatingChevron } from '../../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../../ui/motion';
import { isProtected, type Repository, type WorkbenchState, type Worktree } from '../state/workbench';

export type BranchSection = 'local' | 'remote' | 'worktree';

interface CompactBranchesDrawerProps {
  state: WorkbenchState;
  repository?: Repository;
  worktree: Worktree;
  repoWorktrees: Worktree[];
  branchDisabled: boolean;
  expandedSections: Record<BranchSection, boolean>;
  onToggleSection(section: BranchSection): void;
  onCloseDrawer(): void;
  onSwitchWorktree(worktreeId: string): void;
  onSubmitBranch(kind: GitBranchAction, branch: string): Promise<boolean>;
}

export function CompactBranchesDrawer({
  state,
  repository,
  worktree,
  repoWorktrees,
  branchDisabled,
  expandedSections,
  onToggleSection,
  onCloseDrawer,
  onSwitchWorktree,
  onSubmitBranch,
}: CompactBranchesDrawerProps) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();
  const remoteBranches = repository?.remoteBranches ?? [];

  return (
    <m.div
      key="drawer-branches"
      className="compact-drawer compact-branches-drawer scroll-area"
      role="region"
      aria-label={t.header.branchesTitle}
      initial={instantMotion ? false : { opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
    >
      <div className="branch-group-section">
        <button
          type="button"
          className="branch-group-toggle"
          aria-expanded={expandedSections.local}
          onClick={() => onToggleSection('local')}
        >
          <span className="row"><RotatingChevron open={expandedSections.local} /><span>{t.header.localBranches}</span></span>
          <span className="branch-group-count muted small">{repository?.branches.length ?? 0}</span>
        </button>
        {expandedSections.local && (
          <div className="branch-group-list" data-branch-group="local">
            {repository?.branches.length ? repository.branches.map((branch) => (
              <button
                type="button"
                className="branch-item"
                key={branch.name}
                disabled={!branch.worktreeId && branchDisabled}
                aria-pressed={!worktree.detached && branch.name === worktree.branch}
                onClick={() => {
                  if (branch.worktreeId) {
                    onCloseDrawer();
                    onSwitchWorktree(branch.worktreeId);
                  } else {
                    void onSubmitBranch('switchBranch', branch.name);
                  }
                }}
              >
                <span className="row min-width-zero branch-item-main">
                  <span className="mono truncate">{branch.name}</span>
                  {isProtected(branch.name) && <span className="branch-tag">{t.common.protectedBranch}</span>}
                </span>
                {branch.worktreeId && branch.worktreeId !== worktree.worktreeId ? (
                  <span className="branch-item-meta truncate">
                    {t.header.jumpToWorktree(state.worktrees.find((tree) => tree.worktreeId === branch.worktreeId)?.label ?? '')}
                  </span>
                ) : (
                  !worktree.detached && branch.name === worktree.branch && <Icon name="check" />
                )}
              </button>
            )) : (
              <p className="muted menu-note">{t.header.noLocalBranches}</p>
            )}
          </div>
        )}
      </div>
      <div className="branch-group-section">
        <button
          type="button"
          className="branch-group-toggle"
          aria-expanded={expandedSections.remote}
          onClick={() => onToggleSection('remote')}
        >
          <span className="row"><RotatingChevron open={expandedSections.remote} /><span>{t.header.remoteBranches}</span></span>
          <span className="branch-group-count muted small">{remoteBranches.length}</span>
        </button>
        {expandedSections.remote && (
          <div className="branch-group-list" data-branch-group="remote">
            {remoteBranches.length ? remoteBranches.map((remoteBranch) => {
              const existingLocal = repository?.branches.find((item) => item.name === remoteBranch.localName);
              const isCurrent = !worktree.detached && worktree.branch === remoteBranch.localName;
              return (
                <button
                  type="button"
                  className="branch-item"
                  key={remoteBranch.name}
                  disabled={!existingLocal?.worktreeId && branchDisabled}
                  aria-pressed={isCurrent}
                  onClick={() => {
                    if (existingLocal?.worktreeId) {
                      onCloseDrawer();
                      onSwitchWorktree(existingLocal.worktreeId);
                    } else if (existingLocal) {
                      void onSubmitBranch('switchBranch', existingLocal.name);
                    } else {
                      void onSubmitBranch('checkoutRemoteBranch', remoteBranch.name);
                    }
                  }}
                >
                  <span className="row min-width-zero branch-item-main">
                    <span className="mono truncate">{remoteBranch.name}</span>
                  </span>
                  {existingLocal?.worktreeId && existingLocal.worktreeId !== worktree.worktreeId ? (
                    <span className="branch-item-meta truncate">
                      {t.header.jumpToWorktree(state.worktrees.find((tree) => tree.worktreeId === existingLocal.worktreeId)?.label ?? '')}
                    </span>
                  ) : isCurrent ? (
                    <Icon name="check" />
                  ) : (
                    <span className="branch-item-meta branch-item-hover">
                      {existingLocal ? t.header.switchAction : t.header.checkoutAction}
                    </span>
                  )}
                </button>
              );
            }) : (
              <p className="muted menu-note">{t.header.noRemoteBranches}</p>
            )}
          </div>
        )}
      </div>
      {repoWorktrees.length > 1 && (
        <div className="branch-group-section">
          <button
            type="button"
            className="branch-group-toggle"
            aria-expanded={expandedSections.worktree}
            onClick={() => onToggleSection('worktree')}
          >
            <span className="row"><RotatingChevron open={expandedSections.worktree} /><span>{t.header.worktrees}</span></span>
            <span className="branch-group-count muted small">{repoWorktrees.length}</span>
          </button>
          {expandedSections.worktree && (
            <div className="branch-group-list" data-branch-group="worktree">
              {repoWorktrees.map((tree) => (
                <button
                  type="button"
                  className="branch-item"
                  key={tree.worktreeId}
                  aria-pressed={tree.worktreeId === worktree.worktreeId}
                  onClick={() => {
                    onCloseDrawer();
                    onSwitchWorktree(tree.worktreeId);
                  }}
                >
                  <span className="row min-width-zero branch-item-main">
                    <span className={`status-dot status-${tree.sessionStatus}`} />
                    <span className="mono truncate">{tree.label}</span>
                    <span className="mono muted small truncate">{tree.detached ? t.common.detachedHead : tree.branch}</span>
                  </span>
                  <span className="branch-item-meta">
                    {tree.readError ? t.header.worktreeUnavailable : tree.files.length ? t.header.worktreeChangesCount(tree.files.length) : t.header.worktreeClean}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </m.div>
  );
}
