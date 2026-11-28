import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import {
  CUSTOM_ACTION_LIMITS,
  type GenerateCustomActionProgressEvent,
  type GenerateCustomActionReply,
} from '../../../shared/custom-action-protocol';
import type {
  MessageUserAnswer,
  MessageUserCard,
  MessageUserRequest,
  MessageUserResponse,
} from '../../../shared/message-user-protocol';
import { useI18n } from '../../i18n';
import { Icon } from '../../ui/controls';
import { MessageBlocks } from '../workbench/components/MessageBlocks';

interface PendingGenerateQuestion {
  runId: string;
  callId: string;
  request: MessageUserRequest;
  card?: MessageUserCard;
}

interface CreateWithPiPaneProps {
  hasModel: boolean;
  canGenerateWithPi: boolean;
  nlRequest: string;
  generating: boolean;
  error: string | null;
  cancelRef: MutableRefObject<() => void>;
  onNlRequestChange(value: string): void;
  onGeneratingChange(generating: boolean): void;
  onError(message: string | null): void;
  onDraftReady(draft: { label: string; prompt: string; summary: string }): void;
  onConfigureModel(): void;
  onBackOrClose(): void;
  onGenerateWithPi?(
    requestId: string,
    userRequest: string,
    onProgress: (event: GenerateCustomActionProgressEvent) => void,
  ): Promise<GenerateCustomActionReply>;
  onRespondGenerateQuestion?(
    runId: string,
    callId: string,
    response: MessageUserResponse,
  ): Promise<boolean>;
  onCancelGenerate?(runId: string): void;
}

export function CreateWithPiPane(props: CreateWithPiPaneProps) {
  const { t } = useI18n();
  const [statusLabel, setStatusLabel] = useState('');
  const [pendingQuestion, setPendingQuestion] = useState<PendingGenerateQuestion | null>(null);
  const [liveCard, setLiveCard] = useState<MessageUserCard | null>(null);
  const [activeQuestionIdx, setActiveQuestionIdx] = useState(0);
  const [selectedByQ, setSelectedByQ] = useState<Record<number, string[]>>({});
  const [customByQ, setCustomByQ] = useState<Record<number, string>>({});

  const activeRunIdRef = useRef<string | null>(null);
  const submittingAnswerRef = useRef(false);
  const latestCancelRef = useRef(props.onCancelGenerate);
  latestCancelRef.current = props.onCancelGenerate;

  function handleCancelGenerate() {
    const runId = activeRunIdRef.current;
    if (runId) {
      activeRunIdRef.current = null;
      latestCancelRef.current?.(runId);
    }
    props.onGeneratingChange(false);
    setPendingQuestion(null);
    setLiveCard(null);
    setStatusLabel('');
  }

  props.cancelRef.current = handleCancelGenerate;

  useEffect(() => () => {
    const activeId = activeRunIdRef.current;
    if (activeId) {
      activeRunIdRef.current = null;
      latestCancelRef.current?.(activeId);
    }
  }, []);

  async function handleGenerate() {
    if (!props.onGenerateWithPi || props.generating) return;
    const trimmed = props.nlRequest.trim();
    if (!trimmed) {
      props.onError(t.actions.createPromptRequired);
      return;
    }
    const requestId = crypto.randomUUID();
    activeRunIdRef.current = requestId;
    props.onGeneratingChange(true);
    props.onError(null);
    setPendingQuestion(null);
    setLiveCard(null);
    setStatusLabel(t.actions.createInspectingStatus);

    try {
      const reply = await props.onGenerateWithPi(requestId, trimmed, (event) => {
        if (activeRunIdRef.current !== requestId) return;
        setStatusLabel(event.statusLabel);
        if (event.card) {
          setLiveCard(event.card);
        }
        if (event.status === 'needsDecision' && event.pendingCallId && event.questionnaire) {
          setPendingQuestion({
            runId: event.runId,
            callId: event.pendingCallId,
            request: event.questionnaire,
            ...(event.card ? { card: event.card } : {}),
          });
          setActiveQuestionIdx(0);
          setSelectedByQ({});
          setCustomByQ({});
        } else if (event.status === 'running') {
          setPendingQuestion(null);
        }
      });
      if (activeRunIdRef.current !== requestId) return;
      if (reply.status === 'done' && reply.draft) {
        setPendingQuestion(null);
        setLiveCard(null);
        props.onDraftReady(reply.draft);
      }
    } catch (err) {
      if (activeRunIdRef.current === requestId) {
        props.onError(err instanceof Error ? err.message : t.actions.createFailedFallback);
      }
    } finally {
      if (activeRunIdRef.current === requestId) {
        activeRunIdRef.current = null;
        props.onGeneratingChange(false);
        setPendingQuestion(null);
      }
    }
  }

  async function submitInlineAnswer(
    overrideSelections?: Record<number, string[]>,
    overrideCustoms?: Record<number, string>,
    cancelled = false,
  ) {
    if (submittingAnswerRef.current || !pendingQuestion || !props.onRespondGenerateQuestion) return;
    submittingAnswerRef.current = true;
    const { runId, callId, request } = pendingQuestion;
    const reqQuestions = request.questions ?? [];
    const selMap = overrideSelections ?? selectedByQ;
    const customMap = overrideCustoms ?? customByQ;
    const answers: MessageUserAnswer[] = [];
    const unanswered: number[] = [];

    if (cancelled) {
      reqQuestions.forEach((_, idx) => unanswered.push(idx));
    } else {
      reqQuestions.forEach((q, idx) => {
        const ids = selMap[idx] ?? [];
        const customText = (customMap[idx] ?? '').trim();
        const labels = ids
          .map((id) => q.options.find((opt) => opt.id === id)?.label)
          .filter((item): item is string => Boolean(item));
        if (labels.length === 0 && !customText) {
          unanswered.push(idx);
        } else {
          answers.push({
            questionIndex: idx,
            questionId: q.id,
            header: q.header,
            selectedOptionIds: ids,
            selectedLabels: labels,
            ...(customText ? { customText } : {}),
          });
        }
      });
    }

    setPendingQuestion(null);
    try {
      await props.onRespondGenerateQuestion(runId, callId, {
        cancelled,
        answers,
        unanswered,
      });
    } catch (err) {
      props.onError(err instanceof Error ? err.message : t.actions.createFailedFallback);
    } finally {
      submittingAnswerRef.current = false;
    }
  }

  const pendingQuestions = pendingQuestion?.request.questions ?? [];
  const currentQuestion = pendingQuestions.length > 0
    ? pendingQuestions[Math.min(activeQuestionIdx, pendingQuestions.length - 1)]
    : undefined;
  const isSingleQuick = pendingQuestions.length === 1 && !pendingQuestions[0].multiSelect;
  const currentSelectedIds = selectedByQ[activeQuestionIdx] ?? [];
  const currentCustom = customByQ[activeQuestionIdx] ?? '';
  const hasAnyAnswer = pendingQuestions.some((_, idx) => (
    (selectedByQ[idx]?.length ?? 0) > 0 || Boolean((customByQ[idx] ?? '').trim())
  ));

  return (
    <div className="create-pi-pane">
      {!props.hasModel ? (
        <div className="custom-action-shell-body scroll-area">
          <div className="model-empty">
            <p className="muted">{t.actions.createNeedsModel}</p>
            <button type="button" className="primary compact" onClick={props.onConfigureModel}>
              <Icon name="settings" />
              {t.primaryActions.configureModel}
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="custom-action-shell-body scroll-area">
            <label className="form-field">
              <span>{t.actions.createDescribeLabel}</span>
              <textarea
                data-autofocus
                rows={4}
                maxLength={CUSTOM_ACTION_LIMITS.maxGenerateRequestChars}
                disabled={props.generating}
                value={props.nlRequest}
                onChange={(event) => props.onNlRequestChange(event.target.value)}
                placeholder={t.actions.createDescribePlaceholder}
              />
            </label>

            {props.generating && !pendingQuestion && (
              <div className="create-pi-status" role="status" aria-live="polite">
                <Icon name="loader" className="spinning" />
                <span className="truncate">{statusLabel || t.actions.createInspectingStatus}</span>
              </div>
            )}

            {!pendingQuestion && liveCard?.blocks && liveCard.blocks.length > 0 && (
              <div className="create-pi-question">
                {liveCard.title && <p className="create-pi-question-text">{liveCard.title}</p>}
                <MessageBlocks blocks={liveCard.blocks} editorShort="IDE" />
              </div>
            )}

            {pendingQuestion && currentQuestion && (
              <div className="create-pi-question">
                {pendingQuestion.card?.blocks && pendingQuestion.card.blocks.length > 0 && (
                  <MessageBlocks blocks={pendingQuestion.card.blocks} editorShort="IDE" />
                )}
                {pendingQuestions.length > 1 && (
                  <div className="questionnaire-tabs" role="tablist">
                    {pendingQuestions.map((item, idx) => (
                      <button
                        type="button"
                        role="tab"
                        key={item.id}
                        aria-selected={idx === activeQuestionIdx}
                        className={`questionnaire-tab ${idx === activeQuestionIdx ? 'is-active' : ''}`}
                        onClick={() => setActiveQuestionIdx(idx)}
                      >
                        <span className="truncate">{item.header}</span>
                      </button>
                    ))}
                  </div>
                )}
                <p className="create-pi-question-text">{currentQuestion.question}</p>
                <div className="option-card-list">
                  {currentQuestion.options.map((opt) => {
                    const isSelected = currentSelectedIds.includes(opt.id);
                    return (
                      <button
                        type="button"
                        key={opt.id}
                        data-option-id={opt.id}
                        className={`option-card ${opt.recommended ? 'is-recommended' : ''} ${isSelected ? 'is-selected' : ''}`}
                        onClick={() => {
                          if (isSingleQuick) {
                            void submitInlineAnswer({ [activeQuestionIdx]: [opt.id] }, customByQ, false);
                            return;
                          }
                          setSelectedByQ((prev) => {
                            const cur = prev[activeQuestionIdx] ?? [];
                            const next = currentQuestion.multiSelect
                              ? (cur.includes(opt.id) ? cur.filter((id) => id !== opt.id) : [...cur, opt.id])
                              : [opt.id];
                            return { ...prev, [activeQuestionIdx]: next };
                          });
                        }}
                      >
                        <span className="option-card-content">
                          <span className="option-card-title-row">
                            <span className="option-card-label">{opt.label}</span>
                            {opt.recommended && <span className="option-recommended-badge">{t.dock.recommendedBadge}</span>}
                          </span>
                          <span className="option-card-desc muted">{opt.description}</span>
                        </span>
                      </button>
                    );
                  })}
                </div>
                <div className="message-reply">
                  <input
                    value={currentCustom}
                    onChange={(event) => setCustomByQ((prev) => ({ ...prev, [activeQuestionIdx]: event.target.value }))}
                    placeholder={t.dock.customOptionPlaceholder}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' && currentCustom.trim()) {
                        event.preventDefault();
                        void submitInlineAnswer({}, { [activeQuestionIdx]: currentCustom.trim() }, false);
                      }
                    }}
                  />
                </div>
                <div className="questionnaire-footer">
                  <button type="button" className="secondary compact" onClick={() => void submitInlineAnswer(undefined, undefined, true)}>
                    {t.dock.cancelQuestion}
                  </button>
                  {!isSingleQuick && (
                    <button type="button" className="primary compact" disabled={!hasAnyAnswer} onClick={() => void submitInlineAnswer()}>
                      {t.dock.submitAnswers}
                    </button>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="overlay-footer">
            {props.error && <div className="form-result danger" role="alert">{props.error}</div>}
            <div className="form-actions">
              <button type="button" className="secondary" onClick={props.onBackOrClose}>
                {t.common.cancel}
              </button>
              {props.generating ? (
                <button type="button" className="secondary" onClick={handleCancelGenerate}>
                  {t.actions.cancelGenerateButton}
                </button>
              ) : (
                <button
                  type="button"
                  className="primary"
                  disabled={!props.canGenerateWithPi || !props.nlRequest.trim()}
                  onClick={() => void handleGenerate()}
                >
                  <Icon name="sparkles" />
                  {t.actions.generateButton}
                </button>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
