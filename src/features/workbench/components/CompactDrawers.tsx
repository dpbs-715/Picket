import type { CustomActionConfig } from '../../../../shared/custom-action-protocol';
import {
  isRunProcessActive,
  makeRunProcessId,
  type RunCommandConfig,
  type RunProcessSnapshot,
} from '../../../../shared/run-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../../ui/motion';
import { RunCommandIcon } from '../../runner/RunCommandIcon';
import { resolveRunCommandIcon } from '../../runner/runIcon';
import {
  isActiveSession,
  splitPath,
  type Change,
  type CompactExpandTarget,
  type Repository,
  type WorkbenchState,
  type Worktree,
} from '../state/workbench';
import { FileName } from './ChangeFileRow';

export type CompactDrawer = 'none' | 'projects' | 'branches' | 'remote' | 'files' | 'actions' | 'run';

export function CompactProjectsDrawer({
  state,
  repository,
  opening,
  repositoryReadsAvailable,
  onCloseDrawer,
  onSwitchWorktree,
  onOpenRepository,
  onExpandTarget,
}: {
  state: WorkbenchState;
  repository?: Repository;
  opening: boolean;
  repositoryReadsAvailable: boolean;
  onCloseDrawer(): void;
  onSwitchWorktree(worktreeId: string): void;
  onOpenRepository(): void;
  onExpandTarget(target: CompactExpandTarget): void;
}) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
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
              onCloseDrawer();
              if (trees[0]) onSwitchWorktree(trees[0].worktreeId);
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
          disabled={opening}
          onClick={() => {
            onCloseDrawer();
            if (repositoryReadsAvailable) {
              onOpenRepository();
            } else {
              onExpandTarget('repo');
            }
          }}
        >
          <Icon name="plus" />
          {t.header.openLocalRepository}
        </button>
      </div>
    </m.div>
  );
}

export function CompactRemoteDrawer({
  worktree,
  repoWorktrees,
  pullBusy,
  onCloseDrawer,
  onPull,
}: {
  worktree: Worktree;
  repoWorktrees: Worktree[];
  pullBusy: boolean;
  onCloseDrawer(): void;
  onPull(): void;
}) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
    <m.div
      key="drawer-remote"
      className="compact-drawer compact-remote-drawer scroll-area"
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
          disabled={pullBusy || repoWorktrees.some(isActiveSession)}
          onClick={() => {
            onCloseDrawer();
            onPull();
          }}
        >
          {pullBusy ? t.header.syncing : t.header.pullAndRebase}
        </button>
      </div>
    </m.div>
  );
}

export function CompactFilesDrawer({
  worktree,
  normalFiles,
  keptFiles,
  localKeepOpen,
  onToggleLocalKeep,
  onSelect,
}: {
  worktree: Worktree;
  normalFiles: Change[];
  keptFiles: Change[];
  localKeepOpen: boolean;
  onToggleLocalKeep(): void;
  onSelect(fileId: string): void;
}) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
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
              onChange={() => onSelect(file.id)}
            />
            <span className={`git-status git-${file.status}`} aria-hidden="true">{file.status}</span>
            <FileName name={name} />
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
            onClick={onToggleLocalKeep}
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
                  onChange={() => onSelect(file.id)}
                />
                <span className={`git-status git-${file.status}`} aria-hidden="true">{file.status}</span>
                <FileName name={name} />
                {directory && <span className="file-directory mono truncate">{directory}</span>}
              </label>
            );
          })}
        </div>
      )}
    </m.div>
  );
}

export function CompactActionsDrawer({
  customActions,
  activeAction,
  onCloseDrawer,
  onSelectAction,
  onNewAction,
  onManageActions,
}: {
  customActions: CustomActionConfig[];
  activeAction?: CustomActionConfig;
  onCloseDrawer(): void;
  onSelectAction(actionId: string | null): void;
  onNewAction(mode: 'manual' | 'create'): void;
  onManageActions(): void;
}) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
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
        aria-pressed={!activeAction}
        onClick={() => {
          onSelectAction(null);
          onCloseDrawer();
        }}
      >
        <span className="model-menu-label">
          <span className="model-menu-name truncate">{t.primaryActions.cPush}</span>
          <span className="muted small truncate">{t.footer.defaultCPushSubtitle}</span>
        </span>
        {!activeAction && <Icon name="check" className="success" />}
      </button>
      {customActions.map((item) => (
        <button
          type="button"
          key={item.id}
          className="compact-action-item"
          aria-pressed={activeAction?.id === item.id}
          onClick={() => {
            onSelectAction(item.id);
            onCloseDrawer();
          }}
        >
          <span className="model-menu-label">
            <span className="model-menu-name truncate">{item.label}</span>
            <span className="muted small mono truncate">{item.prompt}</span>
          </span>
          {activeAction?.id === item.id && <Icon name="check" className="success" />}
        </button>
      ))}
      <div className="menu-divider compact-actions-divider">
        <button
          type="button"
          className="compact-open-repo-item"
          onClick={() => {
            onCloseDrawer();
            onNewAction('create');
          }}
        >
          <Icon name="sparkles" />
          {t.footer.createWithPiMenuItem}
        </button>
        <button
          type="button"
          className="compact-open-repo-item"
          onClick={() => {
            onCloseDrawer();
            onNewAction('manual');
          }}
        >
          <Icon name="plus" />
          {t.footer.newActionMenuItem}
        </button>
        <button
          type="button"
          className="compact-open-repo-item"
          onClick={() => {
            onCloseDrawer();
            onManageActions();
          }}
        >
          <Icon name="settings" />
          {t.footer.manageActionsMenuItem}
        </button>
      </div>
    </m.div>
  );
}

export function CompactRunDrawer({
  worktreeId,
  commands,
  processes,
  busyIds,
  selectedCommandId,
  onSelectCommand,
  onStart,
  onStop,
  onRestart,
  onOpenLogs,
  onManageInMain,
}: {
  worktreeId: string;
  commands: RunCommandConfig[];
  processes: RunProcessSnapshot[];
  busyIds: string[];
  selectedCommandId?: string | null;
  onSelectCommand?(commandId: string): void;
  onStart(command: RunCommandConfig): void;
  onStop(processId: string, label: string): void;
  onRestart(command: RunCommandConfig): void;
  onOpenLogs(processId: string): void;
  onManageInMain(mode: 'create' | 'discover' | 'panel'): void;
}) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
    <m.div
      key="drawer-run"
      className="compact-drawer compact-run-drawer scroll-area"
      role="region"
      aria-label={t.run.popoverTitle}
      initial={instantMotion ? false : { opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
    >
      {commands.length === 0 ? (
        <div className="compact-run-empty">
          <p className="muted small">{t.run.compactEmptyHint}</p>
          <div className="row">
            <button
              type="button"
              className="primary compact"
              onClick={() => onManageInMain('discover')}
            >
              <Icon name="sparkles" />
              <span>{t.run.aiDiscoverButton}</span>
            </button>
            <button
              type="button"
              className="secondary compact"
              onClick={() => onManageInMain('create')}
            >
              <Icon name="plus" />
              <span>{t.run.addManualButton}</span>
            </button>
          </div>
        </div>
      ) : (
        <>
          {commands.map((cmd) => {
            const processId = makeRunProcessId(worktreeId, cmd.id);
            const proc = processes.find((item) => item.processId === processId);
            const active = isRunProcessActive(proc?.status);
            const isStarting = proc?.status === 'starting';
            const isStopping = proc?.status === 'stopping' || busyIds.includes(`stopping:${processId}`);
            const isRestarting = busyIds.includes(`restarting:${processId}`);
            const actionBusy = busyIds.includes(processId) || isStopping || isRestarting;
            const hasLogs = Boolean(proc && (proc.logLineCount > 0 || active));
            const iconId = resolveRunCommandIcon(cmd.icon, cmd.command, cmd.label);
            return (
              <div
                key={cmd.id}
                className={`run-command-row compact-run-row ${active ? 'is-running' : ''} ${selectedCommandId === cmd.id ? 'is-selected' : ''}`}
              >
                <button
                  type="button"
                  className="run-command-info compact-run-select"
                  onClick={() => onSelectCommand?.(cmd.id)}
                >
                  <div className="run-command-title-line">
                    <RunCommandIcon icon={iconId} size="sm" running={active} />
                    <span className="run-command-label truncate" title={cmd.label}>{cmd.label}</span>
                  </div>
                  <span className="run-command-preview mono muted small truncate" title={cmd.command}>
                    {cmd.command}
                  </span>
                </button>
                <div className="run-command-controls">
                  {hasLogs && (
                    <IconButton
                      icon="terminal"
                      label={t.run.logsAction}
                      onClick={() => onOpenLogs(processId)}
                    />
                  )}
                  {active ? (
                    <>
                      <IconButton
                        icon="refresh"
                        label={t.run.restartAction}
                        className="run-restart-icon"
                        disabled={actionBusy || isStarting}
                        onClick={() => onRestart(cmd)}
                      />
                      <IconButton
                        icon="square"
                        label={t.run.stopAction}
                        className="run-stop-icon"
                        disabled={isStopping}
                        onClick={() => onStop(processId, cmd.label)}
                      />
                    </>
                  ) : (
                    <IconButton
                      icon="play"
                      label={t.run.startAction}
                      className="run-start-icon"
                      disabled={actionBusy}
                      onClick={() => onStart(cmd)}
                    />
                  )}
                </div>
              </div>
            );
          })}
          <div className="menu-divider compact-run-footer">
            <button type="button" onClick={() => onManageInMain('panel')}>
              <Icon name="expand" />
              <span>{t.run.compactManageInMain}</span>
            </button>
            <button type="button" onClick={() => onManageInMain('discover')}>
              <Icon name="sparkles" />
              <span>{t.run.aiRediscoverButton}</span>
            </button>
          </div>
        </>
      )}
    </m.div>
  );
}
