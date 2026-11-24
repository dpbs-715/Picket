import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../../i18n';
import { HelpTip, Icon, Overlay } from '../../ui/controls';
import {
  apiFormats,
  editModelInput,
  newModelInput,
  normalizedModel,
  providerFor,
  providers,
  resolveDefaultTokenLimits,
  validateModel,
} from './modelProviders';
import type { ApiFormatId, ModelConfiguration, ModelInput } from './modelProviders';

function sanitizeModelError(error: unknown, apiKey: string, fallback: string): string {
  let text = error instanceof Error ? error.message : typeof error === 'string' ? error : fallback;
  const candidates = [apiKey, apiKey.trim()].filter((item) => item.length > 0);
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

interface Props {
  initialModel?: ModelConfiguration;
  onClose(): void;
  onSaved(model: ModelConfiguration): void;
  test(input: ModelInput): Promise<string>;
  save(input: ModelInput): Promise<ModelConfiguration>;
}

export function AddModelForm({ initialModel, onClose, onSaved, test, save }: Props) {
  const { locale, t } = useI18n();
  const [input, setInput] = useState<ModelInput>(() => (initialModel ? editModelInput(initialModel) : newModelInput()));
  const [contextWindowText, setContextWindowText] = useState<string>(() => String((initialModel ? editModelInput(initialModel) : newModelInput()).contextWindow ?? ''));
  const [maxOutputTokensText, setMaxOutputTokensText] = useState<string>(() => String((initialModel ? editModelInput(initialModel) : newModelInput()).maxOutputTokens ?? ''));
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);
  const provider = providerFor(input.provider);
  const effectiveFormat = input.apiFormat ?? provider.defaultApiFormat;
  const defaultLimits = resolveDefaultTokenLimits(input.provider, effectiveFormat);
  const canKeepExistingKey = !!initialModel && providerFor(initialModel.provider).needsKey;
  useEffect(() => () => { generation.current += 1; }, []);

  function resetAsyncState() {
    generation.current += 1;
    inFlight.current = null;
    setResult(null);
    setOperation('idle');
  }

  function change(next: ModelInput) {
    resetAsyncState();
    setInput(next);
  }

  function changeProvider(nextProviderId: ModelInput['provider']) {
    const base = newModelInput(nextProviderId);
    const next = initialModel ? { ...base, id: initialModel.id } : base;
    resetAsyncState();
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
    const candidate: ModelInput = {
      ...input,
      contextWindow: parsedCtx ?? defaultLimits.contextWindow,
      maxOutputTokens: parsedMaxOut ?? defaultLimits.maxOutputTokens,
      locale,
    };
    const error = validateModel(candidate, { canKeepExistingKey, locale });
    if (error) { setResult({ ok: false, text: error }); return; }
    const version = ++generation.current;
    inFlight.current = version;
    setOperation(kind);
    setResult(null);
    try {
      if (kind === 'testing') {
        const description = await test(normalizedModel(candidate));
        if (generation.current === version) setResult({ ok: true, text: description });
      } else {
        const model = await save(normalizedModel(candidate));
        if (generation.current === version) onSaved(model);
      }
    } catch (error) {
      if (generation.current === version) {
        const fallback = kind === 'testing' ? t.modelForm.testErrorFallback : t.modelForm.saveErrorFallback;
        setResult({ ok: false, text: sanitizeModelError(error, candidate.apiKey, fallback) });
      }
    } finally {
      if (inFlight.current === version) inFlight.current = null;
      if (generation.current === version) setOperation('idle');
    }
  }

  return <Overlay title={initialModel ? t.modelForm.editTitle : t.modelForm.addTitle} onClose={onClose}>
    <form className="model-form scroll-area" onSubmit={(event) => { event.preventDefault(); void submit('saving'); }}>
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
      <label>{t.modelForm.modelIdLabel}<input className="mono" disabled={operation === 'saving'} value={input.modelId} onChange={(event) => change({ ...input, modelId: event.target.value })} placeholder={provider.model} required /></label>
      {provider.needsUrl && <label>{t.modelForm.baseUrlLabel}<input className="mono" disabled={operation === 'saving'} value={input.baseUrl} onChange={(event) => change({ ...input, baseUrl: event.target.value })} placeholder={provider.url} required /></label>}
      {provider.needsKey && <label><span className="label-row">API Key<HelpTip align="start" label={t.modelForm.apiKeyTipLabel} text={t.modelForm.apiKeyTipText} /></span><input className="mono" disabled={operation === 'saving'} type="password" value={input.apiKey} onChange={(event) => change({ ...input, apiKey: event.target.value })} autoComplete="off" spellCheck={false} placeholder={canKeepExistingKey ? t.modelForm.apiKeyKeepPlaceholder : t.modelForm.apiKeyEnterPlaceholder} required={!canKeepExistingKey} /></label>}
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
        <div className="row"><button type="button" onClick={onClose}>{t.common.cancel}</button><button type="submit" className="primary" disabled={operation !== 'idle'}>{operation === 'saving' ? t.modelForm.saving : initialModel ? t.modelForm.saveChanges : t.modelForm.saveAndUse}</button></div>
      </div>
    </form>
  </Overlay>;
}
