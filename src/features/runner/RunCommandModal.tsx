import { useId, useMemo, useState } from 'react';
import {
  RUN_COMMAND_LIMITS,
  normalizeRunCwd,
  normalizeRunGroup,
  normalizeRunIcon,
  sanitizeRunCommandConfig,
  type RunCommandConfig,
  type RunCommandIconId,
} from '../../../shared/run-protocol';
import { useI18n } from '../../i18n';
import { Icon, Overlay } from '../../ui/controls';
import { RunCommandIcon } from './RunCommandIcon';
import { formatRunEnvEntries, parseRunEnvText } from './runEnv';
import { RUN_ICON_IDS, inferRunCommandIcon } from './runIcon';

interface Props {
  repositoryName: string;
  existingCommands: RunCommandConfig[];
  initialCommand?: RunCommandConfig;
  runningCount?: number;
  covered?: boolean;
  onClose(): void;
  onSave(command: RunCommandConfig): void;
  onDelete?(commandId: string): void | Promise<void>;
}

export function RunCommandModal(props: Props) {
  const { t } = useI18n();
  const groupListId = useId();
  const isEditing = Boolean(props.initialCommand);
  const runningCount = props.runningCount ?? 0;
  const [label, setLabel] = useState(props.initialCommand?.label ?? '');
  const [group, setGroup] = useState(props.initialCommand?.group ?? '');
  const [command, setCommand] = useState(props.initialCommand?.command ?? '');
  const [cwd, setCwd] = useState(props.initialCommand?.cwd ?? '');
  const [selectedIcon, setSelectedIcon] = useState<'auto' | RunCommandIconId>(
    () => normalizeRunIcon(props.initialCommand?.icon) ?? 'auto',
  );
  const [envText, setEnvText] = useState(() => formatRunEnvEntries(props.initialCommand?.env));
  const [error, setError] = useState<string | null>(null);
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const existingGroups = useMemo(() => {
    const seen = new Set<string>();
    const list: string[] = [];
    for (const item of props.existingCommands) {
      const g = item.group?.trim();
      if (g && !seen.has(g)) {
        seen.add(g);
        list.push(g);
      }
    }
    return list;
  }, [props.existingCommands]);

  const inferredIcon = inferRunCommandIcon(command, label);

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    const cleanLabel = label.trim();
    const cleanCommand = command.replace(/\r\n/g, '\n').trim();
    if (!cleanLabel) {
      setError(t.run.validationLabelRequired);
      return;
    }
    if (!cleanCommand) {
      setError(t.run.validationCommandRequired);
      return;
    }
    if (cleanLabel.includes('\0') || cleanCommand.includes('\0')) {
      setError(t.run.validationInvalidCharacters);
      return;
    }
    if (!isEditing && props.existingCommands.length >= RUN_COMMAND_LIMITS.maxPerRepo) {
      setError(t.run.validationLimitReached);
      return;
    }
    const duplicate = props.existingCommands.some(
      (item) => item.id !== props.initialCommand?.id
        && item.label.trim().toLowerCase() === cleanLabel.toLowerCase(),
    );
    if (duplicate) {
      setError(t.run.validationDuplicateLabel);
      return;
    }
    let cleanGroup: string | undefined;
    try {
      cleanGroup = normalizeRunGroup(group);
    } catch {
      setError(t.run.validationInvalidGroup);
      return;
    }
    let cleanCwd: string | undefined;
    try {
      cleanCwd = normalizeRunCwd(cwd);
    } catch {
      setError(t.run.validationInvalidCwd);
      return;
    }
    const parsedEnv = parseRunEnvText(envText);
    if (!parsedEnv.ok) {
      setError(t.run.validationInvalidEnv);
      return;
    }

    const built = sanitizeRunCommandConfig({
      id: props.initialCommand?.id ?? `run-${crypto.randomUUID().slice(0, 12)}`,
      label: cleanLabel,
      command: cleanCommand,
      ...(cleanCwd ? { cwd: cleanCwd } : {}),
      ...(cleanGroup ? { group: cleanGroup } : {}),
      ...(selectedIcon !== 'auto' ? { icon: selectedIcon } : {}),
      ...(parsedEnv.entries.length ? { env: parsedEnv.entries } : {}),
    });
    setError(null);
    props.onSave(built);
  }

  async function performDelete() {
    if (!props.initialCommand || !props.onDelete || deleting) return;
    setDeleting(true);
    try {
      await props.onDelete(props.initialCommand.id);
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Overlay
      title={isEditing ? t.run.modalEditTitle : t.run.modalNewTitle}
      covered={props.covered}
      onClose={props.onClose}
    >
      <div className="custom-action-modal">
        <div className="custom-action-shell-head">
          <div className="custom-action-repo-meta">
            <Icon name="folder" />
            <span className="mono truncate">{props.repositoryName}</span>
          </div>
        </div>

        <form className="custom-action-form" onSubmit={handleSubmit}>
          <div className="custom-action-shell-body scroll-area">
            <label className="form-field">
              <span>{t.run.labelField}</span>
              <input
                data-autofocus
                maxLength={RUN_COMMAND_LIMITS.maxLabelChars}
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                placeholder={t.run.labelPlaceholder}
              />
            </label>

            <label className="form-field">
              <span>{t.run.groupField}</span>
              <input
                list={existingGroups.length > 0 ? groupListId : undefined}
                maxLength={RUN_COMMAND_LIMITS.maxGroupChars}
                value={group}
                onChange={(e) => setGroup(e.target.value)}
                placeholder={t.run.groupPlaceholder}
              />
              {existingGroups.length > 0 && (
                <datalist id={groupListId}>
                  {existingGroups.map((g) => (
                    <option key={g} value={g} />
                  ))}
                </datalist>
              )}
            </label>

            <label className="form-field">
              <span>{t.run.commandField}</span>
              <textarea
                className="mono"
                rows={3}
                maxLength={RUN_COMMAND_LIMITS.maxCommandChars}
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder={t.run.commandPlaceholder}
                spellCheck={false}
              />
            </label>

            <div className="form-field">
              <div className="run-icon-field-head">
                <span>{t.run.iconField}</span>
                <button
                  type="button"
                  className={`run-icon-auto-btn ${selectedIcon === 'auto' ? 'is-selected' : ''}`}
                  aria-pressed={selectedIcon === 'auto'}
                  onClick={() => setSelectedIcon('auto')}
                >
                  <RunCommandIcon icon={inferredIcon} size="sm" />
                  <span>{t.run.iconAutoOption}</span>
                </button>
              </div>
              <div className="run-icon-picker-grid" role="group" aria-label={t.run.iconField}>
                {RUN_ICON_IDS.map((iconId) => {
                  const title = t.run.iconLabels[iconId] ?? iconId;
                  const active = selectedIcon === iconId;
                  return (
                    <button
                      key={iconId}
                      type="button"
                      className={`run-icon-picker-cell ${active ? 'is-selected' : ''}`}
                      aria-label={title}
                      aria-pressed={active}
                      title={title}
                      onClick={() => setSelectedIcon(iconId)}
                    >
                      <RunCommandIcon icon={iconId} size="md" />
                    </button>
                  );
                })}
              </div>
            </div>

            <label className="form-field">
              <span>{t.run.cwdField}</span>
              <input
                className="mono"
                maxLength={RUN_COMMAND_LIMITS.maxCwdChars}
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                placeholder={t.run.cwdPlaceholder}
                spellCheck={false}
              />
            </label>

            <label className="form-field">
              <span>{t.run.envField}</span>
              <textarea
                className="mono"
                rows={3}
                value={envText}
                onChange={(e) => setEnvText(e.target.value)}
                placeholder={t.run.envPlaceholder}
                spellCheck={false}
              />
            </label>

            {error && <p className="error-text small">{error}</p>}
            {confirmDeleteOpen && runningCount > 0 && (
              <div className="run-delete-confirm" role="alertdialog" aria-label={t.run.deleteRunningConfirmTitle} data-run-delete-confirm>
                <span className="run-delete-confirm-title">{t.run.deleteRunningConfirmTitle}</span>
                <p className="run-delete-confirm-copy">{t.run.deleteRunningConfirmMessage(runningCount)}</p>
                <div className="run-delete-confirm-actions">
                  <button
                    type="button"
                    className="secondary compact"
                    disabled={deleting}
                    onClick={() => setConfirmDeleteOpen(false)}
                  >
                    {t.common.cancel}
                  </button>
                  <button
                    type="button"
                    className="secondary compact danger-text"
                    disabled={deleting}
                    data-run-delete-confirm-btn
                    onClick={() => void performDelete()}
                  >
                    {deleting ? t.run.deleteRunningStopping : t.run.deleteRunningConfirmAction}
                  </button>
                </div>
              </div>
            )}
          </div>

          <div className="overlay-footer">
            <div className="form-actions">
              {isEditing && props.onDelete && props.initialCommand ? (
                <button
                  type="button"
                  className="secondary danger-text"
                  disabled={deleting}
                  onClick={() => {
                    if (runningCount > 0) {
                      setConfirmDeleteOpen(true);
                    } else {
                      void performDelete();
                    }
                  }}
                >
                  <Icon name="trash" />
                  <span>{t.run.deleteAction}</span>
                </button>
              ) : <span />}
              <div className="row">
                <button type="button" className="secondary" disabled={deleting} onClick={props.onClose}>
                  {t.common.cancel}
                </button>
                <button type="submit" className="primary" disabled={deleting}>
                  {t.run.saveButton}
                </button>
              </div>
            </div>
          </div>
        </form>
      </div>
    </Overlay>
  );
}
