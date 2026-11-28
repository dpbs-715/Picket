import { useEffect, useMemo, useState } from 'react';
import {
  isRunProcessActive,
  makeRunProcessId,
  type RunCommandConfig,
  type RunProcessSnapshot,
} from '../../../shared/run-protocol';
import { useI18n } from '../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../ui/controls';
import { RunCommandIcon } from './RunCommandIcon';
import { formatElapsedDuration, formatLocalPortChip, resolveRunCommandIcon } from './runIcon';

const LARGE_LIST_THRESHOLD = 6;
const UNGROUPED_KEY = '__ungrouped__';

interface Props {
  worktreeId: string;
  commands: RunCommandConfig[];
  processes: RunProcessSnapshot[];
  busyIds: string[];
  sharedEnvCount: number;
  layoutMode?: 'idle' | 'peek' | 'hidden';
  peekHeightPx?: number | null;
  resizingDock?: boolean;
  onStart(command: RunCommandConfig): void;
  onStop(processId: string, label: string): void;
  onStopAll(): void;
  onRestart(command: RunCommandConfig): void;
  onOpenLogs(processId: string): void;
  onOpenUrl(url: string): void;
  onEdit(command: RunCommandConfig): void;
  onAddManual(): void;
  onDiscoverWithAi(): void;
  onOpenSharedEnv(): void;
  onRefreshEnv(): void;
}

interface CommandGroupBucket {
  key: string;
  title: string;
  idleCommands: RunCommandConfig[];
  runningCount: number;
  totalCount: number;
}

export function RunServicesPanel(props: Props) {
  const { t } = useI18n();
  const [filterQuery, setFilterQuery] = useState('');
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});

  const envBusy = props.busyIds.includes('global:refreshEnv');
  const stopAllBusy = props.busyIds.includes('global:stopAll');

  const processByCommandId = useMemo(() => {
    const map = new Map<string, RunProcessSnapshot>();
    for (const cmd of props.commands) {
      const pid = makeRunProcessId(props.worktreeId, cmd.id);
      const proc = props.processes.find((p) => p.processId === pid);
      if (proc) map.set(cmd.id, proc);
    }
    return map;
  }, [props.commands, props.processes, props.worktreeId]);

  const configuredCommandIds = useMemo(
    () => new Set(props.commands.map((cmd) => cmd.id)),
    [props.commands],
  );

  const orphanedActiveProcesses = useMemo(
    () => props.processes.filter(
      (proc) => proc.worktreeId === props.worktreeId
        && isRunProcessActive(proc.status)
        && !configuredCommandIds.has(proc.commandId),
    ),
    [props.processes, props.worktreeId, configuredCommandIds],
  );

  const activeCount = useMemo(
    () =>
      props.commands.reduce(
        (count, cmd) => count + (isRunProcessActive(processByCommandId.get(cmd.id)?.status) ? 1 : 0),
        0,
      ) + orphanedActiveProcesses.length,
    [props.commands, processByCommandId, orphanedActiveProcesses.length],
  );

  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (activeCount === 0) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [activeCount]);

  // Clear stale search filter when switching worktrees or dropping below threshold:
  useEffect(() => {
    if (props.commands.length < LARGE_LIST_THRESHOLD && filterQuery) {
      setFilterQuery('');
    }
  }, [props.commands.length, props.worktreeId, filterQuery]);

  const isLargeList = props.commands.length >= LARGE_LIST_THRESHOLD;
  const hasAnyGroup = useMemo(
    () => props.commands.some((cmd) => Boolean(cmd.group?.trim())),
    [props.commands],
  );
  const useStructuredLayout = hasAnyGroup || isLargeList;
  const normalizedQuery = filterQuery.trim().toLowerCase();

  const filteredCommands = useMemo(() => {
    if (!normalizedQuery) return props.commands;
    return props.commands.filter((cmd) => {
      const hay = `${cmd.label} ${cmd.group ?? ''} ${cmd.cwd ?? ''} ${cmd.command}`.toLowerCase();
      return hay.includes(normalizedQuery);
    });
  }, [props.commands, normalizedQuery]);

  const activeCommands = useMemo(
    () => filteredCommands.filter((cmd) => isRunProcessActive(processByCommandId.get(cmd.id)?.status)),
    [filteredCommands, processByCommandId],
  );

  const idleCommands = useMemo(
    () => filteredCommands.filter((cmd) => !isRunProcessActive(processByCommandId.get(cmd.id)?.status)),
    [filteredCommands, processByCommandId],
  );

  const groupedBuckets = useMemo<CommandGroupBucket[]>(() => {
    if (!hasAnyGroup) return [];
    const buckets = new Map<string, CommandGroupBucket>();
    for (const cmd of filteredCommands) {
      const rawGroup = cmd.group?.trim() ?? '';
      const key = rawGroup || UNGROUPED_KEY;
      const title = rawGroup || t.run.ungroupedSectionTitle;
      let bucket = buckets.get(key);
      if (!bucket) {
        bucket = { key, title, idleCommands: [], runningCount: 0, totalCount: 0 };
        buckets.set(key, bucket);
      }
      bucket.totalCount += 1;
      if (isRunProcessActive(processByCommandId.get(cmd.id)?.status)) {
        bucket.runningCount += 1;
      } else {
        bucket.idleCommands.push(cmd);
      }
    }
    // Place named groups first in discovery order, ungrouped bucket last:
    const named: CommandGroupBucket[] = [];
    let ungrouped: CommandGroupBucket | undefined;
    for (const bucket of buckets.values()) {
      if (bucket.idleCommands.length === 0) continue;
      if (bucket.key === UNGROUPED_KEY) {
        ungrouped = bucket;
      } else {
        named.push(bucket);
      }
    }
    if (ungrouped) named.push(ungrouped);
    return named;
  }, [filteredCommands, hasAnyGroup, processByCommandId, t.run.ungroupedSectionTitle]);

  function toggleGroup(groupKey: string) {
    setCollapsedGroups((prev) => ({ ...prev, [groupKey]: !prev[groupKey] }));
  }

  function renderServiceCard(cmd: RunCommandConfig, compact: boolean) {
    const processId = makeRunProcessId(props.worktreeId, cmd.id);
    const proc = processByCommandId.get(cmd.id);
    const active = isRunProcessActive(proc?.status);
    const isStarting = proc?.status === 'starting';
    const isStopping = proc?.status === 'stopping' || props.busyIds.includes(`stopping:${processId}`);
    const isRestarting = props.busyIds.includes(`restarting:${processId}`);
    const startOrRestartBusy = props.busyIds.includes(processId) || isStopping || isRestarting;
    const stopDisabled = isStopping;
    const hasLogs = Boolean(proc && (proc.logLineCount > 0 || active));
    const iconId = resolveRunCommandIcon(cmd.icon, cmd.command, cmd.label);
    const urls = active ? (proc?.detectedUrls ?? []).slice(0, 2) : [];
    const isFailed = proc?.status === 'failed';
    const isExited = proc?.status === 'exited';
    const showTelemetry = active || isFailed || (isExited && hasLogs);

    return (
      <div
        key={cmd.id}
        className={`run-service-card ${compact && !active ? 'is-compact' : ''} ${active ? 'is-running' : isFailed ? 'is-failed' : ''}`}
        data-run-card={cmd.id}
      >
        <div className="run-service-card-main">
          <RunCommandIcon
            icon={iconId}
            size={compact && !active ? 'compact' : 'md'}
            running={active}
          />
          <div className="run-service-card-copy min-width-zero">
            <div className="run-service-card-title-row">
              <span className="run-service-name truncate" title={cmd.label}>
                {cmd.label}
              </span>
              {active && cmd.group && (
                <span className="run-group-inline-pill truncate" title={cmd.group}>
                  {cmd.group}
                </span>
              )}
              {cmd.cwd && (
                <span className="run-cwd-badge mono" title={cmd.cwd}>
                  {cmd.cwd}
                </span>
              )}
              {active && proc?.startedAtMs ? (
                <span className="run-elapsed mono">
                  {formatElapsedDuration(proc.startedAtMs, nowMs)}
                </span>
              ) : null}
            </div>
            <div className="run-service-cmd-line mono muted truncate" title={cmd.command}>
              {cmd.command}
            </div>
          </div>
          <div className="run-service-card-controls">
            {active ? (
              <>
                <IconButton
                  icon="refresh"
                  label={t.run.restartAction}
                  className="run-restart-icon"
                  disabled={startOrRestartBusy || isStarting}
                  onClick={() => props.onRestart(cmd)}
                />
                <IconButton
                  icon="square"
                  label={t.run.stopAction}
                  className="run-stop-icon"
                  disabled={stopDisabled}
                  onClick={() => props.onStop(processId, cmd.label)}
                />
              </>
            ) : (
              <>
                <IconButton
                  icon="pencil"
                  label={t.run.editAction}
                  className="run-hover-reveal"
                  disabled={startOrRestartBusy}
                  onClick={() => props.onEdit(cmd)}
                />
                <IconButton
                  icon="play"
                  label={t.run.startAction}
                  className="run-start-icon"
                  disabled={startOrRestartBusy}
                  onClick={() => props.onStart(cmd)}
                />
              </>
            )}
          </div>
        </div>

        {showTelemetry && (
          <div className={`run-telemetry-strip ${isFailed ? 'is-error' : ''}`}>
            <button
              type="button"
              className="run-telemetry-output mono truncate"
              title={t.run.openLogsHint}
              onClick={() => props.onOpenLogs(processId)}
            >
              {isStarting && (
                <span className="run-inline-status">
                  {t.run.statusStarting}
                </span>
              )}
              {isStopping && (
                <span className="run-inline-status">
                  {t.run.statusStopping}
                </span>
              )}
              {isFailed && (
                <span className="run-inline-status is-failed">
                  {t.run.statusFailed(proc?.exitCode)}
                </span>
              )}
              {isExited && (
                <span className="run-inline-status is-exited">
                  {t.run.statusExited(proc?.exitCode)}
                </span>
              )}
              <span className="truncate">
                {proc?.lastLogLine || proc?.errorMessage || (active ? t.run.logsNoOutputYet : t.run.logsViewPreviousHint)}
              </span>
            </button>
            <div className="run-telemetry-actions">
              {urls.map((url) => (
                <button
                  key={url}
                  type="button"
                  className="run-port-chip mono"
                  title={t.run.logsOpenUrlTitle(url)}
                  onClick={() => props.onOpenUrl(url)}
                >
                  <span>{formatLocalPortChip(url)}</span>
                  <Icon name="external" />
                </button>
              ))}
              <button
                type="button"
                className="run-log-pill"
                aria-label={t.run.logsAction}
                title={t.run.openLogsHint}
                onClick={() => props.onOpenLogs(processId)}
              >
                <Icon name="terminal" />
                <span>{t.run.logsAction}</span>
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  function renderOrphanedProcessCard(proc: RunProcessSnapshot) {
    const isStarting = proc.status === 'starting';
    const isStopping = proc.status === 'stopping' || props.busyIds.includes(`stopping:${proc.processId}`);
    const iconId = resolveRunCommandIcon(undefined, proc.command, proc.label);
    const urls = proc.detectedUrls.slice(0, 2);

    return (
      <div
        key={proc.processId}
        className="run-service-card is-running is-orphaned"
        data-run-orphan-card={proc.processId}
      >
        <div className="run-service-card-main">
          <RunCommandIcon icon={iconId} size="md" running />
          <div className="run-service-card-copy min-width-zero">
            <div className="run-service-card-title-row">
              <span className="run-service-name truncate" title={proc.label}>
                {proc.label}
              </span>
              {proc.cwd !== '.' && (
                <span className="run-cwd-badge mono" title={proc.cwd}>
                  {proc.cwd}
                </span>
              )}
              {proc.startedAtMs ? (
                <span className="run-elapsed mono">
                  {formatElapsedDuration(proc.startedAtMs, nowMs)}
                </span>
              ) : null}
            </div>
            <div className="run-service-cmd-line mono muted truncate" title={proc.command}>
              {proc.command}
            </div>
          </div>
          <div className="run-service-card-controls">
            <IconButton
              icon="square"
              label={t.run.stopAction}
              className="run-stop-icon"
              disabled={isStopping}
              onClick={() => props.onStop(proc.processId, proc.label)}
            />
          </div>
        </div>
        <div className="run-telemetry-strip">
          <button
            type="button"
            className="run-telemetry-output mono truncate"
            title={t.run.openLogsHint}
            onClick={() => props.onOpenLogs(proc.processId)}
          >
            {isStarting && <span className="run-inline-status">{t.run.statusStarting}</span>}
            {isStopping && <span className="run-inline-status">{t.run.statusStopping}</span>}
            <span className="truncate">{proc.lastLogLine || t.run.logsNoOutputYet}</span>
          </button>
          <div className="run-telemetry-actions">
            {urls.map((url) => (
              <button
                key={url}
                type="button"
                className="run-port-chip mono"
                title={t.run.logsOpenUrlTitle(url)}
                onClick={() => props.onOpenUrl(url)}
              >
                <span>{formatLocalPortChip(url)}</span>
                <Icon name="external" />
              </button>
            ))}
            <button
              type="button"
              className="run-log-pill"
              aria-label={t.run.logsAction}
              title={t.run.openLogsHint}
              onClick={() => props.onOpenLogs(proc.processId)}
            >
              <Icon name="terminal" />
              <span>{t.run.logsAction}</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  const sharedEnvLabel = props.sharedEnvCount > 0
    ? t.run.sharedEnvButtonActive(props.sharedEnvCount)
    : t.run.sharedEnvButton;
  const layoutMode = props.layoutMode ?? 'idle';

  return (
    <section
      className={`run-services-panel is-${layoutMode} ${props.resizingDock ? 'is-dock-resizing' : ''}`}
      data-layout-mode={layoutMode}
      style={
        layoutMode === 'peek' && props.peekHeightPx !== null && props.peekHeightPx !== undefined
          ? { flexBasis: `${props.peekHeightPx}px`, height: `${props.peekHeightPx}px` }
          : undefined
      }
      inert={layoutMode === 'hidden'}
      aria-label={t.run.panelAriaLabel}
    >
      <div className="run-panel-toolbar">
        <span className="muted small run-panel-summary truncate">
          {props.commands.length === 0
            ? t.run.panelTitle
            : t.run.panelSummary(props.commands.length, activeCount)}
        </span>
        <div className="row run-panel-actions">
          {activeCount >= 2 && (
            <button
              type="button"
              className="secondary compact run-toolbar-stop-all"
              disabled={stopAllBusy}
              onClick={props.onStopAll}
            >
              <Icon name="square" />
              <span>{t.run.stopAllAction}</span>
            </button>
          )}
          <button
            type="button"
            className="secondary compact run-toolbar-btn"
            onClick={props.onDiscoverWithAi}
          >
            <Icon name="sparkles" />
            <span>{t.run.aiRediscoverButton}</span>
          </button>
          <button
            type="button"
            className={`icon-button run-toolbar-env-btn ${props.sharedEnvCount > 0 ? 'has-env' : ''}`}
            data-run-shared-env-btn
            aria-label={sharedEnvLabel}
            title={sharedEnvLabel}
            onClick={props.onOpenSharedEnv}
          >
            <Icon name="sliders" />
            {props.sharedEnvCount > 0 && (
              <span className="run-toolbar-env-count">{props.sharedEnvCount}</span>
            )}
          </button>
          <IconButton
            icon="plus"
            label={t.run.addManualButton}
            onClick={props.onAddManual}
          />
          <IconButton
            icon="refresh"
            label={t.run.refreshEnvButton}
            disabled={envBusy}
            className={envBusy ? 'spinning' : ''}
            onClick={props.onRefreshEnv}
          />
        </div>
      </div>

      {isLargeList && (
        <div className="run-filter-bar">
          <Icon name="search" className="run-filter-icon" />
          <input
            type="search"
            className="run-filter-input"
            data-run-filter-input
            value={filterQuery}
            onChange={(e) => setFilterQuery(e.target.value)}
            placeholder={t.run.filterPlaceholder}
            aria-label={t.run.filterPlaceholder}
            spellCheck={false}
          />
          {filterQuery && (
            <button
              type="button"
              className="icon-button run-filter-clear"
              aria-label={t.run.filterClearAction}
              title={t.run.filterClearAction}
              onClick={() => setFilterQuery('')}
            >
              <Icon name="close" />
            </button>
          )}
        </div>
      )}

      {props.commands.length === 0 && orphanedActiveProcesses.length === 0 ? (
        <div className="run-panel-empty">
          <div className="run-panel-empty-icon" aria-hidden="true">
            <Icon name="terminal" />
          </div>
          <p className="run-empty-title">{t.run.emptyTitle}</p>
          <div className="run-panel-empty-actions">
            <button
              type="button"
              className="primary"
              onClick={props.onDiscoverWithAi}
            >
              <Icon name="sparkles" />
              <span>{t.run.aiDiscoverButton}</span>
            </button>
            <button
              type="button"
              className="secondary"
              onClick={props.onAddManual}
            >
              <Icon name="plus" />
              <span>{t.run.addManualButton}</span>
            </button>
          </div>
        </div>
      ) : filteredCommands.length === 0 && orphanedActiveProcesses.length === 0 ? (
        <div className="run-panel-empty run-panel-filter-empty">
          <p className="muted small">{t.run.filterEmptyResult(filterQuery.trim())}</p>
          <button
            type="button"
            className="secondary compact"
            onClick={() => setFilterQuery('')}
          >
            {t.run.filterClearAction}
          </button>
        </div>
      ) : !useStructuredLayout ? (
        <div className="run-services-list scroll-area">
          {orphanedActiveProcesses.map((proc) => renderOrphanedProcessCard(proc))}
          {filteredCommands.map((cmd) => renderServiceCard(cmd, false))}
        </div>
      ) : (
        <div className="run-services-list scroll-area">
          {(activeCommands.length > 0 || orphanedActiveProcesses.length > 0) && (
            <div className="run-running-section" data-run-running-section>
              <div className="run-section-header">
                <span className="run-section-dot" aria-hidden="true" />
                <span className="run-section-label">{t.run.runningSectionTitle}</span>
                <span className="run-section-count">{activeCommands.length + orphanedActiveProcesses.length}</span>
              </div>
              <div className="run-group-body">
                {orphanedActiveProcesses.map((proc) => renderOrphanedProcessCard(proc))}
                {activeCommands.map((cmd) => renderServiceCard(cmd, false))}
              </div>
            </div>
          )}

          {hasAnyGroup ? (
            groupedBuckets.map((bucket) => {
              const collapsed = Boolean(collapsedGroups[bucket.key]) && !normalizedQuery;
              return (
                <div key={bucket.key} className="run-group-section" data-run-group={bucket.key}>
                  <button
                    type="button"
                    className="run-group-header"
                    data-run-group-toggle={bucket.key}
                    aria-expanded={!collapsed}
                    onClick={() => toggleGroup(bucket.key)}
                  >
                    <RotatingChevron open={!collapsed} direction="right-down" />
                    <span className="run-group-title truncate">{bucket.title}</span>
                    <span className="run-section-count">{bucket.totalCount}</span>
                    {bucket.runningCount > 0 && (
                      <span className="run-group-running-pill">
                        {t.run.groupRunningBadge(bucket.runningCount)}
                      </span>
                    )}
                  </button>
                  {!collapsed && (
                    <div className={`run-group-body ${isLargeList ? 'is-compact-list' : ''}`}>
                      {bucket.idleCommands.map((cmd) => renderServiceCard(cmd, isLargeList))}
                    </div>
                  )}
                </div>
              );
            })
          ) : (
            <div className={`run-group-body ${isLargeList ? 'is-compact-list' : ''}`}>
              {idleCommands.map((cmd) => renderServiceCard(cmd, isLargeList))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
