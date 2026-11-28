import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ActionApprovalMode } from '../../../shared/custom-action-protocol';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import type { SkillsStateSnapshot } from '../../../shared/skills-protocol';
import type { AppTheme } from '../../../shared/theme-protocol';
import type { UpdateStateSnapshot } from '../../../shared/update-protocol';
import { useI18n } from '../../i18n';
import { Overlay, type OverlayVariant } from '../../ui/controls';
import { useInstantMotion } from '../../ui/motion';
import type { EditorId } from '../workbench/state/workbench';
import { GeneralSection } from './GeneralSection';
import type { ModelConfiguration, ProviderConnection } from './modelProviders';
import { ModelsSection } from './ModelsSection';
import { SkillsSection } from './SkillsSection';

export type SettingsTabId = 'general' | 'models' | 'skills';

interface Props {
  locale: AppLocale;
  theme: AppTheme;
  actionApprovalMode: ActionApprovalMode;
  repositoryName?: string;
  editor: EditorId;
  commitLanguage: CommitLanguage;
  installedEditors?: EditorId[];
  connections: ProviderConnection[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  covered: boolean;
  onLocale(locale: AppLocale): void;
  onTheme(theme: AppTheme): void;
  onActionApprovalMode(mode: ActionApprovalMode): void;
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

export function SettingsPanel(props: Props) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();
  const [activeTab, setActiveTab] = useState<SettingsTabId>(() => (props.focusModels ? 'models' : 'general'));

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

  const skills = props.skillsState;
  const enabledSkillsCount = skills ? skills.skills.filter((item) => item.enabled).length : 0;
  const hasUpdate = props.updateState?.phase === 'available'
    || props.updateState?.phase === 'downloading'
    || props.updateState?.phase === 'installing';

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
          <GeneralSection
            locale={props.locale}
            theme={props.theme}
            actionApprovalMode={props.actionApprovalMode}
            repositoryName={props.repositoryName}
            editor={props.editor}
            commitLanguage={props.commitLanguage}
            installedEditors={props.installedEditors}
            onLocale={props.onLocale}
            onTheme={props.onTheme}
            onActionApprovalMode={props.onActionApprovalMode}
            onEditor={props.onEditor}
            onCommitLanguage={props.onCommitLanguage}
            updateState={props.updateState}
            gitBusyForUpdate={props.gitBusyForUpdate}
            onCheckUpdate={props.onCheckUpdate}
            onInstallUpdate={props.onInstallUpdate}
            onCancelUpdate={props.onCancelUpdate}
            onOpenReleasePage={props.onOpenReleasePage}
          />
        )}

        {activeTab === 'models' && (
          <ModelsSection
            connections={props.connections}
            models={props.models}
            activeModelId={props.activeModelId}
            onSelectModel={props.onSelectModel}
            onAddProvider={props.onAddProvider}
            onEditProvider={props.onEditProvider}
            onDeleteProvider={props.onDeleteProvider}
            onAddModel={props.onAddModel}
            onEditModel={props.onEditModel}
            onDeleteModel={props.onDeleteModel}
          />
        )}

        {activeTab === 'skills' && skills && (
          <SkillsSection
            skills={skills}
            skillsBusy={props.skillsBusy}
            skillsError={props.skillsError}
            skillsNotice={props.skillsNotice}
            onSyncSkills={props.onSyncSkills}
            onToggleSkill={props.onToggleSkill}
            onResetSkills={props.onResetSkills}
          />
        )}
      </div>
    </Overlay>
  );
}
