import { useEffect, useRef, useState } from 'react';
import type {
  AskUserQuestionAnswer,
  AskUserQuestionRequest,
  AskUserQuestionResponse,
} from '../../../shared/ask-user-protocol';
import {
  CUSTOM_ACTION_LIMITS,
  isCustomActionWriteTrusted,
  normalizeCustomActionPrompt,
  sanitizeCustomActionConfig,
  type CustomActionConfig,
  type CustomActionSyncResult,
  type GenerateCustomActionProgressEvent,
  type GenerateCustomActionReply,
} from '../../../shared/custom-action-protocol';
import { useI18n } from '../../i18n';
import { Icon, Overlay } from '../../ui/controls';

interface Props {
  repositoryName: string;
  existingActions: CustomActionConfig[];
  initialAction?: CustomActionConfig;
  initialMode?: 'manual' | 'create';
  otherRepoCount: number;
  hasModel: boolean;
  canGenerateWithPi: boolean;
  covered?: boolean;
  depth?: number;
  onBack?(): void;
  backLabel?: string;
  onClose(): void;
  onSave(action: CustomActionConfig, selectAsActive: boolean): void;
  onDelete?(actionId: string): void;
  onSyncToAllRepos?(action: CustomActionConfig, overwrite: boolean): CustomActionSyncResult;
  onResetWriteTrust?(actionId: string): void;
  onGenerateWithPi?(
    requestId: string,
    userRequest: string,
    onProgress: (event: GenerateCustomActionProgressEvent) => void,
  ): Promise<GenerateCustomActionReply>;
  onRespondGenerateQuestion?(
    runId: string,
    callId: string,
    response: AskUserQuestionResponse,
  ): Promise<boolean>;
  onCancelGenerate?(runId: string): void;
  onConfigureModel(): void;
}

export function CustomActionModal(props: Props) {
  const { t } = useI18n();
  const isEditing = Boolean(props.initialAction);
  const [mode, setMode] = useState<'manual' | 'create'>(
    isEditing ? 'manual' : props.initialMode ?? 'manual',
  );
  const [label, setLabel] = useState(props.initialAction?.label ?? '');
  const [prompt, setPrompt] = useState(props.initialAction?.prompt ?? '');
  const [selectAfterSave, setSelectAfterSave] = useState(!isEditing);
  const [draftSummary, setDraftSummary] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [syncResult, setSyncResult] = useState<CustomActionSyncResult | null>(null);
  const [trustReset, setTrustReset] = useState(false);

  // Create with Pi state
  const [nlRequest, setNlRequest] = useState('');
  const [generating, setGenerating] = useState(false);
  const [statusLabel, setStatusLabel] = useState('');
  const activeRunIdRef = useRef<string | null>(null);
  const latestCancelRef = useRef(props.onCancelGenerate);
  latestCancelRef.current = props.onCancelGenerate;
  useEffect(() => () => {
    const activeId = activeRunIdRef.current;
    if (activeId) {
      activeRunIdRef.current = null;
      latestCancelRef.current?.(activeId);
    }
  }, []);
  const [pendingQuestion, setPendingQuestion] = useState<{
    runId: string;
    callId: string;
    request: AskUserQuestionRequest;
  } | null>(null);
  const [activeQuestionIdx, setActiveQuestionIdx] = useState(0);
  const [selectedByQ, setSelectedByQ] = useState<Record<number, string[]>>({});
  const [customByQ, setCustomByQ] = useState<Record<number, string>>({});

  const promptUnchanged = isEditing
    && normalizeCustomActionPrompt(prompt) === normalizeCustomActionPrompt(props.initialAction?.prompt ?? '');
  const hasActiveWriteTrust = !trustReset
    && promptUnchanged
    && Boolean(props.initialAction && isCustomActionWriteTrusted(props.initialAction));

  function handleSave(event: React.FormEvent) {
    event.preventDefault();
    const cleanLabel = label.trim();
    const cleanPrompt = normalizeCustomActionPrompt(prompt);
    if (!cleanLabel) {
      setError(t.actions.validationLabelRequired);
      return;
    }
    if (!cleanPrompt) {
      setError(t.actions.validationPromptRequired);
      return;
    }
    const duplicate = props.existingActions.some(
      (item) => item.id !== props.initialAction?.id
        && item.label.trim().toLowerCase() === cleanLabel.toLowerCase(),
    );
    if (duplicate) {
      setError(t.actions.validationDuplicateLabel);
      return;
    }
    const built = sanitizeCustomActionConfig({
      id: props.initialAction?.id ?? `act-${crypto.randomUUID().slice(0, 12)}`,
      ...(props.initialAction?.templateKey ? { templateKey: props.initialAction.templateKey } : {}),
      label: cleanLabel,
      prompt: cleanPrompt,
      ...(hasActiveWriteTrust && props.initialAction?.trustedPromptHash
        ? {
            skipWriteConfirm: true,
            trustedPromptHash: props.initialAction.trustedPromptHash,
          }
        : {}),
    });
    setError(null);
    props.onSave(built, selectAfterSave);
  }

  function handleSync(overwrite: boolean) {
    if (!props.onSyncToAllRepos) return;
    const cleanLabel = label.trim();
    const cleanPrompt = normalizeCustomActionPrompt(prompt);
    if (!cleanLabel || !cleanPrompt) {
      setError(t.actions.validationBeforeSync);
      return;
    }
    const candidate = sanitizeCustomActionConfig({
      id: props.initialAction?.id ?? `act-${crypto.randomUUID().slice(0, 12)}`,
      ...(props.initialAction?.templateKey ? { templateKey: props.initialAction.templateKey } : {}),
      label: cleanLabel,
      prompt: cleanPrompt,
    });
    setError(null);
    setSyncResult(props.onSyncToAllRepos(candidate, overwrite));
  }

  async function handleGenerate() {
    if (!props.onGenerateWithPi || generating) return;
    const trimmed = nlRequest.trim();
    if (!trimmed) {
      setError(t.actions.createPromptRequired);
      return;
    }
    const requestId = crypto.randomUUID();
    activeRunIdRef.current = requestId;
    setGenerating(true);
    setError(null);
    setPendingQuestion(null);
    setStatusLabel(t.actions.createInspectingStatus);

    try {
      const reply = await props.onGenerateWithPi(requestId, trimmed, (event) => {
        if (activeRunIdRef.current !== requestId) return;
        setStatusLabel(event.statusLabel);
        if (event.status === 'needsDecision' && event.pendingCallId && event.questionnaire) {
          setPendingQuestion({
            runId: event.runId,
            callId: event.pendingCallId,
            request: event.questionnaire,
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
        setLabel(reply.draft.label);
        setPrompt(reply.draft.prompt);
        setDraftSummary(reply.draft.summary);
        setPendingQuestion(null);
        setMode('manual');
      }
    } catch (err) {
      if (activeRunIdRef.current === requestId) {
        setError(err instanceof Error ? err.message : t.actions.createFailedFallback);
      }
    } finally {
      if (activeRunIdRef.current === requestId) {
        activeRunIdRef.current = null;
        setGenerating(false);
        setPendingQuestion(null);
      }
    }
  }

  function handleCancelGenerate() {
    const runId = activeRunIdRef.current;
    if (runId) {
      activeRunIdRef.current = null;
      props.onCancelGenerate?.(runId);
    }
    setGenerating(false);
    setPendingQuestion(null);
    setStatusLabel('');
  }

  async function submitInlineAnswer(
    overrideSelections?: Record<number, string[]>,
    overrideCustoms?: Record<number, string>,
    cancelled = false,
  ) {
    if (!pendingQuestion || !props.onRespondGenerateQuestion) return;
    const { runId, callId, request } = pendingQuestion;
    const selMap = overrideSelections ?? selectedByQ;
    const customMap = overrideCustoms ?? customByQ;
    const answers: AskUserQuestionAnswer[] = [];
    const unanswered: number[] = [];

    if (cancelled) {
      request.questions.forEach((_, idx) => unanswered.push(idx));
    } else {
      request.questions.forEach((q, idx) => {
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
      setError(err instanceof Error ? err.message : t.actions.createFailedFallback);
    }
  }

  const currentQuestion = pendingQuestion?.request.questions[
    Math.min(activeQuestionIdx, (pendingQuestion?.request.questions.length ?? 1) - 1)
  ];
  const isSingleQuick = Boolean(
    pendingQuestion
    && pendingQuestion.request.questions.length === 1
    && !pendingQuestion.request.questions[0].multiSelect,
  );
  const currentSelectedIds = selectedByQ[activeQuestionIdx] ?? [];
  const currentCustom = customByQ[activeQuestionIdx] ?? '';
  const hasAnyAnswer = Boolean(
    pendingQuestion && pendingQuestion.request.questions.some((_, idx) => (
      (selectedByQ[idx]?.length ?? 0) > 0 || Boolean((customByQ[idx] ?? '').trim())
    )),
  );

  const title = isEditing ? t.actions.editModalTitle : t.actions.newModalTitle;
  const handleBackOrClose = () => {
    handleCancelGenerate();
    if (props.onBack) props.onBack();
    else props.onClose();
  };

  return (
    <Overlay
      title={title}
      covered={props.covered}
      depth={props.depth}
      variant={props.onBack ? 'drill' : 'sheet'}
      onBack={props.onBack ? handleBackOrClose : undefined}
      backLabel={props.backLabel}
      onClose={() => { handleCancelGenerate(); props.onClose(); }}
    >
      <div className="custom-action-modal">
        <div className="custom-action-shell-head">
          <div className="custom-action-repo-meta">
            <Icon name="folder" />
            <span className="mono truncate">{props.repositoryName}</span>
          </div>

          {!isEditing && (
            <div className="action-mode-tabs" role="tablist" aria-label={t.actions.creationModeLabel}>
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'manual'}
                className={mode === 'manual' ? 'is-active' : ''}
                disabled={generating}
                onClick={() => { setError(null); setMode('manual'); }}
              >
                <Icon name="pencil" />
                {t.actions.modeManual}
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'create'}
                className={mode === 'create' ? 'is-active' : ''}
                disabled={generating}
                onClick={() => { setError(null); setMode('create'); }}
              >
                <Icon name="sparkles" />
                {t.actions.modeCreateWithPi}
              </button>
            </div>
          )}
        </div>

        {mode === 'create' && !isEditing ? (
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
                      disabled={generating}
                      value={nlRequest}
                      onChange={(event) => setNlRequest(event.target.value)}
                      placeholder={t.actions.createDescribePlaceholder}
                    />
                  </label>

                  {generating && !pendingQuestion && (
                    <div className="create-pi-status" role="status" aria-live="polite">
                      <Icon name="loader" className="spinning" />
                      <span className="truncate">{statusLabel || t.actions.createInspectingStatus}</span>
                    </div>
                  )}

                  {pendingQuestion && currentQuestion && (
                    <div className="create-pi-question">
                      {pendingQuestion.request.questions.length > 1 && (
                        <div className="questionnaire-tabs" role="tablist">
                          {pendingQuestion.request.questions.map((item, idx) => (
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
                  {error && <div className="form-result danger" role="alert">{error}</div>}
                  <div className="form-actions">
                    <button type="button" className="secondary" onClick={handleBackOrClose}>
                      {t.common.cancel}
                    </button>
                    {generating ? (
                      <button type="button" className="secondary" onClick={handleCancelGenerate}>
                        {t.actions.cancelGenerateButton}
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="primary"
                        disabled={!props.canGenerateWithPi || !nlRequest.trim()}
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
        ) : (
          <form className="custom-action-form" onSubmit={handleSave}>
            <div className="custom-action-shell-body scroll-area">
              {draftSummary && (
                <div className="draft-summary-banner">
                  <Icon name="sparkles" />
                  <span>{draftSummary}</span>
                </div>
              )}

              <label className="form-field">
                <span>{t.actions.labelField}</span>
                <input
                  data-autofocus
                  maxLength={CUSTOM_ACTION_LIMITS.maxLabelChars}
                  value={label}
                  onChange={(event) => setLabel(event.target.value)}
                  placeholder={t.actions.labelPlaceholder}
                />
              </label>

              <label className="form-field">
                <span>{t.actions.promptField}</span>
                <textarea
                  className="mono"
                  rows={5}
                  maxLength={CUSTOM_ACTION_LIMITS.maxPromptChars}
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  placeholder={t.actions.promptPlaceholder}
                  spellCheck={false}
                />
              </label>

              {!isEditing && (
                <label className="dont-ask-toggle">
                  <input
                    type="checkbox"
                    checked={selectAfterSave}
                    onChange={(event) => setSelectAfterSave(event.target.checked)}
                  />
                  <span>{t.actions.selectAfterSaveLabel}</span>
                </label>
              )}

              {hasActiveWriteTrust && props.initialAction && (
                <div className="action-trust-card">
                  <span className="muted small">{t.actions.writeTrustActiveNote}</span>
                  <button
                    type="button"
                    className="secondary compact"
                    onClick={() => {
                      setTrustReset(true);
                      props.onResetWriteTrust?.(props.initialAction!.id);
                    }}
                  >
                    {t.actions.resetWriteTrustButton}
                  </button>
                </div>
              )}

              {props.otherRepoCount > 0 && props.onSyncToAllRepos && (
                <div className="action-sync-card">
                  <div className="row" style={{ justifyContent: 'space-between', width: '100%' }}>
                    <span className="muted small">{t.actions.syncReposHint(props.otherRepoCount)}</span>
                    <button type="button" className="secondary compact" onClick={() => handleSync(false)}>
                      <Icon name="copy" />
                      {t.actions.syncToAllButton}
                    </button>
                  </div>
                  {syncResult && (
                    <div className="action-sync-result">
                      <span className="small">
                        {t.actions.syncResultSummary(syncResult.added, syncResult.updated, syncResult.skipped)}
                      </span>
                      {syncResult.skipped > 0 && (
                        <button type="button" className="secondary compact" onClick={() => handleSync(true)}>
                          {t.actions.syncOverwriteSkippedButton(syncResult.skipped)}
                        </button>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>

            <div className="overlay-footer">
              {error && <div className="form-result danger" role="alert">{error}</div>}
              <div className="form-actions">
                <div>
                  {isEditing && props.onDelete && props.initialAction && (
                    <button
                      type="button"
                      className="secondary danger-action"
                      onClick={() => props.onDelete!(props.initialAction!.id)}
                    >
                      <Icon name="trash" />
                      {t.actions.deleteButton}
                    </button>
                  )}
                </div>
                <div className="row">
                  <button type="button" className="secondary" onClick={handleBackOrClose}>
                    {t.common.cancel}
                  </button>
                  <button type="submit" className="primary">
                    {t.actions.saveButton}
                  </button>
                </div>
              </div>
            </div>
          </form>
        )}
      </div>
    </Overlay>
  );
}
