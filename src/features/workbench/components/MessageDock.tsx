import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import type {
  MessageQuestionItem,
  MessageTone,
  MessageUserAnswer,
  MessageUserResponse,
} from '../../../../shared/message-user-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton, type IconName } from '../../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, MotionPresence, m, useInstantMotion } from '../../../ui/motion';
import { orderedMessages } from '../state/workbench';
import type { Message, Worktree } from '../state/workbench';
import { MessageBlocks } from './MessageBlocks';

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
  ): void;
  onOpenFile(fileId: string): void;
  onOpenRepoPath(path: string, line?: number): void;
  onCancelCustomAction?(): void;
  onFollowUpCustomAction?(message: Message, text: string): void;
}

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

  const [activeQuestionIdx, setActiveQuestionIdx] = useState(0);
  const [selectedByQuestion, setSelectedByQuestion] = useState<Record<number, string[]>>({});
  const [customByQuestion, setCustomByQuestion] = useState<Record<number, string>>({});
  const [expandedPreviewKey, setExpandedPreviewKey] = useState<string | null>(null);
  const [dontAskAgain, setDontAskAgain] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);

  const primaryId = primary?.id ?? '';
  const primaryKind = primary?.kind;
  const activeTimeline = props.worktree.customAction?.timeline ?? props.worktree.cPush?.timeline ?? [];
  const lastTimelineStep = activeTimeline[activeTimeline.length - 1];

  useEffect(() => {
    setActiveQuestionIdx(0);
    setSelectedByQuestion({});
    setCustomByQuestion({});
    setExpandedPreviewKey(null);
    setDontAskAgain(false);
    setDetailsOpen(false);
    stickToBottomRef.current = primaryKind === 'progress';
  }, [primaryId, primaryKind]);

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

  const buildQuestionnaireResponse = (
    overrideSelections?: Record<number, string[]>,
    overrideCustoms?: Record<number, string>,
  ): MessageUserResponse => {
    const selMap = overrideSelections ?? selectedByQuestion;
    const customMap = overrideCustoms ?? customByQuestion;
    const answers: MessageUserAnswer[] = [];
    const unanswered: number[] = [];

    questions.forEach((item, index) => {
      const chosenIds = selMap[index] ?? [];
      const customText = (customMap[index] ?? '').trim();
      const chosenLabels = chosenIds
        .map((id) => item.options.find((opt) => opt.id === id)?.label)
        .filter((label): label is string => Boolean(label));
      if (chosenLabels.length === 0 && !customText) {
        unanswered.push(index);
      } else {
        answers.push({
          questionIndex: index,
          questionId: item.id,
          header: item.header,
          selectedOptionIds: chosenIds,
          selectedLabels: chosenLabels,
          ...(customText ? { customText } : {}),
        });
      }
    });

    return {
      cancelled: false,
      answers,
      unanswered,
      ...(dontAskAgain ? { dontAskAgain: true } : {}),
    };
  };

  const submitQuestionnaire = (
    overrideSelections?: Record<number, string[]>,
    overrideCustoms?: Record<number, string>,
  ) => {
    if (!primary || !questionnaire || props.busy) return;
    const response = buildQuestionnaireResponse(overrideSelections, overrideCustoms);
    props.onRespond(primary, { questionnaireResponse: response });
  };

  const cancelQuestionnaire = () => {
    if (!primary || !questionnaire || props.busy) return;
    const unanswered = questions.map((_, index) => index);
    props.onRespond(primary, {
      questionnaireResponse: {
        cancelled: true,
        answers: [],
        unanswered,
      },
    });
  };

  const currentQuestion: MessageQuestionItem | undefined = questions.length > 0
    ? questions[Math.min(activeQuestionIdx, questions.length - 1)]
    : undefined;
  const isSingleQuickSelect = questions.length === 1 && !questions[0].multiSelect;
  const currentSelectedIds = selectedByQuestion[activeQuestionIdx] ?? [];
  const currentCustomText = customByQuestion[activeQuestionIdx] ?? '';
  const hasAnyQuestionnaireInput = questions.some((_, idx) => (
    (selectedByQuestion[idx]?.length ?? 0) > 0 || Boolean((customByQuestion[idx] ?? '').trim())
  ));

  const toggleQuestionnaireOption = (questionIndex: number, optionId: string, multiSelect: boolean) => {
    if (isSingleQuickSelect) {
      const nextSelections = { [questionIndex]: [optionId] };
      submitQuestionnaire(nextSelections, customByQuestion);
      return;
    }
    setSelectedByQuestion((previous) => {
      const existing = previous[questionIndex] ?? [];
      if (multiSelect) {
        const next = existing.includes(optionId)
          ? existing.filter((id) => id !== optionId)
          : [...existing, optionId];
        return { ...previous, [questionIndex]: next };
      }
      return { ...previous, [questionIndex]: [optionId] };
    });
  };

  const effectiveLinks = primary?.links ?? questionnaire?.context?.links;
  const displayTitle = primary
    ? (primary.card?.title || primary.title || currentQuestion?.header || t.messageLabels[primary.kind])
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
      || (primary.kind === 'progress' && props.worktree.customAction && props.onCancelCustomAction)
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

              <div
                ref={scrollRef}
                className="d2-scroll scroll-area"
                onScroll={(event) => {
                  const el = event.currentTarget;
                  stickToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 24;
                }}
              >
                {hasCardBlocks && primary.card?.blocks && (
                  <MessageBlocks
                    blocks={primary.card.blocks}
                    editorShort={props.editorShort}
                    onOpenRepoPath={props.onOpenRepoPath}
                  />
                )}

                {!hasCardBlocks
                  && !isQuestionnaire
                  && primary.description
                  && primary.description.trim() !== (primary.progress?.label ?? '').trim() && (
                  <p className="d2-fallback-desc">{primary.description}</p>
                )}

                {!hasCardBlocks && isQuestionnaire && primary.description && primary.description !== currentQuestion?.question && (
                  <p className="d2-fallback-desc">{primary.description}</p>
                )}

                {!!primary.files?.length && (
                  <div className="d2-pill-row">
                    {primary.files.map((file) => (
                      <button
                        type="button"
                        key={file.id}
                        className="d2-pill"
                        onClick={() => props.onOpenFile(file.id)}
                      >
                        <Icon name="fileCode" />
                        <span className="mono truncate">{file.label}</span>
                        {file.badge && <span className="d2-pill-badge warning">{file.badge}</span>}
                        <Icon name="arrowUpRight" />
                      </button>
                    ))}
                  </div>
                )}

                {!!primary.commits?.length && (
                  <div className="d2-commits">
                    {primary.commits.map((commit) => (
                      <div className="d2-commit-row mono" key={commit.hash}>
                        <Icon name="commit" className="accent" />
                        <span className="accent d2-commit-hash">{commit.hash}</span>
                        <span className="truncate" title={commit.title}>{commit.title}</span>
                      </div>
                    ))}
                  </div>
                )}

                {!!effectiveLinks?.length && (
                  <div className="d2-pill-row">
                    {effectiveLinks.map((link) => (
                      <a
                        key={`${link.label}:${link.url}`}
                        className="d2-pill d2-link-pill"
                        href={link.url}
                        target="_blank"
                        rel="noreferrer"
                      >
                        <span className="truncate">{link.label}</span>
                        <Icon name="arrowUpRight" />
                      </a>
                    ))}
                  </div>
                )}

                {hasTraceDetails && (
                  collapseTraceByDefault ? (
                    <div className="d2-trace-disclosure">
                      <button
                        type="button"
                        className="d2-skipped-toggle"
                        aria-expanded={detailsOpen}
                        onClick={() => setDetailsOpen((prev) => !prev)}
                      >
                        <Icon name={detailsOpen ? 'down' : 'right'} />
                        <span>{t.dock.detailsToggle}</span>
                      </button>
                      {detailsOpen && (
                        <>
                          {timeline.length > 0 && (
                            <div className="session-timeline" aria-label={t.dock.timelineLabel}>
                              {timeline.map((step, index) => (
                                <div className={`timeline-step timeline-${step.state}`} key={`${index}:${step.label}`}>
                                  <span className="status-dot" />
                                  <span className="truncate">{step.label}</span>
                                </div>
                              ))}
                            </div>
                          )}
                          {qaLog.length > 0 && (
                            <div className="session-qa-log" aria-label={t.dock.qaLogLabel}>
                              {qaLog.map((qa) => (
                                <div className="qa-entry small muted" key={qa.questionId}>
                                  <span className="truncate">{t.dock.qaEntry(qa.question, qa.answer)}</span>
                                </div>
                              ))}
                            </div>
                          )}
                        </>
                      )}
                    </div>
                  ) : (
                    <>
                      {timeline.length > 0 && (
                        <div className="session-timeline" aria-label={t.dock.timelineLabel}>
                          {timeline.map((step, index) => (
                            <div className={`timeline-step timeline-${step.state}`} key={`${index}:${step.label}`}>
                              <span className="status-dot" />
                              <span className="truncate">{step.label}</span>
                            </div>
                          ))}
                        </div>
                      )}
                      {qaLog.length > 0 && (
                        <div className="session-qa-log" aria-label={t.dock.qaLogLabel}>
                          {qaLog.map((qa) => (
                            <div className="qa-entry small muted" key={qa.questionId}>
                              <span className="truncate">{t.dock.qaEntry(qa.question, qa.answer)}</span>
                            </div>
                          ))}
                        </div>
                      )}
                    </>
                  )
                )}

                {secondary.map((message) => (
                  <div className="secondary-message" key={message.id}>
                    <Icon name="history" />
                    <span className="truncate">{message.title}</span>
                  </div>
                ))}
              </div>

              {hasBottomTray && (
                <div className="d2-bottom scroll-area">
                  {isQuestionnaire && questionnaire && currentQuestion ? (
                    <>
                      {questions.length > 1 && (
                        <div className="questionnaire-tabs" role="tablist">
                          {questions.map((item, idx) => {
                            const answered = (selectedByQuestion[idx]?.length ?? 0) > 0
                              || Boolean((customByQuestion[idx] ?? '').trim());
                            return (
                              <button
                                type="button"
                                role="tab"
                                key={item.id}
                                aria-selected={idx === activeQuestionIdx}
                                className={`questionnaire-tab ${idx === activeQuestionIdx ? 'is-active' : ''} ${answered ? 'is-answered' : ''}`}
                                onClick={() => setActiveQuestionIdx(idx)}
                              >
                                <span className="questionnaire-tab-dot" aria-hidden="true" />
                                <span className="truncate">{item.header}</span>
                              </button>
                            );
                          })}
                        </div>
                      )}

                      <div className="d2-question-head">
                        <div className="d2-question-prompt">{currentQuestion.question}</div>
                        {currentQuestion.multiSelect && (
                          <span className="muted small">{t.dock.multiSelectHint}</span>
                        )}
                      </div>

                      <div className="d2-option-list">
                        {currentQuestion.options.map((option) => {
                          const isSelected = currentSelectedIds.includes(option.id);
                          const isPrimaryStyle = Boolean(option.recommended && isSingleQuickSelect);
                          const previewKey = `${currentQuestion.id}:${option.id}`;
                          const previewOpen = expandedPreviewKey === previewKey;
                          return (
                            <div
                              key={option.id}
                              className={`d2-opt-shell ${isPrimaryStyle ? 'is-primary' : ''} ${isSelected ? 'is-selected' : ''}`}
                            >
                              <div className="d2-opt-row">
                                <button
                                  type="button"
                                  data-option-id={option.id}
                                  aria-pressed={!isSingleQuickSelect ? isSelected : undefined}
                                  className={`d2-opt-btn ${option.recommended ? 'is-recommended' : ''} ${isSelected ? 'is-selected' : ''}`}
                                  disabled={props.busy}
                                  onClick={() => toggleQuestionnaireOption(
                                    activeQuestionIdx,
                                    option.id,
                                    Boolean(currentQuestion.multiSelect),
                                  )}
                                >
                                  {!isSingleQuickSelect && (
                                    <span
                                      className={`option-indicator ${currentQuestion.multiSelect ? 'is-checkbox' : 'is-radio'} ${isSelected ? 'is-checked' : ''}`}
                                      aria-hidden="true"
                                    >
                                      {isSelected && <Icon name="check" />}
                                    </span>
                                  )}
                                  <span className="d2-opt-copy">
                                    <span className="d2-opt-label-row">
                                      <span className="d2-opt-label">{option.label}</span>
                                      {option.recommended && !isPrimaryStyle && (
                                        <span className="option-recommended-badge">{t.dock.recommendedBadge}</span>
                                      )}
                                    </span>
                                    <span className="d2-opt-desc">{option.description}</span>
                                  </span>
                                </button>
                                {option.preview && (
                                  <button
                                    type="button"
                                    className="d2-preview-btn"
                                    aria-expanded={previewOpen}
                                    onClick={() => setExpandedPreviewKey(previewOpen ? null : previewKey)}
                                  >
                                    {previewOpen ? t.dock.hidePreview : t.dock.showPreview}
                                  </button>
                                )}
                              </div>
                              {option.preview && previewOpen && (
                                <pre className="d2-pre d2-preview-pre mono scroll-area">{option.preview}</pre>
                              )}
                            </div>
                          );
                        })}
                      </div>

                      {questionnaire.context?.allowDontAskAgain && (
                        <label className="dont-ask-toggle">
                          <input
                            type="checkbox"
                            checked={dontAskAgain}
                            disabled={props.busy}
                            onChange={(event) => setDontAskAgain(event.target.checked)}
                          />
                          <span>{questionnaire.context.dontAskAgainLabel ?? t.dock.dontAskAgainDefault}</span>
                        </label>
                      )}

                      <form
                        className="d2-composer"
                        onSubmit={(event) => {
                          event.preventDefault();
                          if (props.busy) return;
                          if (isSingleQuickSelect) {
                            if (currentCustomText.trim()) {
                              submitQuestionnaire({}, { [activeQuestionIdx]: currentCustomText.trim() });
                            }
                          } else if (hasAnyQuestionnaireInput) {
                            submitQuestionnaire();
                          }
                        }}
                      >
                        <input
                          aria-label={t.dock.replyLabel}
                          value={currentCustomText}
                          disabled={props.busy}
                          onChange={(event) => setCustomByQuestion((previous) => ({
                            ...previous,
                            [activeQuestionIdx]: event.target.value,
                          }))}
                          placeholder={t.dock.customOptionPlaceholder}
                        />
                        {isSingleQuickSelect && (
                          <button
                            type="submit"
                            className="d2-send-btn"
                            aria-label={t.dock.sendReply}
                            title={t.dock.sendReply}
                            disabled={!currentCustomText.trim() || props.busy}
                          >
                            <Icon name="sendUp" />
                          </button>
                        )}
                      </form>

                      <div className="questionnaire-footer">
                        <button
                          type="button"
                          className="secondary compact"
                          disabled={props.busy}
                          onClick={cancelQuestionnaire}
                        >
                          {t.dock.cancelQuestion}
                        </button>
                        {!isSingleQuickSelect && (
                          <button
                            type="button"
                            className="primary compact"
                            disabled={!hasAnyQuestionnaireInput || props.busy}
                            onClick={() => submitQuestionnaire()}
                          >
                            {t.dock.submitAnswers}
                          </button>
                        )}
                      </div>
                    </>
                  ) : (
                    <>
                      {primary.options && primary.options.length > 0 && (
                        <div className="d2-option-list">
                          {primary.options.map((option) => (
                            <button
                              type="button"
                              key={option.id}
                              data-option-id={option.id}
                              className={`d2-action-btn ${option.recommended ? 'is-primary' : ''}`}
                              disabled={props.busy}
                              onClick={() => props.onRespond(primary, { optionId: option.id })}
                            >
                              <span className="d2-opt-copy">
                                <span className="d2-opt-label">{option.label}</span>
                                {option.description && <span className="d2-opt-desc">{option.description}</span>}
                              </span>
                            </button>
                          ))}
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

                      {primary.kind === 'progress' && props.worktree.customAction && props.onCancelCustomAction && (
                        <button
                          type="button"
                          className="d2-action-btn d2-centered-btn"
                          onClick={props.onCancelCustomAction}
                        >
                          {t.dock.cancelAction}
                        </button>
                      )}

                      {(canReply || canFollowUpAction) && (
                        <form
                          className="d2-composer"
                          onSubmit={(event) => {
                            event.preventDefault();
                            if (!text.trim() || props.busy) return;
                            if (canFollowUpAction) {
                              props.onFollowUpCustomAction?.(primary, text.trim());
                            } else {
                              props.onRespond(primary, { text: text.trim() });
                            }
                          }}
                        >
                          <input
                            aria-label={t.dock.replyLabel}
                            value={text}
                            disabled={props.busy}
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
                            title={t.dock.sendReply}
                            disabled={!text.trim() || props.busy}
                          >
                            <Icon name="sendUp" />
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
