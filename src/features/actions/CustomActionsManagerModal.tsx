import { useState } from 'react';
import type {
  CustomActionConfig,
  CustomActionSyncResult,
} from '../../../shared/custom-action-protocol';
import { useI18n } from '../../i18n';
import { Icon, IconButton, Overlay } from '../../ui/controls';

interface Props {
  repositoryName: string;
  actions: CustomActionConfig[];
  activeActionId: string | null;
  otherRepoCount: number;
  covered?: boolean;
  depth?: number;
  onClose(): void;
  onSelectActive(actionId: string | null): void;
  onNewAction(mode: 'manual' | 'create'): void;
  onEditAction(action: CustomActionConfig): void;
  onDeleteAction(actionId: string): void;
  onSyncToAllRepos(action: CustomActionConfig, overwrite: boolean): CustomActionSyncResult;
}

export function CustomActionsManagerModal(props: Props) {
  const { t } = useI18n();
  const [syncNotice, setSyncNotice] = useState<{
    action: CustomActionConfig;
    result: CustomActionSyncResult;
  } | null>(null);

  return (
    <Overlay
      title={t.actions.manageModalTitle}
      covered={props.covered}
      depth={props.depth}
      onClose={props.onClose}
    >
      <div className="custom-action-shell">
        <div className="custom-action-shell-head">
          <div className="custom-action-repo-meta">
            <Icon name="folder" />
            <span className="mono truncate">{props.repositoryName}</span>
            <div className="row" style={{ marginLeft: 'auto' }}>
              <button type="button" className="secondary compact" onClick={() => props.onNewAction('manual')}>
                <Icon name="plus" />
                {t.actions.modeManual}
              </button>
              <button type="button" className="primary compact" onClick={() => props.onNewAction('create')}>
                <Icon name="sparkles" />
                {t.actions.createShortButton}
              </button>
            </div>
          </div>

          {syncNotice && (
            <div className="action-sync-card">
              <span className="small">
                <strong>{syncNotice.action.label}</strong>:{' '}
                {t.actions.syncResultSummary(
                  syncNotice.result.added,
                  syncNotice.result.updated,
                  syncNotice.result.skipped,
                )}
              </span>
              {syncNotice.result.skipped > 0 && (
                <button
                  type="button"
                  className="secondary compact"
                  onClick={() => {
                    const next = props.onSyncToAllRepos(syncNotice.action, true);
                    setSyncNotice({ action: syncNotice.action, result: next });
                  }}
                >
                  {t.actions.syncOverwriteSkippedButton(syncNotice.result.skipped)}
                </button>
              )}
            </div>
          )}
        </div>

        <div className="custom-action-shell-body scroll-area">
          {props.actions.length === 0 ? (
            <div className="model-empty">
              <span className="empty-badge" aria-hidden="true"><Icon name="sparkles" /></span>
              <p className="muted">{t.actions.emptyManagerHint}</p>
              <div className="row">
                <button type="button" className="secondary compact" onClick={() => props.onNewAction('manual')}>
                  <Icon name="plus" />
                  {t.actions.modeManual}
                </button>
                <button type="button" className="primary compact" onClick={() => props.onNewAction('create')}>
                  <Icon name="sparkles" />
                  {t.actions.modeCreateWithPi}
                </button>
              </div>
            </div>
          ) : (
            <div className="custom-action-list">
              {props.actions.map((item) => {
                const isActive = props.activeActionId === item.id;
                return (
                  <div className={`custom-action-item ${isActive ? 'is-active' : ''}`} key={item.id}>
                    <button
                      type="button"
                      className="custom-action-item-select"
                      aria-pressed={isActive}
                      onClick={() => {
                        props.onSelectActive(item.id);
                        props.onClose();
                      }}
                    >
                      <div className="custom-action-item-head">
                        <strong className="truncate" title={item.label}>{item.label}</strong>
                      </div>
                      {isActive && (
                        <div className="custom-action-item-badges">
                          <span className="model-active-badge">{t.actions.activeBadge}</span>
                        </div>
                      )}
                      <p className="mono muted small custom-action-item-prompt">{item.prompt}</p>
                    </button>
                    <div className="model-actions">
                      {props.otherRepoCount > 0 && (
                        <IconButton
                          icon="copy"
                          className="model-edit"
                          label={t.actions.syncToAllButton}
                          onClick={() => {
                            const result = props.onSyncToAllRepos(item, false);
                            setSyncNotice({ action: item, result });
                          }}
                        />
                      )}
                      <IconButton
                        icon="pencil"
                        className="model-edit"
                        label={t.actions.editActionLabel(item.label)}
                        onClick={() => props.onEditAction(item)}
                      />
                      <IconButton
                        icon="trash"
                        className="model-delete"
                        label={t.actions.deleteActionLabel(item.label)}
                        onClick={() => props.onDeleteAction(item.id)}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </Overlay>
  );
}
