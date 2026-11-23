import { Icon, IconButton, Overlay } from '../../ui/controls';
import { editors } from '../workbench/state/workbench';
import type { EditorId } from '../workbench/state/workbench';
import { providerFor } from './modelProviders';
import type { ModelConfiguration } from './modelProviders';

interface Props {
  editor: EditorId;
  models: ModelConfiguration[];
  activeModelId: string | null;
  covered: boolean;
  onEditor(editor: EditorId): void;
  onSelectModel(id: string): void;
  onDeleteModel(id: string): void;
  onAddModel(): void;
  onClose(): void;
  error?: string;
}
export function SettingsPanel(props: Props) {
  const editor = editors.find((editor) => editor.id === props.editor)!;
  return <Overlay title="设置" onClose={props.onClose} covered={props.covered}>
    <div className="settings-content scroll-area">
      <section><h2>编辑器联动</h2><div className="editor-grid" role="group" aria-label="首选编辑器">
        {editors.map((editor) => <button type="button" key={editor.id} aria-pressed={editor.id === props.editor} onClick={() => props.onEditor(editor.id)}>
          {editor.name}{editor.id === props.editor && <Icon name="check" />}
        </button>)}
      </div><p className="mono muted help">CLI：{editor.cli} · 文件、Diff 与行号定位</p><p className="muted help">优先查找命令行入口与 macOS 应用包；首选编辑器及工作树草稿会在本机自动保存。</p></section>
      <section><div className="section-heading"><div className="row"><h2>模型管理</h2><span className="model-count mono">{props.models.length}</span></div>{props.models.length > 0 && <button type="button" className="secondary compact" onClick={props.onAddModel}><Icon name="plus" />添加模型</button>}</div>
        {props.error && <p className="form-result danger" role="alert">{props.error}</p>}
        {!props.models.length ? <div className="model-empty">
          <span className="empty-badge" aria-hidden="true"><Icon name="cpu" /></span>
          <div className="empty-copy"><p className="empty-title">尚未配置可用模型</p><p className="muted help">用于生成提交说明、代码审查与 C-Push</p></div>
          <button type="button" className="primary model-empty-action" onClick={props.onAddModel}>添加第一个模型</button>
        </div>
          : <div className="model-list">{props.models.map((model) => {
            const active = model.id === props.activeModelId;
            const providerLabel = providerFor(model.provider).label;
            const detailTitle = `${providerLabel} · ${model.modelId}${model.baseUrl ? ` · ${model.baseUrl}` : ''}`;
            return <div className={`model-row ${active ? 'is-active' : ''}`} key={model.id}>
              <button type="button" className="model-select" aria-pressed={active} title={detailTitle} onClick={() => props.onSelectModel(model.id)}>
                <span className="model-body">
                  <span className="model-title-row"><strong className="truncate">{model.name}</strong>{active && <span className="model-active-badge"><span className="status-dot" />当前使用</span>}</span>
                  <span className="model-meta-row"><span className="model-provider">{providerLabel}</span><span className="model-sep" aria-hidden="true">·</span><span className="mono model-id truncate">{model.modelId}</span></span>
                </span>
              </button>
              <IconButton icon="trash" className="model-delete" label={`删除模型 ${model.name}`} onClick={() => props.onDeleteModel(model.id)} />
            </div>;
          })}</div>}
      </section>
    </div>
  </Overlay>;
}
