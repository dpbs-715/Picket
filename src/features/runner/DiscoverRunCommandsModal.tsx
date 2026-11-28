import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  RUN_COMMAND_LIMITS,
  normalizeRunIcon,
  sanitizeRunCommandConfig,
  type DiscoverRunCommandsProgressEvent,
  type DiscoverRunCommandsReply,
  type RunCommandConfig,
  type RunCommandIconId,
} from '../../../shared/run-protocol';
import { useI18n } from '../../i18n';
import { Icon, IconButton, Overlay, RotatingChevron } from '../../ui/controls';
import { RunCommandIcon } from './RunCommandIcon';
import { resolveRunCommandIcon } from './runIcon';

const DISCOVER_FILTER_THRESHOLD = 6;
const UNGROUPED_KEY = '__ungrouped__';

interface CandidateRow {
  key: string;
  label: string;
  command: string;
  cwd?: string;
  group?: string;
  icon?: RunCommandIconId;
  description: string;
  category: string;
  selected: boolean;
  alreadyAdded: boolean;
}

interface Props {
  repositoryName: string;
  existingCommands: RunCommandConfig[];
  hasModel: boolean;
  canDiscover: boolean;
  covered?: boolean;
  onClose(): void;
  onConfigureModel(): void;
  onAddManual(): void;
  onDiscover(
    requestId: string,
    onProgress: (event: DiscoverRunCommandsProgressEvent) => void,
  ): Promise<DiscoverRunCommandsReply>;
  onCancelDiscover?(requestId: string): void;
  onAddCommands(commands: RunCommandConfig[]): void;
}

function GroupCheckbox({
  checked,
  indeterminate,
  disabled,
  label,
  onChange,
}: {
  checked: boolean;
  indeterminate: boolean;
  disabled: boolean;
  label: string;
  onChange(): void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return (
    <input
      ref={ref}
      type="checkbox"
      className="file-checkbox"
      checked={checked}
      disabled={disabled}
      aria-label={label}
      onClick={(e) => e.stopPropagation()}
      onChange={onChange}
    />
  );
}

export function DiscoverRunCommandsModal(props: Props) {
  const { t } = useI18n();
  const [scanning, setScanning] = useState(false);
  const [statusLabel, setStatusLabel] = useState('');
  const [scanSteps, setScanSteps] = useState<string[]>([]);
  const [summary, setSummary] = useState('');
  const [rows, setRows] = useState<CandidateRow[]>([]);
  const [filterQuery, setFilterQuery] = useState('');
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [hasScanned, setHasScanned] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeRequestRef = useRef<string | null>(null);
  const mountedRef = useRef(true);
  const didAutoStartRef = useRef(false);

  const existingCommandKeys = useRef<Set<string>>(new Set());
  existingCommandKeys.current = new Set(
    props.existingCommands.map((c) => `${c.cwd ?? '.'}::${c.command.trim()}`),
  );

  const cancelActive = useCallback(() => {
    const rid = activeRequestRef.current;
    if (rid) {
      activeRequestRef.current = null;
      props.onCancelDiscover?.(rid);
    }
  }, [props]);

  const startScan = useCallback(async () => {
    if (!props.hasModel || !props.canDiscover) return;
    cancelActive();
    const requestId = `disc-${crypto.randomUUID().slice(0, 12)}`;
    activeRequestRef.current = requestId;
    setScanning(true);
    setError(null);
    setFilterQuery('');
    setCollapsedGroups({});
    setEditingKey(null);
    setStatusLabel(t.run.discoverScanning);
    setScanSteps([t.run.discoverScanning]);
    try {
      const reply = await props.onDiscover(requestId, (event) => {
        if (mountedRef.current && activeRequestRef.current === requestId) {
          const nextLabel = event.statusLabel?.trim() || t.run.discoverScanning;
          setStatusLabel(nextLabel);
          setScanSteps((prev) => {
            if (prev[prev.length - 1] === nextLabel) return prev;
            return [...prev, nextLabel].slice(-5);
          });
        }
      });
      if (!mountedRef.current || activeRequestRef.current !== requestId) return;
      activeRequestRef.current = null;
      setScanning(false);
      if (reply.status === 'cancelled') return;
      setHasScanned(true);
      setSummary(reply.summary);
      setRows(
        reply.commands.map((item, idx) => {
          const dupKey = `${item.cwd ?? '.'}::${item.command.trim()}`;
          const alreadyAdded = existingCommandKeys.current.has(dupKey);
          const cleanIcon = normalizeRunIcon(item.icon);
          return {
            key: `${idx}:${dupKey}`,
            label: item.label,
            command: item.command,
            ...(item.cwd ? { cwd: item.cwd } : {}),
            ...(item.group?.trim() ? { group: item.group.trim() } : {}),
            ...(cleanIcon ? { icon: cleanIcon } : {}),
            description: item.description,
            category: item.category,
            selected: !alreadyAdded,
            alreadyAdded,
          };
        }),
      );
    } catch (err) {
      if (!mountedRef.current || activeRequestRef.current !== requestId) return;
      activeRequestRef.current = null;
      setScanning(false);
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [cancelActive, props, t.run.discoverScanning]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      cancelActive();
    };
    // Cancel any in-flight discovery when modal unmounts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!props.hasModel || !props.canDiscover || didAutoStartRef.current) return;
    didAutoStartRef.current = true;
    void startScan();
  }, [props.canDiscover, props.hasModel, startScan]);

  const selectableRows = rows.filter((r) => !r.alreadyAdded);
  const selectedRows = selectableRows.filter((r) => r.selected);
  const allSelected = selectableRows.length > 0 && selectedRows.length === selectableRows.length;
  const selectAllIndeterminate = selectedRows.length > 0 && selectedRows.length < selectableRows.length;

  const hasGroups = useMemo(() => rows.some((r) => Boolean(r.group)), [rows]);
  const normalizedQuery = filterQuery.trim().toLowerCase();
  const visibleRows = useMemo(() => {
    if (!normalizedQuery) return rows;
    return rows.filter((row) =>
      `${row.label} ${row.group ?? ''} ${row.cwd ?? ''} ${row.command} ${row.description}`
        .toLowerCase()
        .includes(normalizedQuery),
    );
  }, [rows, normalizedQuery]);

  const groupedVisible = useMemo(() => {
    if (!hasGroups) return [];
    const map = new Map<string, { key: string; title: string; rows: CandidateRow[] }>();
    for (const row of visibleRows) {
      const rawGroup = row.group?.trim() ?? '';
      const key = rawGroup || UNGROUPED_KEY;
      const title = rawGroup || t.run.ungroupedSectionTitle;
      let bucket = map.get(key);
      if (!bucket) {
        bucket = { key, title, rows: [] };
        map.set(key, bucket);
      }
      bucket.rows.push(row);
    }
    const named: Array<{ key: string; title: string; rows: CandidateRow[] }> = [];
    let ungrouped: { key: string; title: string; rows: CandidateRow[] } | undefined;
    for (const bucket of map.values()) {
      if (bucket.key === UNGROUPED_KEY) ungrouped = bucket;
      else named.push(bucket);
    }
    if (ungrouped) named.push(ungrouped);
    return named;
  }, [hasGroups, t.run.ungroupedSectionTitle, visibleRows]);

  function toggleAll() {
    setRows((prev) =>
      prev.map((row) => (row.alreadyAdded ? row : { ...row, selected: !allSelected })),
    );
  }

  function toggleGroupSelection(groupKey: string) {
    const groupSelectable = rows.filter(
      (r) => !r.alreadyAdded && ((r.group?.trim() || UNGROUPED_KEY) === groupKey),
    );
    const allInGroupSelected = groupSelectable.length > 0 && groupSelectable.every((r) => r.selected);
    setRows((prev) =>
      prev.map((row) => {
        if (row.alreadyAdded) return row;
        const k = row.group?.trim() || UNGROUPED_KEY;
        return k === groupKey ? { ...row, selected: !allInGroupSelected } : row;
      }),
    );
  }

  function toggleGroupCollapse(groupKey: string) {
    setCollapsedGroups((prev) => ({ ...prev, [groupKey]: !prev[groupKey] }));
  }

  function toggleRow(key: string) {
    setRows((prev) =>
      prev.map((row) => (row.key === key && !row.alreadyAdded ? { ...row, selected: !row.selected } : row)),
    );
  }

  function updateRowField(key: string, field: 'label' | 'command', value: string) {
    setRows((prev) =>
      prev.map((row) => (row.key === key ? { ...row, [field]: value } : row)),
    );
  }

  function handleConfirm() {
    const remainingCapacity = Math.max(0, RUN_COMMAND_LIMITS.maxPerRepo - props.existingCommands.length);
    const seenKeys = new Set(existingCommandKeys.current);
    const seenLabels = new Set(props.existingCommands.map((c) => c.label.trim().toLowerCase()));
    const chosen: RunCommandConfig[] = [];
    for (const row of selectedRows) {
      if (chosen.length >= remainingCapacity) break;
      const baseLabel = row.label.replace(/\0/g, '').trim();
      const command = row.command.replace(/\0/g, '').trim();
      if (!baseLabel || !command) continue;
      let uniqueLabel = baseLabel.slice(0, RUN_COMMAND_LIMITS.maxLabelChars);
      let suffix = 2;
      while (seenLabels.has(uniqueLabel.toLowerCase())) {
        const tag = ` (${suffix})`;
        uniqueLabel = `${baseLabel.slice(0, Math.max(1, RUN_COMMAND_LIMITS.maxLabelChars - tag.length))}${tag}`;
        suffix += 1;
      }
      try {
        const sanitized = sanitizeRunCommandConfig({
          id: `run-${crypto.randomUUID().slice(0, 12)}`,
          label: uniqueLabel,
          command,
          ...(row.cwd ? { cwd: row.cwd } : {}),
          ...(row.group ? { group: row.group } : {}),
          ...(row.icon ? { icon: row.icon } : {}),
        });
        const key = `${sanitized.cwd ?? '.'}::${sanitized.command}`;
        if (seenKeys.has(key)) continue;
        seenKeys.add(key);
        seenLabels.add(sanitized.label.toLowerCase());
        chosen.push(sanitized);
      } catch {
        continue;
      }
    }
    if (chosen.length > 0) {
      props.onAddCommands(chosen);
    }
    props.onClose();
  }

  function renderCandidateRow(row: CandidateRow) {
    const iconId = resolveRunCommandIcon(row.icon, row.command, row.label);
    const isEditing = editingKey === row.key && !row.alreadyAdded;
    const tooltip = row.description ? `${row.command}\n${row.description}` : row.command;

    if (isEditing) {
      return (
        <div
          key={row.key}
          className="run-discover-row is-editing"
          data-discover-editing={row.key}
        >
          <input
            type="checkbox"
            className="file-checkbox"
            checked={row.selected}
            disabled={row.alreadyAdded}
            aria-label={row.label}
            onChange={() => toggleRow(row.key)}
          />
          <div className="run-discover-edit-box min-width-zero">
            <div className="run-discover-edit-head">
              <RunCommandIcon icon={iconId} size="sm" />
              <input
                type="text"
                className="run-discover-edit-input"
                value={row.label}
                maxLength={RUN_COMMAND_LIMITS.maxLabelChars}
                onChange={(e) => updateRowField(row.key, 'label', e.target.value)}
              />
              <button
                type="button"
                className="secondary compact"
                onClick={() => setEditingKey(null)}
              >
                {t.run.discoverEditDone}
              </button>
            </div>
            <input
              type="text"
              className="run-discover-edit-input mono"
              value={row.command}
              maxLength={RUN_COMMAND_LIMITS.maxCommandChars}
              onChange={(e) => updateRowField(row.key, 'command', e.target.value)}
              spellCheck={false}
            />
            {row.description && (
              <span className="muted small">{row.description}</span>
            )}
          </div>
        </div>
      );
    }

    return (
      <div
        key={row.key}
        className={`run-discover-row ${row.alreadyAdded ? 'is-added' : row.selected ? 'is-selected' : ''}`}
      >
        <input
          type="checkbox"
          className="file-checkbox"
          checked={row.selected}
          disabled={row.alreadyAdded}
          aria-label={row.label}
          onChange={() => toggleRow(row.key)}
        />
        <RunCommandIcon icon={iconId} size="sm" />
        <button
          type="button"
          className="run-discover-row-copy min-width-zero"
          disabled={row.alreadyAdded}
          title={tooltip}
          onClick={() => toggleRow(row.key)}
        >
          <span className="run-discover-row-top">
            <span className="run-discover-row-title truncate">{row.label}</span>
            {row.cwd && <span className="run-cwd-badge mono">{row.cwd}</span>}
            {row.alreadyAdded && (
              <span className="run-added-badge">{t.run.discoverAlreadyAdded}</span>
            )}
          </span>
          <span className="run-discover-row-cmd mono">{row.command}</span>
        </button>
        {!row.alreadyAdded && (
          <IconButton
            icon="pencil"
            label={t.run.editAction}
            className="run-discover-edit-btn"
            onClick={() => setEditingKey(row.key)}
          />
        )}
      </div>
    );
  }

  return (
    <Overlay
      title={t.run.discoverModalTitle}
      covered={props.covered}
      onClose={() => {
        cancelActive();
        props.onClose();
      }}
    >
      <div className="custom-action-modal">
        <div className="custom-action-shell-head run-modal-head-row">
          <div className="custom-action-repo-meta min-width-zero">
            <Icon name="folder" />
            <span className="mono truncate">{props.repositoryName}</span>
          </div>
          {(hasScanned || Boolean(error)) && !scanning && props.hasModel && props.canDiscover && (
            <button
              type="button"
              className="secondary compact"
              onClick={() => void startScan()}
            >
              <Icon name="refresh" />
              <span>{t.run.discoverRescan}</span>
            </button>
          )}
        </div>

        <div className="custom-action-shell-body scroll-area">
          {!props.hasModel ? (
            <div className="run-discover-empty">
              <p className="muted small">{t.run.discoverNoModelNote}</p>
              <div className="row">
                <button type="button" className="primary" onClick={props.onConfigureModel}>
                  <Icon name="settings" />
                  <span>{t.run.discoverConfigureModel}</span>
                </button>
                <button type="button" className="secondary" onClick={props.onAddManual}>
                  <Icon name="plus" />
                  <span>{t.run.addManualButton}</span>
                </button>
              </div>
            </div>
          ) : scanning ? (
            <div className="run-discover-scanning" role="status" aria-live="polite">
              <div className="run-scan-status-card">
                <div className="run-scan-status-head">
                  <span className="run-scan-pulse-ring" aria-hidden="true">
                    <Icon name="sparkles" />
                  </span>
                  <div className="run-scan-status-copy min-width-zero">
                    <span className="run-scan-title">{t.run.discoverScanStepsTitle}</span>
                    <span className="run-scan-current mono truncate" title={statusLabel}>
                      {statusLabel || t.run.discoverScanning}
                    </span>
                  </div>
                </div>
                <div className="run-scan-progress-track" aria-hidden="true">
                  <div className="run-scan-progress-beam" />
                </div>
                {scanSteps.length > 0 && (
                  <ul className="run-scan-trail">
                    {scanSteps.map((step, idx) => {
                      const isCurrent = idx === scanSteps.length - 1;
                      return (
                        <li key={`${idx}:${step}`} className={`run-scan-step ${isCurrent ? 'is-active' : 'is-done'}`}>
                          {isCurrent ? (
                            <span className="run-scan-step-dot" aria-hidden="true" />
                          ) : (
                            <Icon name="check" className="run-scan-step-check" />
                          )}
                          <span className="mono truncate">{step}</span>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>

              <div className="run-scan-skeletons" aria-hidden="true">
                {[0, 1, 2].map((idx) => (
                  <div key={idx} className="run-scan-skeleton-card" style={{ animationDelay: `${idx * 120}ms` }}>
                    <span className="run-skel-box" />
                    <div className="run-skel-lines">
                      <span className="run-skel-line run-skel-title" />
                      <span className="run-skel-line run-skel-cmd" />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <>
              {error && <p className="error-text small">{error}</p>}
              {summary && !error && (
                <p className="run-discover-summary">{summary}</p>
              )}
              {hasScanned && rows.length === 0 && !error && (
                <div className="run-discover-empty">
                  <p className="muted small">{t.run.discoverEmptyResult}</p>
                  <button type="button" className="secondary" onClick={props.onAddManual}>
                    <Icon name="plus" />
                    <span>{t.run.addManualButton}</span>
                  </button>
                </div>
              )}
              {rows.length > 0 && (
                <div className="run-discover-list">
                  {rows.length >= DISCOVER_FILTER_THRESHOLD && (
                    <div className="run-filter-bar run-discover-filter-bar">
                      <Icon name="search" className="run-filter-icon" />
                      <input
                        type="search"
                        className="run-filter-input"
                        data-run-discover-filter
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
                          onClick={() => setFilterQuery('')}
                        >
                          <Icon name="close" />
                        </button>
                      )}
                    </div>
                  )}

                  {selectableRows.length > 1 && (
                    <div className="run-discover-toolbar-row">
                      <label className="run-discover-select-all">
                        <GroupCheckbox
                          checked={allSelected}
                          indeterminate={selectAllIndeterminate}
                          disabled={false}
                          label={t.run.discoverSelectAll}
                          onChange={toggleAll}
                        />
                        <span>{t.run.discoverSelectAll}</span>
                      </label>
                      <span className="run-discover-group-count mono">
                        {selectedRows.length}/{rows.length}
                      </span>
                    </div>
                  )}

                  {visibleRows.length === 0 ? (
                    <div className="run-discover-empty">
                      <p className="muted small">{t.run.filterEmptyResult(filterQuery.trim())}</p>
                    </div>
                  ) : hasGroups ? (
                    groupedVisible.map((bucket) => {
                      const allGroupRows = rows.filter(
                        (r) => (r.group?.trim() || UNGROUPED_KEY) === bucket.key,
                      );
                      const groupSelectable = allGroupRows.filter((r) => !r.alreadyAdded);
                      const groupSelectedCount = groupSelectable.filter((r) => r.selected).length;
                      const groupAllChecked =
                        groupSelectable.length > 0 && groupSelectedCount === groupSelectable.length;
                      const groupIndeterminate =
                        groupSelectedCount > 0 && groupSelectedCount < groupSelectable.length;
                      const collapsed = Boolean(collapsedGroups[bucket.key]) && !normalizedQuery;

                      return (
                        <div key={bucket.key} className="run-discover-group" data-discover-group={bucket.key}>
                          <div className="run-discover-group-head">
                            <GroupCheckbox
                              checked={groupAllChecked}
                              indeterminate={groupIndeterminate}
                              disabled={groupSelectable.length === 0}
                              label={bucket.title}
                              onChange={() => toggleGroupSelection(bucket.key)}
                            />
                            <button
                              type="button"
                              className="run-discover-group-toggle"
                              data-discover-group-toggle={bucket.key}
                              aria-expanded={!collapsed}
                              onClick={() => toggleGroupCollapse(bucket.key)}
                            >
                              <RotatingChevron open={!collapsed} direction="right-down" />
                              <span className="run-discover-group-name truncate">{bucket.title}</span>
                              <span className="run-discover-group-count mono">
                                {groupSelectedCount}/{allGroupRows.length}
                              </span>
                            </button>
                          </div>
                          {!collapsed && (
                            <div className="run-discover-group-list">
                              {bucket.rows.map(renderCandidateRow)}
                            </div>
                          )}
                        </div>
                      );
                    })
                  ) : (
                    <div className="run-discover-group-list">
                      {visibleRows.map(renderCandidateRow)}
                    </div>
                  )}
                </div>
              )}
            </>
          )}
        </div>

        <div className="overlay-footer">
          <div className="form-actions">
            <button
              type="button"
              className="secondary"
              onClick={() => {
                cancelActive();
                props.onClose();
              }}
            >
              {scanning ? t.run.discoverCancel : t.common.cancel}
            </button>
            <button
              type="button"
              className="primary"
              disabled={scanning || selectedRows.length === 0}
              onClick={handleConfirm}
            >
              {t.run.discoverAddSelected(selectedRows.length)}
            </button>
          </div>
        </div>
      </div>
    </Overlay>
  );
}
