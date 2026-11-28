import { useState } from 'react';
import type { GitBranchAction } from '../../../../shared/git-branch-protocol';
import { useI18n } from '../../../i18n';
import { HelpTip, Icon, IconButton, Popover, RotatingChevron, handleDragRegionDoubleClick } from '../../../ui/controls';
import { Collapse, MotionPresence } from '../../../ui/motion';
import { isActiveSession, isProtected, orderedMessages } from '../state/workbench';
import type { Repository, WorkbenchState, Worktree } from '../state/workbench';

export type WorkbenchMainView = 'git' | 'run';

type Menu = 'repository' | 'history' | 'branch' | 'remote' | null;
type BranchSection = 'local' | 'remote' | 'worktree';

interface Props {
  state: WorkbenchState;
  worktree?: Worktree;
  repository?: Repository;
  activeMainView?: WorkbenchMainView;
  activeRunCount?: number;
  pinned: boolean;
  hasUpdate?: boolean;
  opening: boolean;
  branchBusy: boolean;
  pullBusy?: boolean;
  pinBusy: boolean;
  onCollapse(): void;
  onPin(): void;
  onSettings(): void;
  onOpenRepository(): void;
  onSwitch(worktreeId: string, expand?: boolean): void;
  onBranch(kind: GitBranchAction, branch: string): Promise<boolean>;
  onPull(): void;
  onSelectMainView?(view: WorkbenchMainView): void;
}

export function ContextHeader(props: Props) {
  const { t } = useI18n();
  const [menu, setMenu] = useState<Menu>(null);
  const [expandedSections, setExpandedSections] = useState<Record<BranchSection, boolean>>({
    local: true,
    remote: false,
    worktree: false,
  });
  const { state, worktree, repository } = props;
  const repoWorktrees = state.worktrees.filter((tree) => tree.repositoryId === repository?.id);
  const remoteBranches = repository?.remoteBranches ?? [];
  const pending = state.worktrees.some(isActiveSession);
  const branchDisabled = props.branchBusy || !!worktree?.readError || repoWorktrees.some(isActiveSession);
  const activeView: WorkbenchMainView = props.activeMainView ?? 'git';
  const activeRunCount = props.activeRunCount ?? 0;

  async function submitBranch(kind: GitBranchAction, branch: string) {
    const accepted = await props.onBranch(kind, branch);
    if (accepted) setMenu(null);
    return accepted;
  }
  function toggle(next: Menu) { setMenu(menu === next ? null : next); }
  function toggleSection(section: BranchSection) {
    setExpandedSections((previous) => ({ ...previous, [section]: !previous[section] }));
  }
  function switchTree(id: string, expand = false) { props.onSwitch(id, expand); setMenu(null); }
  const close = () => setMenu(null);

  return <header className="context-header">
    <div
      className="context-row context-top"
      data-tauri-drag-region
      onMouseDown={(event) => handleDragRegionDoubleClick(event, () => {
        if (props.pinBusy) return;
        close();
        props.onCollapse();
      })}
    >
      <div className="menu-anchor repository-anchor" data-tauri-drag-region>
        <button type="button" className="context-button repository-button" disabled={props.opening} data-popover-trigger aria-expanded={menu === 'repository'} onClick={() => toggle('repository')}>
          <span className="mono truncate">{repository?.name ?? (props.opening ? t.header.opening : t.header.openRepository)}</span><RotatingChevron open={menu === 'repository'} direction="down-up" />
        </button>
        <MotionPresence>
          {menu === 'repository' && <Popover key="repo-popover" title={t.header.repositoriesTitle} onClose={close} className="repository-menu">
            {state.repositories.map((repo) => {
              const trees = state.worktrees.filter((tree) => tree.repositoryId === repo.id);
              return <button type="button" key={repo.id} aria-pressed={repo.id === repository?.id} onClick={() => { if (trees[0]) switchTree(trees[0].worktreeId); }}>
                <span className="mono truncate">{repo.name}</span><span className="muted small">{trees.length <= 1 ? t.header.singleWorktree : t.header.worktreeCount(trees.length)}</span>
              </button>;
            })}
            {!state.repositories.length && <p className="muted menu-note">{t.header.noRepositoriesOpened}</p>}
            <div className="menu-divider"><button type="button" disabled={props.opening} onClick={() => { close(); props.onOpenRepository(); }}><Icon name="plus" />{t.header.openLocalRepository}</button></div>
          </Popover>}
        </MotionPresence>
      </div>
      <div className="row header-tools">
        <div className="menu-anchor"><IconButton icon="history" label={t.header.sessionHistory} data-popover-trigger aria-expanded={menu === 'history'} onClick={() => toggle('history')} className={pending ? 'has-pending' : ''} />
          <MotionPresence>
            {menu === 'history' && <Popover key="history-popover" title={t.header.sessionHistory} onClose={close} className="history-menu">
              <div className="menu-title">{t.header.sessionHistory} <span className="muted small">{t.header.byWorktree}</span></div>
              {!state.worktrees.length && <p className="muted menu-note">{t.header.noSessionHistory}</p>}
              {state.worktrees.map((tree) => {
                const message = orderedMessages(tree.messages)[0];
                return <button type="button" className="history-entry" key={tree.worktreeId} onClick={() => switchTree(tree.worktreeId, !!message && isActiveSession(tree))}>
                  <span className="row"><span className={`status-dot status-${tree.sessionStatus}`} /><span className="mono truncate">{tree.label}</span><span className="mono muted small truncate">{tree.branch}</span><span className="small muted">{message ? t.messageLabels[message.kind] : t.header.idle}</span></span>
                  <span className="muted small truncate">{message?.title ?? t.header.noActiveSession}</span>
                </button>;
              })}
            </Popover>}
          </MotionPresence>
        </div>
        <IconButton
          icon="collapse"
          label={t.header.collapseWindow}
          disabled={props.pinBusy}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => { close(); props.onCollapse(); }}
        />
        <IconButton icon="pin" className="pin-button" label={props.pinned ? t.header.unpinWindow : t.header.pinWindow} aria-pressed={props.pinned} disabled={props.pinBusy} onMouseDown={(event) => event.preventDefault()} onClick={props.onPin} />
        <IconButton icon="settings" label={t.header.settings} className={props.hasUpdate ? 'has-update' : ''} data-has-update={props.hasUpdate ? 'true' : undefined} onClick={() => { close(); props.onSettings(); }} />
      </div>
    </div>
    <div className="context-row context-bottom">
      <div className="row context-path">
        <div className="menu-anchor branch-anchor">
          <button type="button" className="context-button" disabled={!worktree} data-popover-trigger aria-expanded={menu === 'branch'} onClick={() => toggle('branch')}>
            {worktree && <span className={`status-dot status-${worktree.sessionStatus}`} />}
            <Icon name="branch" />
            <span className="mono truncate">{worktree?.detached ? t.common.detachedHead : worktree?.branch ?? t.header.noBranchSelected}</span>
            {worktree && <RotatingChevron open={menu === 'branch'} direction="down-up" />}
          </button>
          <MotionPresence>
            {menu === 'branch' && worktree && <Popover key="branch-popover" title={t.header.branchesTitle} onClose={close} className="branch-menu">
              <div className="branch-group-section">
                <button type="button" className="branch-group-toggle" aria-expanded={expandedSections.local} onClick={() => toggleSection('local')}>
                  <span className="row"><RotatingChevron open={expandedSections.local} /><span>{t.header.localBranches}</span></span>
                  <span className="branch-group-count muted small">{repository?.branches.length ?? 0}</span>
                </button>
                <Collapse open={expandedSections.local}>
                  <div className="branch-group-list" data-branch-group="local">
                    {repository?.branches.length ? repository.branches.map((branch) => <button type="button" className="branch-item" key={branch.name} disabled={!branch.worktreeId && branchDisabled} aria-pressed={!worktree.detached && branch.name === worktree.branch} onClick={() => {
                      if (branch.worktreeId) switchTree(branch.worktreeId);
                      else { void submitBranch('switchBranch', branch.name); }
                    }}><span className="row min-width-zero branch-item-main"><span className="mono truncate">{branch.name}</span>{isProtected(branch.name) && <span className="branch-tag">{t.common.protectedBranch}</span>}</span>{branch.worktreeId && branch.worktreeId !== worktree.worktreeId ? <span className="branch-item-meta truncate">{t.header.jumpToWorktree(state.worktrees.find((tree) => tree.worktreeId === branch.worktreeId)?.label ?? '')}</span> : !worktree.detached && branch.name === worktree.branch && <Icon name="check" />}</button>)
                      : <p className="muted menu-note">{t.header.noLocalBranches}</p>}
                  </div>
                </Collapse>
              </div>
              <div className="branch-group-section">
                <button type="button" className="branch-group-toggle" aria-expanded={expandedSections.remote} onClick={() => toggleSection('remote')}>
                  <span className="row"><RotatingChevron open={expandedSections.remote} /><span>{t.header.remoteBranches}</span></span>
                  <span className="branch-group-count muted small">{remoteBranches.length}</span>
                </button>
                <Collapse open={expandedSections.remote}>
                  <div className="branch-group-list" data-branch-group="remote">
                    {remoteBranches.length ? remoteBranches.map((remoteBranch) => {
                      const existingLocal = repository?.branches.find((item) => item.name === remoteBranch.localName);
                      const isCurrent = !worktree.detached && worktree.branch === remoteBranch.localName;
                      return <button type="button" className="branch-item" key={remoteBranch.name} disabled={!existingLocal?.worktreeId && branchDisabled} aria-pressed={isCurrent} onClick={() => {
                        if (existingLocal?.worktreeId) switchTree(existingLocal.worktreeId);
                        else if (existingLocal) { void submitBranch('switchBranch', existingLocal.name); }
                        else { void submitBranch('checkoutRemoteBranch', remoteBranch.name); }
                      }}>
                        <span className="row min-width-zero branch-item-main"><span className="mono truncate">{remoteBranch.name}</span></span>
                        {existingLocal?.worktreeId && existingLocal.worktreeId !== worktree.worktreeId
                          ? <span className="branch-item-meta truncate">{t.header.jumpToWorktree(state.worktrees.find((tree) => tree.worktreeId === existingLocal.worktreeId)?.label ?? '')}</span>
                          : isCurrent
                            ? <Icon name="check" />
                            : <span className="branch-item-meta branch-item-hover">{existingLocal ? t.header.switchAction : t.header.checkoutAction}</span>}
                      </button>;
                    }) : <p className="muted menu-note">{t.header.noRemoteBranches}</p>}
                  </div>
                </Collapse>
              </div>
              {repoWorktrees.length > 1 && <div className="branch-group-section">
                <button type="button" className="branch-group-toggle" aria-expanded={expandedSections.worktree} onClick={() => toggleSection('worktree')}>
                  <span className="row"><RotatingChevron open={expandedSections.worktree} /><span>{t.header.worktrees}</span></span>
                  <span className="branch-group-count muted small">{repoWorktrees.length}</span>
                </button>
                <Collapse open={expandedSections.worktree}>
                  <div className="branch-group-list" data-branch-group="worktree">
                    {repoWorktrees.map((tree) => <button type="button" className="branch-item" key={tree.worktreeId} aria-pressed={tree.worktreeId === worktree.worktreeId} onClick={() => switchTree(tree.worktreeId)}>
                      <span className="row min-width-zero branch-item-main"><span className={`status-dot status-${tree.sessionStatus}`} /><span className="mono truncate">{tree.label}</span><span className="mono muted small truncate">{tree.detached ? t.common.detachedHead : tree.branch}</span></span><span className="branch-item-meta">{tree.readError ? t.header.worktreeUnavailable : tree.files.length ? t.header.worktreeChangesCount(tree.files.length) : t.header.worktreeClean}</span>
                    </button>)}
                  </div>
                </Collapse>
              </div>}
            </Popover>}
          </MotionPresence>
        </div>
        <div className="menu-anchor remote-anchor">
          <button
            type="button"
            className={`context-button remote-button mono ${worktree?.remote.kind === 'tracked' && worktree.remote.behind ? 'warning' : ''}`}
            disabled={!worktree}
            data-popover-trigger
            aria-expanded={menu === 'remote'}
            onClick={() => toggle('remote')}
            aria-label={t.header.remoteDetails}
          >
            {worktree?.remote.kind === 'tracked' ? <><span>↑ {worktree.remote.ahead}</span><span>↓ {worktree.remote.behind}</span></> : t.header.untracked}
          </button>
          <MotionPresence>
            {menu === 'remote' && worktree && <Popover key="remote-popover" title={t.header.remoteDetails} onClose={close} className="remote-menu remote-menu-inline"><div className="menu-title muted"><span>{t.header.trackedRemoteBranch}</span>{worktree.remote.kind === 'tracked' && worktree.remote.localOnly && <HelpTip placement="bottom" align="end" label={t.header.remoteCountTipLabel} text={t.header.remoteCountTipText} />}</div>
              {worktree.remote.kind === 'tracked' ? <><p className="mono">{worktree.remote.branch}</p><p className="muted small">{t.header.aheadBehind(worktree.remote.ahead, worktree.remote.behind)}</p></> : <p className="warning help">{worktree.remote.reason}</p>}
              <div className="menu-divider"><button type="button" className="secondary" disabled={props.pullBusy || repoWorktrees.some(isActiveSession)} onClick={() => { close(); props.onPull(); }}>{props.pullBusy ? t.header.syncing : t.header.pullAndRebase}</button></div>
            </Popover>}
          </MotionPresence>
        </div>
      </div>
      {worktree && (
        <div className="main-view-switch" role="group" aria-label={t.run.viewSwitchLabel}>
          <button
            type="button"
            className={`main-view-tab ${activeView === 'git' ? 'is-active' : ''}`}
            aria-pressed={activeView === 'git'}
            data-main-view="git"
            onClick={() => { close(); props.onSelectMainView?.('git'); }}
          >
            {t.run.gitTabLabel}
          </button>
          <button
            type="button"
            className={`main-view-tab ${activeView === 'run' ? 'is-active' : ''} ${activeRunCount > 0 ? 'has-running' : ''}`}
            aria-pressed={activeView === 'run'}
            data-main-view="run"
            data-run-trigger
            title={activeRunCount > 0 ? t.run.runningBadgeTitle(activeRunCount) : t.run.runTabLabel}
            onClick={() => { close(); props.onSelectMainView?.('run'); }}
          >
            <span>{t.run.runTabLabel}</span>
            {activeRunCount > 0 && (
              <span className="main-view-run-pill" aria-hidden="true">
                <span className="main-view-run-dot" />
                <span>{activeRunCount}</span>
              </span>
            )}
          </button>
        </div>
      )}
    </div>
  </header>;
}
