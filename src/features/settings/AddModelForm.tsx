import { useEffect, useRef, useState } from 'react';
import { Icon, Overlay } from '../../ui/controls';
import { apiFormats, editModelInput, newModelInput, normalizedModel, providerFor, providers, validateModel } from './modelProviders';
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

interface Props {
  initialModel?: ModelConfiguration;
  onClose(): void;
  onSaved(model: ModelConfiguration): void;
  test(input: ModelInput): Promise<string>;
  save(input: ModelInput): Promise<ModelConfiguration>;
}

export function AddModelForm({ initialModel, onClose, onSaved, test, save }: Props) {
  const [input, setInput] = useState<ModelInput>(() => initialModel ? editModelInput(initialModel) : newModelInput());
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);
  const provider = providerFor(input.provider);
  const canKeepExistingKey = !!initialModel && providerFor(initialModel.provider).needsKey;
  useEffect(() => () => { generation.current += 1; }, []);

  function change(next: ModelInput) {
    generation.current += 1;
    inFlight.current = null;
    setInput(next);
    setResult(null);
    setOperation('idle');
  }
  async function submit(kind: 'testing' | 'saving') {
    if (inFlight.current !== null) return;
    const error = validateModel(input, { canKeepExistingKey });
    if (error) { setResult({ ok: false, text: error }); return; }
    const version = ++generation.current;
    inFlight.current = version;
    setOperation(kind);
    setResult(null);
    try {
      if (kind === 'testing') {
        const description = await test(normalizedModel(input));
        if (generation.current === version) setResult({ ok: true, text: description });
      } else {
        const model = await save(normalizedModel(input));
        if (generation.current === version) onSaved(model);
      }
    } catch (error) {
      if (generation.current === version) {
        setResult({ ok: false, text: sanitizeModelError(error, input.apiKey, kind) });
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
          onClick={() => { const next = newModelInput(item.id); change(initialModel ? { ...next, id: initialModel.id } : next); }}>{item.label}</button>)}
      </div></fieldset>
      <label>接口格式
        <div className="select-field">
          <select aria-label="接口格式" disabled={operation === 'saving'} value={input.apiFormat ?? provider.defaultApiFormat} onChange={(event) => change({ ...input, apiFormat: event.target.value as ApiFormatId })}>
            {apiFormats.map((format) => <option key={format.id} value={format.id}>{format.label}</option>)}
          </select>
          <Icon name="down" />
        </div>
      </label>
      <label>显示名称<input disabled={operation === 'saving'} value={input.name} onChange={(event) => setInput({ ...input, name: event.target.value })} placeholder="为空时使用模型 ID" /></label>
      <label>模型 ID<input className="mono" disabled={operation === 'saving'} value={input.modelId} onChange={(event) => change({ ...input, modelId: event.target.value })} placeholder={provider.model} required /></label>
      {provider.needsUrl && <label>接口地址（Base URL）<input className="mono" disabled={operation === 'saving'} value={input.baseUrl} onChange={(event) => change({ ...input, baseUrl: event.target.value })} placeholder={provider.url} required /></label>}
      {provider.needsKey && <label>API Key<input className="mono" disabled={operation === 'saving'} type="password" value={input.apiKey} onChange={(event) => change({ ...input, apiKey: event.target.value })} autoComplete="off" spellCheck={false} placeholder={canKeepExistingKey ? '留空则保留原钥匙串密钥；输入新值则替换' : '输入密钥'} required={!canKeepExistingKey} /></label>}
      {result && <p role="status" className={`form-result ${result.ok ? 'success' : 'danger'}`}>{result.text}</p>}
      <p className="muted help">API Key 将存入 macOS 系统钥匙串，不写入普通配置或日志。示例模型 ID 不保证在当前账户可用。</p>
      <div className="form-actions"><button type="button" className="secondary" disabled={operation !== 'idle'} onClick={() => void submit('testing')}>
        {operation === 'testing' && <Icon name="refresh" className="spinning" />}{operation === 'testing' ? '测试中…' : '测试连接'}</button>
        <div className="row"><button type="button" onClick={onClose}>取消</button><button type="submit" className="primary" disabled={operation !== 'idle'}>{operation === 'saving' ? '保存中…' : initialModel ? '保存修改' : '保存并使用'}</button></div>
      </div>
    </form>
  </Overlay>;
}
