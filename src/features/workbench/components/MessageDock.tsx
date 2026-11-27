import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import type { MessageTone, MessageUserResponse } from '../../../../shared/message-user-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, type IconName } from '../../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, MotionPresence, m, useInstantMotion } from '../../../ui/motion';
import { orderedMessages } from '../state/workbench';
import type { Message, Worktree } from '../state/workbench';
import { MessageDockBody } from './MessageDockBody';
import { MessageDockQuestionnaire } from './MessageDockQuestionnaire';

interface Props {
  worktree: Worktree;
  busy: boolean;
  editorShort: string;
  fullHeight: boolean;
  resizing?: boolean;
  dockHeightPx?: number | null;
  onResizeStart?(event: ReactPointerEvent<HTMLDivElement>): void;
  onResizeDoubleClick?(): void;
  onResizeKeyDown?(event: ReactKeyboardEvent<HTMLDivElement>): void;
  onExpanded(expanded: boolean): void;
  onDismiss?(message: Message): void;
  onReply(messageId: string, text: string): void;
  onRespond(
    message: Message,
    answer: {
      optionId?: string;
      text?: string;
      questionnaireResponse?: MessageUserResponse;
    },
  ): void | Promise<boolean | void>;
  onOpenFile(fileId: string): void;
  onOpenRepoPath(path: string, line?: number): void;
  onCancelCustomAction?(): void;
  onFollowUpCustomAction?(message: Message, text: string): void | boolean | Promise<boolean | void>;
}

type PendingDockAction =
  | { kind: 'option'; optionId: string }
  | { kind: 'reply' }
  | { kind: 'cancelAction' }
  | null;

function resolveSheetTone(message: Message): MessageTone {
  if (message.card?.tone) return message.card.tone;
  switch (message.kind) {
    case 'done':
      return 'success';
    case 'conflict':
    case 'pushFailed':
    case 'error':
      return 'error';
    case 'question':
    case 'waitingManual':
    case 'resolved':
      return 'warning';
    default:
      return 'info';
  }
}

function resolveToneIcon(message: Message, tone: MessageTone): { icon: IconName; spin?: boolean } {
  if (message.kind === 'progress') return { icon: 'loader', spin: true };
  if (tone === 'success') return { icon: 'checkCircle' };
  if (tone === 'error') return { icon: 'xCircle' };
  if (tone === 'warning') return { icon: message.kind === 'question' ? 'messageSquare' : 'alertCircle' };
  return { icon: 'sparkles' };
}

export function MessageDock(props: Props) {
  const { t } = useI18n();
  const instant = useInstantMotion();
  const [primary, ...secondary] = orderedMessages(props.worktree.messages);
  const text = primary ? (props.worktree.replies[primary.id] ?? '') : '';
  const questionnaire = primary?.questionnaire;
  const questions = questionnaire?.questions ?? [];
  const isQuestionnaire = questions.length > 0;
  const canReply = primary
    ? !isQuestionnaire && (primary.kind === 'question' || primary.kind === 'conflict')
    : false;
  const canFollowUpAction = primary
    ? !isQuestionnaire
      && Boolean(props.worktree.customAction)
      && Boolean(props.onFollowUpCustomAction)
      && (primary.kind === 'done' || (primary.kind === 'error' && !primary.options?.length))
    : false;
  const canDismiss = Boolean(
    primary
    && !isQuestionnaire
    && (primary.kind === 'done' || (primary.kind === 'error' && !primary.options?.length)),
  );

  const [detailsOpen, setDetailsOpen] = useState(false);
  const [pendingAction, setPendingAction] = useState<PendingDockAction>(null);
  const submittingRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);

  const primaryId = primary?.id ?? '';
  const primaryKind = primary?.kind;
  const activePrimaryIdRef = useRef(primaryId);
  activePrimaryIdRef.current = primaryId;
  const activeTimeline = props.worktree.customAction?.timeline ?? props.worktree.cPush?.timeline ?? [];
  const lastTimelineStep = activeTimeline[activeTimeline.length - 1];

  useEffect(() => {
    setDetailsOpen(false);
    setPendingAction(null);
    submittingRef.current = false;
    stickToBottomRef.current = primaryKind === 'progress';
  }, [primaryId, primaryKind]);

  const dockActionLocked = props.busy || pendingAction !== null;

  const dispatchDockAction = async (
    action: Exclude<PendingDockAction, null>,
    run: () => void | boolean | Promise<boolean | void>,
  ) => {
    if (dockActionLocked || submittingRef.current) return;
    const capturedId = primaryId;
    submittingRef.current = true;
    setPendingAction(action);
    try {
      const outcome = await run();
      if (activePrimaryIdRef.current !== capturedId) return;
      if (outcome !== true) {
        submittingRef.current = false;
        setPendingAction(null);
      }
    } catch {
      if (activePrimaryIdRef.current === capturedId) {
        submittingRef.current = false;
        setPendingAction(null);
      }
    }
  };

  useEffect(() => {
    if (!props.worktree.dockExpanded || primaryKind !== 'progress' || !stickToBottomRef.current) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [
    primaryId,
    primaryKind,
    props.worktree.dockExpanded,
    activeTimeline.length,
    lastTimelineStep?.label,
    lastTimelineStep?.state,
  ]);

  const dismissDone = () => {
    if (!primary) return;
    if (props.onDismiss) {
      props.onDismiss(primary);
    } else {
      props.onExpanded(false);
    }
  };

  const firstQuestion = questions[0];
  const effectiveLinks = primary?.links ?? questionnaire?.context?.links;
  const displayTitle = primary
    ? (primary.card?.title || primary.title || firstQuestion?.header || t.messageLabels[primary.kind])
    : '';
  const displaySubtitle = primary
    ? (primary.card?.subtitle || primary.scope || questionnaire?.context?.scope || t.messageLabels[primary.kind])
    : '';
  const hasCardBlocks = Boolean(primary?.card?.blocks && primary.card.blocks.length > 0);
  const sheetTone = primary ? resolveSheetTone(primary) : 'info';
  const toneIcon = primary ? resolveToneIcon(primary, sheetTone) : { icon: 'sparkles' as IconName };
  const timeline = props.worktree.cPush?.timeline ?? props.worktree.customAction?.timeline ?? [];
  const qaLog = props.worktree.cPush?.qaLog ?? [];
  const hasTraceDetails = !isQuestionnaire && (timeline.length > 0 || qaLog.length > 0);
  const collapseTraceByDefault = Boolean(hasCardBlocks && primary?.kind !== 'progress');

  const hasBottomTray = Boolean(
    primary
    && (
      isQuestionnaire
      || Boolean(primary.options?.length)
      || primary.kind === 'waitingManual'
      || (primary.kind === 'progress' && (props.worktree.customAction || props.worktree.cPush) && props.onCancelCustomAction)
      || canReply
      || canFollowUpAction
    ),
  );

  return (
    <>
      <MotionPresence initial={false}>
        {primary && !props.worktree.dockExpanded && (
          <m.div
            key={`dock-collapsed:${primary.id}`}
            className={`message-collapsed-wrap message-${primary.kind}`}
            initial={instant ? false : { opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={instant ? undefined : { opacity: 0, transition: { duration: 0.08 } }}
            transition={instant ? { duration: 0 } : { duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
          >
            <button
              type="button"
              className={`message-collapsed message-${primary.kind}`}
              onClick={() => props.onExpanded(true)}
              aria-label={t.dock.expandMessage(displayTitle)}
            >
              <span className="status-dot" />
              <span className="message-status">{t.messageLabels[primary.kind]}</span>
              <span className="truncate">{displayTitle}{primary.scope ? ` · ${primary.scope}` : ''}</span>
              {!!secondary.length && <span className="dynamic-count">+{secondary.length}</span>}
              <Icon name="up" />
            </button>
            {canDismiss && (
              <IconButton
                icon="close"
                className="message-collapsed-dismiss"
                label={t.dock.dismissDone}
                onClick={dismissDone}
              />
            )}
          </m.div>
        )}
      </MotionPresence>
      <MotionPresence initial={false}>
        {primary && props.worktree.dockExpanded && (
          <m.div
            key="dock-expanded"
            className={`d2-wrap ${props.fullHeight ? 'is-full-height' : ''}`}
            initial={instant ? false : { opacity: 0, y: 18 }}
            animate={{ opacity: 1, y: 0 }}
            exit={instant ? undefined : { opacity: 0, y: 14, transition: { duration: 0.13, ease: MOTION_EASE } }}
            transition={instant ? { duration: 0 } : { duration: MOTION_DURATION.overlay, ease: MOTION_EASE }}
          >
            <section
              className={`d2-sheet message-expanded message-${primary.kind} tone-${sheetTone} ${props.resizing ? 'is-resizing' : ''}`}
              role="region"
              aria-label={t.dock.activeRegionLabel}
            >
              <div
                role="separator"
                aria-orientation="horizontal"
                aria-label={t.dock.resizeDockLabel}
                aria-valuenow={props.dockHeightPx ?? undefined}
                title={t.dock.resizeDockTitle}
                tabIndex={0}
                className={`d2-grab-zone ${props.resizing ? 'is-resizing' : ''}`}
                onPointerDown={props.onResizeStart}
                onDoubleClick={props.onResizeDoubleClick}
                onKeyDown={props.onResizeKeyDown}
              >
                <span className="d2-grab" aria-hidden="true" />
              </div>

              <header className="d2-header">
                <div className={`d2-squircle tone-${sheetTone}`} aria-hidden="true">
                  <Icon name={toneIcon.icon} className={toneIcon.spin ? 'spinning' : ''} />
                </div>
                <div className="d2-header-copy min-width-zero">
                  <h2 className="d2-title">{displayTitle}</h2>
                  {displaySubtitle && <div className="d2-subtitle truncate">{displaySubtitle}</div>}
                </div>
                <div className="d2-header-tools">
                  <button
                    type="button"
                    className="d2-ibtn"
                    title={t.dock.collapseMessage}
                    aria-label={t.dock.collapseMessage}
                    onClick={() => props.onExpanded(false)}
                  >
                    <Icon name="down" />
                  </button>
                  {canDismiss && (
                    <button
                      type="button"
                      className="d2-ibtn"
                      title={t.dock.dismissDone}
                      aria-label={t.dock.dismissDone}
                      onClick={dismissDone}
                    >
                      <Icon name="close" />
                    </button>
                  )}
                </div>
              </header>

              {primary.progress && (
                <div className="d2-progress-pinned message-progress">
                  <div className="d2-progress-head">
                    <p className="d2-progress-label truncate" title={primary.progress.label}>
                      {primary.progress.label}
                    </p>
                    {timeline.length >= 2 && (
                      <span className="d2-progress-step mono">
                        {timeline.filter((step) => step.state === 'done').length}/{timeline.length}
                      </span>
                    )}
                  </div>
                  <progress
                    aria-label={t.dock.taskProgressLabel}
                    max={primary.progress.total}
                    value={primary.progress.completed}
                  />
                </div>
              )}

              <MessageDockBody
                primary={primary}
                secondary={secondary}
                scrollRef={scrollRef}
                editorShort={props.editorShort}
                hasCardBlocks={hasCardBlocks}
                isQuestionnaire={isQuestionnaire}
                currentQuestionText={firstQuestion?.question}
                effectiveLinks={effectiveLinks}
                timeline={timeline}
                qaLog={qaLog}
                hasTraceDetails={hasTraceDetails}
                collapseTraceByDefault={collapseTraceByDefault}
                detailsOpen={detailsOpen}
                onToggleDetails={() => setDetailsOpen((prev) => !prev)}
                onScrollStickChange={(atBottom) => { stickToBottomRef.current = atBottom; }}
                onOpenFile={props.onOpenFile}
                onOpenRepoPath={props.onOpenRepoPath}
              />

              {hasBottomTray && (
                <div className="d2-bottom scroll-area">
                  {isQuestionnaire && questionnaire ? (
                    <MessageDockQuestionnaire
                      messageId={primary.id}
                      questionnaire={questionnaire}
                      questions={questions}
                      busy={props.busy}
                      onSubmit={(questionnaireResponse) => props.onRespond(primary, { questionnaireResponse })}
                    />
                  ) : (
                    <>
                      {primary.options && primary.options.length > 0 && (
                        <div className="d2-option-list">
                          {primary.options.map((option) => {
                            const isOptionLoading = pendingAction?.kind === 'option' && pendingAction.optionId === option.id;
                            return (
                              <button
                                type="button"
                                key={option.id}
                                data-option-id={option.id}
                                aria-busy={isOptionLoading || undefined}
                                className={`d2-action-btn ${option.recommended ? 'is-primary' : ''} ${isOptionLoading ? 'is-loading' : ''}`}
                                disabled={dockActionLocked}
                                onClick={() => void dispatchDockAction(
                                  { kind: 'option', optionId: option.id },
                                  () => props.onRespond(primary, { optionId: option.id }),
                                )}
                              >
                                <span className="d2-opt-copy">
                                  <span className="d2-opt-label">{option.label}</span>
                                  {option.description && <span className="d2-opt-desc">{option.description}</span>}
                                </span>
                                {isOptionLoading && (
                                  <span className="d2-opt-spinner" aria-hidden="true">
                                    <Icon name="loader" className="spinning" />
                                  </span>
                                )}
                              </button>
                            );
                          })}
                        </div>
                      )}

                      {primary.kind === 'waitingManual' && (
                        <button
                          type="button"
                          className="d2-action-btn is-primary d2-centered-btn"
                          disabled={!primary.files?.length}
                          onClick={() => { const file = primary.files?.[0]; if (file) props.onOpenFile(file.id); }}
                        >
                          <Icon name="external" />
                          <span>{t.dock.openConflictInEditor(props.editorShort)}</span>
                        </button>
                      )}

                      {primary.kind === 'progress' && (props.worktree.customAction || props.worktree.cPush) && props.onCancelCustomAction && (
                        <button
                          type="button"
                          className={`d2-action-btn d2-centered-btn ${pendingAction?.kind === 'cancelAction' ? 'is-loading' : ''}`}
                          aria-busy={pendingAction?.kind === 'cancelAction' || undefined}
                          disabled={pendingAction?.kind === 'cancelAction'}
                          onClick={() => {
                            if (submittingRef.current) return;
                            submittingRef.current = true;
                            setPendingAction({ kind: 'cancelAction' });
                            props.onCancelCustomAction?.();
                          }}
                        >
                          {pendingAction?.kind === 'cancelAction' && (
                            <Icon name="loader" className="spinning d2-btn-spinner" />
                          )}
                          <span>{t.dock.cancelAction}</span>
                        </button>
                      )}

                      {(canReply || canFollowUpAction) && (
                        <form
                          className="d2-composer"
                          onSubmit={(event) => {
                            event.preventDefault();
                            if (!text.trim() || dockActionLocked) return;
                            const trimmed = text.trim();
                            void dispatchDockAction({ kind: 'reply' }, () => (
                              canFollowUpAction
                                ? props.onFollowUpCustomAction?.(primary, trimmed)
                                : props.onRespond(primary, { text: trimmed })
                            ));
                          }}
                        >
                          <input
                            aria-label={t.dock.replyLabel}
                            value={text}
                            disabled={dockActionLocked}
                            onChange={(event) => props.onReply(primary.id, event.target.value)}
                            placeholder={
                              canFollowUpAction
                                ? t.dock.followUpActionPlaceholder
                                : primary.kind === 'conflict'
                                  ? t.dock.conflictReplyPlaceholder
                                  : t.dock.questionReplyPlaceholder
                            }
                          />
                          <button
                            type="submit"
                            className="d2-send-btn"
                            aria-label={t.dock.sendReply}
                            aria-busy={pendingAction?.kind === 'reply' || undefined}
                            title={t.dock.sendReply}
                            disabled={!text.trim() || dockActionLocked}
                          >
                            <Icon
                              name={pendingAction?.kind === 'reply' ? 'loader' : 'sendUp'}
                              className={pendingAction?.kind === 'reply' ? 'spinning' : ''}
                            />
                          </button>
                        </form>
                      )}
                    </>
                  )}
                </div>
              )}
            </section>
          </m.div>
        )}
      </MotionPresence>
    </>
  );
}
