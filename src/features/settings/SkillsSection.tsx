import { useState } from 'react';
import type { SkillsStateSnapshot } from '../../../shared/skills-protocol';
import { useI18n } from '../../i18n';
import { Icon } from '../../ui/controls';

function formatSyncedTimestamp(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

interface SkillsSectionProps {
  skills: SkillsStateSnapshot;
  skillsBusy?: 'sync' | 'toggle' | 'reset' | null;
  skillsError?: string;
  skillsNotice?: string;
  onSyncSkills?(sourceRepo?: string): Promise<boolean> | void;
  onToggleSkill?(name: string, enabled: boolean): void;
  onResetSkills?(): void;
}

export function SkillsSection({
  skills,
  skillsBusy,
  skillsError,
  skillsNotice,
  onSyncSkills,
  onToggleSkill,
  onResetSkills,
}: SkillsSectionProps) {
  const { t } = useI18n();
  const [editingSkillSource, setEditingSkillSource] = useState(false);
  const [skillSourceDraft, setSkillSourceDraft] = useState('');
  const isSkillsBusy = Boolean(skillsBusy);

  async function triggerSyncSkills() {
    if (!onSyncSkills || isSkillsBusy) return;
    const requestedRepo = editingSkillSource
      ? (skillSourceDraft.trim() || skills.defaultSourceRepo)
      : skills.sourceRepo;
    const succeeded = await onSyncSkills(requestedRepo);
    if (succeeded !== false && editingSkillSource) {
      setEditingSkillSource(false);
    }
  }

  return (
    <section id="settings-skills-section" className="settings-tab-section" data-skills-mode={skills.mode}>
      <div className="section-heading">
        <div className="row">
          <h2>{t.settings.skills.heading}</h2>
          <span className={`skills-mode-badge ${skills.mode === 'synced' ? 'is-synced' : ''}`}>
            {skills.mode === 'synced' ? t.settings.skills.modeSynced : t.settings.skills.modeBundled}
          </span>
        </div>
        {onSyncSkills && (
          <button
            type="button"
            className="secondary compact"
            data-skills-sync
            disabled={isSkillsBusy}
            onClick={() => void triggerSyncSkills()}
          >
            <Icon name="refresh" className={skillsBusy === 'sync' ? 'spinning' : ''} />
            {skillsBusy === 'sync' ? t.settings.skills.syncingButton : t.settings.skills.syncButton}
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
            {onResetSkills && (
              <button
                type="button"
                className="skills-inline-link skills-reset-link"
                data-skills-reset
                disabled={isSkillsBusy}
                onClick={onResetSkills}
              >
                {skillsBusy === 'reset' ? t.settings.skills.restoringButton : t.settings.skills.restoreBundledButton}
              </button>
            )}
          </div>
        )}

        {skillsError && (
          <p className="help danger skills-feedback" role="alert">{skillsError}</p>
        )}
        {!skillsError && skillsNotice && (
          <p className="help success skills-feedback" role="status">{skillsNotice}</p>
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
                disabled={skills.mode !== 'synced' || isSkillsBusy || !onToggleSkill}
                aria-label={skill.name}
                onChange={(event) => onToggleSkill?.(skill.name, event.target.checked)}
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
  );
}
