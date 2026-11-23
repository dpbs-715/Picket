import { HelpTip, Icon, IconButton, Overlay } from '../../ui/controls';
import { editors } from '../workbench/state/workbench';
import type { EditorId } from '../workbench/state/workbench';
import { apiFormatFor, providerFor } from './modelProviders';
import type { ModelConfiguration } from './modelProviders';

interface Props {
  editor: EditorId;
  installedEditors?: EditorId[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  covered: boolean;
  onEditor(editor: EditorId): void;
  onSelectModel(id: string): void;
  onEditModel(model: ModelConfiguration): void;
  onDeleteModel(id: string): void;
  onAddModel(): void;
  onClose(): void;
  error?: string;
}
export function SettingsPanel(props: Props) {
  const visibleEditors = props.installedEditors
    ? editors.filter((item) => props.installedEditors!.includes(item.id))
    : editors;
  const editor = visibleEditors.find((item) => item.id === props.editor)
    ?? editors.find((item) => item.id === props.editor)
    ?? editors[0];
  return <Overlay title="设置" onClose={props.onClose} covered={props.covered}>
    <div className="settings-content scroll-area">
      <section>
        <div className="row section-title-row">
          <h2>编辑器联动</h2>
          {visibleEditors.length > 0 && <HelpTip placement="bottom" align="start" label="编辑器联动说明" text={`调用 ${editor.cli} 打开文件、已暂存 Diff 与审查行号；自动识别本机已安装编辑器并保存选择。`} />}
        </div>
        {visibleEditors.length === 0 ? <div className="model-empty">
          <div className="empty-copy">
            <p className="empty-title">未检测到已安装的受支持编辑器</p>
            <p className="muted help">安装 IntelliJ IDEA、RustRover、WebStorm、VS Code 或 Cursor 后重新打开即可识别。</p>
          </div>
        </div> : <div className="editor-grid" role="group" aria-label="首选编辑器">
          {visibleEditors.map((item) => <button type="button" key={item.id} aria-pressed={item.id === editor.id} onClick={() => props.onEditor(item.id)}>
            {item.name}{item.id === editor.id && <Icon name="check" />}
          </button>)}
        </div>}
      </section>
      <section><div className="section-heading"><div className="row"><h2>模型管理</h2><span className="model-count mono">{props.models.length}</span></div>{props.models.length > 0 && <button type="button" className="secondary compact" onClick={props.onAddModel}><Icon name="plus" />添加模型</button>}</div>
        {props.error && <p className="form-result danger" role="alert">{props.error}</p>}
        {!props.models.length ? <div className="model-empty">
          <span className="empty-badge" aria-hidden="true"><Icon name="cpu" /></span>
          <div className="empty-copy"><p className="empty-title">尚未配置可用模型</p></div>
          <button type="button" className="primary model-empty-action" onClick={props.onAddModel}>添加第一个模型</button>
        </div>
          : <div className="model-list">{props.models.map((model) => {
            const active = model.id === props.activeModelId;
            const providerMeta = providerFor(model.provider);
            const effectiveFormat = model.apiFormat ?? providerMeta.defaultApiFormat;
            const formatShort = apiFormatFor(effectiveFormat).short;
            const showFormat = model.provider === 'custom' || effectiveFormat !== providerMeta.defaultApiFormat;
            const detailTitle = `${providerMeta.label}（${formatShort}）· ${model.modelId}${model.baseUrl ? ` · ${model.baseUrl}` : ''}`;
            return <div className={`model-row ${active ? 'is-active' : ''}`} key={model.id}>
              <button type="button" className="model-select" aria-pressed={active} title={detailTitle} onClick={() => props.onSelectModel(model.id)}>
                <span className="model-body">
                  <span className="model-title-row"><strong className="truncate">{model.name}</strong>{active && <span className="model-active-badge"><span className="status-dot" />当前使用</span>}</span>
                  <span className="model-meta-row"><span className="model-provider">{providerMeta.label}{showFormat ? ` · ${formatShort}` : ''}</span><span className="model-sep" aria-hidden="true">·</span><span className="mono model-id truncate">{model.modelId}</span></span>
                </span>
              </button>
              <div className="model-actions">
                <IconButton icon="pencil" className="model-edit" label={`编辑模型 ${model.name}`} onClick={() => props.onEditModel(model)} />
                <IconButton icon="trash" className="model-delete" label={`删除模型 ${model.name}`} onClick={() => props.onDeleteModel(model.id)} />
              </div>
            </div>;
          })}</div>}
      </section>
    </div>
  </Overlay>;
}
