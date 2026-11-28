import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AppLocale } from '../../../shared/language-protocol';
import {
  isRunProcessActive,
  type RunLogBatchEvent,
  type RunLogLine,
  type RunProcessSnapshot,
} from '../../../shared/run-protocol';
import type { AppTheme } from '../../../shared/theme-protocol';
import { getMessages, I18nProvider } from '../../i18n';
import { Icon } from '../../ui/controls';
import { useResolvedTheme } from '../../ui/theme';

export interface RunLogsWindowProps {
  locale: AppLocale;
  theme: AppTheme;
  processes: RunProcessSnapshot[];
  selectedProcessId: string | null;
  onSelectProcess: (processId: string) => void;
  loadLogs: (processId: string) => Promise<RunLogLine[]>;
  subscribeLogs: (receive: (batch: RunLogBatchEvent) => void) => () => void;
  onStop: (processId: string) => Promise<void>;
  onClear: (processId: string) => Promise<void>;
  onOpenUrl: (url: string) => Promise<void>;
}

const MAX_RENDERED_LINES = 2000;

export interface RunLogBucket {
  generation: number;
  clearedSeq: number;
  lines: RunLogLine[];
}

export function mergeLoadedLogBucket(
  existing: RunLogBucket | undefined,
  generation: number,
  initialLines: RunLogLine[],
): RunLogBucket {
  const sameGen = existing && existing.generation === generation;
  const clearedSeq = sameGen ? existing.clearedSeq : 0;
  const filteredInitial = initialLines.filter((line) => line.seq > clearedSeq);
  const maxInitialSeq = filteredInitial.length > 0
    ? filteredInitial[filteredInitial.length - 1].seq
    : clearedSeq;
  const newerLive = sameGen
    ? existing.lines.filter((line) => line.seq > maxInitialSeq)
    : [];
  const combined = [...filteredInitial, ...newerLive];
  return {
    generation,
    clearedSeq,
    lines: combined.length > MAX_RENDERED_LINES
      ? combined.slice(-MAX_RENDERED_LINES)
      : combined,
  };
}

export function applyLogBatchToBucket(
  existing: RunLogBucket | undefined,
  batchGeneration: number,
  batchLines: RunLogLine[],
): RunLogBucket | undefined {
  if (!batchLines.length) return existing;
  if (existing && batchGeneration < existing.generation) {
    return existing;
  }
  const base: RunLogBucket = !existing || batchGeneration > existing.generation
    ? { generation: batchGeneration, clearedSeq: 0, lines: [] }
    : existing;
  const lastSeq = base.lines.length > 0 ? base.lines[base.lines.length - 1].seq : 0;
  const floorSeq = Math.max(lastSeq, base.clearedSeq);
  const fresh = batchLines.filter((line) => line.seq > floorSeq);
  if (!fresh.length) return base;
  const merged = [...base.lines, ...fresh];
  return {
    generation: base.generation,
    clearedSeq: base.clearedSeq,
    lines: merged.length > MAX_RENDERED_LINES
      ? merged.slice(-MAX_RENDERED_LINES)
      : merged,
  };
}

export function applyClearToBucket(
  existing: RunLogBucket | undefined,
  generation: number,
  watermarkSeq: number,
): RunLogBucket {
  if (!existing || existing.generation !== generation) {
    return { generation, clearedSeq: watermarkSeq, lines: [] };
  }
  const nextCleared = Math.max(existing.clearedSeq, watermarkSeq);
  return {
    generation: existing.generation,
    clearedSeq: nextCleared,
    lines: existing.lines.filter((line) => line.seq > nextCleared),
  };
}

export function RunLogsWindow(props: RunLogsWindowProps) {
  useResolvedTheme(props.theme);
  const t = getMessages(props.locale).run;
  const [logsByProcess, setLogsByProcess] = useState<Record<string, RunLogBucket>>({});
  const logsByProcessRef = useRef(logsByProcess);
  logsByProcessRef.current = logsByProcess;
  const [stoppingIds, setStoppingIds] = useState<string[]>([]);
  const [followBottom, setFollowBottom] = useState(true);
  const viewportRef = useRef<HTMLDivElement>(null);

  const activeProcess = useMemo(() => {
    if (!props.processes.length) return undefined;
    return (
      props.processes.find((p) => p.processId === props.selectedProcessId)
      ?? props.processes[0]
    );
  }, [props.processes, props.selectedProcessId]);

  const activeProcessId = activeProcess?.processId ?? null;
  const activeGeneration = activeProcess?.generation ?? 0;

  // Reset stored log bucket when a process restarts onto a newer generation:
  useEffect(() => {
    if (!activeProcessId) return;
    setLogsByProcess((prev) => {
      const existing = prev[activeProcessId];
      if (!existing || existing.generation >= activeGeneration) return prev;
      return {
        ...prev,
        [activeProcessId]: { generation: activeGeneration, clearedSeq: 0, lines: [] },
      };
    });
  }, [activeProcessId, activeGeneration]);

  useEffect(() => {
    if (!activeProcessId) return;
    let mounted = true;
    const targetPid = activeProcessId;
    const targetGen = activeGeneration;
    void props.loadLogs(targetPid)
      .then((initialLines) => {
        if (!mounted) return;
        setLogsByProcess((prev) => {
          const existing = prev[targetPid];
          if (existing && existing.generation > targetGen) return prev;
          return {
            ...prev,
            [targetPid]: mergeLoadedLogBucket(existing, targetGen, initialLines),
          };
        });
      })
      .catch(() => {});
    return () => {
      mounted = false;
    };
  }, [activeProcessId, activeGeneration, props.loadLogs]);

  useEffect(() => {
    return props.subscribeLogs((batch) => {
      if (!batch.lines.length) return;
      const batchGen = batch.generation ?? 0;
      setLogsByProcess((prev) => {
        const current = prev[batch.processId];
        const next = applyLogBatchToBucket(current, batchGen, batch.lines);
        if (!next || next === current) return prev;
        return {
          ...prev,
          [batch.processId]: next,
        };
      });
    });
  }, [props.subscribeLogs]);

  const activeBucket = activeProcessId ? logsByProcess[activeProcessId] : undefined;
  const activeLines = activeBucket && activeBucket.generation === activeGeneration
    ? activeBucket.lines
    : [];

  useEffect(() => {
    if (!followBottom) return;
    const node = viewportRef.current;
    if (node) {
      node.scrollTop = node.scrollHeight;
    }
  }, [activeLines.length, activeProcessId, followBottom]);

  const handleScroll = useCallback(() => {
    const node = viewportRef.current;
    if (!node) return;
    const distanceFromBottom = node.scrollHeight - node.scrollTop - node.clientHeight;
    setFollowBottom(distanceFromBottom <= 32);
  }, []);

  const jumpToBottom = useCallback(() => {
    const node = viewportRef.current;
    if (node) {
      node.scrollTop = node.scrollHeight;
    }
    setFollowBottom(true);
  }, []);

  const handleStop = useCallback(async () => {
    if (!activeProcessId) return;
    const targetPid = activeProcessId;
    setStoppingIds((prev) => (prev.includes(targetPid) ? prev : [...prev, targetPid]));
    try {
      await props.onStop(targetPid);
    } catch {
      // Failure state is reflected by backend process snapshots.
    } finally {
      setStoppingIds((prev) => prev.filter((id) => id !== targetPid));
    }
  }, [activeProcessId, props]);

  const handleClear = useCallback(async () => {
    if (!activeProcessId) return;
    const targetPid = activeProcessId;
    const targetGen = activeGeneration;
    const beforeBucket = logsByProcessRef.current[targetPid];
    const watermarkSeq = beforeBucket && beforeBucket.generation === targetGen && beforeBucket.lines.length > 0
      ? beforeBucket.lines[beforeBucket.lines.length - 1].seq
      : (beforeBucket?.clearedSeq ?? 0);
    try {
      await props.onClear(targetPid);
      setLogsByProcess((prev) => ({
        ...prev,
        [targetPid]: applyClearToBucket(prev[targetPid], targetGen, watermarkSeq),
      }));
    } catch {
      // Ignore clear failures without wiping unread output.
    }
  }, [activeProcessId, activeGeneration, props]);

  const statusText = activeProcess
    ? activeProcess.status === 'starting'
      ? t.statusStarting
      : activeProcess.status === 'running'
        ? t.statusRunning
        : activeProcess.status === 'stopping'
          ? t.statusStopping
          : activeProcess.status === 'failed'
            ? t.statusFailed(activeProcess.exitCode)
            : t.statusExited(activeProcess.exitCode)
    : '';

  return (
    <I18nProvider locale={props.locale}>
      <div className="run-logs-shell">
        <header className="run-logs-titlebar" data-tauri-drag-region>
          <div className="run-logs-tabs" role="tablist" aria-label={t.logsWindowTitle}>
            {props.processes.map((proc) => {
              const selected = proc.processId === activeProcessId;
              return (
                <button
                  type="button"
                  role="tab"
                  key={proc.processId}
                  aria-selected={selected}
                  className={`run-logs-tab ${selected ? 'is-active' : ''}`}
                  title={`${proc.repositoryName} · ${proc.label}`}
                  onClick={() => {
                    setFollowBottom(true);
                    props.onSelectProcess(proc.processId);
                  }}
                >
                  <span className={`run-logs-dot is-${proc.status}`} aria-hidden="true" />
                  <span className="truncate">{proc.label}</span>
                  <span className="run-logs-tab-repo truncate">{proc.repositoryName}</span>
                </button>
              );
            })}
          </div>
        </header>

        {!activeProcess ? (
          <div className="run-logs-empty">
            <Icon name="terminal" />
            <h2>{t.logsEmptyTitle}</h2>
            <p>{t.logsEmptyDescription}</p>
          </div>
        ) : (
          <>
            <div className="run-logs-toolbar">
              <div className="run-logs-meta">
                <div className="run-logs-command" title={activeProcess.command}>
                  $ {activeProcess.command}
                </div>
                <div className="run-logs-submeta">
                  <span>{statusText}</span>
                  <span>·</span>
                  <span>{activeProcess.repositoryName}</span>
                  {activeProcess.cwd !== '.' && (
                    <>
                      <span>·</span>
                      <span>{activeProcess.cwd}</span>
                    </>
                  )}
                  {activeProcess.detectedUrls.length > 0 && (
                    <span className="run-logs-urls">
                      {activeProcess.detectedUrls.map((url) => (
                        <button
                          type="button"
                          key={url}
                          className="run-logs-url-chip"
                          title={t.logsOpenUrlTitle(url)}
                          onClick={() => void props.onOpenUrl(url).catch(() => {})}
                        >
                          <Icon name="external" />
                          <span>{url.replace(/^https?:\/\//, '')}</span>
                        </button>
                      ))}
                    </span>
                  )}
                </div>
              </div>
              <div className="run-logs-actions">
                <button
                  type="button"
                  className="run-logs-btn"
                  disabled={activeLines.length === 0}
                  onClick={() => void handleClear()}
                >
                  <Icon name="trash" />
                  <span>{t.logsClearButton}</span>
                </button>
                {isRunProcessActive(activeProcess.status) && (
                  <button
                    type="button"
                    className="run-logs-btn is-danger"
                    disabled={stoppingIds.includes(activeProcess.processId) || activeProcess.status === 'stopping'}
                    onClick={() => void handleStop()}
                  >
                    <Icon name="square" />
                    <span>{t.stopAction}</span>
                  </button>
                )}
              </div>
            </div>

            <div className="run-logs-viewport-wrap">
              <div
                ref={viewportRef}
                className="run-logs-viewport"
                role="log"
                aria-live="off"
                onScroll={handleScroll}
              >
                {activeLines.length === 0 ? (
                  <p className="run-logs-line stream-system">{t.logsNoOutputYet}</p>
                ) : (
                  activeLines.map((line) => (
                    <div
                      key={line.seq}
                      className={`run-logs-line stream-${line.stream}`}
                    >
                      {line.text || '\u00a0'}
                    </div>
                  ))
                )}
              </div>
              {!followBottom && activeLines.length > 0 && (
                <button
                  type="button"
                  className="run-logs-follow-pill"
                  onClick={jumpToBottom}
                >
                  <Icon name="down" />
                  <span>{t.logsFollowBottom}</span>
                </button>
              )}
            </div>
          </>
        )}
      </div>
    </I18nProvider>
  );
}
