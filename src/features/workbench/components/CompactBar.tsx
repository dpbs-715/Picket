import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CustomActionConfig } from '../../../../shared/custom-action-protocol';
import type { GitBranchAction } from '../../../../shared/git-branch-protocol';
import { WINDOW_DIMENSIONS } from '../../../../shared/workbench-persistence-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { PresenceNotice } from '../../../ui/motion';
import {
  compactStatusSummary,
  hasEmptyValidPlanMessage,
  isActiveSession,
  isPlanOutdated,
  localKeepChanges,
  normalChanges,
  primaryAction,
  selectedFiles,
  selectedLocalKeepChanges,
  type CompactExpandTarget,
  type Repository,
  type WorkbenchState,
  type Worktree,
} from '../state/workbench';
import { CompactBranchesDrawer, type BranchSection } from './CompactBranchesDrawer';
import {
  CompactActionsDrawer,
  CompactFilesDrawer,
  CompactProjectsDrawer,
  CompactRemoteDrawer,
  type CompactDrawer,
} from './CompactDrawers';

interface CompactNotice {
  text: string;
  tone?: 'info' | 'success' | 'warning' | 'error';
  showModelSettings?: boolean;
}

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
        <CompactProjectsDrawer
          state={state}
          repository={repository}
          opening={props.opening}
          repositoryReadsAvailable={props.repositoryReadsAvailable}
          onCloseDrawer={() => setDrawer('none')}
          onSwitchWorktree={props.onSwitchWorktree}
          onOpenRepository={props.onOpenRepository}
          onExpandTarget={props.onExpandTarget}
        />
      )}

      {effectiveDrawer === 'branches' && worktree && (
        <CompactBranchesDrawer
          state={state}
          repository={repository}
          worktree={worktree}
          repoWorktrees={repoWorktrees}
          branchDisabled={branchDisabled}
          expandedSections={expandedSections}
          onToggleSection={toggleSection}
          onCloseDrawer={() => setDrawer('none')}
          onSwitchWorktree={props.onSwitchWorktree}
          onSubmitBranch={submitBranch}
        />
      )}

      {effectiveDrawer === 'remote' && worktree && (
        <CompactRemoteDrawer
          worktree={worktree}
          repoWorktrees={repoWorktrees}
          pullBusy={props.pullBusy}
          onCloseDrawer={() => setDrawer('none')}
          onPull={props.onPull}
        />
      )}

      {effectiveDrawer === 'files' && worktree && (
        <CompactFilesDrawer
          worktree={worktree}
          normalFiles={normalFiles}
          keptFiles={keptFiles}
          localKeepOpen={localKeepOpen}
          onToggleLocalKeep={() => setLocalKeepOpen((prev) => !prev)}
          onSelect={props.onSelect}
        />
      )}

      {effectiveDrawer === 'actions' && worktree && (
        <CompactActionsDrawer
          customActions={props.customActions}
          activeAction={props.activeAction}
          onCloseDrawer={() => setDrawer('none')}
          onSelectAction={props.onSelectAction}
          onNewAction={props.onNewAction}
          onManageActions={props.onManageActions}
        />
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
