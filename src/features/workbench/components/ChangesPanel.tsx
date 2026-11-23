import { useState } from 'react';
import { Icon, IconButton } from '../../../ui/controls';
import { groupFiles, scopeSummary, splitPath } from '../state/workbench';
import type { Change, Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  editorShort: string;
  opening: boolean;
  repositoryReadsAvailable: boolean;
  onOpenRepository(): void;
  onSelect(fileId: string): void;
  onOpenFile(fileId: string): void;
  onReview(): void;
  onRefresh(): void;
  refreshing: boolean;
}
export function ChangesPanel(props: Props) {
  const [view, setView] = useState<'list' | 'tree'>('list');
  const worktree = props.worktree;
  function fileRow(file: Change, grouped = false) {
    const { name, directory } = splitPath(file.path);
    const findings = worktree!.review.findings.filter((finding) => finding.fileId === file.id);
    const hasHigh = findings.some((finding) => finding.severity === 'high');
    const action = file.status === 'U' ? '合并' : file.range.kind === 'partial' ? '已暂存 Diff' : file.status === 'A' ? '打开' : 'Diff';
    return <div className={`file-row ${grouped ? 'grouped' : ''} ${file.status === 'U' ? 'conflicted' : ''} ${file.ignoredForReview && !worktree!.selectedIds.includes(file.id) ? 'ignored' : ''}`} key={file.id}>
      <input type="checkbox" className="file-checkbox" checked={worktree!.selectedIds.includes(file.id)} aria-label={`选择 ${file.path}`} onChange={() => props.onSelect(file.id)} />
      <button type="button" className="file-open" title={`${file.previousPath ? `${file.previousPath} → ` : ''}${file.path} · ${props.editorShort} ${action}`} onClick={() => props.onOpenFile(file.id)}>
        <span className={`git-status git-${file.status}`} aria-label={`Git 状态 ${file.status}`}>{file.status === 'U' ? '!' : file.status}</span>
        <span className="file-label"><span className="mono file-name truncate">{name}</span>{!grouped && directory && <span className="mono file-directory truncate">{directory}</span>}
          {file.status === 'U' ? <span className="file-badge danger">· 冲突</span> : file.range.kind === 'partial' && <span className="file-badge warning">· 部分暂存</span>}
          {file.ignoredForReview && <span className="file-badge muted">· 忽略审查</span>}
        </span>
        <span className="file-stats mono">{file.binary ? <span className="muted">二进制</span> : <><span className="success">+{file.range.added}</span> <span className="danger">-{file.range.removed}</span></>}</span>
        <span className="file-hover">{props.editorShort} {action}<Icon name="external" /></span>
      </button>
      {!!findings.length && <button type="button" className={`finding-count ${hasHigh ? 'danger' : 'warning'}`} aria-label={`${file.path}：${findings.length} 项审查意见`} onClick={props.onReview}><span className="status-dot" />{findings.length}</button>}
    </div>;
  }
  return <section className="changes-panel" aria-label="变更文件">
    {worktree && <div className="changes-heading">
      <div className="row min-width-zero"><h2>变更</h2><span className="muted truncate">{scopeSummary(worktree)}</span></div>
      <div className="row">
        {worktree.files.length > 0 && <IconButton icon={view === 'list' ? 'tree' : 'list'} label={view === 'list' ? '切换目录树视图' : '切换列表视图'} onClick={() => setView(view === 'list' ? 'tree' : 'list')} />}
        <IconButton icon="refresh" label="刷新变更" disabled={props.refreshing} className={props.refreshing ? 'spinning' : ''} onClick={props.onRefresh} />
      </div>
    </div>}
    <div className="changes-list scroll-area">
      {!worktree ? <div className="empty-state repository-empty">
        <span className="empty-badge" aria-hidden="true"><Icon name="folder" /></span>
        <div className="empty-copy">
          <p className="empty-title">打开你的 Git 仓库</p>
          <p className="muted help">{props.repositoryReadsAvailable ? '只读加载分支与变更，不改动暂存区或草稿' : '选择本地目录加载分支与变更（浏览器预览不读取本机）'}</p>
        </div>
        <button type="button" className="primary repository-open-button" disabled={props.opening} onClick={props.onOpenRepository}>{props.opening ? '正在打开…' : '打开本机仓库…'}</button>
      </div>
        : worktree.readError ? <div className="empty-state">
          <span className="empty-badge warning" aria-hidden="true"><Icon name="alert" /></span>
          <div className="empty-copy"><p className="empty-title">工作树状态不可用</p><p className="warning help">{worktree.readError}</p></div>
          <button type="button" className="secondary compact" disabled={props.refreshing} onClick={props.onRefresh}>重试读取</button>
        </div>
        : !worktree.files.length ? <div className="empty-state">
          <span className="empty-badge success" aria-hidden="true"><Icon name="check" /></span>
          <div className="empty-copy"><p className="empty-title">工作区干净</p><p className="muted help">可在顶部切换工作树或继续编写代码</p></div>
        </div>
        : view === 'list' ? worktree.files.map((file) => fileRow(file))
        : groupFiles(worktree.files).map((group) => <div key={group.directory}><div className="directory-heading mono"><Icon name="folder" /><span className="truncate">{group.directory}</span></div>{group.files.map((file) => fileRow(file, true))}</div>)}
    </div>
  </section>;
}
