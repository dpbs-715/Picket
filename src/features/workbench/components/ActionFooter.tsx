import { useState } from 'react';
import type { CustomActionConfig } from '../../../../shared/custom-action-protocol';
import { useI18n } from '../../../i18n';
import { Icon, Popover, RotatingChevron } from '../../../ui/controls';
import { MotionPresence } from '../../../ui/motion';
import { formatModelDisplayLabel } from '../../settings/modelProviders';
import type { ModelConfiguration, ProviderConnection } from '../../settings/modelProviders';
import { primaryAction } from '../state/workbench';
import type { Worktree } from '../state/workbench';

interface Props {
  worktree?: Worktree;
  connections: ProviderConnection[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  customActions: CustomActionConfig[];
  activeAction?: CustomActionConfig;
  busy: boolean;
  onConfigureModel(): void;
  onManageModels(): void;
  onSelectModel(id: string): void;
  onSelectAction(actionId: string | null): void;
  onNewAction(mode: 'manual' | 'create'): void;
  onManageActions(): void;
  onPrimary(): void;
}

export function ActionFooter(props: Props) {
  const { locale, t } = useI18n();
  const [menuOpen, setMenuOpen] = useState(false);
  const [actionMenuOpen, setActionMenuOpen] = useState(false);
  const model = props.models.find((item) => item.id === props.activeModelId);
  const action = primaryAction(props.worktree, !!model, locale, props.activeAction);
  const worktree = props.worktree;
  const showSplitToggle = Boolean(model && worktree && !action.resume && !action.configureModel);
  const primaryIcon = action.configureModel
    ? 'settings'
    : action.resume
      ? worktree?.sessionStatus === 'pushFailed' || worktree?.sessionStatus === 'conflict'
        ? 'alert'
        : 'history'
      : props.activeAction
        ? 'sparkles'
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
              <RotatingChevron open={menuOpen} direction="down-up" />
            </button>
            <MotionPresence>
              {menuOpen && (
                <Popover
                  key="model-popover"
                  origin="bottom"
                  title={t.footer.selectModelTitle}
                  onClose={() => setMenuOpen(false)}
                  className="model-menu"
                >
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
            </MotionPresence>
          </>
        )}
      </div>
      <div className="menu-anchor action-anchor">
        <div className={`primary-button-group ${showSplitToggle ? 'is-split' : ''} ${action.resume ? `resume-button-group resume-${worktree?.sessionStatus ?? 'running'}` : ''}`}>
          <button
            type="button"
            className="primary action-split-main"
            disabled={action.disabled || (props.busy && !action.resume && !action.configureModel)}
            title={action.label}
            onClick={action.configureModel ? props.onConfigureModel : props.onPrimary}
          >
            <Icon name={primaryIcon} />
            <span className="truncate">{action.label}</span>
          </button>
          {showSplitToggle && (
            <button
              type="button"
              className="primary action-split-toggle"
              data-popover-trigger
              aria-label={t.footer.switchActionAriaLabel}
              aria-expanded={actionMenuOpen}
              disabled={props.busy}
              onClick={() => setActionMenuOpen((prev) => !prev)}
            >
              <RotatingChevron open={actionMenuOpen} direction="down-up" />
            </button>
          )}
        </div>
        <MotionPresence>
          {actionMenuOpen && showSplitToggle && (
            <Popover
              key="action-switcher-popover"
              origin="bottom"
              title={t.footer.switchActionTitle}
              onClose={() => setActionMenuOpen(false)}
              className="action-switcher-menu"
            >
              <button
                type="button"
                aria-pressed={!props.activeAction}
                onClick={() => {
                  props.onSelectAction(null);
                  setActionMenuOpen(false);
                }}
              >
                <span className="model-menu-label">
                  <span className="model-menu-name truncate">{t.primaryActions.cPush}</span>
                  <span className="muted small truncate">{t.footer.defaultCPushSubtitle}</span>
                </span>
                {!props.activeAction && <Icon name="check" className="success" />}
              </button>
              {props.customActions.map((item) => (
                <button
                  type="button"
                  key={item.id}
                  aria-pressed={props.activeAction?.id === item.id}
                  onClick={() => {
                    props.onSelectAction(item.id);
                    setActionMenuOpen(false);
                  }}
                >
                  <span className="model-menu-label">
                    <span className="model-menu-name truncate">{item.label}</span>
                    <span className="muted small mono truncate">{item.prompt}</span>
                  </span>
                  {props.activeAction?.id === item.id && <Icon name="check" className="success" />}
                </button>
              ))}
              <div className="menu-divider">
                <button
                  type="button"
                  onClick={() => {
                    setActionMenuOpen(false);
                    props.onNewAction('create');
                  }}
                >
                  <Icon name="sparkles" />
                  {t.footer.createWithPiMenuItem}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setActionMenuOpen(false);
                    props.onNewAction('manual');
                  }}
                >
                  <Icon name="plus" />
                  {t.footer.newActionMenuItem}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setActionMenuOpen(false);
                    props.onManageActions();
                  }}
                >
                  <Icon name="settings" />
                  {t.footer.manageActionsMenuItem}
                </button>
              </div>
            </Popover>
          )}
        </MotionPresence>
      </div>
    </footer>
  );
}
