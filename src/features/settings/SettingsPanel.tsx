import { useEffect, useRef } from 'react';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import { LANGUAGE_OPTIONS, useI18n } from '../../i18n';
import { HelpTip, Icon, IconButton, Overlay } from '../../ui/controls';
import { editors } from '../workbench/state/workbench';
import type { EditorId } from '../workbench/state/workbench';
import { formatTokenCount, providerFor, resolveDefaultTokenLimits } from './modelProviders';
import type { ModelConfiguration } from './modelProviders';

interface Props {
  locale: AppLocale;
  repositoryName?: string;
  editor: EditorId;
  commitLanguage: CommitLanguage;
  installedEditors?: EditorId[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  covered: boolean;
  onLocale(locale: AppLocale): void;
  onEditor(editor: EditorId): void;
  onCommitLanguage(commitLanguage: CommitLanguage): void;
  onSelectModel(id: string): void;
  onEditModel(model: ModelConfiguration): void;
  onDeleteModel(id: string): void;
  onAddModel(): void;
  onClose(): void;
  error?: string;
  focusModels?: boolean;
}

export function SettingsPanel(props: Props) {
  const { t } = useI18n();
  const modelsSectionRef = useRef<HTMLElement>(null);
  useEffect(() => {
    if (props.focusModels) {
      modelsSectionRef.current?.scrollIntoView({ block: 'nearest' });
    }
  }, [props.focusModels]);
  const visibleEditors = props.installedEditors
    ? editors.filter((item) => props.installedEditors!.includes(item.id))
    : editors;
  const editor = visibleEditors.find((item) => item.id === props.editor)
    ?? editors.find((item) => item.id === props.editor)
    ?? editors[0];
  const badgeText = props.repositoryName ?? t.settings.noProjectBadge;

  return <Overlay title={t.settings.title} onClose={props.onClose} covered={props.covered}>
    <div className="settings-content scroll-area">
      <section ref={modelsSectionRef} id="settings-models-section">
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
          {visibleEditors.length === 0 ? <div className="model-empty">
            <div className="empty-copy">
              <p className="empty-title">{t.settings.noEditorsDetected}</p>
              <p className="muted help">{t.settings.noEditorsHelp}</p>
            </div>
          </div> : <div className="editor-grid" data-setting-group="editor" role="group" aria-label={t.settings.preferredEditor}>
            {visibleEditors.map((item) => <button type="button" key={item.id} aria-pressed={item.id === editor.id} onClick={() => props.onEditor(item.id)}>
              {item.name}{item.id === editor.id && <Icon name="check" />}
            </button>)}
          </div>}
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
            {LANGUAGE_OPTIONS.map((option) => <button type="button" key={option.id} aria-pressed={option.id === props.commitLanguage} onClick={() => props.onCommitLanguage(option.id)}>
              {option.label}{option.id === props.commitLanguage && <Icon name="check" />}
            </button>)}
          </div>
        </div>
      </section>
      <section>
        <div className="row section-title-row">
          <h2>{t.settings.appLanguage}</h2>
        </div>
        <div className="editor-grid" data-setting-group="app-language" role="group" aria-label={t.settings.appLanguage}>
          {LANGUAGE_OPTIONS.map((option) => <button type="button" key={option.id} aria-pressed={option.id === props.locale} onClick={() => props.onLocale(option.id)}>
            {option.label}{option.id === props.locale && <Icon name="check" />}
          </button>)}
        </div>
      </section>
      <section ref={modelsSectionRef} id="settings-models-section">
        <div className="section-heading">
          <div className="row">
            <h2>{t.settings.modelsHeading}</h2>
            <span className="model-count mono">{props.models.length}</span>
          </div>
          {props.models.length > 0 && <button type="button" className="secondary compact" onClick={props.onAddModel}><Icon name="plus" />{t.settings.addModel}</button>}
        </div>
        {props.error && <p className="form-result danger" role="alert">{props.error}</p>}
        {!props.models.length ? <div className="model-empty">
          <span className="empty-badge" aria-hidden="true"><Icon name="cpu" /></span>
          <div className="empty-copy"><p className="empty-title">{t.settings.noModelsConfigured}</p></div>
          <button type="button" className="primary model-empty-action" onClick={props.onAddModel}>{t.settings.addFirstModel}</button>
        </div>
          : <div className="model-list">{props.models.map((model) => {
            const active = model.id === props.activeModelId;
            const providerDefaults = providerFor(model.provider);
            const providerLabel = t.providers[model.provider];
            const effectiveFormat = model.apiFormat ?? providerDefaults.defaultApiFormat;
            const formatShort = t.apiFormats[effectiveFormat].short;
            const showFormat = model.provider === 'custom' || effectiveFormat !== providerDefaults.defaultApiFormat;
            const fallbackLimits = resolveDefaultTokenLimits(model.provider, effectiveFormat);
            const contextWindow = model.contextWindow ?? fallbackLimits.contextWindow;
            const maxOutputTokens = model.maxOutputTokens ?? fallbackLimits.maxOutputTokens;
            const tokenSummary = `${formatTokenCount(contextWindow)}/${formatTokenCount(maxOutputTokens)}`;
            const detailTitle = t.settings.modelDetailTitle({
              providerLabel,
              formatShort,
              modelId: model.modelId,
              contextWindow,
              maxOutputTokens,
              baseUrl: model.baseUrl,
            });
            return <div className={`model-row ${active ? 'is-active' : ''}`} key={model.id}>
              <button type="button" className="model-select" aria-pressed={active} title={detailTitle} onClick={() => props.onSelectModel(model.id)}>
                <span className="model-body">
                  <span className="model-title-row"><strong className="truncate">{model.name}</strong>{active && <span className="model-active-badge"><span className="status-dot" />{t.settings.activeModelBadge}</span>}</span>
                  <span className="model-meta-row"><span className="model-provider">{providerLabel}{showFormat ? ` · ${formatShort}` : ''}</span><span className="model-sep" aria-hidden="true">·</span><span className="mono model-id truncate">{model.modelId}</span><span className="model-sep" aria-hidden="true">·</span><span className="mono model-tokens">{tokenSummary}</span></span>
                </span>
              </button>
              <div className="model-actions">
                <IconButton icon="pencil" className="model-edit" label={t.settings.editModelLabel(model.name)} onClick={() => props.onEditModel(model)} />
                <IconButton icon="trash" className="model-delete" label={t.settings.deleteModelLabel(model.name)} onClick={() => props.onDeleteModel(model.id)} />
              </div>
            </div>;
          })}</div>}
      </section>
    </div>
  </Overlay>;
}
