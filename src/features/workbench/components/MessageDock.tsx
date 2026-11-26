import { useEffect, useState } from 'react';
import type {
  AskUserQuestionAnswer,
  AskUserQuestionItem,
  AskUserQuestionResponse,
} from '../../../../shared/ask-user-protocol';
import { useI18n } from '../../../i18n';
import { Icon, IconButton } from '../../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, MotionPresence, m, useInstantMotion } from '../../../ui/motion';
import { orderedMessages } from '../state/workbench';
import type { Message, Worktree } from '../state/workbench';

interface Props {
  worktree: Worktree;
  busy: boolean;
  editorShort: string;
  onExpanded(expanded: boolean): void;
  onDismiss?(message: Message): void;
  onReply(messageId: string, text: string): void;
  onRespond(
    message: Message,
    answer: {
      optionId?: string;
      text?: string;
      questionnaireResponse?: AskUserQuestionResponse;
    },
  ): void;
  onOpenFile(fileId: string): void;
  onReview(): void;
  onCancelCustomAction?(): void;
}

export function MessageDock(props: Props) {
  const { t } = useI18n();
  const instant = useInstantMotion();
  const [primary, ...secondary] = orderedMessages(props.worktree.messages);
  const text = primary ? (props.worktree.replies[primary.id] ?? '') : '';
  const questionnaire = primary?.questionnaire;
  const isQuestionnaire = !!questionnaire && questionnaire.questions.length > 0;
  const canReply = primary
    ? !isQuestionnaire && (primary.kind === 'question' || primary.kind === 'conflict')
    : false;

  const [activeQuestionIdx, setActiveQuestionIdx] = useState(0);
  const [selectedByQuestion, setSelectedByQuestion] = useState<Record<number, string[]>>({});
  const [customByQuestion, setCustomByQuestion] = useState<Record<number, string>>({});
  const [expandedPreviewKey, setExpandedPreviewKey] = useState<string | null>(null);
  const [dontAskAgain, setDontAskAgain] = useState(false);

  const primaryId = primary?.id ?? '';
  useEffect(() => {
    setActiveQuestionIdx(0);
    setSelectedByQuestion({});
    setCustomByQuestion({});
    setExpandedPreviewKey(null);
    setDontAskAgain(false);
  }, [primaryId]);

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
  ): AskUserQuestionResponse => {
    const questions = questionnaire?.questions ?? [];
    const selMap = overrideSelections ?? selectedByQuestion;
    const customMap = overrideCustoms ?? customByQuestion;
    const answers: AskUserQuestionAnswer[] = [];
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
    const unanswered = questionnaire.questions.map((_, index) => index);
    props.onRespond(primary, {
      questionnaireResponse: {
        cancelled: true,
        answers: [],
        unanswered,
      },
    });
  };

  const currentQuestion: AskUserQuestionItem | undefined = questionnaire
    ? questionnaire.questions[Math.min(activeQuestionIdx, questionnaire.questions.length - 1)]
    : undefined;
  const isSingleQuickSelect = !!questionnaire
    && questionnaire.questions.length === 1
    && !questionnaire.questions[0].multiSelect;
  const currentSelectedIds = selectedByQuestion[activeQuestionIdx] ?? [];
  const currentCustomText = customByQuestion[activeQuestionIdx] ?? '';
  const hasAnyQuestionnaireInput = !!questionnaire && questionnaire.questions.some((_, idx) => (
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
              aria-label={t.dock.expandMessage(primary.title)}
            >
              <span className="status-dot" />
              <span className="message-status">{t.messageLabels[primary.kind]}</span>
              <span className="truncate">{primary.title}{primary.scope ? ` · ${primary.scope}` : ''}</span>
              {!!secondary.length && <span className="dynamic-count">+{secondary.length}</span>}
              <Icon name="up" />
            </button>
            {(primary.kind === 'done' || primary.kind === 'error') && (
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
      <MotionPresence>
        {primary && props.worktree.dockExpanded && (
          <m.div
            key="dock-expanded"
            className={`message-expanded message-${primary.kind}`}
            role="region"
            aria-label={t.dock.activeRegionLabel}
            initial={instant ? false : { opacity: 0, y: 18 }}
            animate={{ opacity: 1, y: 0 }}
            exit={instant ? undefined : { opacity: 0, y: 14, transition: { duration: 0.13, ease: MOTION_EASE } }}
            transition={instant ? { duration: 0 } : { duration: MOTION_DURATION.overlay, ease: MOTION_EASE }}
          >
            <div className="message-heading">
              <div className="row min-width-zero">
                <span className="status-dot" />
                <span className="message-status">{t.messageLabels[primary.kind]}</span>
                {(primary.scope ?? questionnaire?.context?.scope) && (
                  <span className="mono muted small truncate">
                    · {primary.scope ?? questionnaire?.context?.scope}
                  </span>
                )}
              </div>
              <IconButton icon="down" label={t.dock.collapseMessage} onClick={() => props.onExpanded(false)} />
            </div>
            <div className="message-body scroll-area">
              {isQuestionnaire && questionnaire && currentQuestion ? (
                <>
                  {questionnaire.questions.length > 1 && (
                    <div className="questionnaire-tabs" role="tablist">
                      {questionnaire.questions.map((item, idx) => {
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
                  <h2>{currentQuestion.question}</h2>
                  {currentQuestion.multiSelect && (
                    <p className="muted small questionnaire-hint">{t.dock.multiSelectHint}</p>
                  )}
                </>
              ) : (
                <>
                  <h2>{primary.title}</h2>
                  <p className="muted">{primary.description}</p>
                </>
              )}
              {!!primary.files?.length && (
                <div className="file-chips">
                  {primary.files.map((file) => (
                    <button type="button" key={file.id} onClick={() => props.onOpenFile(file.id)}>
                      <span className="mono truncate">{file.label}</span>
                      {file.badge && <span className="warning small">{file.badge}</span>}
                      <Icon name="external" />
                    </button>
                  ))}
                </div>
              )}
              {!!effectiveLinks?.length && (
                <div className="file-chips">
                  {effectiveLinks.map((link) => (
                    <a
                      key={`${link.label}:${link.url}`}
                      className="message-link-chip"
                      href={link.url}
                      target="_blank"
                      rel="noreferrer"
                    >
                      <span className="truncate">{link.label}</span>
                      <Icon name="external" />
                    </a>
                  ))}
                </div>
              )}
              {!!primary.commits?.length && (
                <div className="message-commits">
                  {primary.commits.map((commit) => (
                    <div className="row mono" key={commit.hash}>
                      <Icon name="commit" />
                      <span className="accent">{commit.hash}</span>
                      <span className="truncate" title={commit.title}>{commit.title}</span>
                    </div>
                  ))}
                </div>
              )}
              {primary.progress && (
                <div className="message-progress">
                  <p className="small muted">{primary.progress.label}</p>
                  <progress aria-label={t.dock.taskProgressLabel} max={primary.progress.total} value={primary.progress.completed} />
                </div>
              )}
              {!!(props.worktree.cPush?.timeline.length || props.worktree.customAction?.timeline.length) && !isQuestionnaire && (
                <div className="session-timeline" aria-label={t.dock.timelineLabel}>
                  {(props.worktree.cPush?.timeline ?? props.worktree.customAction?.timeline ?? []).map((step, index) => (
                    <div className={`timeline-step timeline-${step.state}`} key={`${index}:${step.label}`}>
                      <span className="status-dot" />
                      <span className="truncate">{step.label}</span>
                    </div>
                  ))}
                </div>
              )}
              {!!props.worktree.cPush?.qaLog.length && !isQuestionnaire && (
                <div className="session-qa-log" aria-label={t.dock.qaLogLabel}>
                  {props.worktree.cPush.qaLog.map((qa) => (
                    <div className="qa-entry small muted" key={qa.questionId}>
                      <span className="truncate">{t.dock.qaEntry(qa.question, qa.answer)}</span>
                    </div>
                  ))}
                </div>
              )}
              {secondary.map((message) => (
                <div className="secondary-message" key={message.id}>
                  <Icon name={message.kind === 'review' ? 'shield' : 'history'} />
                  <span className="truncate">{message.title}</span>
                  {message.kind === 'review' && <button type="button" onClick={props.onReview}>{t.dock.viewReview}</button>}
                </div>
              ))}
            </div>
            <div className="message-actions scroll-area">
              {isQuestionnaire && questionnaire && currentQuestion ? (
                <>
                  <div className="option-card-list">
                    {currentQuestion.options.map((option) => {
                      const isSelected = currentSelectedIds.includes(option.id);
                      const previewKey = `${currentQuestion.id}:${option.id}`;
                      const previewOpen = expandedPreviewKey === previewKey;
                      return (
                        <div className={`option-card-shell ${isSelected ? 'is-selected' : ''}`} key={option.id}>
                          <div className="option-card-row">
                            <button
                              type="button"
                              data-option-id={option.id}
                              aria-pressed={!isSingleQuickSelect ? isSelected : undefined}
                              className={`option-card ${option.recommended ? 'is-recommended' : ''} ${isSelected ? 'is-selected' : ''}`}
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
                              <span className="option-card-content">
                                <span className="option-card-title-row">
                                  <span className="option-card-label">{option.label}</span>
                                  {option.recommended && (
                                    <span className="option-recommended-badge">{t.dock.recommendedBadge}</span>
                                  )}
                                </span>
                                <span className="option-card-desc muted">{option.description}</span>
                              </span>
                            </button>
                            {option.preview && (
                              <button
                                type="button"
                                className="option-preview-toggle"
                                aria-expanded={previewOpen}
                                onClick={() => setExpandedPreviewKey(previewOpen ? null : previewKey)}
                              >
                                {previewOpen ? t.dock.hidePreview : t.dock.showPreview}
                              </button>
                            )}
                          </div>
                          {option.preview && previewOpen && (
                            <pre className="option-preview-block mono scroll-area">{option.preview}</pre>
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
                    className="message-reply"
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
                      <IconButton
                        icon="send"
                        label={t.dock.sendReply}
                        type="submit"
                        disabled={!currentCustomText.trim() || props.busy}
                      />
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
                  {primary.options?.map((option) => (
                    <button
                      type="button"
                      data-option-id={option.id}
                      className={option.description
                        ? `option-card ${option.recommended ? 'is-recommended' : ''}`
                        : option.recommended
                          ? 'primary'
                          : 'secondary'}
                      disabled={props.busy}
                      key={option.id}
                      onClick={() => props.onRespond(primary, { optionId: option.id })}
                    >
                      {option.description ? (
                        <span className="option-card-content">
                          <span className="option-card-title-row">
                            <span className="option-card-label">{option.label}</span>
                            {option.recommended && (
                              <span className="option-recommended-badge">{t.dock.recommendedBadge}</span>
                            )}
                          </span>
                          <span className="option-card-desc muted">{option.description}</span>
                        </span>
                      ) : option.label}
                    </button>
                  ))}
                  {primary.kind === 'waitingManual' && (
                    <button
                      type="button"
                      className="primary"
                      disabled={!primary.files?.length}
                      onClick={() => { const file = primary.files?.[0]; if (file) props.onOpenFile(file.id); }}
                    >
                      <Icon name="external" />{t.dock.openConflictInEditor(props.editorShort)}
                    </button>
                  )}
                  {primary.kind === 'progress' && props.worktree.customAction && props.onCancelCustomAction && (
                    <button type="button" className="secondary" onClick={props.onCancelCustomAction}>
                      {t.dock.cancelAction}
                    </button>
                  )}
                  {(primary.kind === 'done' || primary.kind === 'error') && (
                    <button type="button" className="secondary" onClick={dismissDone}>{t.dock.gotIt}</button>
                  )}
                  {canReply && (
                    <form className="message-reply" onSubmit={(event) => { event.preventDefault(); if (text.trim() && !props.busy) props.onRespond(primary, { text: text.trim() }); }}>
                      <input
                        aria-label={t.dock.replyLabel}
                        value={text}
                        onChange={(event) => props.onReply(primary.id, event.target.value)}
                        placeholder={primary.kind === 'conflict' ? t.dock.conflictReplyPlaceholder : t.dock.questionReplyPlaceholder}
                      />
                      <IconButton icon="send" label={t.dock.sendReply} type="submit" disabled={!text.trim() || props.busy} />
                    </form>
                  )}
                </>
              )}
            </div>
          </m.div>
        )}
      </MotionPresence>
    </>
  );
}
