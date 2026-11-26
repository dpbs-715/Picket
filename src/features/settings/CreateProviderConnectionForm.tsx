import { useEffect, useId, useRef, useState } from 'react';
import type {
  CreateProviderConnectionInput,
  RemoteModelOption,
} from '../../../shared/model-protocol';
import { useI18n } from '../../i18n';
import { HelpTip, Icon, Overlay } from '../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../ui/motion';
import { CreateProviderAdvancedFields } from './CreateProviderAdvancedFields';
import { ModelIdField } from './ModelIdField';
import {
  newProviderConnectionInput,
  normalizeCreateProviderConnection,
  providerFor,
  providers,
  resolveDefaultTokenLimits,
  validateBaseUrl,
  validateCreateProviderConnection,
} from './modelProviders';
import type { ApiFormatId } from './modelProviders';
import {
  parseOptionalPositiveInt,
  sanitizeModelError,
  type CreateProviderConnectionFormProps,
} from './providerConnectionFormUtils';

export function CreateProviderConnectionForm({
  keepActiveModel,
  overlayVariant = 'drill',
  covered,
  depth,
  onBack,
  backLabel,
  closeDisabled,
  onClose,
  onCreated,
  test,
  listModels,
  create,
}: CreateProviderConnectionFormProps) {
  const { locale, t } = useI18n();
  const instantMotion = useInstantMotion();
  const apiKeyInputId = useId();
  const [input, setInput] = useState<CreateProviderConnectionInput>(() =>
    newProviderConnectionInput('anthropic', locale),
  );
  const [contextWindowText, setContextWindowText] = useState<string>(() =>
    String(newProviderConnectionInput('anthropic', locale).contextWindow ?? ''),
  );
  const [maxOutputTokensText, setMaxOutputTokensText] = useState<string>(() =>
    String(newProviderConnectionInput('anthropic', locale).maxOutputTokens ?? ''),
  );
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [failedSignature, setFailedSignature] = useState<string | null>(null);
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [fetchedModels, setFetchedModels] = useState<RemoteModelOption[]>([]);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [fetchStatus, setFetchStatus] = useState<string | null>(null);

  const generation = useRef(0);
  const fetchGeneration = useRef(0);
  const inFlight = useRef<number | null>(null);
  const autoLimitsRef = useRef<{ contextWindow: number; maxOutputTokens: number }>(
    resolveDefaultTokenLimits('anthropic', 'anthropic-messages', providerFor('anthropic').model),
  );
  const autoModelNameRef = useRef<string>(providerFor('anthropic').name);

  useEffect(() => () => {
    generation.current += 1;
    fetchGeneration.current += 1;
  }, []);

  const provider = providerFor(input.provider);
  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const defaultLimits = resolveDefaultTokenLimits(input.provider, effectiveFormat, input.modelId);

  function resetAsyncState() {
    generation.current += 1;
    inFlight.current = null;
    setFailedSignature(null);
    setResult(null);
    setOperation('idle');
  }

  function resetFetchedModels() {
    fetchGeneration.current += 1;
    setFetchingModels(false);
    setFetchedModels([]);
    setFetchError(null);
    setFetchStatus(null);
  }

  function change(next: CreateProviderConnectionInput, clearFetched = false) {
    resetAsyncState();
    if (clearFetched) resetFetchedModels();
    setInput(next);
  }

  function applyAutoLimits(
    nextLimits: { contextWindow: number; maxOutputTokens: number },
    baseInput: CreateProviderConnectionInput,
  ) {
    const prevLimits = autoLimitsRef.current;
    const useDefaultCtx = !contextWindowText.trim() || contextWindowText.trim() === String(prevLimits.contextWindow);
    const useDefaultOut = !maxOutputTokensText.trim() || maxOutputTokensText.trim() === String(prevLimits.maxOutputTokens);
    const nextCtx = useDefaultCtx ? nextLimits.contextWindow : baseInput.contextWindow;
    const nextOut = useDefaultOut ? nextLimits.maxOutputTokens : baseInput.maxOutputTokens;
    autoLimitsRef.current = nextLimits;
    resetAsyncState();
    setInput({ ...baseInput, contextWindow: nextCtx, maxOutputTokens: nextOut });
    if (useDefaultCtx) setContextWindowText(String(nextLimits.contextWindow));
    if (useDefaultOut) setMaxOutputTokensText(String(nextLimits.maxOutputTokens));
  }

  function changeModelId(nextModelId: string) {
    const nextDefaults = resolveDefaultTokenLimits(input.provider, effectiveFormat, nextModelId);
    applyAutoLimits(nextDefaults, { ...input, modelId: nextModelId });
  }

  function selectFetchedModel(option: RemoteModelOption) {
    const presetDefaults = resolveDefaultTokenLimits(input.provider, effectiveFormat, option.id);
    const nextCtx = option.contextWindow ?? presetDefaults.contextWindow;
    const rawOut = option.maxOutputTokens ?? presetDefaults.maxOutputTokens;
    const nextOut = Math.min(rawOut, nextCtx);
    const currentName = input.modelName.trim();
    const shouldUpdateName = !currentName || currentName === autoModelNameRef.current;
    const nextName = shouldUpdateName ? (option.displayName ?? '') : input.modelName;
    if (shouldUpdateName) autoModelNameRef.current = nextName;
    applyAutoLimits(
      { contextWindow: nextCtx, maxOutputTokens: nextOut },
      { ...input, modelId: option.id, modelName: nextName },
    );
  }

  function changeProvider(nextProviderId: CreateProviderConnectionInput['provider']) {
    const base = newProviderConnectionInput(nextProviderId, locale);
    const nextLimits = resolveDefaultTokenLimits(nextProviderId, base.apiFormat, base.modelId);
    autoLimitsRef.current = nextLimits;
    autoModelNameRef.current = base.modelName;
    resetAsyncState();
    resetFetchedModels();
    setInput(base);
    setContextWindowText(String(base.contextWindow ?? ''));
    setMaxOutputTokensText(String(base.maxOutputTokens ?? ''));
  }

  function changeApiFormat(nextFormat: ApiFormatId) {
    const nextDefaults = resolveDefaultTokenLimits(input.provider, nextFormat, input.modelId);
    resetFetchedModels();
    applyAutoLimits(nextDefaults, { ...input, apiFormat: nextFormat });
  }

  async function handleFetchModels() {
    if (!listModels || fetchingModels || operation === 'saving') return;
    if (provider.needsUrl) {
      const urlErr = validateBaseUrl(input.baseUrl, locale);
      if (urlErr) {
        setFetchError(urlErr);
        setFetchStatus(null);
        return;
      }
    }
    if (provider.needsKey && !input.apiKey.trim()) {
      setFetchError(t.modelForm.validation.missingApiKey);
      setFetchStatus(null);
      return;
    }
    const version = ++fetchGeneration.current;
    setFetchingModels(true);
    setFetchError(null);
    setFetchStatus(null);
    try {
      const models = await listModels({
        kind: 'newConnection',
        provider: input.provider,
        baseUrl: provider.needsUrl ? input.baseUrl.trim() : '',
        apiKey: provider.needsKey ? input.apiKey.trim() : '',
        apiFormat: effectiveFormat,
        locale,
      });
      if (fetchGeneration.current !== version) return;
      setFetchedModels(models);
      setFetchStatus(models.length > 0 ? null : t.modelForm.fetchedModelsEmpty);
    } catch (err) {
      if (fetchGeneration.current !== version) return;
      setFetchError(sanitizeModelError(err, [input.apiKey], t.modelForm.fetchModelsErrorFallback));
    } finally {
      if (fetchGeneration.current === version) setFetchingModels(false);
    }
  }

  function buildNormalized(): { normalized?: CreateProviderConnectionInput; error?: string } {
    const parsedCtx = parseOptionalPositiveInt(contextWindowText);
    if (parsedCtx === 'invalid') {
      setAdvancedOpen(true);
      return { error: t.modelForm.validation.tokenLimit('context_range') };
    }
    const parsedOut = parseOptionalPositiveInt(maxOutputTokensText);
    if (parsedOut === 'invalid') {
      setAdvancedOpen(true);
      return { error: t.modelForm.validation.tokenLimit('output_range') };
    }
    const candidate: CreateProviderConnectionInput = {
      ...input,
      contextWindow: parsedCtx ?? defaultLimits.contextWindow,
      maxOutputTokens: parsedOut ?? defaultLimits.maxOutputTokens,
      ...(keepActiveModel ? { keepActiveModel: true } : {}),
      locale,
    };
    const error = validateCreateProviderConnection(candidate, locale);
    if (error) return { error };
    return { normalized: normalizeCreateProviderConnection(candidate, locale) };
  }

  const currentSignature = JSON.stringify([
    input.provider,
    input.baseUrl.trim(),
    input.apiKey.trim(),
    input.modelId.trim(),
    effectiveFormat,
    contextWindowText.trim(),
    maxOutputTokensText.trim(),
  ]);

  async function submit(kind: 'testing' | 'saving') {
    if (inFlight.current !== null) return;
    const { normalized, error } = buildNormalized();
    if (error || !normalized) {
      setResult({ ok: false, text: error ?? t.modelForm.saveErrorFallback });
      return;
    }
    const version = ++generation.current;
    inFlight.current = version;
    setResult(null);

    try {
      if (kind === 'testing') {
        setOperation('testing');
        const summary = await test({
          kind: 'newConnection',
          provider: normalized.provider,
          baseUrl: normalized.baseUrl,
          apiKey: normalized.apiKey,
          modelId: normalized.modelId,
          apiFormat: normalized.apiFormat,
          contextWindow: normalized.contextWindow,
          maxOutputTokens: normalized.maxOutputTokens,
          locale,
        });
        if (generation.current === version) {
          setFailedSignature(null);
          setResult({ ok: true, text: summary });
        }
        return;
      }

      setOperation('saving');
      const reply = await create(normalized);
      if (generation.current === version) {
        onCreated(reply);
      }
    } catch (err) {
      if (generation.current === version) {
        if (kind === 'testing') setFailedSignature(currentSignature);
        const fallback = kind === 'testing' ? t.modelForm.testErrorFallback : t.modelForm.saveErrorFallback;
        setResult({ ok: false, text: sanitizeModelError(err, [input.apiKey], fallback) });
      }
    } finally {
      if (inFlight.current === version) inFlight.current = null;
      if (generation.current === version) setOperation('idle');
    }
  }

  const submitLabel = operation === 'saving'
    ? t.modelForm.saving
    : failedSignature === currentSignature
      ? t.modelForm.saveAnyway
      : keepActiveModel
        ? t.modelForm.saveModel
        : t.modelForm.saveAndUse;

  return (
    <Overlay
      title={t.modelForm.addProviderTitle}
      covered={covered}
      depth={depth}
      onBack={onBack}
      backLabel={backLabel}
      closeDisabled={closeDisabled}
      onClose={onClose}
      variant={overlayVariant}
    >
      <form
        className="model-form"
        data-form-mode="create-provider"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void submit('saving');
        }}
      >
        <div className="model-form-body scroll-area">
          <fieldset>
            <legend>{t.modelForm.providerLegend}</legend>
            <div className="provider-grid" role="group" aria-label={t.modelForm.providerLegend}>
              {providers.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  disabled={operation === 'saving'}
                  aria-pressed={input.provider === item.id}
                  onClick={() => changeProvider(item.id)}
                >
                  {t.providers[item.id]}
                </button>
              ))}
            </div>
          </fieldset>

          <div className="form-section-label">{t.modelForm.connectionSectionHeading}</div>
          <label>
            {t.modelForm.connectionNameLabel}
            <input
              data-field="connection-name"
              disabled={operation === 'saving'}
              value={input.connectionName}
              onChange={(event) => setInput({ ...input, connectionName: event.target.value })}
              placeholder={t.modelForm.connectionNamePlaceholder}
            />
          </label>
          {provider.needsUrl && (
            <label>
              {t.modelForm.baseUrlLabel}
              <input
                className="mono"
                data-field="base-url"
                disabled={operation === 'saving'}
                value={input.baseUrl}
                onChange={(event) => change({ ...input, baseUrl: event.target.value }, true)}
                placeholder={provider.url}
                required
              />
            </label>
          )}
          {provider.needsKey && (
            <div className="form-field">
              <div className="label-row api-key-label-row">
                <label htmlFor={apiKeyInputId} className="label-row">
                  <span>API Key</span>
                  <HelpTip align="start" label={t.modelForm.apiKeyTipLabel} text={t.modelForm.apiKeyTipText} />
                </label>
              </div>
              <input
                id={apiKeyInputId}
                className="mono"
                data-field="api-key"
                disabled={operation === 'saving'}
                type="password"
                value={input.apiKey}
                onChange={(event) => change({ ...input, apiKey: event.target.value }, true)}
                autoComplete="off"
                spellCheck={false}
                placeholder={t.modelForm.apiKeyEnterPlaceholder}
                required
              />
            </div>
          )}

          <div className="form-section-label">{t.modelForm.firstModelSectionHeading}</div>
          <ModelIdField
            value={input.modelId}
            placeholder={provider.model}
            disabled={operation === 'saving'}
            fetchedModels={fetchedModels}
            fetching={fetchingModels}
            fetchError={fetchError}
            fetchStatus={fetchStatus}
            canFetch={Boolean(listModels)}
            onValueChange={changeModelId}
            onSelectModel={selectFetchedModel}
            onFetchModels={() => void handleFetchModels()}
          />
          <label>
            {t.modelForm.displayNameLabel}
            <input
              data-field="model-name"
              disabled={operation === 'saving'}
              value={input.modelName}
              onChange={(event) => setInput({ ...input, modelName: event.target.value })}
              placeholder={t.modelForm.displayNamePlaceholder}
            />
          </label>

          <CreateProviderAdvancedFields
            open={advancedOpen}
            disabled={operation === 'saving'}
            effectiveFormat={effectiveFormat}
            contextWindowText={contextWindowText}
            maxOutputTokensText={maxOutputTokensText}
            defaultLimits={defaultLimits}
            onToggle={() => setAdvancedOpen((open) => !open)}
            onChangeApiFormat={changeApiFormat}
            onChangeContextWindow={(value) => {
              resetAsyncState();
              setContextWindowText(value);
            }}
            onChangeMaxOutputTokens={(value) => {
              resetAsyncState();
              setMaxOutputTokensText(value);
            }}
          />
        </div>

        <div className="overlay-footer">
          {result && (
            <m.p
              key={result.text}
              role="status"
              className={`form-result ${result.ok ? 'success' : 'danger'}`}
              initial={instantMotion ? false : { opacity: 0, y: 3 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
            >
              {result.text}
            </m.p>
          )}
          <div className="form-actions">
            <button
              type="button"
              className="secondary"
              disabled={operation !== 'idle'}
              onClick={() => void submit('testing')}
            >
              {operation === 'testing' && <Icon name="refresh" className="spinning" />}
              {operation === 'testing' ? t.modelForm.testing : t.modelForm.testConnection}
            </button>
            <div className="row">
              <button type="button" onClick={onBack ?? onClose}>{t.common.cancel}</button>
              <button type="submit" className="primary" disabled={operation !== 'idle'}>{submitLabel}</button>
            </div>
          </div>
        </div>
      </form>
    </Overlay>
  );
}
