import { useState } from 'react';
import { useI18n } from '../../../i18n';
import { Icon, Popover } from '../../../ui/controls';
import { formatModelDisplayLabel } from '../../settings/modelProviders';
import type { ModelConfiguration, ProviderConnection } from '../../settings/modelProviders';
import { primaryAction } from '../state/workbench';
import type { Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  connections: ProviderConnection[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  busy: boolean;
  onConfigureModel(): void;
  onManageModels(): void;
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
  const activeDisplayLabel = model
    ? formatModelDisplayLabel(model, props.connections, props.models)
    : '';

  return (
    <footer className="action-footer">
      <div className="menu-anchor model-anchor">
        {!model ? (
          <button type="button" className="add-model-button" onClick={props.onConfigureModel}>
            <Icon name="plus" />{t.footer.addModel}
          </button>
        ) : (
          <>
            <button
              type="button"
              className="context-button model-button"
              data-popover-trigger
              aria-expanded={menuOpen}
              title={activeDisplayLabel}
              onClick={() => setMenuOpen(!menuOpen)}
            >
              <Icon name="cpu" />
              <span className="mono truncate">{activeDisplayLabel}</span>
              <Icon name="down" />
            </button>
            {menuOpen && (
              <Popover title={t.footer.selectModelTitle} onClose={() => setMenuOpen(false)} className="model-menu">
                {props.models.map((item) => {
                  const conn = props.connections.find((entry) => entry.id === item.connectionId);
                  const displayName = formatModelDisplayLabel(item, props.connections, props.models);
                  const subSource = conn?.name ?? t.providers[item.provider];
                  return (
                    <button
                      type="button"
                      key={item.id}
                      aria-pressed={item.id === props.activeModelId}
                      onClick={() => {
                        props.onSelectModel(item.id);
                        setMenuOpen(false);
                      }}
                    >
                      <span className="model-menu-label">
                        <span className="model-menu-name truncate">{displayName}</span>
                        <span className="muted small truncate">
                          {subSource} · <span className="mono">{item.modelId}</span>
                        </span>
                      </span>
                      {item.id === props.activeModelId && <Icon name="check" className="success" />}
                    </button>
                  );
                })}
                <div className="menu-divider">
                  <button
                    type="button"
                    onClick={() => {
                      setMenuOpen(false);
                      props.onManageModels();
                    }}
                  >
                    <Icon name="settings" />{t.footer.manageModelsMenuItem}
                  </button>
                </div>
              </Popover>
            )}
          </>
        )}
      </div>
      <div className="menu-anchor action-anchor">
        <div className={`primary-button-group ${action.resume ? `resume-button-group resume-${worktree?.sessionStatus ?? 'running'}` : ''}`}>
          <button
            type="button"
            className="primary"
            disabled={action.disabled || (props.busy && !action.resume && !action.configureModel)}
            onClick={action.configureModel ? props.onConfigureModel : props.onPrimary}
          >
            <Icon name={primaryIcon} />
            {action.label}
          </button>
        </div>
      </div>
    </footer>
  );
}
