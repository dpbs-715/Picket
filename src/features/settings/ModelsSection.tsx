import { useEffect, useState } from 'react';
import { useI18n } from '../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../ui/controls';
import { Collapse } from '../../ui/motion';
import {
  formatTokenCount,
  providerFor,
  resolveDefaultTokenLimits,
  type ModelConfiguration,
  type ProviderConnection,
} from './modelProviders';

interface ModelsSectionProps {
  connections: ProviderConnection[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  onSelectModel(id: string): void;
  onAddProvider(): void;
  onEditProvider(connection: ProviderConnection): void;
  onDeleteProvider(connectionId: string): void;
  onAddModel(connection: ProviderConnection): void;
  onEditModel(connection: ProviderConnection, model: ModelConfiguration): void;
  onDeleteModel(id: string): void;
}

export function ModelsSection(props: ModelsSectionProps) {
  const { t } = useI18n();
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  useEffect(() => {
    if (confirmDeleteId && !props.connections.some((c) => c.id === confirmDeleteId)) {
      setConfirmDeleteId(null);
    }
  }, [confirmDeleteId, props.connections]);

  function toggleConnection(connectionId: string) {
    setCollapsed((prev) => ({ ...prev, [connectionId]: !prev[connectionId] }));
  }

  return (
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
  );
}
