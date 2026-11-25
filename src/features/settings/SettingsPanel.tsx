import { useEffect, useRef, useState } from 'react';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import { LANGUAGE_OPTIONS, useI18n } from '../../i18n';
import { HelpTip, Icon, IconButton, Overlay } from '../../ui/controls';
import { editors } from '../workbench/state/workbench';
import type { EditorId } from '../workbench/state/workbench';
import { formatTokenCount, providerFor, resolveDefaultTokenLimits } from './modelProviders';
import type { ModelConfiguration, ProviderConnection } from './modelProviders';

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
  onClose(): void;
  closeDisabled?: boolean;
  error?: string;
  focusModels?: boolean;
}

export function SettingsPanel(props: Props) {
  const { t } = useI18n();
  const modelsSectionRef = useRef<HTMLElement>(null);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  useEffect(() => {
    if (props.focusModels && !props.closeDisabled) {
      modelsSectionRef.current?.scrollIntoView({ block: 'nearest' });
    }
  }, [props.focusModels, props.closeDisabled]);

  useEffect(() => {
    if (confirmDeleteId && !props.connections.some((c) => c.id === confirmDeleteId)) {
      setConfirmDeleteId(null);
    }
  }, [confirmDeleteId, props.connections]);

  const visibleEditors = props.installedEditors
    ? editors.filter((item) => props.installedEditors!.includes(item.id))
    : editors;
  const editor = visibleEditors.find((item) => item.id === props.editor)
    ?? editors.find((item) => item.id === props.editor)
    ?? editors[0];
  const badgeText = props.repositoryName ?? t.settings.noProjectBadge;

  function toggleConnection(connectionId: string) {
    setCollapsed((prev) => ({ ...prev, [connectionId]: !prev[connectionId] }));
  }

  return (
    <Overlay title={t.settings.title} onClose={props.onClose} covered={props.covered} closeDisabled={props.closeDisabled}>
      <div className="settings-content scroll-area">
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
              {visibleEditors.length > 0 && (
                <HelpTip
                  placement="bottom"
                  align="start"
                  label={t.settings.editorIntegrationTipLabel}
                  text={t.settings.editorIntegrationTip(editor.cli, Boolean(props.repositoryName))}
                />
              )}
            </div>
            {visibleEditors.length === 0 ? (
              <div className="model-empty">
                <div className="empty-copy">
                  <p className="empty-title">{t.settings.noEditorsDetected}</p>
                  <p className="muted help">{t.settings.noEditorsHelp}</p>
                </div>
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
          <div className="settings-subgroup">
            <div className="settings-subheading">
              <span>{t.settings.commitLanguage}</span>
              <HelpTip
                placement="bottom"
                align="start"
                label={t.settings.commitLanguageTipLabel}
                text={t.settings.commitLanguageTip(Boolean(props.repositoryName))}
              />
            </div>
            <div className="editor-grid" data-setting-group="commit-language" role="group" aria-label={t.settings.commitLanguage}>
              {LANGUAGE_OPTIONS.map((option) => (
                <button type="button" key={option.id} aria-pressed={option.id === props.commitLanguage} onClick={() => props.onCommitLanguage(option.id)}>
                  {option.label}{option.id === props.commitLanguage && <Icon name="check" />}
                </button>
              ))}
            </div>
          </div>
        </section>

        <section>
          <div className="row section-title-row">
            <h2>{t.settings.appLanguage}</h2>
          </div>
          <div className="editor-grid" data-setting-group="app-language" role="group" aria-label={t.settings.appLanguage}>
            {LANGUAGE_OPTIONS.map((option) => (
              <button type="button" key={option.id} aria-pressed={option.id === props.locale} onClick={() => props.onLocale(option.id)}>
                {option.label}{option.id === props.locale && <Icon name="check" />}
              </button>
            ))}
          </div>
        </section>

        <section ref={modelsSectionRef} id="settings-models-section">
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
          {props.error && <p className="form-result danger" role="alert">{props.error}</p>}

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
                        <Icon name={isExpanded ? 'down' : 'right'} />
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

                    {confirmingDelete && (
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
                    )}

                    {isExpanded && (
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
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </Overlay>
  );
}
