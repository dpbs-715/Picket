import { useLayoutEffect, useRef, useState, type RefObject } from 'react';
import {
  isCustomActionWriteTrusted,
  type CustomActionConfig,
} from '../../../../shared/custom-action-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, RotatingChevron } from '../../../ui/controls';
import { Collapse } from '../../../ui/motion';
import type { Worktree } from '../state/workbench';
import type { WorkbenchDispatch } from '../types';

interface Props {
  worktree: Worktree | undefined;
  viewKey: string;
  promptText: string;
  showPromptBox: boolean;
  activeCustomAction: CustomActionConfig | undefined;
  inert?: boolean;
  stripRef: RefObject<HTMLDivElement | null>;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  onClosePrompt: () => void;
  onEditAction: (action: CustomActionConfig) => void;
  dispatch: WorkbenchDispatch;
}

export function ActionContextStrip({
  worktree,
  viewKey,
  promptText,
  showPromptBox,
  activeCustomAction,
  inert = false,
  stripRef,
  textareaRef,
  onClosePrompt,
  onEditAction,
  dispatch,
}: Props) {
  const { t } = useI18n();
  const [actionPreviewOpen, setActionPreviewOpen] = useState(false);
  const prevViewKeyRef = useRef(viewKey);
  const prevActionIdRef = useRef(activeCustomAction?.id);

  const trustedAction = activeCustomAction ? isCustomActionWriteTrusted(activeCustomAction) : false;

  useLayoutEffect(() => {
    if (prevViewKeyRef.current !== viewKey) {
      prevViewKeyRef.current = viewKey;
      setActionPreviewOpen(false);
    }
  }, [viewKey]);

  useLayoutEffect(() => {
    if (prevActionIdRef.current !== activeCustomAction?.id) {
      prevActionIdRef.current = activeCustomAction?.id;
      setActionPreviewOpen(false);
    }
  }, [activeCustomAction?.id]);

  return (
    <div className="action-context-strip" ref={stripRef} inert={inert}>
      {activeCustomAction && (
        <>
          <div className="action-context-bar">
            <button
              type="button"
              className={`action-context-pill min-width-zero ${actionPreviewOpen ? 'is-open' : ''}`}
              aria-expanded={actionPreviewOpen}
              aria-label={actionPreviewOpen ? t.actions.collapseActionPreview : t.actions.expandActionPreview}
              onClick={() => setActionPreviewOpen((prev) => !prev)}
            >
              <Icon name="sparkles" className="accent" />
              <span className="action-context-name truncate">{activeCustomAction.label}</span>
              <span className="action-context-summary muted truncate">{activeCustomAction.prompt.replace(/\s+/g, ' ')}</span>
              {trustedAction && <span className="action-meta-badge is-amber">{t.actions.autoApproveBadge}</span>}
              <RotatingChevron open={actionPreviewOpen} direction="down-up" className="action-context-chevron" />
            </button>
          </div>

          <Collapse key={`action-preview:${activeCustomAction.id}`} open={actionPreviewOpen}>
            <div className="action-context-drawer">
              <div className="action-context-drawer-head">
                <div className="custom-action-preview-meta">
                  <span className="action-meta-badge is-accent">{t.actions.customActionBadge}</span>
                  {trustedAction && <span className="action-meta-badge is-amber">{t.actions.autoApproveBadge}</span>}
                </div>
                <div className="custom-action-preview-actions">
                  <button
                    type="button"
                    className="secondary compact"
                    onClick={() => onEditAction(activeCustomAction)}
                  >
                    <Icon name="pencil" />
                    {t.actions.previewEditAction}
                  </button>
                  {worktree && (
                    <button
                      type="button"
                      className="secondary compact"
                      onClick={() => dispatch({ type: 'selectActiveAction', repositoryId: worktree.repositoryId, actionId: null })}
                    >
                      {t.actions.switchBackCPush}
                    </button>
                  )}
                </div>
              </div>
              <pre className="custom-action-prompt-preview mono scroll-area">{activeCustomAction.prompt}</pre>
            </div>
          </Collapse>
        </>
      )}

      <Collapse key={`prompt:${viewKey}`} open={showPromptBox}>
        <div className={`prompt-box-wrap ${activeCustomAction ? 'has-action-above' : ''}`}>
          <div className="prompt-box">
            <textarea
              ref={textareaRef}
              className="prompt-textarea"
              aria-label={t.commit.promptLabel}
              rows={2}
              value={promptText}
              onChange={(event) => dispatch({ type: 'prompt', worktreeId: worktree?.worktreeId ?? null, text: event.target.value })}
              placeholder={t.commit.promptPlaceholder}
              spellCheck={false}
            />
            <IconButton
              icon="close"
              label={promptText ? t.commit.clearPromptLabel : t.commit.hidePromptLabel}
              onClick={() => {
                if (promptText) {
                  dispatch({ type: 'clearPrompt', worktreeId: worktree?.worktreeId ?? null });
                }
                onClosePrompt();
              }}
            />
          </div>
        </div>
      </Collapse>
    </div>
  );
}
