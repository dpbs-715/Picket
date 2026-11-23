import { Icon } from '../../../ui/controls';
import { scopeSummary, selectedFiles, splitPath } from '../state/workbench';
import type { Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  hasModel: boolean;
  canReview: boolean;
  onAddModel(): void;
  onReview(changedOnly?: boolean): void;
  onOpenFile(fileId: string, line: number): void;
}
export function ReviewPanel(props: Props) {
  const review = props.worktree?.review;
  if (review?.status === 'running') return <div className="empty-state muted"><Icon name="refresh" className="spinning accent" /><p>正在审查已选范围…</p></div>;
  if (review?.status !== 'done') return <div className="empty-state review-empty">
    <button type="button" className="primary" disabled={props.hasModel && !props.canReview} onClick={props.hasModel ? () => props.onReview() : props.onAddModel}><Icon name={props.hasModel ? 'shield' : 'plus'} />{props.hasModel ? '开始审查已选范围' : '添加模型'}</button>
    <p className="muted help">{props.hasModel ? `将审查${scopeSummary(props.worktree)}，自动过滤锁文件` : '添加模型后即可启用提交前代码审查'}</p>
    {review?.error && <p className="danger help" role="alert">{review.error}</p>}
  </div>;
  const uncovered = props.worktree ? selectedFiles(props.worktree).filter((file) => !review.scopeFileIds?.includes(file.id)) : [];
  return <div className="review-panel">
    {!!uncovered.length && <div className="change-notice">当前选择有 {uncovered.length} 个文件尚未审查，请重新审查已选范围。</div>}
    {!!review.changedPaths.length && <div className="change-notice"><span>审查后有 {review.changedPaths.length} 个文件已变化</span><button type="button" disabled={!props.canReview} onClick={() => props.onReview(true)}>复审变动文件</button></div>}
    <div className="review-results scroll-area">
      {!review.findings.length ? <div className="empty-state review-passed"><Icon name="shield" className={review.changedPaths.length ? 'warning' : 'success'} /><p>{review.changedPaths.length || uncovered.length ? '原审查范围未发现风险项' : '审查通过'}</p><span className="muted help">{review.changedPaths.length || uncovered.length ? '当前范围已有变化，需审查后才能判断最新内容' : '仅代表实际已审范围内未发现风险项'}</span></div>
        : review.findings.map((finding) => <button type="button" className={`review-finding finding-${finding.severity}`} key={finding.id} onClick={() => props.onOpenFile(finding.fileId, finding.line)}>
          <span className="finding-location"><span className="mono muted">{splitPath(finding.path).name}:{finding.line}</span><span className={finding.severity === 'high' ? 'danger' : 'warning'}>{finding.severity === 'high' ? '高危' : '建议'} <Icon name="external" /></span></span>
          <strong>{finding.title}</strong><span className="muted">{finding.reason}</span>
        </button>)}
      {!!review.skippedPaths.length && <div className="review-coverage"><p className="warning">以下内容未覆盖审查</p>{review.skippedPaths.map((path) => <p className="mono muted truncate" key={path} title={path}>{path}</p>)}<p className="muted help">忽略审查不影响勾选提交。</p></div>}
    </div>
  </div>;
}
