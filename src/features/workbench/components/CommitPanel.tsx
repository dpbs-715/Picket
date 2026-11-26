import type { RefObject } from 'react';
import {
  isCustomActionWriteTrusted,
  type CustomActionConfig,
} from '../../../../shared/custom-action-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton } from '../../../ui/controls';
import { splitPath } from '../state/workbench';
import type { Change, EditableCommitPlan } from '../state/workbench';

interface Props {
  files: Change[];
  plan?: EditableCommitPlan;
  planOutdated: boolean;
  activeAction?: CustomActionConfig;
  hasWorktree?: boolean;
  hasModel: boolean;
  canPlan: boolean;
  planning: boolean;
  panelRef?: RefObject<HTMLDivElement | null>;
  onPlan(): void;
  onCancelPlan?(): void;
  onEditPlanGroup(groupId: string, message: string): void;
  onClearPlan(): void;
  onConfigureModel(): void;
  onEditAction?(action: CustomActionConfig): void;
  onSwitchBackCPush?(): void;
}

export function CommitPanel(props: Props) {
  const { t } = useI18n();
  const emphasizePrimary = props.hasWorktree ?? true;
  const fileById = new Map(props.files.map((file) => [file.id, file]));

  if (props.activeAction) {
    const activeAction = props.activeAction;
    const trusted = isCustomActionWriteTrusted(activeAction);
    return (
      <div className="commit-plan-panel" ref={props.panelRef}>
        <div className="plan-region">
          <div className="custom-action-preview-card">
            <div className="custom-action-preview-head">
              <div className="custom-action-preview-top">
                <div className="custom-action-preview-title">
                  <Icon name="sparkles" className="accent" />
                  <strong className="truncate" title={activeAction.label}>{activeAction.label}</strong>
                </div>
                <div className="custom-action-preview-actions">
                  {props.onEditAction && (
                    <button
                      type="button"
                      className="secondary compact"
                      onClick={() => props.onEditAction!(activeAction)}
                    >
                      <Icon name="pencil" />
                      {t.actions.previewEditAction}
                    </button>
                  )}
                  {props.onSwitchBackCPush && (
                    <button
                      type="button"
                      className="secondary compact"
                      onClick={props.onSwitchBackCPush}
                    >
                      {t.actions.switchBackCPush}
                    </button>
                  )}
                </div>
              </div>
              <div className="custom-action-preview-meta">
                <span className="action-meta-badge is-accent">{t.actions.customActionBadge}</span>
                {trusted && <span className="action-meta-badge is-amber">{t.actions.autoApproveBadge}</span>}
              </div>
            </div>
            <pre className="custom-action-prompt-preview mono scroll-area">{activeAction.prompt}</pre>
          </div>
        </div>
      </div>
    );
  }

  return <div className="commit-plan-panel" ref={props.panelRef}>
    <div className="plan-region">
      {props.planning ? (
        <div className="empty-state commit-generating" role="status" aria-live="polite">
          <span className="empty-badge accent" aria-hidden="true"><Icon name="loader" className="spinning" /></span>
          <p className="commit-generating-label">{t.commit.planningStatus}</p>
          {props.onCancelPlan && (
            <button type="button" className="secondary compact" onClick={props.onCancelPlan}>
              {t.commit.cancelPlanning}
            </button>
          )}
        </div>
      ) : !props.plan ? (
        <div className="empty-state plan-empty">
          <button
            type="button"
            className={emphasizePrimary ? 'primary' : 'secondary'}
            disabled={props.hasModel && !props.canPlan}
            onClick={props.hasModel ? props.onPlan : props.onConfigureModel}
          >
            <Icon name={props.hasModel ? 'sparkles' : 'plus'} />
            {props.hasModel ? t.commit.previewPlanButton : t.commit.configureModelButton}
          </button>
        </div>
      ) : (
        <div className="plan-editor">
          <div className="plan-toolbar">
            <div className="row min-width-zero">
              <span className="plan-summary-title truncate">
                {t.commit.planSummary(props.plan.groups.length, props.plan.skippedFileIds.length)}
              </span>
              {props.planOutdated && <span className="branch-tag">{t.commit.planOutdatedBadge}</span>}
            </div>
            <div className="row">
              <IconButton
                icon="refresh"
                label={t.commit.regeneratePlanAction}
                disabled={!props.canPlan}
                onClick={props.onPlan}
              />
              <IconButton icon="close" label={t.commit.clearPlanAction} onClick={props.onClearPlan} />
            </div>
          </div>
          <div className="plan-groups scroll-area">
            {props.plan.groups.length === 0 ? (
              <p className="muted help plan-all-skipped">
                {t.commit.allSkippedPlanNote(props.plan.skippedFileIds.length)}
              </p>
            ) : (
              props.plan.groups.map((group, index) => {
                const total = props.plan!.groups.length;
                const lineCount = Math.min(6, Math.max(3, group.message.split('\n').length));
                const emptyMessage = !group.message.trim() && !props.planOutdated;
                return (
                  <div className={`plan-group-card ${props.planOutdated ? 'is-outdated' : ''}`} key={group.id}>
                    <div className="plan-group-header">
                      <strong className="plan-group-index">{t.commit.groupHeading(index + 1, total)}</strong>
                      <div className="plan-group-files">
                        {group.fileIds.map((fileId) => {
                          const matched = fileById.get(fileId);
                          const pathLabel = matched ? splitPath(matched.path).name : fileId;
                          return (
                            <span className="plan-file-chip mono truncate" key={fileId} title={matched?.path ?? fileId}>
                              {pathLabel}
                            </span>
                          );
                        })}
                      </div>
                    </div>
                    <textarea
                      className="plan-group-textarea"
                      aria-label={t.commit.groupMessageLabel(index + 1, total)}
                      rows={lineCount}
                      value={group.message}
                      onChange={(event) => props.onEditPlanGroup(group.id, event.target.value)}
                      placeholder={t.commit.groupMessagePlaceholder}
                      spellCheck={false}
                    />
                    {emptyMessage && <p className="danger help plan-group-error">{t.commit.emptyPlanMessageHint}</p>}
                  </div>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  </div>;
}
