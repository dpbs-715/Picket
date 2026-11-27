import { useEffect, useRef, useState } from 'react';
import type {
  MessageQuestionItem,
  MessageUserAnswer,
  MessageUserRequest,
  MessageUserResponse,
} from '../../../../shared/message-user-protocol';
import { useI18n } from '../../../i18n';
import { Icon } from '../../../ui/controls';

type PendingAction =
  | { kind: 'option'; optionId: string }
  | { kind: 'submit' }
  | { kind: 'cancel' }
  | { kind: 'custom' }
  | null;

interface MessageDockQuestionnaireProps {
  messageId: string;
  questionnaire: MessageUserRequest;
  questions: MessageQuestionItem[];
  busy: boolean;
  onSubmit(response: MessageUserResponse): void | Promise<boolean | void>;
}

export function MessageDockQuestionnaire({
  messageId,
  questionnaire,
  questions,
  busy,
  onSubmit,
}: MessageDockQuestionnaireProps) {
  const { t } = useI18n();
  const [activeQuestionIdx, setActiveQuestionIdx] = useState(0);
  const [selectedByQuestion, setSelectedByQuestion] = useState<Record<number, string[]>>({});
  const [customByQuestion, setCustomByQuestion] = useState<Record<number, string>>({});
  const [expandedPreviewKey, setExpandedPreviewKey] = useState<string | null>(null);
  const [dontAskAgain, setDontAskAgain] = useState(false);
  const [pendingAction, setPendingAction] = useState<PendingAction>(null);
  const submittingRef = useRef(false);
  const activeMessageIdRef = useRef(messageId);
  activeMessageIdRef.current = messageId;

  useEffect(() => {
    setActiveQuestionIdx(0);
    setSelectedByQuestion({});
    setCustomByQuestion({});
    setExpandedPreviewKey(null);
    setDontAskAgain(false);
    setPendingAction(null);
    submittingRef.current = false;
  }, [messageId]);

  const currentQuestion = questions[Math.min(activeQuestionIdx, questions.length - 1)];
  if (!currentQuestion) return null;

  const locked = busy || pendingAction !== null;
  const isSingleQuickSelect = questions.length === 1 && !questions[0].multiSelect;
  const currentSelectedIds = selectedByQuestion[activeQuestionIdx] ?? [];
  const currentCustomText = customByQuestion[activeQuestionIdx] ?? '';
  const hasAnyQuestionnaireInput = questions.some((_, idx) => (
    (selectedByQuestion[idx]?.length ?? 0) > 0 || Boolean((customByQuestion[idx] ?? '').trim())
  ));

  const buildResponse = (
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

  const dispatchResponse = async (
    action: Exclude<PendingAction, null>,
    response: MessageUserResponse,
  ) => {
    if (locked || submittingRef.current) return;
    const capturedMessageId = messageId;
    submittingRef.current = true;
    setPendingAction(action);
    try {
      const outcome = await onSubmit(response);
      if (activeMessageIdRef.current !== capturedMessageId) return;
      // Stay locked with the spinner when backend explicitly accepts (`true`) until the
      // incoming progress/state transition replaces this question, closing the IPC->event gap.
      if (outcome !== true) {
        submittingRef.current = false;
        setPendingAction(null);
      }
    } catch {
      if (activeMessageIdRef.current === capturedMessageId) {
        submittingRef.current = false;
        setPendingAction(null);
      }
    }
  };

  const submit = (
    action: Exclude<PendingAction, null>,
    overrideSelections?: Record<number, string[]>,
    overrideCustoms?: Record<number, string>,
  ) => {
    void dispatchResponse(action, buildResponse(overrideSelections, overrideCustoms));
  };

  const cancel = () => {
    void dispatchResponse({ kind: 'cancel' }, {
      cancelled: true,
      answers: [],
      unanswered: questions.map((_, index) => index),
    });
  };

  const toggleOption = (questionIndex: number, optionId: string, multiSelect: boolean) => {
    if (locked) return;
    if (isSingleQuickSelect) {
      submit({ kind: 'option', optionId }, { [questionIndex]: [optionId] }, customByQuestion);
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

  const isCancelLoading = pendingAction?.kind === 'cancel';
  const isSubmitLoading = pendingAction?.kind === 'submit';
  const isCustomLoading = pendingAction?.kind === 'custom';

  return (
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
                disabled={locked}
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
          const isOptionLoading = pendingAction?.kind === 'option' && pendingAction.optionId === option.id;
          const previewKey = `${currentQuestion.id}:${option.id}`;
          const previewOpen = expandedPreviewKey === previewKey;
          return (
            <div
              key={option.id}
              className={`d2-opt-shell ${isPrimaryStyle ? 'is-primary' : ''} ${isSelected ? 'is-selected' : ''} ${locked ? 'is-disabled' : ''} ${isOptionLoading ? 'is-loading' : ''}`}
            >
              <div className="d2-opt-row">
                <button
                  type="button"
                  data-option-id={option.id}
                  aria-pressed={!isSingleQuickSelect ? isSelected : undefined}
                  aria-busy={isOptionLoading || undefined}
                  className={`d2-opt-btn ${option.recommended ? 'is-recommended' : ''} ${isSelected ? 'is-selected' : ''}`}
                  disabled={locked}
                  onClick={() => toggleOption(
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
                      {option.recommended && (
                        <span className="option-recommended-badge">{t.dock.recommendedBadge}</span>
                      )}
                    </span>
                    <span className="d2-opt-desc">{option.description}</span>
                  </span>
                  {isOptionLoading && (
                    <span className="d2-opt-spinner" aria-hidden="true">
                      <Icon name="loader" className="spinning" />
                    </span>
                  )}
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
            disabled={locked}
            onChange={(event) => setDontAskAgain(event.target.checked)}
          />
          <span>{questionnaire.context.dontAskAgainLabel ?? t.dock.dontAskAgainDefault}</span>
        </label>
      )}

      <form
        className="d2-composer"
        onSubmit={(event) => {
          event.preventDefault();
          if (locked) return;
          if (isSingleQuickSelect) {
            if (currentCustomText.trim()) {
              submit({ kind: 'custom' }, {}, { [activeQuestionIdx]: currentCustomText.trim() });
            }
          } else if (hasAnyQuestionnaireInput) {
            submit({ kind: 'submit' });
          }
        }}
      >
        <input
          aria-label={t.dock.replyLabel}
          value={currentCustomText}
          disabled={locked}
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
            aria-busy={isCustomLoading || undefined}
            title={t.dock.sendReply}
            disabled={!currentCustomText.trim() || locked}
          >
            <Icon name={isCustomLoading ? 'loader' : 'sendUp'} className={isCustomLoading ? 'spinning' : ''} />
          </button>
        )}
      </form>

      <div className="questionnaire-footer">
        <button
          type="button"
          className={`secondary compact ${isCancelLoading ? 'd2-btn-loading' : ''}`}
          aria-busy={isCancelLoading || undefined}
          disabled={locked}
          onClick={cancel}
        >
          {isCancelLoading && <Icon name="loader" className="spinning d2-btn-spinner" />}
          <span>{t.dock.cancelQuestion}</span>
        </button>
        {!isSingleQuickSelect && (
          <button
            type="button"
            className={`primary compact ${isSubmitLoading ? 'd2-btn-loading' : ''}`}
            aria-busy={isSubmitLoading || undefined}
            disabled={!hasAnyQuestionnaireInput || locked}
            onClick={() => submit({ kind: 'submit' })}
          >
            {isSubmitLoading && <Icon name="loader" className="spinning d2-btn-spinner" />}
            <span>{t.dock.submitAnswers}</span>
          </button>
        )}
      </div>
    </>
  );
}
