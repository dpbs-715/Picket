import { useRef, useState } from 'react';
import {
  CUSTOM_ACTION_LIMITS,
  isCustomActionWriteTrusted,
  normalizeCustomActionPrompt,
  sanitizeCustomActionConfig,
  type CustomActionConfig,
  type GenerateCustomActionProgressEvent,
  type GenerateCustomActionReply,
} from '../../../shared/custom-action-protocol';
import type { MessageUserResponse } from '../../../shared/message-user-protocol';
import { useI18n } from '../../i18n';
import { Icon, Overlay } from '../../ui/controls';
import {
  classifyCustomActionSyncTarget,
  type CustomActionSyncRepoTarget,
} from '../workbench/state/workbench';
import { ActionSyncRepoPicker } from './ActionSyncRepoPicker';
import { CreateWithPiPane } from './CreateWithPiPane';

interface Props {
  repositoryName: string;
  existingActions: CustomActionConfig[];
  initialAction?: CustomActionConfig;
  initialMode?: 'manual' | 'create';
  otherRepositories: CustomActionSyncRepoTarget[];
  hasModel: boolean;
  canGenerateWithPi: boolean;
  covered?: boolean;
  depth?: number;
  onBack?(): void;
  backLabel?: string;
  onClose(): void;
  onSave(action: CustomActionConfig, selectAsActive: boolean, syncTargetRepositoryIds: string[]): void;
  onDelete?(actionId: string): void;
  onResetWriteTrust?(actionId: string): void;
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
  const [nlRequest, setNlRequest] = useState('');
  const [generating, setGenerating] = useState(false);
  const [selectAfterSave, setSelectAfterSave] = useState(!isEditing);
  const [draftSummary, setDraftSummary] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedSyncRepoIds, setSelectedSyncRepoIds] = useState<string[]>([]);
  const [trustReset, setTrustReset] = useState(false);
  const cancelGenerateRef = useRef<() => void>(() => {});

  const syncTargets = props.otherRepositories.map((repo) => ({
    ...repo,
    status: classifyCustomActionSyncTarget(repo.existingActions, {
      id: props.initialAction?.id,
      templateKey: props.initialAction?.templateKey,
      label,
    }),
  }));
  const selectableSyncIds = syncTargets
    .filter((repo) => repo.status !== 'full')
    .map((repo) => repo.id);
  const effectiveSelectedSyncIds = selectedSyncRepoIds.filter((id) => selectableSyncIds.includes(id));
  const allSyncSelected = selectableSyncIds.length > 0
    && effectiveSelectedSyncIds.length === selectableSyncIds.length;

  function toggleSelectAllSyncRepos() {
    setSelectedSyncRepoIds(allSyncSelected ? [] : selectableSyncIds);
  }

  function toggleSyncRepo(repoId: string) {
    setSelectedSyncRepoIds((prev) =>
      prev.includes(repoId) ? prev.filter((id) => id !== repoId) : [...prev, repoId],
    );
  }

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
    props.onSave(built, selectAfterSave, effectiveSelectedSyncIds);
  }

  const title = isEditing ? t.actions.editModalTitle : t.actions.newModalTitle;
  const handleBackOrClose = () => {
    cancelGenerateRef.current();
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
      onClose={() => { cancelGenerateRef.current(); props.onClose(); }}
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
          <CreateWithPiPane
            hasModel={props.hasModel}
            canGenerateWithPi={props.canGenerateWithPi}
            nlRequest={nlRequest}
            generating={generating}
            error={error}
            cancelRef={cancelGenerateRef}
            onNlRequestChange={setNlRequest}
            onGeneratingChange={setGenerating}
            onError={setError}
            onDraftReady={(draft) => {
              setLabel(draft.label);
              setPrompt(draft.prompt);
              setDraftSummary(draft.summary);
              setMode('manual');
            }}
            onConfigureModel={props.onConfigureModel}
            onBackOrClose={handleBackOrClose}
            onGenerateWithPi={props.onGenerateWithPi}
            onRespondGenerateQuestion={props.onRespondGenerateQuestion}
            onCancelGenerate={props.onCancelGenerate}
          />
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

              <ActionSyncRepoPicker
                syncTargets={syncTargets}
                selectableSyncIds={selectableSyncIds}
                effectiveSelectedSyncIds={effectiveSelectedSyncIds}
                onToggleSelectAll={toggleSelectAllSyncRepos}
                onToggleRepo={toggleSyncRepo}
              />
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
                    {effectiveSelectedSyncIds.length > 0
                      ? t.actions.saveAndSyncButton(effectiveSelectedSyncIds.length)
                      : t.actions.saveButton}
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
