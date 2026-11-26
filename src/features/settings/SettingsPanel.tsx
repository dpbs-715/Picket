import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import type { SkillsStateSnapshot } from '../../../shared/skills-protocol';
import type { UpdateStateSnapshot } from '../../../shared/update-protocol';
import { LANGUAGE_OPTIONS, useI18n } from '../../i18n';
import { Icon, IconButton, Overlay, RotatingChevron, type OverlayVariant } from '../../ui/controls';
import { Collapse, MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../ui/motion';
import { editors } from '../workbench/state/workbench';
import type { EditorId } from '../workbench/state/workbench';
import { formatTokenCount, providerFor, resolveDefaultTokenLimits } from './modelProviders';
import type { ModelConfiguration, ProviderConnection } from './modelProviders';

export type SettingsTabId = 'general' | 'models' | 'skills';

interface Props {
  locale: AppLocale;
  repositoryName?: string;
  editor: EditorId;
  commitLanguage: CommitLanguage;
  installedEditors?: EditorId[];
  connections: ProviderConnection[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  covered: boolean;
  onLocale(locale: AppLocale): void;
  onEditor(editor: EditorId): void;
  onCommitLanguage(commitLanguage: CommitLanguage): void;
  onSelectModel(id: string): void;
  onAddProvider(): void;
  onEditProvider(connection: ProviderConnection): void;
  onDeleteProvider(connectionId: string): void;
  onAddModel(connection: ProviderConnection): void;
  onEditModel(connection: ProviderConnection, model: ModelConfiguration): void;
  onDeleteModel(id: string): void;
  skillsState?: SkillsStateSnapshot;
  skillsBusy?: 'sync' | 'toggle' | 'reset' | null;
  skillsError?: string;
  skillsNotice?: string;
  onSyncSkills?(sourceRepo?: string): Promise<boolean> | void;
  onToggleSkill?(name: string, enabled: boolean): void;
  onResetSkills?(): void;
  updateState?: UpdateStateSnapshot;
  gitBusyForUpdate?: boolean;
  onCheckUpdate?(): void;
  onInstallUpdate?(version: string): void;
  onCancelUpdate?(): void;
  onOpenReleasePage?(): void;
  onClose(): void;
  closeDisabled?: boolean;
  overlayVariant?: OverlayVariant;
  error?: string;
  focusModels?: boolean;
}

function formatMegabytes(bytes: number): string {
  return (bytes / (1024 * 1024)).toFixed(1);
}

function formatSyncedTimestamp(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function SettingsPanel(props: Props) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();
  const [activeTab, setActiveTab] = useState<SettingsTabId>(() => (props.focusModels ? 'models' : 'general'));
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [editingSkillSource, setEditingSkillSource] = useState(false);
  const [skillSourceDraft, setSkillSourceDraft] = useState('');

  const tabListRef = useRef<HTMLDivElement>(null);
  const generalTabRef = useRef<HTMLButtonElement>(null);
  const modelsTabRef = useRef<HTMLButtonElement>(null);
  const skillsTabRef = useRef<HTMLButtonElement>(null);
  const [tabIndicator, setTabIndicator] = useState<{ left: number; width: number } | null>(null);

  useEffect(() => {
    if (props.focusModels) {
      setActiveTab('models');
    }
  }, [props.focusModels]);

  useEffect(() => {
    if (confirmDeleteId && !props.connections.some((c) => c.id === confirmDeleteId)) {
      setConfirmDeleteId(null);
    }
  }, [confirmDeleteId, props.connections]);

  const syncTabIndicator = useCallback(() => {
    const activeButton = activeTab === 'models'
      ? modelsTabRef.current
      : activeTab === 'skills'
        ? skillsTabRef.current
        : generalTabRef.current;
    if (!activeButton || activeButton.offsetWidth <= 0) {
      setTabIndicator(null);
      return;
    }
    const nextLeft = activeButton.offsetLeft;
    const nextWidth = activeButton.offsetWidth;
    setTabIndicator((previous) =>
      previous && previous.left === nextLeft && previous.width === nextWidth
        ? previous
        : { left: nextLeft, width: nextWidth },
    );
  }, [activeTab]);

  useLayoutEffect(() => {
    syncTabIndicator();
  }, [syncTabIndicator, props.locale, props.models.length, props.skillsState?.skills.length]);

  useEffect(() => {
    if (typeof ResizeObserver === 'undefined') return;
    const listEl = tabListRef.current;
    if (!listEl) return;
    const observer = new ResizeObserver(() => syncTabIndicator());
    observer.observe(listEl);
    if (generalTabRef.current) observer.observe(generalTabRef.current);
    if (modelsTabRef.current) observer.observe(modelsTabRef.current);
    if (skillsTabRef.current) observer.observe(skillsTabRef.current);
    return () => observer.disconnect();
  }, [syncTabIndicator]);

  const visibleEditors = props.installedEditors
    ? editors.filter((item) => props.installedEditors!.includes(item.id))
    : editors;
  const editor = visibleEditors.find((item) => item.id === props.editor)
    ?? editors.find((item) => item.id === props.editor)
    ?? editors[0];
  const badgeText = props.repositoryName ?? t.settings.noProjectBadge;

  const skills = props.skillsState;
  const isSkillsBusy = Boolean(props.skillsBusy);
  const enabledSkillsCount = skills ? skills.skills.filter((item) => item.enabled).length : 0;
  const hasUpdate = props.updateState?.phase === 'available'
    || props.updateState?.phase === 'downloading'
    || props.updateState?.phase === 'installing';

  function toggleConnection(connectionId: string) {
    setCollapsed((prev) => ({ ...prev, [connectionId]: !prev[connectionId] }));
  }

  async function triggerSyncSkills() {
    if (!skills || !props.onSyncSkills || isSkillsBusy) return;
    const requestedRepo = editingSkillSource
      ? (skillSourceDraft.trim() || skills.defaultSourceRepo)
      : skills.sourceRepo;
    const succeeded = await props.onSyncSkills(requestedRepo);
    if (succeeded !== false && editingSkillSource) {
      setEditingSkillSource(false);
    }
  }

  return (
    <Overlay
      title={t.settings.title}
      onClose={props.onClose}
      covered={props.covered}
      closeDisabled={props.closeDisabled}
      variant={props.overlayVariant}
    >
      <div className="settings-tabs-bar">
        <div
          ref={tabListRef}
          className={`tab-list settings-tab-list ${tabIndicator ? '' : 'no-indicator'}`}
          role="tablist"
          aria-label={t.settings.tabsLabel}
        >
          {tabIndicator && (
            <span
              className={`tab-active-indicator ${instantMotion ? 'is-instant' : ''}`}
              aria-hidden="true"
              style={{ transform: `translateX(${tabIndicator.left}px)`, width: `${tabIndicator.width}px` }}
            />
          )}
          <button
            ref={generalTabRef}
            id="settings-tab-general"
            type="button"
            role="tab"
            data-settings-tab="general"
            aria-selected={activeTab === 'general'}
            onClick={() => setActiveTab('general')}
          >
            <span>{t.settings.tabs.general}</span>
            {hasUpdate && <span className="settings-tab-dot" aria-hidden="true" />}
          </button>
          <button
            ref={modelsTabRef}
            id="settings-tab-models"
            type="button"
            role="tab"
            data-settings-tab="models"
            aria-selected={activeTab === 'models'}
            onClick={() => setActiveTab('models')}
          >
            <span>{t.settings.tabs.models}</span>
            <span className="settings-tab-count mono">{props.models.length}</span>
          </button>
          {skills && (
            <button
              ref={skillsTabRef}
              id="settings-tab-skills"
              type="button"
              role="tab"
              data-settings-tab="skills"
              aria-selected={activeTab === 'skills'}
              onClick={() => setActiveTab('skills')}
            >
              <span>{t.settings.tabs.skills}</span>
              <span className="settings-tab-count mono">{enabledSkillsCount}/{skills.skills.length}</span>
            </button>
          )}
        </div>
      </div>

      <div className="settings-content scroll-area" role="tabpanel" aria-labelledby={`settings-tab-${activeTab}`}>
        {props.error && (
          <div className="settings-global-error">
            <p className="form-result danger" role="alert">{props.error}</p>
          </div>
        )}

        {activeTab === 'general' && (
          <>
            <section>
              <div className="section-heading">
                <div className="row min-width-zero">
                  <h2>{props.repositoryName ? t.settings.currentProject : t.settings.projectDefaults}</h2>
                  <span className="model-count project-badge mono truncate" title={badgeText}>{badgeText}</span>
                </div>
              </div>

              <div className="settings-subgroup">
                <div className="settings-subheading">
                  <span>{t.settings.editorIntegration}</span>
                </div>
                {visibleEditors.length === 0 ? (
                  <div className="settings-empty-inline">
                    <p className="muted help">{t.settings.noEditorsDetected}</p>
                  </div>
                ) : (
                  <div className="editor-grid" data-setting-group="editor" role="group" aria-label={t.settings.preferredEditor}>
                    {visibleEditors.map((item) => (
                      <button type="button" key={item.id} aria-pressed={item.id === editor.id} onClick={() => props.onEditor(item.id)}>
                        {item.name}{item.id === editor.id && <Icon name="check" />}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              <div className="settings-row-item">
                <span className="settings-row-label">{t.settings.commitLanguage}</span>
                <div className="segmented-control" data-setting-group="commit-language" role="group" aria-label={t.settings.commitLanguage}>
                  {LANGUAGE_OPTIONS.map((option) => (
                    <button type="button" key={option.id} aria-pressed={option.id === props.commitLanguage} onClick={() => props.onCommitLanguage(option.id)}>
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            </section>

            <section>
              <div className="row section-title-row">
                <h2>{t.settings.appSectionHeading}</h2>
              </div>

              <div className="settings-row-item">
                <span className="settings-row-label">{t.settings.appLanguage}</span>
                <div className="segmented-control" data-setting-group="app-language" role="group" aria-label={t.settings.appLanguage}>
                  {LANGUAGE_OPTIONS.map((option) => (
                    <button type="button" key={option.id} aria-pressed={option.id === props.locale} onClick={() => props.onLocale(option.id)}>
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>

              {props.updateState && (
                <div id="settings-updates-section" className="settings-updates-block" data-update-phase={props.updateState.phase}>
                  <div className="settings-row-item settings-update-row">
                    <div className="row min-width-zero">
                      <span className="settings-row-label">{t.settings.updates.heading}</span>
                      <span className="model-count mono">{t.settings.updates.currentVersion(props.updateState.currentVersion)}</span>
                      {props.updateState.phase === 'upToDate' && (
                        <span className="settings-uptodate-inline" role="status">
                          <Icon name="check" className="success" />
                          <span>{t.settings.updates.upToDate}</span>
                        </span>
                      )}
                    </div>
                    {props.onCheckUpdate && (
                      <button
                        type="button"
                        className="secondary compact"
                        disabled={
                          props.updateState.phase === 'checking'
                          || props.updateState.phase === 'downloading'
                          || props.updateState.phase === 'installing'
                        }
                        onClick={props.onCheckUpdate}
                      >
                        {props.updateState.phase === 'checking' && <Icon name="refresh" className="spinning" />}
                        {props.updateState.phase === 'checking' ? t.settings.updates.checking : t.settings.updates.checkButton}
                      </button>
                    )}
                  </div>

                  {props.updateState.phase !== 'idle'
                    && props.updateState.phase !== 'checking'
                    && props.updateState.phase !== 'upToDate' && (
                    <m.div
                      key={`update-card:${props.updateState.phase}:${props.updateState.availableVersion ?? ''}`}
                      initial={instantMotion ? false : { opacity: 0, y: 4 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
                    >
                      {props.updateState.phase === 'available' && props.updateState.availableVersion && (
                        <div className="update-card" role="region" aria-label={t.settings.updates.availableTitle(props.updateState.availableVersion)}>
                          <div className="row update-card-title">
                            <strong>{t.settings.updates.availableTitle(props.updateState.availableVersion)}</strong>
                          </div>
                          {props.updateState.releaseNotes && (
                            <p className="muted update-notes">{props.updateState.releaseNotes}</p>
                          )}
                          {props.updateState.installBlocker !== 'none' && (
                            <p className="help muted">
                              {t.settings.updates.blockers[props.updateState.installBlocker]}
                            </p>
                          )}
                          {props.updateState.installBlocker === 'none' && props.gitBusyForUpdate && (
                            <p className="help warning">{t.settings.updates.gitBusyNote}</p>
                          )}
                          {props.updateState.error && (
                            <p className="help danger" role="alert">{props.updateState.error}</p>
                          )}
                          <div className="row update-actions">
                            {props.updateState.canInstall && props.onInstallUpdate && (
                              <button
                                type="button"
                                className="primary compact"
                                disabled={props.gitBusyForUpdate}
                                onClick={() => props.onInstallUpdate?.(props.updateState!.availableVersion!)}
                              >
                                {t.settings.updates.updateAndRestart}
                              </button>
                            )}
                            {props.onOpenReleasePage && (
                              <button type="button" className="secondary compact" onClick={props.onOpenReleasePage}>
                                {t.settings.updates.downloadFromGithub}
                              </button>
                            )}
                          </div>
                        </div>
                      )}

                      {props.updateState.phase === 'downloading' && (
                        <div className="update-card" role="status">
                          {(() => {
                            const downloaded = props.updateState.downloadedBytes ?? 0;
                            const total = props.updateState.totalBytes;
                            const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
                            return (
                              <>
                                <div className="row update-progress-row">
                                  <span className="small">
                                    {percent !== null
                                      ? t.settings.updates.downloading(percent, formatMegabytes(downloaded), formatMegabytes(total!))
                                      : t.settings.updates.downloadingUnknown(formatMegabytes(downloaded))}
                                  </span>
                                  {props.onCancelUpdate && (
                                    <button type="button" className="compact" onClick={props.onCancelUpdate}>
                                      {t.common.cancel}
                                    </button>
                                  )}
                                </div>
                                <div className="update-progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? undefined}>
                                  <div className="update-progress-fill" style={{ width: `${percent ?? 15}%` }} />
                                </div>
                              </>
                            );
                          })()}
                        </div>
                      )}

                      {props.updateState.phase === 'installing' && (
                        <div className="update-card update-card-installing" role="status">
                          <Icon name="refresh" className="spinning" />
                          <span>{t.settings.updates.installing}</span>
                        </div>
                      )}

                      {props.updateState.phase === 'error' && (
                        <div className="update-card update-card-error" role="alert">
                          {props.updateState.error && <p className="help danger">{props.updateState.error}</p>}
                          <div className="row update-actions">
                            {props.onCheckUpdate && (
                              <button type="button" className="secondary compact" onClick={props.onCheckUpdate}>
                                {t.settings.updates.retry}
                              </button>
                            )}
                            {props.onOpenReleasePage && (
                              <button type="button" className="secondary compact" onClick={props.onOpenReleasePage}>
                                {t.settings.updates.downloadFromGithub}
                              </button>
                            )}
                          </div>
                        </div>
                      )}
                    </m.div>
                  )}
                </div>
              )}
            </section>
          </>
        )}

        {activeTab === 'models' && (
          <section id="settings-models-section" className="settings-tab-section">
            <div className="section-heading">
              <div className="row">
                <h2>{t.settings.modelsHeading}</h2>
                <span className="model-count mono">{props.models.length}</span>
              </div>
              {props.connections.length > 0 && (
                <button type="button" className="secondary compact" onClick={props.onAddProvider}>
                  <Icon name="plus" />{t.settings.addProvider}
                </button>
              )}
            </div>

            {!props.connections.length ? (
              <div className="model-empty">
                <span className="empty-badge" aria-hidden="true"><Icon name="cpu" /></span>
                <div className="empty-copy"><p className="empty-title">{t.settings.noModelsConfigured}</p></div>
                <button type="button" className="primary model-empty-action" onClick={props.onAddProvider}>
                  {t.settings.addFirstProvider}
                </button>
              </div>
            ) : (
              <div className="provider-list">
                {props.connections.map((conn) => {
                  const childModels = props.models.filter((item) => item.connectionId === conn.id);
                  const isExpanded = !collapsed[conn.id];
                  const providerDefaults = providerFor(conn.provider);
                  const providerLabel = t.providers[conn.provider];
                  const includesActive = childModels.some((item) => item.id === props.activeModelId);
                  const confirmingDelete = confirmDeleteId === conn.id;

                  return (
                    <div className="provider-card" key={conn.id} data-connection-id={conn.id}>
                      <div className="provider-card-header">
                        <button
                          type="button"
                          className="provider-card-toggle"
                          aria-expanded={isExpanded}
                          aria-label={t.settings.toggleConnectionLabel(conn.name, isExpanded)}
                          onClick={() => toggleConnection(conn.id)}
                        >
                          <RotatingChevron open={isExpanded} />
                          <span className="provider-card-title-group min-width-zero">
                            <strong className="truncate">{conn.name}</strong>
                            <span className="provider-card-subline">
                              <span className="model-provider">{providerLabel}</span>
                              {providerDefaults.needsKey && conn.keyHint && (
                                <>
                                  <span className="model-sep" aria-hidden="true">·</span>
                                  <span className="mono model-key-hint">{conn.keyHint}</span>
                                </>
                              )}
                              {!isExpanded && (
                                <>
                                  <span className="model-sep" aria-hidden="true">·</span>
                                  <span className="muted small">{t.settings.connectionModelCount(childModels.length)}</span>
                                </>
                              )}
                            </span>
                          </span>
                        </button>
                        <div className="model-actions">
                          <IconButton
                            icon="pencil"
                            className="model-edit"
                            label={t.settings.editConnectionLabel(conn.name)}
                            onClick={() => props.onEditProvider(conn)}
                          />
                          <IconButton
                            icon="trash"
                            className="model-delete"
                            label={t.settings.deleteConnectionLabel(conn.name)}
                            onClick={() => setConfirmDeleteId(confirmingDelete ? null : conn.id)}
                          />
                        </div>
                      </div>

                      <Collapse open={confirmingDelete}>
                        <div className="provider-delete-confirm" role="alertdialog" aria-label={t.settings.confirmDeleteConnection}>
                          <p className="help">{t.settings.deleteConnectionConfirm(conn.name, childModels.length, includesActive)}</p>
                          <div className="row provider-delete-actions">
                            <button type="button" className="compact" onClick={() => setConfirmDeleteId(null)}>
                              {t.common.cancel}
                            </button>
                            <button
                              type="button"
                              className="compact danger-action"
                              onClick={() => {
                                setConfirmDeleteId(null);
                                props.onDeleteProvider(conn.id);
                              }}
                            >
                              {t.settings.confirmDeleteConnection}
                            </button>
                          </div>
                        </div>
                      </Collapse>

                      <Collapse open={isExpanded}>
                        <div className="provider-card-body">
                          {childModels.length === 0 ? (
                            <p className="muted help provider-empty-note">{t.settings.emptyConnectionHint}</p>
                          ) : (
                            <div className="provider-model-list">
                              {childModels.map((model) => {
                                const active = model.id === props.activeModelId;
                                const effectiveFormat = model.apiFormat ?? providerDefaults.defaultApiFormat;
                                const formatShort = t.apiFormats[effectiveFormat].short;
                                const showFormat = conn.provider === 'custom' || effectiveFormat !== providerDefaults.defaultApiFormat;
                                const fallbackLimits = resolveDefaultTokenLimits(conn.provider, effectiveFormat);
                                const contextWindow = model.contextWindow ?? fallbackLimits.contextWindow;
                                const maxOutputTokens = model.maxOutputTokens ?? fallbackLimits.maxOutputTokens;
                                const tokenSummary = `${formatTokenCount(contextWindow)}/${formatTokenCount(maxOutputTokens)}`;
                                const detailTitle = t.settings.modelDetailTitle({
                                  connectionName: conn.name,
                                  providerLabel,
                                  formatShort,
                                  modelId: model.modelId,
                                  contextWindow,
                                  maxOutputTokens,
                                  baseUrl: conn.baseUrl,
                                });
                                return (
                                  <div className={`model-row ${active ? 'is-active' : ''}`} key={model.id}>
                                    <button
                                      type="button"
                                      className="model-select"
                                      aria-pressed={active}
                                      title={detailTitle}
                                      onClick={() => props.onSelectModel(model.id)}
                                    >
                                      <span className="model-body">
                                        <span className="model-title-row">
                                          <strong className="truncate">{model.name}</strong>
                                          {active && (
                                            <span className="model-active-badge">
                                              <span className="status-dot" />{t.settings.activeModelBadge}
                                            </span>
                                          )}
                                        </span>
                                        <span className="model-meta-row">
                                          <span className="mono model-id truncate">{model.modelId}</span>
                                          {showFormat && (
                                            <>
                                              <span className="model-sep" aria-hidden="true">·</span>
                                              <span className="model-provider">{formatShort}</span>
                                            </>
                                          )}
                                          <span className="model-sep" aria-hidden="true">·</span>
                                          <span className="mono model-tokens">{tokenSummary}</span>
                                        </span>
                                      </span>
                                    </button>
                                    <div className="model-actions">
                                      <IconButton
                                        icon="pencil"
                                        className="model-edit"
                                        label={t.settings.editModelLabel(model.name)}
                                        onClick={() => props.onEditModel(conn, model)}
                                      />
                                      <IconButton
                                        icon="trash"
                                        className="model-delete"
                                        label={t.settings.deleteModelLabel(model.name)}
                                        onClick={() => props.onDeleteModel(model.id)}
                                      />
                                    </div>
                                  </div>
                                );
                              })}
                            </div>
                          )}
                          <button
                            type="button"
                            className="provider-add-model"
                            onClick={() => props.onAddModel(conn)}
                          >
                            <Icon name="plus" />
                            <span>{t.settings.addChildModel}</span>
                          </button>
                        </div>
                      </Collapse>
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        )}

        {activeTab === 'skills' && skills && (
          <section id="settings-skills-section" className="settings-tab-section" data-skills-mode={skills.mode}>
            <div className="section-heading">
              <div className="row">
                <h2>{t.settings.skills.heading}</h2>
                <span className={`skills-mode-badge ${skills.mode === 'synced' ? 'is-synced' : ''}`}>
                  {skills.mode === 'synced' ? t.settings.skills.modeSynced : t.settings.skills.modeBundled}
                </span>
              </div>
              {props.onSyncSkills && (
                <button
                  type="button"
                  className="secondary compact"
                  data-skills-sync
                  disabled={isSkillsBusy}
                  onClick={() => void triggerSyncSkills()}
                >
                  <Icon name="refresh" className={props.skillsBusy === 'sync' ? 'spinning' : ''} />
                  {props.skillsBusy === 'sync' ? t.settings.skills.syncingButton : t.settings.skills.syncButton}
                </button>
              )}
            </div>

            <div className="skills-card">
              {!editingSkillSource ? (
                <div className="skills-source-row">
                  <span className="skills-source-url mono truncate" title={skills.sourceRepo}>{skills.sourceRepo}</span>
                  <button
                    type="button"
                    className="skills-inline-link"
                    data-skills-edit-source
                    disabled={isSkillsBusy}
                    onClick={() => {
                      setSkillSourceDraft(skills.sourceRepo);
                      setEditingSkillSource(true);
                    }}
                  >
                    {t.settings.skills.editSourceButton}
                  </button>
                </div>
              ) : (
                <div className="skills-source-editor">
                  <input
                    type="url"
                    className="mono skills-source-input"
                    data-skills-source-input
                    aria-label={t.settings.skills.sourceRepoLabel}
                    placeholder={t.settings.skills.sourceRepoPlaceholder}
                    value={skillSourceDraft}
                    disabled={isSkillsBusy}
                    onChange={(event) => setSkillSourceDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault();
                        void triggerSyncSkills();
                      }
                    }}
                  />
                  <div className="row skills-source-editor-actions">
                    <button
                      type="button"
                      className="skills-inline-link"
                      disabled={isSkillsBusy || skillSourceDraft.trim() === skills.defaultSourceRepo}
                      onClick={() => setSkillSourceDraft(skills.defaultSourceRepo)}
                    >
                      {t.settings.skills.resetSourceDefault}
                    </button>
                    <button
                      type="button"
                      className="skills-inline-link"
                      disabled={isSkillsBusy}
                      onClick={() => setEditingSkillSource(false)}
                    >
                      {t.settings.skills.cancelEditSource}
                    </button>
                  </div>
                </div>
              )}

              {skills.mode === 'synced' && (
                <div className="skills-meta-row">
                  <div className="row min-width-zero">
                    {skills.sourceCommit && (
                      <span className="mono skills-commit-badge" title={skills.sourceCommit}>
                        {skills.sourceCommit.slice(0, 7)}
                      </span>
                    )}
                    {skills.syncedAt && (
                      <span className="muted small truncate">{formatSyncedTimestamp(skills.syncedAt)}</span>
                    )}
                  </div>
                  {props.onResetSkills && (
                    <button
                      type="button"
                      className="skills-inline-link skills-reset-link"
                      data-skills-reset
                      disabled={isSkillsBusy}
                      onClick={props.onResetSkills}
                    >
                      {props.skillsBusy === 'reset' ? t.settings.skills.restoringButton : t.settings.skills.restoreBundledButton}
                    </button>
                  )}
                </div>
              )}

              {props.skillsError && (
                <p className="help danger skills-feedback" role="alert">{props.skillsError}</p>
              )}
              {!props.skillsError && props.skillsNotice && (
                <p className="help success skills-feedback" role="status">{props.skillsNotice}</p>
              )}
            </div>

            <div className="skills-catalog-list" data-skills-catalog>
              {skills.skills.map((skill) => {
                const isSyncedOverride = skill.source === 'synced' && skill.enabled;
                const badgeLabel = isSyncedOverride
                  ? t.settings.skills.badgeSynced
                  : !skill.builtIn
                    ? t.settings.skills.badgeCustom
                    : t.settings.skills.badgeBuiltIn;
                return (
                  <label
                    key={skill.name}
                    className={`skill-catalog-row ${!skill.enabled ? 'is-disabled' : ''}`}
                    data-skill-name={skill.name}
                    title={skill.description || skill.name}
                  >
                    <input
                      type="checkbox"
                      checked={skill.enabled}
                      disabled={skills.mode !== 'synced' || isSkillsBusy || !props.onToggleSkill}
                      aria-label={skill.name}
                      onChange={(event) => props.onToggleSkill?.(skill.name, event.target.checked)}
                    />
                    <span className="skill-catalog-title-row min-width-zero">
                      <span className="mono skill-catalog-name truncate">{skill.name}</span>
                      {skill.referenceCount > 0 && (
                        <span className="muted mono skill-refs-badge">
                          {t.settings.skills.refsCount(skill.referenceCount)}
                        </span>
                      )}
                      <span className={`skill-origin-badge ${isSyncedOverride ? 'is-synced' : ''}`}>{badgeLabel}</span>
                    </span>
                  </label>
                );
              })}
            </div>
          </section>
        )}
      </div>
    </Overlay>
  );
}
