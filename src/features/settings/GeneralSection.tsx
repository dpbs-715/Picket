import {
  ACTION_APPROVAL_MODES,
  type ActionApprovalMode,
} from '../../../shared/custom-action-protocol';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import { APP_THEMES, type AppTheme } from '../../../shared/theme-protocol';
import type { UpdateStateSnapshot } from '../../../shared/update-protocol';
import { LANGUAGE_OPTIONS, useI18n } from '../../i18n';
import { HelpTip, Icon } from '../../ui/controls';
import { editors, type EditorId } from '../workbench/state/workbench';
import { UpdatesSection } from './UpdatesSection';

interface GeneralSectionProps {
  locale: AppLocale;
  theme: AppTheme;
  actionApprovalMode: ActionApprovalMode;
  repositoryName?: string;
  editor: EditorId;
  commitLanguage: CommitLanguage;
  installedEditors?: EditorId[];
  onLocale(locale: AppLocale): void;
  onTheme(theme: AppTheme): void;
  onActionApprovalMode(mode: ActionApprovalMode): void;
  onEditor(editor: EditorId): void;
  onCommitLanguage(commitLanguage: CommitLanguage): void;
  updateState?: UpdateStateSnapshot;
  gitBusyForUpdate?: boolean;
  onCheckUpdate?(): void;
  onInstallUpdate?(version: string): void;
  onCancelUpdate?(): void;
  onOpenReleasePage?(): void;
}

export function GeneralSection(props: GeneralSectionProps) {
  const { t } = useI18n();
  const visibleEditors = props.installedEditors
    ? editors.filter((item) => props.installedEditors!.includes(item.id))
    : editors;
  const editor = visibleEditors.find((item) => item.id === props.editor)
    ?? editors.find((item) => item.id === props.editor)
    ?? editors[0];
  const badgeText = props.repositoryName ?? t.settings.noProjectBadge;

  return (
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

        <div className="settings-row-item">
          <span className="settings-row-label">{t.settings.appTheme}</span>
          <div className="segmented-control" data-setting-group="app-theme" role="group" aria-label={t.settings.appTheme}>
            {APP_THEMES.map((option) => (
              <button
                type="button"
                key={option}
                data-theme-option={option}
                aria-pressed={option === props.theme}
                onClick={() => props.onTheme(option)}
              >
                {t.settings.themeOptions[option]}
              </button>
            ))}
          </div>
        </div>

        <div className="settings-row-item">
          <span className="settings-row-label">
            <span>{t.settings.actionApproval}</span>
            <HelpTip
              align="start"
              label={t.settings.actionApprovalTipLabel}
              text={t.settings.actionApprovalHelp}
            />
          </span>
          <div
            className="segmented-control"
            data-setting-group="action-approval"
            role="group"
            aria-label={t.settings.actionApproval}
          >
            {ACTION_APPROVAL_MODES.map((option) => (
              <button
                type="button"
                key={option}
                data-approval-option={option}
                aria-pressed={option === props.actionApprovalMode}
                onClick={() => props.onActionApprovalMode(option)}
              >
                {t.settings.actionApprovalOptions[option]}
              </button>
            ))}
          </div>
        </div>

        {props.updateState && (
          <UpdatesSection
            updateState={props.updateState}
            gitBusyForUpdate={props.gitBusyForUpdate}
            onCheckUpdate={props.onCheckUpdate}
            onInstallUpdate={props.onInstallUpdate}
            onCancelUpdate={props.onCancelUpdate}
            onOpenReleasePage={props.onOpenReleasePage}
          />
        )}
      </section>
    </>
  );
}
