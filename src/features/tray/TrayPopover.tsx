import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { DEFAULT_APP_THEME } from '../../../shared/theme-protocol';
import type {
  TrayMainWindowAction,
  TrayRepositorySummary,
  TrayStateSnapshot,
} from '../../../shared/tray-protocol';
import { getMessages, I18nProvider } from '../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../ui/controls';
import { useResolvedTheme } from '../../ui/theme';
import {
  isTrayRepoCleanIdle,
  isTrayRepoNeedsDock,
  partitionTrayRepositories,
} from './trayModel';

export interface TrayPopoverProps {
  snapshot: TrayStateSnapshot;
  onRefresh: () => void;
  onOpenSettings: () => void;
  onOpenMainWindow: (action?: TrayMainWindowAction) => void;
  onStopRunService?: (processId: string) => void;
  onOpenRunLogs?: (processId: string) => void;
  onQuit: () => void;
  onDismiss: () => void;
  onHeightChange?: (height: number) => void;
  /** Incremented by the host whenever the popover is re-opened to reset transient fold state. */
  openGeneration?: number;
}

const WHALE_MARK_PATH =
  'M 320.25 694.63 C 302.67 694.59, 285.47 684.78, 280.67 666.99 C 278.10 657.45, 278.62 646.66, 279.24 636.89 C 281.05 608.41, 289.41 585.58, 304.64 561.58 C 318.29 540.08, 338.97 516.80, 338.12 489.68 C 337.60 473.21, 326.79 460.86, 315.77 449.74 C 304.34 438.22, 292.07 427.54, 280.18 416.50 C 272.02 408.93, 263.52 401.36, 257.07 392.21 C 246.62 377.40, 240.71 347.74, 251.06 331.65 C 261.48 315.46, 287.55 319.83, 304.24 319.78 C 312.31 319.76, 321.97 319.14, 327.76 325.91 C 335.71 335.18, 334.08 350.12, 336.41 361.37 C 339.87 378.01, 361.16 407.65, 379.43 408.99 C 400.91 410.56, 422.39 378.99, 427.08 360.59 C 429.82 349.83, 430.15 336.17, 437.09 327.06 C 439.52 323.87, 442.77 321.68, 446.61 320.55 C 452.48 318.82, 458.97 319.48, 465.00 319.47 C 473.96 319.46, 482.94 319.33, 491.90 319.53 C 515.23 320.07, 522.91 339.39, 519.78 359.97 C 516.24 383.22, 496.56 401.34, 482.27 418.62 C 476.99 425.01, 471.28 431.57, 467.56 439.04 C 466.34 441.50, 464.50 446.14, 468.06 447.66 C 472.59 449.61, 482.80 443.78, 487.07 441.84 C 502.98 434.61, 518.72 427.93, 535.90 424.33 C 579.21 415.26, 630.47 421.85, 670.53 440.50 C 740.53 473.09, 773.04 536.22, 781.60 610.59 C 783.44 626.63, 786.77 655.76, 781.07 670.65 C 777.49 680.03, 769.51 687.75, 760.22 691.41 C 747.99 696.22, 728.48 694.16, 715.19 694.13 C 685.50 694.07, 655.80 694.25, 626.11 694.25 C 524.16 694.25, 422.20 694.85, 320.25 694.63 Z M 599.46 587.74 C 606.72 587.70, 613.64 584.39, 617.06 577.74 C 620.71 570.64, 619.60 562.01, 619.58 554.27 C 619.57 548.41, 619.62 542.55, 619.61 536.68 C 619.61 532.58, 620.05 528.18, 619.30 524.14 C 617.52 514.47, 609.07 508.33, 599.50 508.27 C 591.68 508.22, 584.71 512.40, 581.39 519.50 C 578.25 526.20, 579.46 534.64, 579.44 541.84 C 579.43 547.70, 579.42 553.56, 579.45 559.43 C 579.47 563.57, 579.05 567.88, 579.75 571.97 C 581.42 581.74, 589.81 587.79, 599.46 587.74 Z M 673.79 593.76 C 680.25 593.67, 686.15 590.49, 689.55 584.97 C 693.06 579.29, 692.89 571.93, 693.97 565.51 C 694.81 560.55, 695.54 555.57, 696.17 550.58 C 696.64 546.85, 697.39 543.09, 697.27 539.31 C 697.02 530.97, 691.35 524.17, 683.40 521.89 C 680.99 521.19, 678.41 520.98, 675.91 521.06 C 668.21 521.34, 661.40 526.63, 658.96 533.93 C 657.41 538.58, 657.17 543.96, 656.40 548.79 C 655.61 553.76, 654.95 558.75, 654.27 563.74 C 652.58 576.10, 651.43 588.29, 666.34 592.73 C 668.76 593.45, 671.27 593.79, 673.79 593.76 Z';

type DotTone = 'danger' | 'warning' | 'accent' | 'success' | 'muted';

function describeRepository(
  repo: TrayRepositorySummary,
  t: ReturnType<typeof getMessages>['tray'],
): { dot: DotTone; text: string; toneClass: string } {
  if (repo.sessionStatus === 'conflict') {
    const suffix = repo.statusDetail ? ` · ${repo.statusDetail}` : '';
    return { dot: 'danger', text: `${t.statusConflict}${suffix}`, toneClass: 'is-danger' };
  }
  if (repo.sessionStatus === 'needsDecision') {
    const suffix = repo.statusDetail ? ` · ${repo.statusDetail}` : '';
    return { dot: 'warning', text: `${t.statusNeedsDecision}${suffix}`, toneClass: 'is-warning' };
  }
  if (repo.sessionStatus === 'pushFailed') {
    return { dot: 'danger', text: t.statusPushFailed, toneClass: 'is-danger' };
  }
  if (repo.sessionStatus === 'waitingManual') {
    return { dot: 'warning', text: t.statusWaitingManual, toneClass: 'is-warning' };
  }
  if (repo.sessionStatus === 'running') {
    return { dot: 'accent', text: repo.statusDetail || t.statusRunning, toneClass: 'is-accent' };
  }
  if (repo.readError) {
    return { dot: 'danger', text: t.statusUnavailable, toneClass: 'is-danger' };
  }
  if (repo.detached) {
    return { dot: 'warning', text: t.detachedHead, toneClass: 'is-warning' };
  }
  if (repo.changeCount > 0) {
    return { dot: 'success', text: t.changesCount(repo.changeCount), toneClass: 'is-active' };
  }
  if (repo.ahead > 0 || repo.behind > 0) {
    return { dot: 'success', text: t.clean, toneClass: 'is-muted' };
  }
  if (repo.localKeepCount > 0) {
    return { dot: 'muted', text: t.localKeepOnly(repo.localKeepCount), toneClass: 'is-muted' };
  }
  return { dot: 'muted', text: t.clean, toneClass: 'is-muted' };
}

function TrayRepoRow({
  repo,
  t,
  onSelect,
}: {
  repo: TrayRepositorySummary;
  t: ReturnType<typeof getMessages>['tray'];
  onSelect: (repo: TrayRepositorySummary) => void;
}) {
  const status = describeRepository(repo, t);
  const syncText = t.aheadBehind(repo.ahead, repo.behind);
  const cleanIdle = isTrayRepoCleanIdle(repo);

  return (
    <button
      type="button"
      className={`tray-repo-card ${repo.active ? 'is-current' : ''} ${cleanIdle ? 'is-clean-idle' : ''}`}
      onClick={() => onSelect(repo)}
      aria-label={t.openRepositoryHint(repo.name)}
      data-tray-repo={repo.repositoryId}
    >
      <span className={`tray-status-dot tone-${status.dot}`} aria-hidden="true" />
      <span className="tray-repo-content">
        <span className="tray-repo-top">
          <span className="tray-repo-name truncate">{repo.name}</span>
          <span className="tray-repo-meta">
            <span className="tray-repo-branch mono truncate">
              {repo.detached ? t.detachedHead : repo.branch}
            </span>
            {syncText && <span className="tray-repo-sync mono">{syncText}</span>}
          </span>
        </span>
        <span className={`tray-repo-summary truncate ${status.toneClass}`}>
          {status.text}
        </span>
      </span>
      <Icon name="right" className="tray-repo-chevron" />
    </button>
  );
}

export function TrayPopover({
  snapshot,
  onRefresh,
  onOpenSettings,
  onOpenMainWindow,
  onStopRunService,
  onOpenRunLogs,
  onQuit,
  onDismiss,
  onHeightChange,
  openGeneration = 0,
}: TrayPopoverProps) {
  const t = getMessages(snapshot.locale);
  const trayText = t.tray;
  const runText = t.run;
  const runningServices = snapshot.runningServices ?? [];
  useResolvedTheme(snapshot.theme ?? DEFAULT_APP_THEME);
  const [cleanExpanded, setCleanExpanded] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);
  const lastReportedHeightRef = useRef(0);
  const latestHeightChangeRef = useRef(onHeightChange);
  latestHeightChangeRef.current = onHeightChange;

  useEffect(() => {
    setCleanExpanded(false);
  }, [openGeneration]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onDismiss();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onDismiss]);

  useLayoutEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = snapshot.locale === 'zh' ? 'zh-CN' : 'en';
    }
  }, [snapshot.locale]);

  const { primary, foldedClean } = useMemo(
    () => partitionTrayRepositories(snapshot.repositories),
    [snapshot.repositories],
  );

  useLayoutEffect(() => {
    const card = cardRef.current;
    if (!card || !latestHeightChangeRef.current) return;
    const report = () => {
      const measured = Math.ceil(card.getBoundingClientRect().height);
      if (measured > 0 && Math.abs(measured - lastReportedHeightRef.current) >= 1) {
        lastReportedHeightRef.current = measured;
        latestHeightChangeRef.current?.(measured);
      }
    };
    report();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(report);
    observer.observe(card);
    return () => observer.disconnect();
  }, [primary.length, foldedClean.length, cleanExpanded, snapshot.repositories.length, runningServices.length]);

  const handleSelectRepo = (repo: TrayRepositorySummary) => {
    onOpenMainWindow({
      kind: 'focusWorktree',
      repositoryId: repo.repositoryId,
      worktreeId: repo.worktreeId,
      openDock: isTrayRepoNeedsDock(repo),
    });
  };

  return (
    <I18nProvider locale={snapshot.locale}>
      <div className="tray-shell">
        <section
          ref={cardRef}
          className="tray-card"
          aria-label={trayText.ariaLabel}
          data-tray-popover
        >
          <header className="tray-header">
            <div className="tray-brand">
              <svg className="tray-brand-mark" viewBox="230 300 570 420" aria-hidden="true">
                <path d={WHALE_MARK_PATH} fill="currentColor" fillRule="evenodd" />
              </svg>
              <h1 className="tray-title">Picket</h1>
            </div>
            <div className="tray-header-actions">
              <IconButton
                icon="refresh"
                className={`tray-icon-btn ${snapshot.refreshing ? 'is-spinning' : ''}`}
                label={trayText.refresh}
                disabled={snapshot.refreshing || snapshot.repositories.length === 0}
                onClick={onRefresh}
              />
              <span className="tray-settings-anchor">
                <IconButton
                  icon="settings"
                  className="tray-icon-btn"
                  label={trayText.settings}
                  onClick={onOpenSettings}
                />
                {snapshot.hasUpdate && <span className="tray-update-dot" aria-hidden="true" />}
              </span>
            </div>
          </header>

          <div className="tray-body-scroll" data-tray-list>
            {runningServices.length > 0 && (
              <div className="tray-running-section" data-tray-running-services>
                <div className="tray-running-heading">
                  <span className="tray-status-dot tone-success" aria-hidden="true" />
                  <span>{runText.trayRunningSectionTitle(runningServices.length)}</span>
                </div>
                <div className="tray-running-list">
                  {runningServices.map((svc) => (
                    <div key={svc.processId} className="tray-running-row">
                      <button
                        type="button"
                        className="tray-running-info"
                        title={runText.trayOpenLogsTitle}
                        onClick={() => onOpenRunLogs?.(svc.processId)}
                      >
                        <span className="tray-running-label truncate">{svc.label}</span>
                        <span className="tray-running-repo mono truncate">{svc.repositoryName}</span>
                      </button>
                      <div className="tray-running-actions">
                        <IconButton
                          icon="terminal"
                          className="tray-icon-btn tray-run-btn"
                          label={runText.trayOpenLogsTitle}
                          onClick={() => onOpenRunLogs?.(svc.processId)}
                        />
                        <button
                          type="button"
                          className="tray-stop-btn"
                          disabled={svc.status === 'stopping'}
                          onClick={() => onStopRunService?.(svc.processId)}
                        >
                          <Icon name="square" />
                          <span>{runText.trayStopButton}</span>
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
            {snapshot.repositories.length === 0 ? (
              <div className="tray-empty">
                <p className="tray-empty-title">{trayText.emptyTitle}</p>
                <p className="tray-empty-subtitle">{trayText.emptySubtitle}</p>
              </div>
            ) : (
              <div className="tray-repo-list">
                {primary.map((repo) => (
                  <TrayRepoRow
                    key={repo.repositoryId}
                    repo={repo}
                    t={trayText}
                    onSelect={handleSelectRepo}
                  />
                ))}
                {foldedClean.length > 0 && (
                  <div className="tray-clean-group" data-tray-clean-group>
                    <button
                      type="button"
                      className="tray-clean-toggle"
                      aria-expanded={cleanExpanded}
                      onClick={() => setCleanExpanded((prev) => !prev)}
                      data-tray-clean-toggle
                    >
                      <RotatingChevron open={cleanExpanded} direction="right-down" />
                      <span className="truncate">{trayText.foldedCleanRow(foldedClean.length)}</span>
                    </button>
                    {cleanExpanded && (
                      <div className="tray-clean-list">
                        {foldedClean.map((repo) => (
                          <TrayRepoRow
                            key={repo.repositoryId}
                            repo={repo}
                            t={trayText}
                            onSelect={handleSelectRepo}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>

          <footer className="tray-footer">
            <button
              type="button"
              className="tray-footer-btn"
              onClick={() => onOpenMainWindow({ kind: 'show' })}
              data-tray-open-main
            >
              <Icon name="external" />
              <span>{trayText.openMainWindow}</span>
            </button>
            <button
              type="button"
              className="tray-footer-btn tray-footer-quit"
              onClick={onQuit}
              data-tray-quit
            >
              <Icon name="power" />
              <span>{trayText.quit}</span>
            </button>
          </footer>
        </section>
      </div>
    </I18nProvider>
  );
}
