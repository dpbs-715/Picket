import { useEffect, useRef, useState } from 'react';
import { HelpTip, Icon, Overlay } from '../../ui/controls';
import {
  apiFormats,
  editModelInput,
  MODEL_TOKEN_LIMITS,
  newModelInput,
  normalizedModel,
  providerFor,
  providers,
  resolveDefaultTokenLimits,
  validateModel,
} from './modelProviders';
import type { ApiFormatId, ModelConfiguration, ModelInput } from './modelProviders';

function sanitizeModelError(error: unknown, apiKey: string, kind: 'testing' | 'saving'): string {
  const fallback = kind === 'testing'
    ? '连接测试未完成，请检查接口地址、模型 ID 或网络连接；输入已保留。'
    : '保存模型未完成，没有保存模型或密钥；输入已保留。';
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
        text: `上下文长度须为 ${MODEL_TOKEN_LIMITS.minContextWindow} 至 ${MODEL_TOKEN_LIMITS.maxContextWindow} 之间的整数`,
      });
      return;
    }
    const parsedMaxOut = parseOptionalPositiveInt(maxOutputTokensText);
    if (parsedMaxOut === 'invalid') {
      setResult({
        ok: false,
        text: `最大输出 Token 须为 ${MODEL_TOKEN_LIMITS.minMaxOutputTokens} 至 ${MODEL_TOKEN_LIMITS.maxMaxOutputTokens} 之间的整数`,
      });
      return;
    }
    const candidate: ModelInput = {
      ...input,
      contextWindow: parsedCtx ?? defaultLimits.contextWindow,
      maxOutputTokens: parsedMaxOut ?? defaultLimits.maxOutputTokens,
    };
    const error = validateModel(candidate, { canKeepExistingKey });
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
        setResult({ ok: false, text: sanitizeModelError(error, candidate.apiKey, kind) });
      }
    } finally {
      if (inFlight.current === version) inFlight.current = null;
      if (generation.current === version) setOperation('idle');
    }
  }

  return <Overlay title={initialModel ? '编辑模型' : '添加模型'} onClose={onClose}>
    <form className="model-form scroll-area" onSubmit={(event) => { event.preventDefault(); void submit('saving'); }}>
      <fieldset><legend>提供商</legend><div className="provider-grid" role="group" aria-label="提供商">
        {providers.map((item) => <button key={item.id} type="button" disabled={operation === 'saving'} aria-pressed={input.provider === item.id}
          onClick={() => changeProvider(item.id)}>{item.label}</button>)}
      </div></fieldset>
      <label>接口格式
        <div className="select-field">
          <select aria-label="接口格式" disabled={operation === 'saving'} value={effectiveFormat} onChange={(event) => changeApiFormat(event.target.value as ApiFormatId)}>
            {apiFormats.map((format) => <option key={format.id} value={format.id}>{format.label}</option>)}
          </select>
          <Icon name="down" />
        </div>
      </label>
      <label>显示名称<input disabled={operation === 'saving'} value={input.name} onChange={(event) => setInput({ ...input, name: event.target.value })} placeholder="为空时使用模型 ID" /></label>
      <label>模型 ID<input className="mono" disabled={operation === 'saving'} value={input.modelId} onChange={(event) => change({ ...input, modelId: event.target.value })} placeholder={provider.model} required /></label>
      {provider.needsUrl && <label>接口地址（Base URL）<input className="mono" disabled={operation === 'saving'} value={input.baseUrl} onChange={(event) => change({ ...input, baseUrl: event.target.value })} placeholder={provider.url} required /></label>}
      {provider.needsKey && <label><span className="label-row">API Key<HelpTip align="start" label="API Key 安全存储说明" text="密钥仅存入 macOS 系统钥匙串，不写入普通配置文件或日志。" /></span><input className="mono" disabled={operation === 'saving'} type="password" value={input.apiKey} onChange={(event) => change({ ...input, apiKey: event.target.value })} autoComplete="off" spellCheck={false} placeholder={canKeepExistingKey ? '留空则保留原钥匙串密钥；输入新值则替换' : '输入密钥'} required={!canKeepExistingKey} /></label>}
      <div className="model-token-grid">
        <label>
          <span className="label-row">上下文长度<HelpTip align="start" label="上下文长度说明" text={`模型支持的最大上下文窗口，留空回退默认值（${defaultLimits.contextWindow}）。`} /></span>
          <input className="mono" aria-label="上下文长度" inputMode="numeric" disabled={operation === 'saving'} value={contextWindowText} onChange={(event) => changeTokens(event.target.value, maxOutputTokensText)} placeholder={`默认 ${defaultLimits.contextWindow}`} />
        </label>
        <label>
          <span className="label-row">最大输出 Token<HelpTip align="end" label="最大输出 Token 说明" text={`单次回复（含推理思考）的最大输出上限，留空默认 ${defaultLimits.maxOutputTokens}，且不能超过上下文长度。`} /></span>
          <input className="mono" aria-label="最大输出 Token" inputMode="numeric" disabled={operation === 'saving'} value={maxOutputTokensText} onChange={(event) => changeTokens(contextWindowText, event.target.value)} placeholder={`默认 ${defaultLimits.maxOutputTokens}`} />
        </label>
      </div>
      {result && <p role="status" className={`form-result ${result.ok ? 'success' : 'danger'}`}>{result.text}</p>}
      <div className="form-actions"><button type="button" className="secondary" disabled={operation !== 'idle'} onClick={() => void submit('testing')}>
        {operation === 'testing' && <Icon name="refresh" className="spinning" />}{operation === 'testing' ? '测试中…' : '测试连接'}</button>
        <div className="row"><button type="button" onClick={onClose}>取消</button><button type="submit" className="primary" disabled={operation !== 'idle'}>{operation === 'saving' ? '保存中…' : initialModel ? '保存修改' : '保存并使用'}</button></div>
      </div>
    </form>
  </Overlay>;
}
