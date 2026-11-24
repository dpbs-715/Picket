import { useI18n } from '../../../i18n';
import { HelpTip, Icon } from '../../../ui/controls';
import { isModelErrorText, selectedFiles, splitPath } from '../state/workbench';
import type { Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  hasModel: boolean;
  canReview: boolean;
  onAddModel(): void;
  onOpenModelSettings(): void;
  onReview(changedOnly?: boolean): void;
  onCancel?(): void;
  onOpenFile(fileId: string, line: number): void;
}
export function ReviewPanel(props: Props) {
  const { t } = useI18n();
  const review = props.worktree?.review;
  if (review?.status === 'running') return <div className="empty-state muted"><Icon name="refresh" className="spinning accent" /><p>{t.review.runningStatus}</p>{props.onCancel && <button type="button" className="secondary compact" onClick={props.onCancel}>{t.review.cancelReview}</button>}</div>;
  if (review?.status !== 'done') {
    const emphasizePrimary = !!props.worktree;
    const showModelSettings = !!review?.error && isModelErrorText(review.error);
    return <div className="empty-state review-empty">
      <button type="button" className={emphasizePrimary ? 'primary' : 'secondary'} disabled={props.hasModel && !props.canReview} onClick={props.hasModel ? () => props.onReview() : props.onAddModel}><Icon name={props.hasModel ? 'shield' : 'plus'} />{props.hasModel ? t.review.startReview : t.review.addModelButton}</button>
      {review?.error && <p className="danger help" role="alert">{review.error}</p>}
      {showModelSettings && (
        <button type="button" className="secondary compact" onClick={props.onOpenModelSettings}>
          <Icon name="settings" />
          {t.workbench.openModelSettingsAction}
        </button>
      )}
    </div>;
  }
  const uncovered = props.worktree ? selectedFiles(props.worktree).filter((file) => !review.scopeFileIds?.includes(file.id)) : [];
  const noFilesReviewed = review.reviewedFileIds !== undefined && review.reviewedFileIds.length === 0 && review.skippedPaths.length > 0;
  const passedTip = noFilesReviewed
    ? t.review.noFilesReviewedTip
    : review.changedPaths.length || uncovered.length
      ? t.review.changedScopeTip
      : t.review.passedScopeTip;
  return <div className="review-panel">
    {!!uncovered.length && <div className="change-notice">{t.review.uncoveredNotice(uncovered.length)}</div>}
    {!!review.changedPaths.length && <div className="change-notice"><span>{t.review.changedNotice(review.changedPaths.length)}</span><button type="button" disabled={!props.canReview} onClick={() => props.onReview(true)}>{t.review.reReviewChangedButton}</button></div>}
    <div className="review-results scroll-area">
      {!review.findings.length ? <div className="empty-state review-passed"><Icon name="shield" className={review.changedPaths.length || noFilesReviewed ? 'warning' : 'success'} /><p className="row">{noFilesReviewed ? t.review.noFilesReviewedTitle : review.changedPaths.length || uncovered.length ? t.review.previousScopePassedTitle : t.review.passedTitle}<HelpTip align="end" label={t.review.resultTipLabel} text={passedTip} /></p></div>
        : review.findings.map((finding) => <button type="button" className={`review-finding finding-${finding.severity}`} key={finding.id} onClick={() => props.onOpenFile(finding.fileId, finding.line)}>
          <span className="finding-location"><span className="mono muted">{splitPath(finding.path).name}:{finding.line}</span><span className={finding.severity === 'high' ? 'danger' : 'warning'}>{finding.severity === 'high' ? t.review.severityHigh : t.review.severitySuggestion} <Icon name="external" /></span></span>
          <strong>{finding.title}</strong><span className="muted">{finding.reason}</span>
        </button>)}
      {!!review.skippedPaths.length && <div className="review-coverage"><p className="warning row">{t.review.skippedHeading}<HelpTip align="start" label={t.review.skippedTipLabel} text={t.review.skippedTipText} /></p>{review.skippedPaths.map((path) => <p className="mono muted truncate" key={path} title={path}>{path}</p>)}</div>}
    </div>
  </div>;
}
