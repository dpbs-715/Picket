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
      </div><p className="mono muted help">CLI：{editor.cli} · 文件、Diff 与行号定位</p><p className="muted help">需安装编辑器命令行工具。真实 IDE 联动在 M2 接入；本次选择仅在当前窗口保留。</p></section>
      <section><div className="section-heading"><h2>模型管理 ({props.models.length})</h2><button type="button" className="secondary compact" onClick={props.onAddModel}><Icon name="plus" />添加模型</button></div>
        {props.error && <p className="form-result danger" role="alert">{props.error}</p>}
        {!props.models.length ? <div className="model-empty"><p>尚未配置可用模型</p><button type="button" className="primary" onClick={props.onAddModel}>添加第一个模型</button></div>
          : <div className="model-list">{props.models.map((model) => <div className="model-row" key={model.id}>
            <button type="button" className="model-select" aria-pressed={model.id === props.activeModelId} onClick={() => props.onSelectModel(model.id)}>
              <span className="row"><strong className="truncate">{model.name}</strong>{model.id === props.activeModelId && <span className="success small">当前使用</span>}</span>
              <span className="mono muted small truncate">{providerFor(model.provider).label} · {model.modelId}</span>
            </button><IconButton icon="trash" label={`删除模型 ${model.name}`} onClick={() => props.onDeleteModel(model.id)} />
          </div>)}</div>}
      </section>
    </div>
  </Overlay>;
}
