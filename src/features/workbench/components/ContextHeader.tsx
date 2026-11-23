import { useState } from 'react';
import { Icon, IconButton, Popover } from '../../../ui/controls';
import { isActiveSession, isProtected, messageLabels, orderedMessages } from '../state/workbench';
import type { Repository, WorkbenchState, Worktree } from '../state/workbench';

type Menu = 'repository' | 'history' | 'worktree' | 'branch' | 'remote' | null;
interface Props {
  state: WorkbenchState;
  worktree?: Worktree;
  repository?: Repository;
  pinned: boolean;
  opening: boolean;
  branchBusy: boolean;
  pullBusy?: boolean;
  pinBusy: boolean;
  onPin(): void;
  onSettings(): void;
  onOpenRepository(): void;
  onSwitch(worktreeId: string, expand?: boolean): void;
  onBranch(kind: 'switchBranch', branch: string): Promise<boolean>;
  onPull(): void;
}

export function ContextHeader(props: Props) {
  const [menu, setMenu] = useState<Menu>(null);
  const { state, worktree, repository } = props;
  const repoWorktrees = state.worktrees.filter((tree) => tree.repositoryId === repository?.id);
  const pending = state.worktrees.some(isActiveSession);
  const branchDisabled = props.branchBusy || !!worktree?.readError || repoWorktrees.some(isActiveSession);
  async function submitBranch(branch: string) {
    const accepted = await props.onBranch('switchBranch', branch);
    if (accepted) setMenu(null);
    return accepted;
  }
  function toggle(next: Menu) { setMenu(menu === next ? null : next); }
  function switchTree(id: string, expand = false) { props.onSwitch(id, expand); setMenu(null); }
  const close = () => setMenu(null);

  return <header className="context-header">
    <div className="context-row context-top" data-tauri-drag-region>
      <div className="menu-anchor repository-anchor" data-tauri-drag-region>
        <button type="button" className="context-button repository-button" disabled={props.opening} data-popover-trigger aria-expanded={menu === 'repository'} onClick={() => toggle('repository')}>
          <span className="mono truncate">{repository?.name ?? (props.opening ? '正在打开…' : '打开仓库')}</span><Icon name="down" />
        </button>
        {menu === 'repository' && <Popover title="仓库" onClose={close} className="repository-menu">
          {state.repositories.map((repo) => {
            const trees = state.worktrees.filter((tree) => tree.repositoryId === repo.id);
            return <button type="button" key={repo.id} aria-pressed={repo.id === repository?.id} onClick={() => { if (trees[0]) switchTree(trees[0].worktreeId); }}>
              <span className="mono truncate">{repo.name}</span><span className="muted small">{trees.length <= 1 ? '单目录' : `${trees.length} 工作树`}</span>
            </button>;
          })}
          {!state.repositories.length && <p className="muted menu-note">尚未打开仓库</p>}
          <div className="menu-divider"><button type="button" disabled={props.opening} onClick={() => { close(); props.onOpenRepository(); }}><Icon name="plus" />打开本机仓库…</button></div>
        </Popover>}
      </div>
      <div className="row header-tools">
        <div className="menu-anchor"><IconButton icon="history" label="C-Push 会话记录" data-popover-trigger aria-expanded={menu === 'history'} onClick={() => toggle('history')} className={pending ? 'has-pending' : ''} />
          {menu === 'history' && <Popover title="C-Push 会话记录" onClose={close} className="history-menu">
            <div className="menu-title">C-Push 会话记录 <span className="muted small">按工作树</span></div>
            {!state.worktrees.length && <p className="muted menu-note">暂无会话记录</p>}
            {state.worktrees.map((tree) => {
              const message = orderedMessages(tree.messages)[0];
              return <button type="button" className="history-entry" key={tree.worktreeId} onClick={() => switchTree(tree.worktreeId, !!message && isActiveSession(tree))}>
                <span className="row"><span className={`status-dot status-${tree.sessionStatus}`} /><span className="mono truncate">{tree.label}</span><span className="mono muted small truncate">{tree.branch}</span><span className="small muted">{message ? messageLabels[message.kind] : '空闲'}</span></span>
                <span className="muted small truncate">{message?.title ?? '暂无进行中的会话'}</span>
              </button>;
            })}
          </Popover>}
        </div>
        <IconButton icon="pin" label={props.pinned ? '取消置顶' : '窗口置顶'} aria-pressed={props.pinned} disabled={props.pinBusy} onClick={props.onPin} />
        <IconButton icon="settings" label="设置" onClick={() => { close(); props.onSettings(); }} />
      </div>
    </div>
    <div className="context-row context-bottom">
      <div className="row context-path">
        {worktree && repoWorktrees.length > 1 && <><div className="menu-anchor worktree-anchor">
          <button type="button" className="context-button" data-popover-trigger aria-expanded={menu === 'worktree'} onClick={() => toggle('worktree')}><span className={`status-dot status-${worktree.sessionStatus}`} /><Icon name="branch" /><span className="mono truncate">{worktree.label}</span><Icon name="down" /></button>
          {menu === 'worktree' && <Popover title="工作树" onClose={close} className="worktree-menu">
            {repoWorktrees.map((tree) => <button type="button" key={tree.worktreeId} aria-pressed={tree.worktreeId === worktree.worktreeId} onClick={() => switchTree(tree.worktreeId)}>
              <span className="row min-width-zero"><span className={`status-dot status-${tree.sessionStatus}`} /><span className="mono truncate">{tree.label}</span><span className="mono muted small truncate">{tree.branch}</span></span><span className="muted small">{tree.readError ? '不可用' : tree.files.length ? `${tree.files.length} 项` : '干净'}</span>
            </button>)}
          </Popover>}
        </div><span className="muted">/</span></>}
        <div className="menu-anchor branch-anchor">
          <button type="button" className={`context-button ${worktree && isProtected(worktree.branch) ? 'warning' : ''}`} disabled={!worktree} data-popover-trigger aria-expanded={menu === 'branch'} onClick={() => toggle('branch')}><Icon name="branch" /><span className="mono truncate">{worktree?.detached ? '分离 HEAD' : worktree?.branch ?? '未选择分支'}</span>{worktree && <Icon name="down" />}</button>
          {menu === 'branch' && worktree && <Popover title="分支" onClose={close} className="branch-menu">
            {repository?.branches.length ? repository.branches.map((branch) => <button type="button" key={branch.name} disabled={!branch.worktreeId && branchDisabled} aria-pressed={branch.name === worktree.branch} onClick={() => {
              if (branch.worktreeId) switchTree(branch.worktreeId);
              else { void submitBranch(branch.name); }
            }}><span className="row min-width-zero"><span className="mono truncate">{branch.name}</span>{isProtected(branch.name) && <span className="branch-tag">保护分支</span>}</span>{branch.worktreeId && branch.worktreeId !== worktree.worktreeId ? <span className="small muted truncate">跳转 {state.worktrees.find((tree) => tree.worktreeId === branch.worktreeId)?.label}</span> : branch.name === worktree.branch && <Icon name="check" />}</button>)
              : <p className="muted menu-note">尚无本地分支</p>}
          </Popover>}
        </div>
      </div>
      <div className="menu-anchor remote-anchor"><button type="button" className={`context-button remote-button mono ${worktree?.remote.kind === 'tracked' && worktree.remote.behind ? 'warning' : ''}`} disabled={!worktree} data-popover-trigger aria-expanded={menu === 'remote'} onClick={() => toggle('remote')} aria-label="远端详情">
        {worktree?.remote.kind === 'tracked' ? <><span>↑ {worktree.remote.ahead}</span><span>↓ {worktree.remote.behind}</span></> : '未同步'}
      </button>
        {menu === 'remote' && worktree && <Popover title="远端详情" onClose={close} className="remote-menu"><div className="menu-title muted">跟踪远端分支</div>
          {worktree.remote.kind === 'tracked' ? <><p className="mono">{worktree.remote.branch}</p><p className="muted small">领先 {worktree.remote.ahead} 个提交　落后 {worktree.remote.behind} 个提交</p></> : <p className="warning help">{worktree.remote.reason}</p>}
          {worktree.remote.kind === 'tracked' && worktree.remote.localOnly && <p className="muted help">仅基于本地跟踪引用，尚未联网检查远端。</p>}
          <div className="menu-divider"><button type="button" className="secondary" disabled={props.pullBusy || repoWorktrees.some(isActiveSession)} onClick={() => { close(); props.onPull(); }}>{props.pullBusy ? '同步中…' : '拉取并变基'}</button></div>
        </Popover>}
      </div>
    </div>
  </header>;
}
