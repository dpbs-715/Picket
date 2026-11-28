import { useLayoutEffect, useRef } from 'react';
import { useI18n } from '../../i18n';
import { Icon } from '../../ui/controls';
import type {
  classifyCustomActionSyncTarget,
  CustomActionSyncRepoTarget,
} from '../workbench/state/workbench';

export interface ClassifiedSyncTarget extends CustomActionSyncRepoTarget {
  status: ReturnType<typeof classifyCustomActionSyncTarget>;
}

interface ActionSyncRepoPickerProps {
  syncTargets: ClassifiedSyncTarget[];
  selectableSyncIds: string[];
  effectiveSelectedSyncIds: string[];
  onToggleSelectAll(): void;
  onToggleRepo(repoId: string): void;
}

export function ActionSyncRepoPicker({
  syncTargets,
  selectableSyncIds,
  effectiveSelectedSyncIds,
  onToggleSelectAll,
  onToggleRepo,
}: ActionSyncRepoPickerProps) {
  const { t } = useI18n();
  const selectAllSyncRef = useRef<HTMLInputElement>(null);

  const allSyncSelected = selectableSyncIds.length > 0
    && effectiveSelectedSyncIds.length === selectableSyncIds.length;
  const someSyncSelected = effectiveSelectedSyncIds.length > 0 && !allSyncSelected;

  useLayoutEffect(() => {
    if (selectAllSyncRef.current) {
      selectAllSyncRef.current.indeterminate = someSyncSelected;
    }
  }, [someSyncSelected, allSyncSelected]);

  if (syncTargets.length === 0) return null;

  return (
    <div className="action-sync-section" data-sync-picker>
      <div className="action-sync-section-head">
        <span className="action-sync-section-title">{t.actions.syncReposSectionTitle}</span>
        <span className={`action-sync-count mono ${effectiveSelectedSyncIds.length > 0 ? 'is-active' : ''}`}>
          {t.actions.syncSelectedCount(effectiveSelectedSyncIds.length, syncTargets.length)}
        </span>
      </div>
      <div className="action-sync-card is-picker">
        <label className={`action-sync-select-all ${selectableSyncIds.length === 0 ? 'is-disabled' : ''}`}>
          <input
            ref={selectAllSyncRef}
            type="checkbox"
            data-sync-select-all
            checked={allSyncSelected}
            disabled={selectableSyncIds.length === 0}
            aria-checked={someSyncSelected ? 'mixed' : allSyncSelected}
            onChange={onToggleSelectAll}
          />
          <span className="action-sync-select-all-label">{t.actions.syncSelectAllLabel}</span>
          <span className="action-sync-on-save-hint">{t.actions.syncOnSaveHint}</span>
        </label>
        <div className="action-sync-repo-list scroll-area" role="group" aria-label={t.actions.syncReposSectionTitle}>
          {syncTargets.map((repo) => {
            const disabled = repo.status === 'full';
            const checked = !disabled && effectiveSelectedSyncIds.includes(repo.id);
            const badge = repo.status === 'synced'
              ? (checked
                  ? { text: t.actions.syncStatusWillUpdate, tone: 'indigo' }
                  : { text: t.actions.syncStatusSynced, tone: 'muted' })
              : repo.status === 'sameLabel'
                ? (checked
                    ? { text: t.actions.syncStatusWillOverwrite, tone: 'amber' }
                    : { text: t.actions.syncStatusSameName, tone: 'amber' })
                : repo.status === 'full'
                  ? { text: t.actions.syncStatusFull, tone: 'muted' }
                  : null;
            return (
              <label
                key={repo.id}
                data-sync-repo-id={repo.id}
                className={`action-sync-repo-row ${checked ? 'is-checked' : ''} ${disabled ? 'is-disabled' : ''}`}
              >
                <input
                  type="checkbox"
                  checked={checked}
                  disabled={disabled}
                  onChange={() => onToggleRepo(repo.id)}
                />
                <Icon name="folder" />
                <span className="mono truncate action-sync-repo-name" title={repo.name}>{repo.name}</span>
                {badge && (
                  <span className={`action-sync-badge is-${badge.tone}`}>{badge.text}</span>
                )}
              </label>
            );
          })}
        </div>
      </div>
    </div>
  );
}
