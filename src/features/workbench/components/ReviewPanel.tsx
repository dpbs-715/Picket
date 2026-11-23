import { HelpTip, Icon } from '../../../ui/controls';
import { selectedFiles, splitPath } from '../state/workbench';
import type { Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  hasModel: boolean;
  canReview: boolean;
  onAddModel(): void;
  onReview(changedOnly?: boolean): void;
  onCancel?(): void;
  onOpenFile(fileId: string, line: number): void;
}
export function ReviewPanel(props: Props) {
  const review = props.worktree?.review;
  if (review?.status === 'running') return <div className="empty-state muted"><Icon name="refresh" className="spinning accent" /><p>正在审查已选范围…</p>{props.onCancel && <button type="button" className="secondary compact" onClick={props.onCancel}>取消审查</button>}</div>;
  if (review?.status !== 'done') {
    const emphasizePrimary = !!props.worktree;
    return <div className="empty-state review-empty">
      <div className="row">
        <button type="button" className={emphasizePrimary ? 'primary' : 'secondary'} disabled={props.hasModel && !props.canReview} onClick={props.hasModel ? () => props.onReview() : props.onAddModel}><Icon name={props.hasModel ? 'shield' : 'plus'} />{props.hasModel ? '开始审查已选范围' : '添加模型'}</button>
        {props.hasModel && <HelpTip align="end" label="代码审查范围说明" text="仅审查当前勾选的变更范围；锁文件与二进制内容自动跳过审查，不影响正常提交。" />}
      </div>
      {review?.error && <p className="danger help" role="alert">{review.error}</p>}
    </div>;
  }
  const uncovered = props.worktree ? selectedFiles(props.worktree).filter((file) => !review.scopeFileIds?.includes(file.id)) : [];
  const noFilesReviewed = review.reviewedFileIds !== undefined && review.reviewedFileIds.length === 0 && review.skippedPaths.length > 0;
  const passedTip = noFilesReviewed
    ? '锁文件或二进制内容默认跳过审查，仍可正常勾选提交。'
    : review.changedPaths.length || uncovered.length
      ? '当前范围已有变化，需重新审查后才能判断最新内容。'
      : '仅代表实际已审范围内未发现风险项。';
  return <div className="review-panel">
    {!!uncovered.length && <div className="change-notice">当前选择有 {uncovered.length} 个文件尚未审查，请重新审查已选范围。</div>}
    {!!review.changedPaths.length && <div className="change-notice"><span>审查后有 {review.changedPaths.length} 个文件已变化</span><button type="button" disabled={!props.canReview} onClick={() => props.onReview(true)}>复审变动文件</button></div>}
    <div className="review-results scroll-area">
      {!review.findings.length ? <div className="empty-state review-passed"><Icon name="shield" className={review.changedPaths.length || noFilesReviewed ? 'warning' : 'success'} /><p className="row">{noFilesReviewed ? '所选范围均未纳入审查' : review.changedPaths.length || uncovered.length ? '原审查范围未发现风险项' : '审查通过'}<HelpTip align="end" label="审查结果说明" text={passedTip} /></p></div>
        : review.findings.map((finding) => <button type="button" className={`review-finding finding-${finding.severity}`} key={finding.id} onClick={() => props.onOpenFile(finding.fileId, finding.line)}>
          <span className="finding-location"><span className="mono muted">{splitPath(finding.path).name}:{finding.line}</span><span className={finding.severity === 'high' ? 'danger' : 'warning'}>{finding.severity === 'high' ? '高危' : '建议'} <Icon name="external" /></span></span>
          <strong>{finding.title}</strong><span className="muted">{finding.reason}</span>
        </button>)}
      {!!review.skippedPaths.length && <div className="review-coverage"><p className="warning row">以下内容未覆盖审查<HelpTip align="start" label="未覆盖审查说明" text="锁文件、二进制或超长改动默认跳过审查，不影响正常勾选提交。" /></p>{review.skippedPaths.map((path) => <p className="mono muted truncate" key={path} title={path}>{path}</p>)}</div>}
    </div>
  </div>;
}
