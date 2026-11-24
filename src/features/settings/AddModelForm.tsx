import { useEffect, useId, useRef, useState } from 'react';
import type { ModelRevealKeyReply } from '../../../shared/model-protocol';
import { useI18n } from '../../i18n';
import { HelpTip, Icon, IconButton, Overlay } from '../../ui/controls';
import type { SaveModelOutcome } from '../workbench/commands';
import {
  apiFormats,
  duplicateModelInput,
  editModelInput,
  newModelInput,
  normalizedModel,
  providerFor,
  providers,
  resolveDefaultTokenLimits,
  sameCredentialEndpoint,
  validateModel,
} from './modelProviders';
import type { ApiFormatId, ModelConfiguration, ModelInput, SharedKeyUpdateScope } from './modelProviders';

const REVEAL_AUTO_HIDE_MS = 30_000;

function sanitizeModelError(error: unknown, secrets: readonly string[], fallback: string): string {
  let text = error instanceof Error ? error.message : typeof error === 'string' ? error : fallback;
  const candidates = secrets.flatMap((item) => [item, item.trim()]).filter((item) => item.length > 0);
  for (const secret of candidates) {
    text = text.split(secret).join('[REDACTED]');
    if (secret.length >= 4) {
      const encoded = encodeURIComponent(secret);
      if (encoded !== secret) text = text.split(encoded).join('[REDACTED]');
    }
  }
  return text.trim() || fallback;
}

function parseOptionalPositiveInt(raw: string): number | 'invalid' | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  if (!/^\d+$/.test(trimmed)) return 'invalid';
  const parsed = Number(trimmed);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) return 'invalid';
  return parsed;
}

function connectionSignature(normalized: ModelInput): string {
  return JSON.stringify([
    normalized.provider,
    normalized.apiFormat,
    normalized.modelId,
    normalized.baseUrl,
    normalized.apiKey,
    normalized.sourceModelId ?? '',
    normalized.contextWindow ?? 0,
    normalized.maxOutputTokens ?? 0,
  ]);
}

interface Props {
  initialModel?: ModelConfiguration;
  sourceModel?: ModelConfiguration;
  existingModels?: readonly ModelConfiguration[];
  onClose(): void;
  onSaved(outcome: SaveModelOutcome): void;
  onKeyHintResolved?: (modelId: string, keyHint: string) => void;
  test(input: ModelInput): Promise<string>;
  save(input: ModelInput): Promise<SaveModelOutcome>;
  revealApiKey?: (modelId: string) => Promise<ModelRevealKeyReply>;
}

export function AddModelForm({
  initialModel,
  sourceModel,
  existingModels = [],
  onClose,
  onSaved,
  onKeyHintResolved,
  test,
  save,
  revealApiKey,
}: Props) {
  const { locale, t } = useI18n();
  const apiKeyInputId = useId();
  const isEditing = !!initialModel;
  const isDuplicating = !initialModel && !!sourceModel;
  const originModel = initialModel ?? sourceModel;

  const [input, setInput] = useState<ModelInput>(() => (
    initialModel
      ? editModelInput(initialModel)
      : sourceModel
        ? duplicateModelInput(sourceModel, existingModels, locale)
        : newModelInput()
  ));
  const [contextWindowText, setContextWindowText] = useState<string>(() => {
    const seeded = initialModel
      ? editModelInput(initialModel)
      : sourceModel
        ? duplicateModelInput(sourceModel, existingModels, locale)
        : newModelInput();
    return String(seeded.contextWindow ?? '');
  });
  const [maxOutputTokensText, setMaxOutputTokensText] = useState<string>(() => {
    const seeded = initialModel
      ? editModelInput(initialModel)
      : sourceModel
        ? duplicateModelInput(sourceModel, existingModels, locale)
        : newModelInput();
    return String(seeded.maxOutputTokens ?? '');
  });
  const [sharedScope, setSharedScope] = useState<SharedKeyUpdateScope>('updateAll');
  const [localKeyHint, setLocalKeyHint] = useState<string | undefined>(() => originModel?.keyHint);
  const [revealedApiKey, setRevealedApiKey] = useState<string | null>(null);
  const [revealing, setRevealing] = useState(false);
  const [revealNotice, setRevealNotice] = useState<{ danger: boolean; text: string } | null>(null);
  const [testedSignature, setTestedSignature] = useState<string | null>(null);
  const [failedTestSignature, setFailedTestSignature] = useState<string | null>(null);
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);
  const revealGeneration = useRef(0);

  const provider = providerFor(input.provider);
  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const defaultLimits = resolveDefaultTokenLimits(input.provider, effectiveFormat);
  const originNeedsKey = !!originModel && providerFor(originModel.provider).needsKey;
  const sharedPeerCount = isEditing ? (initialModel.sharedKeyCount ?? 1) : 1;
  const isSharedEdit = isEditing && originNeedsKey && sharedPeerCount > 1;
  const endpointMatchesOrigin = !!originModel && sameCredentialEndpoint(
    originModel.provider,
    originModel.baseUrl,
    input.provider,
    input.baseUrl,
  );
  const canKeepExistingKey = isEditing
    ? originNeedsKey && (!isSharedEdit || endpointMatchesOrigin)
    : isDuplicating
      ? originNeedsKey && provider.needsKey && endpointMatchesOrigin
      : false;
  const showEndpointChangedKeyNotice = provider.needsKey
    && originNeedsKey
    && !endpointMatchesOrigin
    && (isDuplicating || isSharedEdit);
  const effectiveKeyHint = localKeyHint ?? originModel?.keyHint;
  const showKeyHint = provider.needsKey && canKeepExistingKey && !!effectiveKeyHint;
  const canRevealSavedKey = isEditing && originNeedsKey && provider.needsKey && !!revealApiKey;
  const showSharedScopeOptions = isSharedEdit
    && provider.needsKey
    && endpointMatchesOrigin
    && input.apiKey.trim().length > 0;

  function clearRevealedKey() {
    revealGeneration.current += 1;
    setRevealing(false);
    setRevealedApiKey(null);
  }

  useEffect(() => () => {
    generation.current += 1;
    revealGeneration.current += 1;
  }, []);

  useEffect(() => {
    clearRevealedKey();
    setRevealNotice(null);
    setLocalKeyHint(originModel?.keyHint);
  }, [initialModel?.id, sourceModel?.id]);

  useEffect(() => {
    if (revealedApiKey === null) return;
    const timer = window.setTimeout(() => {
      clearRevealedKey();
    }, REVEAL_AUTO_HIDE_MS);
    const handleBlur = () => {
      clearRevealedKey();
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        clearRevealedKey();
      }
    };
    window.addEventListener('blur', handleBlur);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener('blur', handleBlur);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [revealedApiKey]);

  function resetAsyncState() {
    generation.current += 1;
    inFlight.current = null;
    setTestedSignature(null);
    setFailedTestSignature(null);
    setResult(null);
    setOperation('idle');
  }

  function change(next: ModelInput) {
    resetAsyncState();
    setInput(next);
  }

  function changeProvider(nextProviderId: ModelInput['provider']) {
    const base = newModelInput(nextProviderId);
    const next: ModelInput = initialModel
      ? { ...base, id: initialModel.id }
      : sourceModel
        ? {
            ...base,
            name: input.name,
            keepActiveModel: true,
            ...(originNeedsKey ? { sourceModelId: sourceModel.id } : {}),
          }
        : base;
    resetAsyncState();
    if (!providerFor(nextProviderId).needsKey) {
      clearRevealedKey();
      setRevealNotice(null);
    }
    setInput(next);
    setContextWindowText(String(next.contextWindow ?? ''));
    setMaxOutputTokensText(String(next.maxOutputTokens ?? ''));
  }

  function changeApiFormat(nextFormat: ApiFormatId) {
    const prevDefaults = resolveDefaultTokenLimits(input.provider, effectiveFormat);
    const nextDefaults = resolveDefaultTokenLimits(input.provider, nextFormat);
    const nextCtx = !contextWindowText.trim() || contextWindowText.trim() === String(prevDefaults.contextWindow)
      ? nextDefaults.contextWindow
      : input.contextWindow;
    const nextMaxOut = !maxOutputTokensText.trim() || maxOutputTokensText.trim() === String(prevDefaults.maxOutputTokens)
      ? nextDefaults.maxOutputTokens
      : input.maxOutputTokens;
    resetAsyncState();
    setInput({ ...input, apiFormat: nextFormat, contextWindow: nextCtx, maxOutputTokens: nextMaxOut });
    if (!contextWindowText.trim() || contextWindowText.trim() === String(prevDefaults.contextWindow)) {
      setContextWindowText(String(nextDefaults.contextWindow));
    }
    if (!maxOutputTokensText.trim() || maxOutputTokensText.trim() === String(prevDefaults.maxOutputTokens)) {
      setMaxOutputTokensText(String(nextDefaults.maxOutputTokens));
    }
  }

  function changeTokens(nextCtxRaw: string, nextMaxOutRaw: string) {
    resetAsyncState();
    setContextWindowText(nextCtxRaw);
    setMaxOutputTokensText(nextMaxOutRaw);
  }

  async function toggleRevealApiKey() {
    if (!initialModel || !revealApiKey) return;
    if (revealedApiKey !== null || revealing) {
      clearRevealedKey();
      return;
    }
    const token = ++revealGeneration.current;
    setRevealing(true);
    setRevealNotice(null);
    try {
      const reply = await revealApiKey(initialModel.id);
      if (revealGeneration.current !== token) return;
      if (reply.outcome === 'authenticated') {
        setRevealedApiKey(reply.apiKey);
        setLocalKeyHint(reply.keyHint);
        if (initialModel.keyHint !== reply.keyHint) {
          onKeyHintResolved?.(initialModel.id, reply.keyHint);
        }
      } else {
        setRevealedApiKey(null);
        setRevealNotice({
          danger: reply.outcome === 'failed',
          text: reply.message,
        });
      }
    } catch (error) {
      if (revealGeneration.current !== token) return;
      setRevealedApiKey(null);
      setRevealNotice({
        danger: true,
        text: sanitizeModelError(error, [input.apiKey], t.modelForm.revealKeyErrorFallback),
      });
    } finally {
      if (revealGeneration.current === token) {
        setRevealing(false);
      }
    }
  }

  function buildCandidate(parsedCtx: number | undefined, parsedMaxOut: number | undefined): ModelInput {
    const trimmedKey = input.apiKey.trim();
    return {
      ...input,
      sourceModelId: isDuplicating && canKeepExistingKey && !trimmedKey ? sourceModel!.id : undefined,
      sharedKeyUpdateScope: isSharedEdit && provider.needsKey && trimmedKey.length > 0
        ? (endpointMatchesOrigin ? sharedScope : 'detachCurrent')
        : undefined,
      keepActiveModel: isDuplicating ? true : undefined,
      contextWindow: parsedCtx ?? defaultLimits.contextWindow,
      maxOutputTokens: parsedMaxOut ?? defaultLimits.maxOutputTokens,
      locale,
    };
  }

  const previewCtx = parseOptionalPositiveInt(contextWindowText);
  const previewMaxOut = parseOptionalPositiveInt(maxOutputTokensText);
  const previewSignature = previewCtx !== 'invalid' && previewMaxOut !== 'invalid'
    ? connectionSignature(normalizedModel(buildCandidate(previewCtx, previewMaxOut)))
    : null;
  const duplicateModelIdChanged = isDuplicating && input.modelId.trim() !== sourceModel!.modelId.trim();
  const saveAnywayArmed = duplicateModelIdChanged
    && previewSignature !== null
    && failedTestSignature === previewSignature;

  async function submit(kind: 'testing' | 'saving') {
    if (inFlight.current !== null) return;
    const parsedCtx = parseOptionalPositiveInt(contextWindowText);
    if (parsedCtx === 'invalid') {
      setResult({
        ok: false,
        text: t.modelForm.validation.tokenLimit('context_range'),
      });
      return;
    }
    const parsedMaxOut = parseOptionalPositiveInt(maxOutputTokensText);
    if (parsedMaxOut === 'invalid') {
      setResult({
        ok: false,
        text: t.modelForm.validation.tokenLimit('output_range'),
      });
      return;
    }
    const candidate = buildCandidate(parsedCtx, parsedMaxOut);
    const error = validateModel(candidate, { canKeepExistingKey, locale });
    if (error) { setResult({ ok: false, text: error }); return; }

    const normalized = normalizedModel(candidate);
    const signature = connectionSignature(normalized);
    const version = ++generation.current;
    inFlight.current = version;
    setResult(null);

    const activeSecrets = [candidate.apiKey, revealedApiKey ?? ''];

    try {
      if (kind === 'testing') {
        setOperation('testing');
        const description = await test(normalized);
        if (generation.current === version) {
          setTestedSignature(signature);
          setFailedTestSignature(null);
          setResult({ ok: true, text: description });
        }
        return;
      }

      const needsAutoTest = isDuplicating
        && normalized.modelId !== sourceModel!.modelId.trim()
        && testedSignature !== signature
        && failedTestSignature !== signature;

      if (needsAutoTest) {
        setOperation('testing');
        try {
          await test(normalized);
          if (generation.current !== version) return;
          setTestedSignature(signature);
          setFailedTestSignature(null);
        } catch (testError) {
          if (generation.current === version) {
            setTestedSignature(null);
            setFailedTestSignature(signature);
            setResult({
              ok: false,
              text: sanitizeModelError(testError, activeSecrets, t.modelForm.testErrorFallback),
            });
          }
          return;
        }
      }

      setOperation('saving');
      const saved = await save(normalized);
      if (generation.current === version) {
        clearRevealedKey();
        onSaved(saved);
      }
    } catch (error) {
      if (generation.current === version) {
        if (kind === 'testing') {
          setTestedSignature(null);
          setFailedTestSignature(signature);
        }
        const fallback = kind === 'testing' ? t.modelForm.testErrorFallback : t.modelForm.saveErrorFallback;
        setResult({ ok: false, text: sanitizeModelError(error, activeSecrets, fallback) });
      }
    } finally {
      if (inFlight.current === version) inFlight.current = null;
      if (generation.current === version) setOperation('idle');
    }
  }

  const overlayTitle = isEditing
    ? t.modelForm.editTitle
    : isDuplicating
      ? t.modelForm.duplicateTitle
      : t.modelForm.addTitle;

  const apiKeyPlaceholder = canKeepExistingKey
    ? (isDuplicating ? t.modelForm.apiKeySharePlaceholder : t.modelForm.apiKeyKeepPlaceholder)
    : t.modelForm.apiKeyEnterPlaceholder;

  const submitLabel = operation === 'saving'
    ? t.modelForm.saving
    : saveAnywayArmed
      ? t.modelForm.saveAnyway
      : isEditing
        ? t.modelForm.saveChanges
        : isDuplicating
          ? t.modelForm.saveDuplicate
          : t.modelForm.saveAndUse;

  return <Overlay title={overlayTitle} onClose={() => { clearRevealedKey(); onClose(); }}>
    <form className="model-form scroll-area" noValidate onSubmit={(event) => { event.preventDefault(); void submit('saving'); }}>
      <fieldset><legend>{t.modelForm.providerLegend}</legend><div className="provider-grid" role="group" aria-label={t.modelForm.providerLegend}>
        {providers.map((item) => <button key={item.id} type="button" disabled={operation === 'saving'} aria-pressed={input.provider === item.id}
          onClick={() => changeProvider(item.id)}>{t.providers[item.id]}</button>)}
      </div></fieldset>
      <label>{t.modelForm.apiFormatLabel}
        <div className="select-field">
          <select aria-label={t.modelForm.apiFormatLabel} disabled={operation === 'saving'} value={effectiveFormat} onChange={(event) => changeApiFormat(event.target.value as ApiFormatId)}>
            {apiFormats.map((format) => <option key={format.id} value={format.id}>{t.apiFormats[format.id].label}</option>)}
          </select>
          <Icon name="down" />
        </div>
      </label>
      <label>{t.modelForm.displayNameLabel}<input disabled={operation === 'saving'} value={input.name} onChange={(event) => setInput({ ...input, name: event.target.value })} placeholder={t.modelForm.displayNamePlaceholder} /></label>
      <label>{t.modelForm.modelIdLabel}<input className="mono" data-autofocus={isDuplicating ? '' : undefined} disabled={operation === 'saving'} value={input.modelId} onChange={(event) => change({ ...input, modelId: event.target.value })} placeholder={provider.model} required /></label>
      {provider.needsUrl && <label>{t.modelForm.baseUrlLabel}<input className="mono" disabled={operation === 'saving'} value={input.baseUrl} onChange={(event) => change({ ...input, baseUrl: event.target.value })} placeholder={provider.url} required /></label>}
      {provider.needsKey && (
        <div className="form-field">
          <div className="label-row api-key-label-row">
            <label htmlFor={apiKeyInputId} className="label-row">
              <span>API Key</span>
              <HelpTip align="start" label={t.modelForm.apiKeyTipLabel} text={t.modelForm.apiKeyTipText} />
            </label>
            {showKeyHint && <span className="mono model-key-hint">{effectiveKeyHint}</span>}
          </div>
          <div className="api-key-input-row">
            <input
              id={apiKeyInputId}
              className="mono"
              disabled={operation === 'saving'}
              type="password"
              value={input.apiKey}
              onChange={(event) => change({ ...input, apiKey: event.target.value })}
              autoComplete="off"
              spellCheck={false}
              placeholder={apiKeyPlaceholder}
              required={!canKeepExistingKey}
            />
            {canRevealSavedKey && (
              <IconButton
                icon={revealedApiKey !== null ? 'eyeOff' : 'eye'}
                className={`api-key-reveal-button ${revealedApiKey !== null ? 'is-active' : ''}`}
                disabled={operation === 'saving'}
                aria-pressed={revealedApiKey !== null}
                label={
                  revealing
                    ? t.modelForm.revealingApiKeyLabel
                    : revealedApiKey !== null
                      ? t.modelForm.hideApiKeyLabel
                      : t.modelForm.revealApiKeyLabel
                }
                onClick={() => void toggleRevealApiKey()}
              />
            )}
          </div>
          {revealedApiKey !== null && (
            <div className="revealed-api-key mono" role="region" aria-label={t.modelForm.revealedKeyReadonlyLabel}>
              {revealedApiKey}
            </div>
          )}
          {revealNotice && revealedApiKey === null && (
            <p className={`field-inline-note ${revealNotice.danger ? 'danger' : 'muted'}`}>{revealNotice.text}</p>
          )}
          {showEndpointChangedKeyNotice && (
            <p className="field-inline-note">{t.modelForm.endpointChangedKeyNotice}</p>
          )}
          {showSharedScopeOptions && (
            <div className="shared-key-scope" role="radiogroup">
              <label className="shared-key-option">
                <input
                  type="radio"
                  name="shared-key-update-scope"
                  value="updateAll"
                  disabled={operation === 'saving'}
                  checked={sharedScope === 'updateAll'}
                  onChange={() => setSharedScope('updateAll')}
                />
                <span>{t.modelForm.sharedKeyUpdateAll(sharedPeerCount)}</span>
              </label>
              <label className="shared-key-option">
                <input
                  type="radio"
                  name="shared-key-update-scope"
                  value="detachCurrent"
                  disabled={operation === 'saving'}
                  checked={sharedScope === 'detachCurrent'}
                  onChange={() => setSharedScope('detachCurrent')}
                />
                <span>{t.modelForm.sharedKeyDetachCurrent}</span>
              </label>
            </div>
          )}
        </div>
      )}
      <div className="model-token-grid">
        <label>
          <span className="label-row">{t.modelForm.contextWindowLabel}<HelpTip align="start" label={t.modelForm.contextWindowTipLabel} text={t.modelForm.contextWindowTipText(defaultLimits.contextWindow)} /></span>
          <input className="mono" aria-label={t.modelForm.contextWindowLabel} inputMode="numeric" disabled={operation === 'saving'} value={contextWindowText} onChange={(event) => changeTokens(event.target.value, maxOutputTokensText)} placeholder={t.modelForm.defaultPlaceholder(defaultLimits.contextWindow)} />
        </label>
        <label>
          <span className="label-row">{t.modelForm.maxOutputTokensLabel}<HelpTip align="end" label={t.modelForm.maxOutputTokensTipLabel} text={t.modelForm.maxOutputTokensTipText(defaultLimits.maxOutputTokens)} /></span>
          <input className="mono" aria-label={t.modelForm.maxOutputTokensLabel} inputMode="numeric" disabled={operation === 'saving'} value={maxOutputTokensText} onChange={(event) => changeTokens(contextWindowText, event.target.value)} placeholder={t.modelForm.defaultPlaceholder(defaultLimits.maxOutputTokens)} />
        </label>
      </div>
      {result && <p role="status" className={`form-result ${result.ok ? 'success' : 'danger'}`}>{result.text}</p>}
      <div className="form-actions"><button type="button" className="secondary" disabled={operation !== 'idle'} onClick={() => void submit('testing')}>
        {operation === 'testing' && <Icon name="refresh" className="spinning" />}{operation === 'testing' ? t.modelForm.testing : t.modelForm.testConnection}</button>
        <div className="row"><button type="button" onClick={() => { clearRevealedKey(); onClose(); }}>{t.common.cancel}</button><button type="submit" className="primary" disabled={operation !== 'idle'}>{submitLabel}</button></div>
      </div>
    </form>
  </Overlay>;
}
