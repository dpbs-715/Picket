import { useState } from 'react';
import { useI18n } from '../../../i18n';
import { Icon, Popover } from '../../../ui/controls';
import type { ModelConfiguration } from '../../settings/modelProviders';
import { primaryAction } from '../state/workbench';
import type { Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  models: ModelConfiguration[];
  activeModelId: string | null;
  busy: boolean;
  onAddModel(): void;
  onSelectModel(id: string): void;
  onPrimary(): void;
}

export function ActionFooter(props: Props) {
  const { locale, t } = useI18n();
  const [menuOpen, setMenuOpen] = useState(false);
  const model = props.models.find((item) => item.id === props.activeModelId);
  const action = primaryAction(props.worktree, !!model, locale);
  const worktree = props.worktree;
  const primaryIcon = action.configureModel
    ? 'settings'
    : action.resume
      ? worktree?.sessionStatus === 'pushFailed' || worktree?.sessionStatus === 'conflict'
        ? 'alert'
        : 'history'
      : 'upload';

  return <footer className="action-footer">
    <div className="menu-anchor model-anchor">
      {!model ? <button type="button" className="add-model-button" onClick={props.onAddModel}><Icon name="plus" />{t.footer.addModel}</button>
        : <><button type="button" className="context-button model-button" data-popover-trigger aria-expanded={menuOpen} onClick={() => setMenuOpen(!menuOpen)}><Icon name="cpu" /><span className="mono truncate">{model.name}</span><Icon name="down" /></button>
          {menuOpen && <Popover title={t.footer.selectModelTitle} onClose={() => setMenuOpen(false)} className="model-menu">{props.models.map((item) => <button type="button" key={item.id} aria-pressed={item.id === props.activeModelId} onClick={() => { props.onSelectModel(item.id); setMenuOpen(false); }}><span className="model-menu-label"><span className="model-menu-name truncate">{item.name}</span><span className="muted small truncate">{t.providers[item.provider]} · <span className="mono">{item.modelId}</span></span></span>{item.id === props.activeModelId && <Icon name="check" className="success" />}</button>)}
            <div className="menu-divider"><button type="button" onClick={() => { setMenuOpen(false); props.onAddModel(); }}><Icon name="plus" />{t.footer.addModelMenuItem}</button></div>
          </Popover>}
        </>}
    </div>
    <div className="menu-anchor action-anchor">
      <div className={`primary-button-group ${action.resume ? `resume-button-group resume-${worktree?.sessionStatus ?? 'running'}` : ''}`}>
        <button
          type="button"
          className="primary"
          disabled={action.disabled || (props.busy && !action.resume && !action.configureModel)}
          onClick={action.configureModel ? props.onAddModel : props.onPrimary}
        >
          <Icon name={primaryIcon} />
          {action.label}
        </button>
      </div>
    </div>
  </footer>;
}
