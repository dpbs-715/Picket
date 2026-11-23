import type { RefObject } from 'react';
import { Icon } from '../../../ui/controls';

interface Props {
  text: string;
  editing: boolean;
  outdated: boolean;
  hasWorktree?: boolean;
  hasModel: boolean;
  canGenerate: boolean;
  generating: boolean;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  onChange(text: string): void;
  onEdit(): void;
  onGenerate(): void;
  onCancelGenerate?(): void;
  onAddModel(): void;
}
export function CommitPanel(props: Props) {
  const emphasizePrimary = props.hasWorktree ?? true;
  if (!props.editing && !props.text && !props.generating) return <div className="empty-state commit-empty">
    <div className="row"><button type="button" className={emphasizePrimary ? 'primary' : 'secondary'} disabled={props.hasModel && !props.canGenerate} onClick={props.hasModel ? props.onGenerate : props.onAddModel}><Icon name={props.hasModel ? 'sparkles' : 'plus'} />{props.hasModel ? '生成提交说明' : '添加模型'}</button>
      <button type="button" className="secondary" onClick={props.onEdit}><Icon name="pencil" />手动编写</button></div>
    <p className="muted help">{props.hasModel ? '默认遵循 Conventional Commits，参考近期提交风格' : '尚未配置模型，可先添加模型或直接手写提交说明'}</p>
  </div>;
  return <div className="commit-editor">
    {props.outdated && <div className="change-notice"><span>变更已更新，已保留当前草稿</span><button type="button" disabled={!props.canGenerate || props.generating} onClick={props.onGenerate}>重新生成</button></div>}
    {props.generating && <div className="generation-status" role="status"><span>正在生成提交说明，当前文字仍可编辑…</span>{props.onCancelGenerate && <button type="button" onClick={props.onCancelGenerate}>取消</button>}</div>}
    <textarea ref={props.textareaRef} aria-label="提交说明" value={props.text} onChange={(event) => props.onChange(event.target.value)} placeholder="编写提交说明…" spellCheck={false} />
  </div>;
}
