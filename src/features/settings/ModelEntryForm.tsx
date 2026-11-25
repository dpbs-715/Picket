import { useEffect, useRef, useState } from 'react';
import type { ModelSaveReply, SaveModelInput, TestConnectionInput } from '../../../shared/model-protocol';
import { useI18n } from '../../i18n';
import { HelpTip, Icon, Overlay, RotatingChevron, type OverlayVariant } from '../../ui/controls';
import { Collapse, MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../ui/motion';
import {
  apiFormats,
  editModelEntryInput,
  newModelEntryInput,
  normalizeModelEntry,
  providerFor,
  resolveDefaultTokenLimits,
  validateModelEntry,
} from './modelProviders';
import type { ApiFormatId, ModelConfiguration, ProviderConnection } from './modelProviders';

function parseOptionalPositiveInt(raw: string): number | 'invalid' | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  if (!/^\d+$/.test(trimmed)) return 'invalid';
  const parsed = Number(trimmed);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) return 'invalid';
  return parsed;
}

interface Props {
  overlayVariant?: OverlayVariant;
  connection: ProviderConnection;
  initialModel?: ModelConfiguration;
  onClose(): void;
  onSaved(reply: ModelSaveReply): void;
  test(input: TestConnectionInput): Promise<string>;
  save(input: SaveModelInput): Promise<ModelSaveReply>;
}

export function ModelEntryForm({
  overlayVariant = 'drill',
  connection,
  initialModel,
  onClose,
  onSaved,
  test,
  save,
}: Props) {
  const { locale, t } = useI18n();
  const instantMotion = useInstantMotion();
  const isEditing = !!initialModel;
  const provider = providerFor(connection.provider);

  const [input, setInput] = useState<SaveModelInput>(() =>
    initialModel ? editModelEntryInput(initialModel) : newModelEntryInput(connection),
  );
  const [contextWindowText, setContextWindowText] = useState<string>(() => {
    const seeded = initialModel ? editModelEntryInput(initialModel) : newModelEntryInput(connection);
    return String(seeded.contextWindow ?? '');
  });
  const [maxOutputTokensText, setMaxOutputTokensText] = useState<string>(() => {
    const seeded = initialModel ? editModelEntryInput(initialModel) : newModelEntryInput(connection);
    return String(seeded.maxOutputTokens ?? '');
  });
  const [advancedOpen, setAdvancedOpen] = useState<boolean>(() => {
    if (!initialModel) return false;
    const format = initialModel.apiFormat ?? provider.defaultApiFormat;
    const defaults = resolveDefaultTokenLimits(connection.provider, format);
    return format !== provider.defaultApiFormat
      || (initialModel.contextWindow !== undefined && initialModel.contextWindow !== defaults.contextWindow)
      || (initialModel.maxOutputTokens !== undefined && initialModel.maxOutputTokens !== defaults.maxOutputTokens);
  });
  const [failedSignature, setFailedSignature] = useState<string | null>(null);
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);

  useEffect(() => () => {
    generation.current += 1;
  }, []);

  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const defaultLimits = resolveDefaultTokenLimits(connection.provider, effectiveFormat);

  function resetAsyncState() {
    generation.current += 1;
    inFlight.current = null;
    setFailedSignature(null);
    setResult(null);
    setOperation('idle');
  }

  function changeApiFormat(nextFormat: ApiFormatId) {
    const prevDefaults = resolveDefaultTokenLimits(connection.provider, effectiveFormat);
    const nextDefaults = resolveDefaultTokenLimits(connection.provider, nextFormat);
    const useDefaultCtx = !contextWindowText.trim() || contextWindowText.trim() === String(prevDefaults.contextWindow);
    const useDefaultOut = !maxOutputTokensText.trim() || maxOutputTokensText.trim() === String(prevDefaults.maxOutputTokens);
    const nextCtx = useDefaultCtx ? nextDefaults.contextWindow : input.contextWindow;
    const nextOut = useDefaultOut ? nextDefaults.maxOutputTokens : input.maxOutputTokens;
    resetAsyncState();
    setInput({ ...input, apiFormat: nextFormat, contextWindow: nextCtx, maxOutputTokens: nextOut });
    if (useDefaultCtx) setContextWindowText(String(nextDefaults.contextWindow));
    if (useDefaultOut) setMaxOutputTokensText(String(nextDefaults.maxOutputTokens));
  }

  const currentSignature = JSON.stringify([
    connection.id,
    input.modelId.trim(),
    effectiveFormat,
    contextWindowText.trim(),
    maxOutputTokensText.trim(),
  ]);

  function buildNormalized(): { normalized?: SaveModelInput; error?: string } {
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
    const candidate: SaveModelInput = {
      ...input,
      connectionId: connection.id,
      contextWindow: parsedCtx ?? defaultLimits.contextWindow,
      maxOutputTokens: parsedOut ?? defaultLimits.maxOutputTokens,
      locale,
    };
    const error = validateModelEntry(connection.provider, candidate, locale);
    if (error) return { error };
    return { normalized: normalizeModelEntry(connection.provider, candidate) };
  }

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
          kind: 'connectionModel',
          connectionId: connection.id,
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
      const reply = await save(normalized);
      if (generation.current === version) {
        onSaved(reply);
      }
    } catch (err) {
      if (generation.current === version) {
        if (kind === 'testing') setFailedSignature(currentSignature);
        const message = err instanceof Error
          ? err.message
          : typeof err === 'string'
            ? err
            : kind === 'testing'
              ? t.modelForm.testErrorFallback
              : t.modelForm.saveErrorFallback;
        setResult({ ok: false, text: message });
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
      : isEditing
        ? t.modelForm.saveChanges
        : t.modelForm.saveModel;

  return (
    <Overlay title={isEditing ? t.modelForm.editModelTitle : t.modelForm.addModelTitle} onClose={onClose} variant={overlayVariant}>
      <form
        className="model-form scroll-area"
        data-form-mode={isEditing ? 'edit-model' : 'add-model'}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void submit('saving');
        }}
      >
        <div className="connection-identity-card">
          <div className="row min-width-zero">
            <strong>{connection.name}</strong>
            <span className="muted small">{t.providers[connection.provider]}</span>
            {connection.keyHint && <span className="mono model-key-hint">{connection.keyHint}</span>}
          </div>
          {connection.baseUrl && <p className="mono muted small truncate">{connection.baseUrl}</p>}
        </div>

        <label>
          {t.modelForm.modelIdLabel}
          <input
            className="mono"
            data-field="model-id"
            data-autofocus={!isEditing ? '' : undefined}
            disabled={operation === 'saving'}
            value={input.modelId}
            onChange={(event) => {
              resetAsyncState();
              setInput({ ...input, modelId: event.target.value });
            }}
            placeholder={provider.model}
            required
          />
        </label>
        <label>
          {t.modelForm.displayNameLabel}
          <input
            data-field="model-name"
            disabled={operation === 'saving'}
            value={input.name}
            onChange={(event) => setInput({ ...input, name: event.target.value })}
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
                <span className="label-row">
                  {t.modelForm.contextWindowLabel}
                  <HelpTip align="start" label={t.modelForm.contextWindowTipLabel} text={t.modelForm.contextWindowTipText(defaultLimits.contextWindow)} />
                </span>
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
                <span className="label-row">
                  {t.modelForm.maxOutputTokensLabel}
                  <HelpTip align="end" label={t.modelForm.maxOutputTokensTipLabel} text={t.modelForm.maxOutputTokensTipText(defaultLimits.maxOutputTokens)} />
                </span>
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
            <button type="button" onClick={onClose}>{t.common.cancel}</button>
            <button type="submit" className="primary" disabled={operation !== 'idle'}>{submitLabel}</button>
          </div>
        </div>
      </form>
    </Overlay>
  );
}
