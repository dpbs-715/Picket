import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CustomActionConfig } from '../../../../shared/custom-action-protocol';
import type { GitBranchAction } from '../../../../shared/git-branch-protocol';
import { WINDOW_DIMENSIONS } from '../../../../shared/workbench-persistence-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, PresenceNotice, m, useInstantMotion } from '../../../ui/motion';
import {
  compactStatusSummary,
  hasEmptyValidPlanMessage,
  isActiveSession,
  isPlanOutdated,
  isProtected,
  localKeepChanges,
  normalChanges,
  primaryAction,
  selectedFiles,
  selectedLocalKeepChanges,
  splitPath,
  type CompactExpandTarget,
  type Repository,
  type WorkbenchState,
  type Worktree,
} from '../state/workbench';

interface CompactNotice {
  text: string;
  tone?: 'info' | 'success' | 'warning' | 'error';
  showModelSettings?: boolean;
}

type CompactDrawer = 'none' | 'projects' | 'branches' | 'remote' | 'files' | 'actions';
type BranchSection = 'local' | 'remote' | 'worktree';

interface Props {
  state: WorkbenchState;
  repository?: Repository;
  worktree?: Worktree;
  hasModel: boolean;
  customActions: CustomActionConfig[];
  activeAction?: CustomActionConfig;
  opening: boolean;
  branchBusy: boolean;
  pullBusy: boolean;
  busy: boolean;
  pinned: boolean;
  pinBusy: boolean;
  hasUpdate?: boolean;
  repositoryReadsAvailable: boolean;
  notice?: CompactNotice;
  onSwitchWorktree(worktreeId: string): void;
  onOpenRepository(): void;
  onBranch(kind: GitBranchAction, branch: string): Promise<boolean>;
  onPull(): void;
  onSelect(fileId: string): void;
  onSelectAll(): void;
  onSelectAction(actionId: string | null): void;
  onNewAction(mode: 'manual' | 'create'): void;
  onManageActions(): void;
  onExpand(): void;
  onExpandTarget(target: CompactExpandTarget): void;
  onPin(): void;
  onSettings(drawerWasOpen?: boolean): void;
  onPrimary(): void;
  onConfigureModel(): void;
  onOpenModelSettings(): void;
  onDismissNotice(): void;
  onDesiredHeightChange?(height: number, drawerOpen: boolean): void;
}

export function CompactBar(props: Props) {
  const { locale, t } = useI18n();
  const instantMotion = useInstantMotion();
  const barRef = useRef<HTMLDivElement>(null);
  const selectAllRef = useRef<HTMLInputElement>(null);
  const [drawer, setDrawer] = useState<CompactDrawer>('none');
  const [localKeepOpen, setLocalKeepOpen] = useState(false);
  const [expandedSections, setExpandedSections] = useState<Record<BranchSection, boolean>>({
    local: true,
    remote: false,
    worktree: false,
  });

  const { state, repository, worktree } = props;
  const repoLabel = repository?.name ?? (props.opening ? t.header.opening : t.header.openRepository);
  const branchLabel = worktree ? (worktree.detached ? t.common.detachedHead : worktree.branch) : '';
  const trackedRemote = worktree?.remote.kind === 'tracked' ? worktree.remote : undefined;
  const repoWorktrees = state.worktrees.filter((tree) => tree.repositoryId === repository?.id);
  const remoteBranches = repository?.remoteBranches ?? [];
  const branchDisabled = props.branchBusy || !!worktree?.readError || repoWorktrees.some(isActiveSession);

  const canShowSelectionTrigger = !!worktree
    && !isActiveSession(worktree)
    && !worktree.requests.plan
    && !worktree.readError
    && !worktree.detached
    && worktree.files.length > 0
    && !hasEmptyValidPlanMessage(worktree);

  const effectiveDrawer: CompactDrawer =
    (drawer === 'files' && !canShowSelectionTrigger)
    || ((drawer === 'branches' || drawer === 'remote' || drawer === 'actions') && !worktree)
      ? 'none'
      : drawer;

  const normalFiles = worktree ? normalChanges(worktree) : [];
  const keptFiles = worktree ? localKeepChanges(worktree) : [];
  const selectedKeptFiles = worktree ? selectedLocalKeepChanges(worktree) : [];
  const selectedList = worktree ? selectedFiles(worktree) : [];
  const selectedNormalCount = normalFiles.filter((file) => worktree?.selectedIds.includes(file.id)).length;
  const allNormalSelected = normalFiles.length > 0 && selectedNormalCount === normalFiles.length;
  const selectAllIndeterminate = selectedNormalCount > 0 && selectedNormalCount < normalFiles.length;
  const totalSelectable = normalFiles.length + selectedKeptFiles.length;
  const planOutdated = worktree ? isPlanOutdated(worktree) : false;

  function toggleSection(section: BranchSection) {
    setExpandedSections((previous) => ({ ...previous, [section]: !previous[section] }));
  }

  async function submitBranch(kind: GitBranchAction, branch: string) {
    const accepted = await props.onBranch(kind, branch);
    if (accepted) setDrawer('none');
    return accepted;
  }

  useEffect(() => {
    if (selectAllRef.current) {
      selectAllRef.current.indeterminate = selectAllIndeterminate;
    }
  }, [selectAllIndeterminate, allNormalSelected, canShowSelectionTrigger]);

  useEffect(() => {
    if (effectiveDrawer === 'none') return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        setDrawer('none');
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [effectiveDrawer]);

  const status = compactStatusSummary(worktree, props.hasModel, locale);
  const action = primaryAction(worktree, props.hasModel, locale, props.activeAction);
  const showSplitToggle = Boolean(props.hasModel && worktree && !action.resume && !action.configureModel);
  const primaryDisabled = action.disabled
    || (!!worktree?.detached && !props.activeAction && !action.resume && !action.configureModel)
    || (props.busy && !action.resume && !action.configureModel);

  const primaryIcon = action.configureModel
    ? 'settings'
    : action.resume
      ? worktree?.sessionStatus === 'pushFailed' || worktree?.sessionStatus === 'conflict'
        ? 'alert'
        : 'history'
      : props.activeAction
        ? 'sparkles'
        : 'upload';

  useLayoutEffect(() => {
    const bar = barRef.current;
    if (bar && props.onDesiredHeightChange) {
      const measured = Math.ceil(bar.scrollHeight);
      const clamped = Math.min(
        WINDOW_DIMENSIONS.compactMaxHeight,
        Math.max(WINDOW_DIMENSIONS.compactMinHeight, measured),
      );
      props.onDesiredHeightChange(clamped, effectiveDrawer !== 'none');
    }
  }, [
    effectiveDrawer,
    expandedSections.local,
    expandedSections.remote,
    expandedSections.worktree,
    keptFiles.length,
    localKeepOpen,
    normalFiles.length,
    props.customActions.length,
    props.activeAction?.id,
    props.notice?.text,
    props.onDesiredHeightChange,
    remoteBranches.length,
    repoWorktrees.length,
    repository?.branches.length,
    selectedKeptFiles.length,
    state.repositories.length,
    worktree?.remote.kind,
  ]);

  const statusIndicator = status.tone === 'running'
    ? <Icon name="loader" className="compact-status-spinner spinning" />
    : <span className="status-dot" />;

  return (
    <div className="compact-bar" ref={barRef} role="region" aria-label={t.compact.barLabel}>
      <header className="compact-top" data-tauri-drag-region>
        <div className="compact-context" data-tauri-drag-region>
          <button
            type="button"
            className="compact-repo-button"
            disabled={props.opening}
            aria-expanded={effectiveDrawer === 'projects'}
            aria-label={`${repoLabel} · ${t.compact.switchProject}`}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => setDrawer(effectiveDrawer === 'projects' ? 'none' : 'projects')}
            title={repoLabel}
          >
            <span className="compact-repo mono truncate">{repoLabel}</span>
            <RotatingChevron open={effectiveDrawer === 'projects'} direction="down-up" className="compact-chevron" />
          </button>
          {worktree && (
            <>
              <span className="compact-sep muted" data-tauri-drag-region aria-hidden="true">/</span>
              <button
                type="button"
                className="compact-branch-button"
                aria-expanded={effectiveDrawer === 'branches'}
                aria-label={t.header.branchesTitle}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => setDrawer(effectiveDrawer === 'branches' ? 'none' : 'branches')}
                title={branchLabel}
              >
                <span className="compact-branch mono truncate">{branchLabel}</span>
                <RotatingChevron open={effectiveDrawer === 'branches'} direction="down-up" className="compact-chevron" />
              </button>
            </>
          )}
          <div className="compact-drag-spacer" data-tauri-drag-region />
        </div>
        <div className="row compact-tools">
          {worktree && (
            <button
              type="button"
              className={`compact-remote-button mono ${trackedRemote?.behind ? 'warning' : trackedRemote ? '' : 'muted'}`}
              aria-label={t.header.remoteDetails}
              aria-expanded={effectiveDrawer === 'remote'}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => setDrawer(effectiveDrawer === 'remote' ? 'none' : 'remote')}
              title={
                trackedRemote
                  ? `${trackedRemote.branch} · ${t.header.aheadBehind(trackedRemote.ahead, trackedRemote.behind)}`
                  : t.header.untracked
              }
            >
              {trackedRemote ? (
                <>
                  <span>↑{trackedRemote.ahead}</span>{' '}
                  <span>↓{trackedRemote.behind}</span>
                </>
              ) : (
                <span className="truncate">{t.header.untracked}</span>
              )}
            </button>
          )}
          <IconButton
            icon="expand"
            label={t.compact.expandWindow}
            disabled={props.pinBusy}
            onMouseDown={(event) => event.preventDefault()}
            onClick={props.onExpand}
          />
          <IconButton
            icon="pin"
            className="pin-button"
            label={props.pinned ? t.header.unpinWindow : t.header.pinWindow}
            aria-pressed={props.pinned}
            disabled={props.pinBusy}
            onMouseDown={(event) => event.preventDefault()}
            onClick={props.onPin}
          />
          <IconButton
            icon="settings"
            label={t.header.settings}
            className={props.hasUpdate ? 'has-update' : ''}
            data-has-update={props.hasUpdate ? 'true' : undefined}
            disabled={props.pinBusy}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => props.onSettings(effectiveDrawer !== 'none')}
          />
        </div>
      </header>

      <PresenceNotice show={Boolean(props.notice?.text)} motionKey={`compact-notice:${props.notice?.text ?? ''}`}>
        {props.notice?.text && (() => {
          const tone = props.notice.tone ?? 'info';
          const noticeIcon = tone === 'success' ? 'check' : tone === 'info' ? 'help' : 'alert';
          return (
            <div
              className={`operation-notice compact-notice operation-notice-${tone}`}
              data-notice-tone={tone}
              role={tone === 'error' || tone === 'warning' ? 'alert' : 'status'}
            >
              <span className="operation-notice-body">
                <Icon name={noticeIcon} className="operation-notice-icon" />
                <span className="truncate" title={props.notice.text}>{props.notice.text}</span>
              </span>
              <div className="operation-notice-actions">
                {props.notice.showModelSettings && (
                  <button type="button" className="secondary operation-notice-action" onClick={props.onOpenModelSettings}>
                    {t.workbench.openModelSettingsAction}
                  </button>
                )}
                <IconButton icon="close" label={t.common.dismissNotice} onClick={props.onDismissNotice} />
              </div>
            </div>
          );
        })()}
      </PresenceNotice>

      {effectiveDrawer === 'projects' && (
        <m.div
          key="drawer-projects"
          className="compact-drawer scroll-area"
          role="region"
          aria-label={t.compact.projectDrawerLabel}
          initial={instantMotion ? false : { opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
        >
          {state.repositories.map((repo) => {
            const trees = state.worktrees.filter((tree) => tree.repositoryId === repo.id);
            const active = repo.id === repository?.id;
            return (
              <button
                type="button"
                className="compact-project-item"
                key={repo.id}
                aria-pressed={active}
                onClick={() => {
                  setDrawer('none');
                  if (trees[0]) props.onSwitchWorktree(trees[0].worktreeId);
                }}
              >
                <span className="mono truncate">{repo.name}</span>
                <span className="muted small">
                  {trees.length <= 1 ? t.header.singleWorktree : t.header.worktreeCount(trees.length)}
                </span>
              </button>
            );
          })}
          {!state.repositories.length && (
            <p className="muted menu-note">{t.header.noRepositoriesOpened}</p>
          )}
          <div className="menu-divider">
            <button
              type="button"
              className="compact-open-repo-item"
              disabled={props.opening}
              onClick={() => {
                setDrawer('none');
                if (props.repositoryReadsAvailable) {
                  props.onOpenRepository();
                } else {
                  props.onExpandTarget('repo');
                }
              }}
            >
              <Icon name="plus" />
              {t.header.openLocalRepository}
            </button>
          </div>
        </m.div>
      )}

      {effectiveDrawer === 'branches' && worktree && (
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
              onClick={() => toggleSection('local')}
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
                        setDrawer('none');
                        props.onSwitchWorktree(branch.worktreeId);
                      } else {
                        void submitBranch('switchBranch', branch.name);
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
              onClick={() => toggleSection('remote')}
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
                          setDrawer('none');
                          props.onSwitchWorktree(existingLocal.worktreeId);
                        } else if (existingLocal) {
                          void submitBranch('switchBranch', existingLocal.name);
                        } else {
                          void submitBranch('checkoutRemoteBranch', remoteBranch.name);
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
                onClick={() => toggleSection('worktree')}
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
                        setDrawer('none');
                        props.onSwitchWorktree(tree.worktreeId);
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
      )}

      {effectiveDrawer === 'remote' && worktree && (
        <m.div
          key="drawer-remote"
          className="compact-drawer compact-remote-drawer"
          role="region"
          aria-label={t.header.remoteDetails}
          initial={instantMotion ? false : { opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
        >
          <div className="compact-remote-info">
            <span className="muted small">{t.header.trackedRemoteBranch}</span>
            {worktree.remote.kind === 'tracked' ? (
              <>
                <div className="compact-remote-branch-row">
                  <span className="mono truncate">{worktree.remote.branch}</span>
                  <span className="muted small">{t.header.aheadBehind(worktree.remote.ahead, worktree.remote.behind)}</span>
                </div>
                {worktree.remote.localOnly && (
                  <p className="muted small compact-remote-tip">{t.header.remoteCountTipText}</p>
                )}
              </>
            ) : (
              <p className="warning help">{worktree.remote.reason}</p>
            )}
          </div>
          <div className="menu-divider">
            <button
              type="button"
              className="secondary compact-pull-button"
              disabled={props.pullBusy || repoWorktrees.some(isActiveSession)}
              onClick={() => {
                setDrawer('none');
                props.onPull();
              }}
            >
              {props.pullBusy ? t.header.syncing : t.header.pullAndRebase}
            </button>
          </div>
        </m.div>
      )}

      {effectiveDrawer === 'files' && worktree && (
        <m.div
          key="drawer-files"
          className="compact-drawer compact-files-drawer scroll-area"
          role="region"
          aria-label={t.compact.filesDrawerLabel}
          initial={instantMotion ? false : { opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
        >
          {normalFiles.length === 0 && (
            <p className="muted compact-files-empty">{t.changes.noNormalChanges}</p>
          )}
          {normalFiles.map((file) => {
            const { directory, name } = splitPath(file.path);
            const checked = worktree.selectedIds.includes(file.id);
            return (
              <label className="compact-file-row" key={file.id} data-file-path={file.path} title={file.path}>
                <input
                  type="checkbox"
                  className="file-checkbox"
                  checked={checked}
                  aria-label={t.changes.selectFile(file.path)}
                  onChange={() => props.onSelect(file.id)}
                />
                <span className={`git-status git-${file.status}`} aria-hidden="true">{file.status}</span>
                <span className="file-name mono truncate">{name}</span>
                {directory && <span className="file-directory mono truncate">{directory}</span>}
                {file.range.kind === 'partial' && (
                  <span className="accent file-badge">{t.changes.partialStagedBadge}</span>
                )}
              </label>
            );
          })}
          {keptFiles.length > 0 && (
            <div className="compact-keep-group">
              <button
                type="button"
                className="compact-keep-toggle"
                aria-expanded={localKeepOpen}
                onClick={() => setLocalKeepOpen((prev) => !prev)}
              >
                <span className="row min-width-zero">
                  <RotatingChevron open={localKeepOpen} />
                  <span className="truncate">{t.changes.localKeepTitle}</span>
                </span>
                <span className="muted small mono">{keptFiles.length}</span>
              </button>
              {localKeepOpen && keptFiles.map((file) => {
                const { directory, name } = splitPath(file.path);
                const checked = worktree.selectedIds.includes(file.id);
                return (
                  <label className="compact-file-row compact-keep-row" key={file.id} data-file-path={file.path} title={file.path}>
                    <input
                      type="checkbox"
                      className="file-checkbox"
                      checked={checked}
                      aria-label={t.changes.selectFile(file.path)}
                      onChange={() => props.onSelect(file.id)}
                    />
                    <span className={`git-status git-${file.status}`} aria-hidden="true">{file.status}</span>
                    <span className="file-name mono truncate">{name}</span>
                    {directory && <span className="file-directory mono truncate">{directory}</span>}
                  </label>
                );
              })}
            </div>
          )}
        </m.div>
      )}

      {effectiveDrawer === 'actions' && worktree && (
        <m.div
          key="drawer-actions"
          className="compact-drawer compact-actions-drawer scroll-area"
          role="region"
          aria-label={t.compact.actionsDrawerLabel}
          initial={instantMotion ? false : { opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
        >
          <button
            type="button"
            className="compact-action-item"
            aria-pressed={!props.activeAction}
            onClick={() => {
              props.onSelectAction(null);
              setDrawer('none');
            }}
          >
            <span className="model-menu-label">
              <span className="model-menu-name truncate">{t.primaryActions.cPush}</span>
              <span className="muted small truncate">{t.footer.defaultCPushSubtitle}</span>
            </span>
            {!props.activeAction && <Icon name="check" className="success" />}
          </button>
          {props.customActions.map((item) => (
            <button
              type="button"
              key={item.id}
              className="compact-action-item"
              aria-pressed={props.activeAction?.id === item.id}
              onClick={() => {
                props.onSelectAction(item.id);
                setDrawer('none');
              }}
            >
              <span className="model-menu-label">
                <span className="model-menu-name truncate">{item.label}</span>
                <span className="muted small mono truncate">{item.prompt}</span>
              </span>
              {props.activeAction?.id === item.id && <Icon name="check" className="success" />}
            </button>
          ))}
          <div className="menu-divider compact-actions-divider">
            <button
              type="button"
              className="compact-open-repo-item"
              onClick={() => {
                setDrawer('none');
                props.onNewAction('create');
              }}
            >
              <Icon name="sparkles" />
              {t.footer.createWithPiMenuItem}
            </button>
            <button
              type="button"
              className="compact-open-repo-item"
              onClick={() => {
                setDrawer('none');
                props.onNewAction('manual');
              }}
            >
              <Icon name="plus" />
              {t.footer.newActionMenuItem}
            </button>
            <button
              type="button"
              className="compact-open-repo-item"
              onClick={() => {
                setDrawer('none');
                props.onManageActions();
              }}
            >
              <Icon name="settings" />
              {t.footer.manageActionsMenuItem}
            </button>
          </div>
        </m.div>
      )}

      <footer className="compact-bottom">
        {!worktree ? (
          <button
            type="button"
            className="compact-status compact-status-muted"
            data-expand-target="repo"
            disabled={props.opening}
            onClick={() => {
              if (props.repositoryReadsAvailable) {
                props.onOpenRepository();
              } else {
                props.onExpandTarget('repo');
              }
            }}
          >
            <Icon name="folder" className="compact-folder-icon" />
            <span className="compact-status-text truncate">{t.changes.openLocalRepository}</span>
          </button>
        ) : canShowSelectionTrigger ? (
          <div className="compact-selection-group min-width-zero">
            <input
              ref={selectAllRef}
              type="checkbox"
              className="file-checkbox"
              checked={allNormalSelected}
              disabled={normalFiles.length === 0 && selectedList.length === 0}
              aria-label={
                allNormalSelected || (normalFiles.length === 0 && selectedList.length > 0)
                  ? t.changes.deselectAll
                  : t.changes.selectAll
              }
              onChange={props.onSelectAll}
            />
            <button
              type="button"
              className={`compact-files-trigger ${selectedList.length === 0 ? 'is-muted' : props.hasModel ? 'is-ready' : 'is-warning'}`}
              aria-expanded={effectiveDrawer === 'files'}
              onClick={() => setDrawer(effectiveDrawer === 'files' ? 'none' : 'files')}
            >
              <span className="truncate">
                {t.compact.selectedTrigger(selectedList.length, totalSelectable)}
              </span>
              {!props.hasModel && selectedList.length > 0 && (
                <span className="warning small compact-inline-note">· {t.compact.noModelBadge}</span>
              )}
              <RotatingChevron open={effectiveDrawer === 'files'} direction="down-up" className="compact-chevron" />
            </button>
            {worktree.plan && (
              <button
                type="button"
                className={`compact-plan-pill ${planOutdated ? 'warning' : 'accent'}`}
                data-expand-target="plan"
                onClick={() => props.onExpandTarget('plan')}
                title={status.label}
              >
                <span className="truncate">{t.compact.planBadge(worktree.plan.groups.length, planOutdated)}</span>
              </button>
            )}
          </div>
        ) : status.expandTarget ? (
          <button
            type="button"
            className={`compact-status compact-status-${status.tone}`}
            data-expand-target={status.expandTarget}
            onClick={() => props.onExpandTarget(status.expandTarget!)}
            title={status.label}
          >
            {statusIndicator}
            <span className="compact-status-text truncate">{status.label}</span>
          </button>
        ) : (
          <div
            className={`compact-status compact-status-static compact-status-${status.tone}`}
            role="status"
            title={status.label}
          >
            {statusIndicator}
            <span className="compact-status-text truncate">{status.label}</span>
          </div>
        )}

        <div className={`primary-button-group compact-primary-group ${showSplitToggle ? 'is-split' : ''} ${action.resume ? `resume-button-group resume-${worktree?.sessionStatus ?? 'running'}` : ''}`}>
          <button
            type="button"
            className="primary action-split-main"
            disabled={primaryDisabled}
            title={action.label}
            onClick={
              action.configureModel
                ? props.onConfigureModel
                : action.resume
                  ? () => props.onExpandTarget('dock')
                  : props.onPrimary
            }
          >
            <Icon name={primaryIcon} />
            <span className="truncate">{action.label}</span>
          </button>
          {showSplitToggle && (
            <button
              type="button"
              className="primary action-split-toggle"
              aria-label={t.footer.switchActionAriaLabel}
              aria-expanded={effectiveDrawer === 'actions'}
              disabled={props.busy}
              onClick={() => setDrawer(effectiveDrawer === 'actions' ? 'none' : 'actions')}
            >
              <RotatingChevron open={effectiveDrawer === 'actions'} direction="down-up" />
            </button>
          )}
        </div>
      </footer>
    </div>
  );
}
