import { useEffect, useId, useRef, useState } from 'react';
import type {
  CreateProviderConnectionInput,
  ListModelsInput,
  ProviderRevealKeyReply,
  ProviderSaveReply,
  ProviderUpdateReply,
  RemoteModelOption,
  TestConnectionInput,
  UpdateProviderConnectionInput,
} from '../../../shared/model-protocol';
import { useI18n } from '../../i18n';
import { HelpTip, Icon, IconButton, Overlay, RotatingChevron, type OverlayVariant } from '../../ui/controls';
import { Collapse, MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../ui/motion';
import { ModelIdField } from './ModelIdField';
import {
  apiFormats,
  editProviderConnectionInput,
  newProviderConnectionInput,
  normalizeCreateProviderConnection,
  providerFor,
  providers,
  resolveDefaultTokenLimits,
  validateBaseUrl,
  validateCreateProviderConnection,
} from './modelProviders';
import type { ApiFormatId, ModelConfiguration, ProviderConnection } from './modelProviders';

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

type CreateProps = {
  mode: 'create';
  keepActiveModel?: boolean;
  overlayVariant?: OverlayVariant;
  covered?: boolean;
  depth?: number;
  onBack?(): void;
  backLabel?: string;
  closeDisabled?: boolean;
  onClose(): void;
  onCreated(reply: ProviderSaveReply): void;
  test(input: TestConnectionInput): Promise<string>;
  listModels?(input: ListModelsInput): Promise<RemoteModelOption[]>;
  create(input: CreateProviderConnectionInput): Promise<ProviderSaveReply>;
};

type EditProps = {
  mode: 'edit';
  overlayVariant?: OverlayVariant;
  covered?: boolean;
  depth?: number;
  onBack?(): void;
  backLabel?: string;
  closeDisabled?: boolean;
  connection: ProviderConnection;
  childModels: readonly ModelConfiguration[];
  onClose(): void;
  onUpdated(reply: ProviderUpdateReply): void;
  onKeyHintResolved?: (connectionId: string, keyHint: string) => void;
  test(input: TestConnectionInput): Promise<string>;
  update(input: UpdateProviderConnectionInput): Promise<ProviderUpdateReply>;
  revealApiKey?: (connectionId: string) => Promise<ProviderRevealKeyReply>;
};

export type ProviderConnectionFormProps = CreateProps | EditProps;

export function ProviderConnectionForm(props: ProviderConnectionFormProps) {
  if (props.mode === 'edit') {
    return <EditProviderConnectionForm {...props} />;
  }
  return <CreateProviderConnectionForm {...props} />;
}

function CreateProviderConnectionForm({
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
}: CreateProps) {
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

          <button
            type="button"
            className="advanced-toggle"
            aria-expanded={advancedOpen}
            onClick={() => setAdvancedOpen((open) => !open)}
          >
            <RotatingChevron open={advancedOpen} />
            <span>{t.modelForm.advancedToggle}</span>
            <span className="muted small mono truncate">
              {t.apiFormats[effectiveFormat].short}
            </span>
          </button>

          <Collapse open={advancedOpen}>
            <div className="advanced-fields">
              <label>
                {t.modelForm.apiFormatLabel}
                <div className="select-field">
                  <select
                    aria-label={t.modelForm.apiFormatLabel}
                    disabled={operation === 'saving'}
                    value={effectiveFormat}
                    onChange={(event) => changeApiFormat(event.target.value as ApiFormatId)}
                  >
                    {apiFormats.map((format) => (
                      <option key={format.id} value={format.id}>{t.apiFormats[format.id].label}</option>
                    ))}
                  </select>
                  <Icon name="down" />
                </div>
              </label>
              <div className="model-token-grid">
                <label>
                  <span>{t.modelForm.contextWindowLabel}</span>
                  <input
                    className="mono"
                    aria-label={t.modelForm.contextWindowLabel}
                    inputMode="numeric"
                    disabled={operation === 'saving'}
                    value={contextWindowText}
                    onChange={(event) => {
                      resetAsyncState();
                      setContextWindowText(event.target.value);
                    }}
                    placeholder={t.modelForm.defaultPlaceholder(defaultLimits.contextWindow)}
                  />
                </label>
                <label>
                  <span>{t.modelForm.maxOutputTokensLabel}</span>
                  <input
                    className="mono"
                    aria-label={t.modelForm.maxOutputTokensLabel}
                    inputMode="numeric"
                    disabled={operation === 'saving'}
                    value={maxOutputTokensText}
                    onChange={(event) => {
                      resetAsyncState();
                      setMaxOutputTokensText(event.target.value);
                    }}
                    placeholder={t.modelForm.defaultPlaceholder(defaultLimits.maxOutputTokens)}
                  />
                </label>
              </div>
            </div>
          </Collapse>
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

function EditProviderConnectionForm({
  overlayVariant = 'drill',
  covered,
  depth,
  onBack,
  backLabel,
  closeDisabled,
  connection,
  childModels,
  onClose,
  onUpdated,
  onKeyHintResolved,
  test,
  update,
  revealApiKey,
}: EditProps) {
  const { locale, t } = useI18n();
  const instantMotion = useInstantMotion();
  const apiKeyInputId = useId();
  const [input, setInput] = useState<UpdateProviderConnectionInput>(() =>
    editProviderConnectionInput(connection),
  );
  const [localKeyHint, setLocalKeyHint] = useState<string | undefined>(() => connection.keyHint);
  const [revealedApiKey, setRevealedApiKey] = useState<string | null>(null);
  const [revealing, setRevealing] = useState(false);
  const [revealNotice, setRevealNotice] = useState<{ danger: boolean; text: string } | null>(null);
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);
  const revealGeneration = useRef(0);

  const provider = providerFor(connection.provider);
  const effectiveKeyHint = localKeyHint ?? connection.keyHint;
  const canRevealSavedKey = provider.needsKey && !!revealApiKey;
  const canTestConnection = childModels.length > 0;

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
    setLocalKeyHint(connection.keyHint);
  }, [connection.id]);

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

  async function toggleRevealApiKey() {
    if (!revealApiKey) return;
    if (revealedApiKey !== null || revealing) {
      clearRevealedKey();
      return;
    }
    const token = ++revealGeneration.current;
    setRevealing(true);
    setRevealNotice(null);
    try {
      const reply = await revealApiKey(connection.id);
      if (revealGeneration.current !== token) return;
      if (reply.outcome === 'authenticated') {
        setRevealedApiKey(reply.apiKey);
        setLocalKeyHint(reply.keyHint);
        if (connection.keyHint !== reply.keyHint) {
          onKeyHintResolved?.(connection.id, reply.keyHint);
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

  async function submit(kind: 'testing' | 'saving') {
    if (inFlight.current !== null) return;
    const version = ++generation.current;
    inFlight.current = version;
    setResult(null);
    const activeSecrets = [input.apiKey, revealedApiKey ?? ''];

    try {
      if (kind === 'testing') {
        setOperation('testing');
        const summary = await test({
          kind: 'existingConnection',
          connectionId: connection.id,
          apiKey: provider.needsKey ? input.apiKey.trim() : '',
          ...(childModels[0] ? { modelId: childModels[0].modelId } : {}),
          locale,
        });
        if (generation.current === version) {
          setResult({ ok: true, text: summary });
        }
        return;
      }

      setOperation('saving');
      const reply = await update({
        connectionId: connection.id,
        name: input.name.trim() || t.providers[connection.provider],
        apiKey: provider.needsKey ? input.apiKey.trim() : '',
        locale,
      });
      if (generation.current === version) {
        clearRevealedKey();
        onUpdated(reply);
      }
    } catch (error) {
      if (generation.current === version) {
        const fallback = kind === 'testing' ? t.modelForm.testErrorFallback : t.modelForm.saveErrorFallback;
        setResult({ ok: false, text: sanitizeModelError(error, activeSecrets, fallback) });
      }
    } finally {
      if (inFlight.current === version) inFlight.current = null;
      if (generation.current === version) setOperation('idle');
    }
  }

  const handleBackOrClose = () => {
    clearRevealedKey();
    if (onBack) onBack();
    else onClose();
  };

  return (
    <Overlay
      title={t.modelForm.editConnectionTitle}
      covered={covered}
      depth={depth}
      onBack={onBack ? handleBackOrClose : undefined}
      backLabel={backLabel}
      closeDisabled={closeDisabled}
      onClose={() => { clearRevealedKey(); onClose(); }}
      variant={overlayVariant}
    >
      <form
        className="model-form"
        data-form-mode="edit-provider"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void submit('saving');
        }}
      >
        <div className="model-form-body scroll-area">
          <div className="connection-identity-card">
            <div className="row min-width-zero">
              <strong>{t.providers[connection.provider]}</strong>
              {connection.baseUrl && <span className="mono muted small truncate">{connection.baseUrl}</span>}
            </div>
          </div>

          <label>
            {t.modelForm.connectionNameLabel}
            <input
              data-field="connection-name"
              disabled={operation === 'saving'}
              value={input.name}
              onChange={(event) => {
                setResult(null);
                setInput({ ...input, name: event.target.value });
              }}
              placeholder={t.modelForm.connectionNamePlaceholder}
            />
          </label>

          {provider.needsKey && (
            <div className="form-field">
              <div className="label-row api-key-label-row">
                <label htmlFor={apiKeyInputId} className="label-row">
                  <span>API Key</span>
                  <HelpTip align="start" label={t.modelForm.apiKeyTipLabel} text={t.modelForm.apiKeyTipText} />
                </label>
                {effectiveKeyHint && <span className="mono model-key-hint">{effectiveKeyHint}</span>}
              </div>
              <div className="api-key-input-row">
                <input
                  id={apiKeyInputId}
                  className="mono"
                  data-field="api-key"
                  disabled={operation === 'saving'}
                  type="password"
                  value={input.apiKey}
                  onChange={(event) => {
                    setResult(null);
                    setInput({ ...input, apiKey: event.target.value });
                  }}
                  autoComplete="off"
                  spellCheck={false}
                  placeholder={t.modelForm.apiKeyKeepPlaceholder}
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
                <m.div
                  className="revealed-api-key mono"
                  role="region"
                  aria-label={t.modelForm.revealedKeyReadonlyLabel}
                  initial={instantMotion ? false : { opacity: 0, y: -3 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
                >
                  {revealedApiKey}
                </m.div>
              )}
              {revealNotice && revealedApiKey === null && (
                <p className={`field-inline-note ${revealNotice.danger ? 'danger' : 'muted'}`}>{revealNotice.text}</p>
              )}
              {input.apiKey.trim().length > 0 && childModels.length > 0 && (
                <p className="field-inline-note">{t.modelForm.rotateKeyImpactNote(childModels.length)}</p>
              )}
            </div>
          )}

          {!canTestConnection && <p className="muted help">{t.modelForm.testNeedsModelHint}</p>}
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
              disabled={operation !== 'idle' || !canTestConnection}
              onClick={() => void submit('testing')}
            >
              {operation === 'testing' && <Icon name="refresh" className="spinning" />}
              {operation === 'testing' ? t.modelForm.testing : t.modelForm.testConnection}
            </button>
            <div className="row">
              <button type="button" onClick={handleBackOrClose}>{t.common.cancel}</button>
              <button type="submit" className="primary" disabled={operation !== 'idle'}>
                {operation === 'saving' ? t.modelForm.saving : t.modelForm.saveChanges}
              </button>
            </div>
          </div>
        </div>
      </form>
    </Overlay>
  );
}
