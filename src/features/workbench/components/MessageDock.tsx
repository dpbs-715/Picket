import { Icon, IconButton } from '../../../ui/controls';
import { messageLabels, orderedMessages } from '../state/workbench';
import type { Message, Worktree } from '../state/workbench';

interface Props {
  worktree: Worktree;
  busy: boolean;
  editorShort: string;
  onExpanded(expanded: boolean): void;
  onReply(messageId: string, text: string): void;
  onRespond(message: Message, answer: { optionId?: string; text?: string }): void;
  onOpenFile(fileId: string): void;
  onReview(): void;
}

export function MessageDock(props: Props) {
  const [primary, ...secondary] = orderedMessages(props.worktree.messages);
  if (!primary) return null;
  const text = props.worktree.replies[primary.id] ?? '';
  if (!props.worktree.dockExpanded) return <button type="button" className={`message-collapsed message-${primary.kind}`} onClick={() => props.onExpanded(true)} aria-label={`展开消息：${primary.title}`}>
    <span className="status-dot" /><span className="message-status">{messageLabels[primary.kind]}</span><span className="truncate">{primary.title}{primary.scope ? ` · ${primary.scope}` : ''}</span>
    {!!secondary.length && <span className="dynamic-count">+{secondary.length}</span>}<Icon name="up" />
  </button>;
  const canReply = primary.kind === 'question' || primary.kind === 'conflict';
  return <div className={`message-expanded message-${primary.kind}`} role="region" aria-label="当前事项">
    <div className="message-heading"><div className="row min-width-zero"><span className="status-dot" /><span className="message-status">{messageLabels[primary.kind]}</span>{primary.scope && <span className="mono muted small truncate">· {primary.scope}</span>}</div><IconButton icon="down" label="收起消息" onClick={() => props.onExpanded(false)} /></div>
    <div className="message-body scroll-area">
      <h2>{primary.title}</h2><p className="muted">{primary.description}</p>
      {!!primary.files?.length && <div className="file-chips">{primary.files.map((file) => <button type="button" key={file.id} onClick={() => props.onOpenFile(file.id)}><span className="mono truncate">{file.label}</span>{file.badge && <span className="warning small">{file.badge}</span>}<Icon name="external" /></button>)}</div>}
      {!!primary.commits?.length && <div className="message-commits">{primary.commits.map((commit) => <div className="row mono" key={commit.hash}><Icon name="commit" /><span className="accent">{commit.hash}</span><span className="truncate" title={commit.title}>{commit.title}</span></div>)}</div>}
      {primary.progress && <div className="message-progress"><p className="small muted">{primary.progress.label}</p><progress aria-label="任务进度" max={primary.progress.total} value={primary.progress.completed} /></div>}
      {secondary.map((message) => <div className="secondary-message" key={message.id}><Icon name={message.kind === 'review' ? 'shield' : 'history'} /><span className="truncate">{message.title}</span>{message.kind === 'review' && <button type="button" onClick={props.onReview}>查看</button>}</div>)}
    </div>
    <div className="message-actions scroll-area">
      {primary.options?.map((option) => <button type="button" className={option.recommended ? 'primary' : 'secondary'} disabled={props.busy} key={option.id} onClick={() => props.onRespond(primary, { optionId: option.id })}>{option.label}</button>)}
      {primary.kind === 'waitingManual' && <><button type="button" className="primary" disabled={!primary.files?.length} onClick={() => { const file = primary.files?.[0]; if (file) props.onOpenFile(file.id); }}><Icon name="external" />在 {props.editorShort} 中打开冲突文件</button><p className="muted help">对应写入已暂停。切回后将由桌面层重新核对 Git 状态。</p></>}
      {primary.kind === 'done' && <button type="button" className="secondary" onClick={() => props.onExpanded(false)}>知道了</button>}
      {canReply && <form className="message-reply" onSubmit={(event) => { event.preventDefault(); if (text.trim() && !props.busy) props.onRespond(primary, { text: text.trim() }); }}>
        <input aria-label="回复当前事项" value={text} onChange={(event) => props.onReply(primary.id, event.target.value)} placeholder={primary.kind === 'conflict' ? '输入合并要求，回车发送…' : '或输入更具体的要求，回车发送…'} />
        <IconButton icon="send" label="发送回复" type="submit" disabled={!text.trim() || props.busy} />
      </form>}
    </div>
  </div>;
}
