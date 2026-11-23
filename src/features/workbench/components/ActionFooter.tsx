import { useState } from 'react';
import { Icon, Popover } from '../../../ui/controls';
import { providerFor } from '../../settings/modelProviders';
import type { ModelConfiguration } from '../../settings/modelProviders';
import { isProtected, primaryAction, selectedFiles, splitPath } from '../state/workbench';
import type { Change, Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  models: ModelConfiguration[];
  activeModelId: string | null;
  busy: boolean;
  onAddModel(): void;
  onSelectModel(id: string): void;
  onPrimary(): void;
  onCommit(selected: Change[], push: boolean): Promise<boolean>;
}
interface Confirmation { branch: string; detached: boolean; scopeVersion: string; selected: Change[] }
export function ActionFooter(props: Props) {
  const [menu, setMenu] = useState<'model' | 'scope' | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [error, setError] = useState('');
  const model = props.models.find((model) => model.id === props.activeModelId);
  const action = primaryAction(props.worktree, !!model);
  const worktree = props.worktree;
  const branchBlocked = !!worktree?.detached;
  const scopeChanged = !!confirmation && !!worktree && (confirmation.branch !== worktree.branch
    || confirmation.detached !== !!worktree.detached
    || confirmation.scopeVersion !== worktree.scopeVersion
    || confirmation.selected.map((file) => file.id).sort().join('\0') !== selectedFiles(worktree).map((file) => file.id).sort().join('\0'));
  function openScope() {
    if (!worktree) return;
    setConfirmation({ branch: worktree.branch, detached: !!worktree.detached, scopeVersion: worktree.scopeVersion, selected: selectedFiles(worktree) });
    setError(''); setMenu(menu === 'scope' ? null : 'scope');
  }
  async function commit(push: boolean) {
    if (!worktree || worktree.readError || branchBlocked || !confirmation || scopeChanged || props.busy) return;
    if (!worktree.draft.text.trim()) { setError('请先填写提交说明，当前草稿与选择已保留。'); return; }
    if (!await props.onCommit(confirmation.selected, push)) setError('提交尚未执行。请查看上方错误说明；草稿与选择已保留。');
    else setMenu(null);
  }
  const highCount = worktree?.review.findings.filter((finding) => finding.severity === 'high').length ?? 0;
  return <footer className="action-footer">
    <div className="menu-anchor model-anchor">
      {!model ? <button type="button" className="add-model-button" onClick={props.onAddModel}><Icon name="plus" />添加模型</button>
        : <><button type="button" className="context-button model-button" data-popover-trigger aria-expanded={menu === 'model'} onClick={() => setMenu(menu === 'model' ? null : 'model')}><Icon name="cpu" /><span className="mono truncate">{model.name}</span><Icon name="down" /></button>
          {menu === 'model' && <Popover title="选择模型" onClose={() => setMenu(null)} className="model-menu">{props.models.map((item) => <button type="button" key={item.id} aria-pressed={item.id === props.activeModelId} onClick={() => { props.onSelectModel(item.id); setMenu(null); }}><span className="model-menu-label"><span className="model-menu-name truncate">{item.name}</span><span className="muted small truncate">{providerFor(item.provider).label} · <span className="mono">{item.modelId}</span></span></span>{item.id === props.activeModelId && <Icon name="check" className="success" />}</button>)}
            <div className="menu-divider"><button type="button" onClick={() => { setMenu(null); props.onAddModel(); }}><Icon name="plus" />添加模型…</button></div>
          </Popover>}
        </>}
    </div>
    <div className="menu-anchor action-anchor"><div className={`primary-button-group ${action.resume ? `resume-button-group resume-${worktree?.sessionStatus ?? 'running'}` : ''}`}>
      <button type="button" className="primary" disabled={action.disabled || (props.busy && !action.resume)} onClick={action.resume || model ? props.onPrimary : openScope}><Icon name={action.resume ? worktree?.sessionStatus === 'pushFailed' || worktree?.sessionStatus === 'conflict' ? 'alert' : 'history' : model ? 'upload' : 'commit'} />{action.label}</button>
      {model && !action.resume && <button type="button" className="primary action-dropdown" aria-label="确认提交范围或手动提交" data-popover-trigger aria-expanded={menu === 'scope'} disabled={action.disabled || props.busy} onClick={openScope}><Icon name="down" /></button>}
    </div>
      {menu === 'scope' && confirmation && worktree && <Popover title="确认本次提交范围" onClose={() => setMenu(null)} className="scope-menu">
        <div className="section-heading"><strong>确认本次提交范围</strong><span className="row min-width-zero"><span className="mono muted small truncate">{worktree.detached ? '分离 HEAD' : worktree.branch}</span>{!worktree.detached && isProtected(worktree.branch) && <span className="branch-tag">保护分支</span>}</span></div>
        <div className="scope-files scroll-area">{confirmation.selected.map((file) => <div className="row mono small" key={file.id}><span className="truncate" title={file.path}>{splitPath(file.path).name}</span><span className="muted">{file.range.kind === 'partial' ? '仅已暂存 ' : ''}{file.binary ? '二进制' : `+${file.range.added} -${file.range.removed}`}</span></div>)}</div>
        {worktree.remote.kind === 'tracked' ? <p className="muted help">推送目标：<span className="mono">{worktree.remote.branch}</span>；还会包含当前分支已有的 {worktree.remote.ahead} 个领先提交。</p> : <p className="warning help">{worktree.remote.reason}；推送前需明确确认目标。</p>}
        {highCount > 0 && <p className="form-result danger">仍有 {highCount} 项高危审查意见，可继续提交或先修复。</p>}
        {worktree.readError && <p className="form-result warning" role="alert">状态读取失败，请重新读取后再确认。当前草稿已保留。</p>}
        {worktree.detached && <p className="form-result warning" role="alert">当前处于分离 HEAD，请先在顶部切换已有分支，或在 IDE / 终端切好分支后再提交。</p>}
        {scopeChanged && <p className="form-result warning" role="alert">范围已变化，请关闭后重新确认。当前草稿已保留。</p>}
        {error && <p className="danger help" role="alert">{error}</p>}
        <div className="scope-actions"><button type="button" className="secondary" disabled={props.busy || scopeChanged || !!worktree.readError || branchBlocked} onClick={() => void commit(false)}>仅本地提交</button><button type="button" className="primary" disabled={props.busy || scopeChanged || !!worktree.readError || branchBlocked} onClick={() => void commit(true)}>提交并推送</button></div>
        {!model && <button type="button" className="add-model-hint" onClick={() => { setMenu(null); props.onAddModel(); }}>添加模型后可一键使用 C-Push →</button>}
      </Popover>}
    </div>
  </footer>;
}
