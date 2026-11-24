import { useI18n } from '../../../i18n';
import { Icon, IconButton } from '../../../ui/controls';
import { orderedMessages } from '../state/workbench';
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
  const { t } = useI18n();
  const [primary, ...secondary] = orderedMessages(props.worktree.messages);
  if (!primary) return null;
  const text = props.worktree.replies[primary.id] ?? '';
  if (!props.worktree.dockExpanded) return <button type="button" className={`message-collapsed message-${primary.kind}`} onClick={() => props.onExpanded(true)} aria-label={t.dock.expandMessage(primary.title)}>
    <span className="status-dot" /><span className="message-status">{t.messageLabels[primary.kind]}</span><span className="truncate">{primary.title}{primary.scope ? ` · ${primary.scope}` : ''}</span>
    {!!secondary.length && <span className="dynamic-count">+{secondary.length}</span>}<Icon name="up" />
  </button>;
  const canReply = primary.kind === 'question' || primary.kind === 'conflict';
  return <div className={`message-expanded message-${primary.kind}`} role="region" aria-label={t.dock.activeRegionLabel}>
    <div className="message-heading"><div className="row min-width-zero"><span className="status-dot" /><span className="message-status">{t.messageLabels[primary.kind]}</span>{primary.scope && <span className="mono muted small truncate">· {primary.scope}</span>}</div><IconButton icon="down" label={t.dock.collapseMessage} onClick={() => props.onExpanded(false)} /></div>
    <div className="message-body scroll-area">
      <h2>{primary.title}</h2><p className="muted">{primary.description}</p>
      {!!primary.files?.length && <div className="file-chips">{primary.files.map((file) => <button type="button" key={file.id} onClick={() => props.onOpenFile(file.id)}><span className="mono truncate">{file.label}</span>{file.badge && <span className="warning small">{file.badge}</span>}<Icon name="external" /></button>)}</div>}
      {!!primary.commits?.length && <div className="message-commits">{primary.commits.map((commit) => <div className="row mono" key={commit.hash}><Icon name="commit" /><span className="accent">{commit.hash}</span><span className="truncate" title={commit.title}>{commit.title}</span></div>)}</div>}
      {primary.progress && <div className="message-progress"><p className="small muted">{primary.progress.label}</p><progress aria-label={t.dock.taskProgressLabel} max={primary.progress.total} value={primary.progress.completed} /></div>}
      {!!props.worktree.cPush?.timeline.length && <div className="session-timeline" aria-label={t.dock.timelineLabel}>{props.worktree.cPush.timeline.map((step, index) => <div className={`timeline-step timeline-${step.state}`} key={`${index}:${step.label}`}><span className="status-dot" /><span className="truncate">{step.label}</span></div>)}</div>}
      {!!props.worktree.cPush?.qaLog.length && <div className="session-qa-log" aria-label={t.dock.qaLogLabel}>{props.worktree.cPush.qaLog.map((qa) => <div className="qa-entry small muted" key={qa.questionId}><span className="truncate">{t.dock.qaEntry(qa.question, qa.answer)}</span></div>)}</div>}
      {secondary.map((message) => <div className="secondary-message" key={message.id}><Icon name={message.kind === 'review' ? 'shield' : 'history'} /><span className="truncate">{message.title}</span>{message.kind === 'review' && <button type="button" onClick={props.onReview}>{t.dock.viewReview}</button>}</div>)}
    </div>
    <div className="message-actions scroll-area">
      {primary.options?.map((option) => <button type="button" data-option-id={option.id} className={option.recommended ? 'primary' : 'secondary'} disabled={props.busy} key={option.id} onClick={() => props.onRespond(primary, { optionId: option.id })}>{option.label}</button>)}
      {primary.kind === 'waitingManual' && <button type="button" className="primary" disabled={!primary.files?.length} onClick={() => { const file = primary.files?.[0]; if (file) props.onOpenFile(file.id); }}><Icon name="external" />{t.dock.openConflictInEditor(props.editorShort)}</button>}
      {primary.kind === 'done' && <button type="button" className="secondary" onClick={() => props.onExpanded(false)}>{t.dock.gotIt}</button>}
      {canReply && <form className="message-reply" onSubmit={(event) => { event.preventDefault(); if (text.trim() && !props.busy) props.onRespond(primary, { text: text.trim() }); }}>
        <input aria-label={t.dock.replyLabel} value={text} onChange={(event) => props.onReply(primary.id, event.target.value)} placeholder={primary.kind === 'conflict' ? t.dock.conflictReplyPlaceholder : t.dock.questionReplyPlaceholder} />
        <IconButton icon="send" label={t.dock.sendReply} type="submit" disabled={!text.trim() || props.busy} />
      </form>}
    </div>
  </div>;
}
