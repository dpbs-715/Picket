import { useEffect, useRef, useState } from 'react';
import { Icon, Overlay } from '../../ui/controls';
import { newModelInput, normalizedModel, providerFor, providers, validateModel } from './modelProviders';
import type { ModelConfiguration, ModelInput } from './modelProviders';

interface Props {
  onClose(): void;
  onSaved(model: ModelConfiguration): void;
  test(input: ModelInput): Promise<string>;
  save(input: ModelInput): Promise<ModelConfiguration>;
}

export function AddModelForm({ onClose, onSaved, test, save }: Props) {
  const [input, setInput] = useState(newModelInput);
  const [operation, setOperation] = useState<'idle' | 'testing' | 'saving'>('idle');
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);
  const provider = providerFor(input.provider);
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
    const error = validateModel(input);
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
    } catch {
      // Provider errors can echo the request (including its key). Until the
      // credential adapter supplies sanitized failures, do not render raw errors.
      if (generation.current === version) setResult({ ok: false, text: kind === 'testing'
        ? '连接未完成。当前版本尚未接入在线模型；未发送连接请求，输入已保留。'
        : '保存未完成。当前版本尚未接入系统钥匙串；没有保存模型或密钥，输入已保留。' });
    } finally {
      if (inFlight.current === version) inFlight.current = null;
      if (generation.current === version) setOperation('idle');
    }
  }

  return <Overlay title="添加模型" onClose={onClose}>
    <form className="model-form scroll-area" onSubmit={(event) => { event.preventDefault(); void submit('saving'); }}>
      <fieldset><legend>提供商</legend><div className="provider-grid" role="group" aria-label="提供商">
        {providers.map((item) => <button key={item.id} type="button" disabled={operation === 'saving'} aria-pressed={input.provider === item.id}
          onClick={() => change(newModelInput(item.id))}>{item.label}</button>)}
      </div></fieldset>
      <label>显示名称<input disabled={operation === 'saving'} value={input.name} onChange={(event) => setInput({ ...input, name: event.target.value })} placeholder="为空时使用模型 ID" /></label>
      <label>模型 ID<input className="mono" disabled={operation === 'saving'} value={input.modelId} onChange={(event) => change({ ...input, modelId: event.target.value })} placeholder={provider.model} required /></label>
      {provider.needsUrl && <label>接口地址（Base URL）<input className="mono" disabled={operation === 'saving'} value={input.baseUrl} onChange={(event) => change({ ...input, baseUrl: event.target.value })} placeholder={provider.url} required /></label>}
      {provider.needsKey && <label>API Key<input className="mono" disabled={operation === 'saving'} type="password" value={input.apiKey} onChange={(event) => change({ ...input, apiKey: event.target.value })} autoComplete="off" spellCheck={false} placeholder="输入密钥" required /></label>}
      {result && <p role="status" className={`form-result ${result.ok ? 'success' : 'danger'}`}>{result.text}</p>}
      <p className="muted help">模型连接与钥匙串保存在 M3 接入，当前请勿填写真实密钥。示例模型 ID 不保证可用。</p>
      <div className="form-actions"><button type="button" className="secondary" disabled={operation !== 'idle'} onClick={() => void submit('testing')}>
        {operation === 'testing' && <Icon name="refresh" className="spinning" />}{operation === 'testing' ? '测试中…' : '测试连接'}</button>
        <div className="row"><button type="button" onClick={onClose}>取消</button><button type="submit" className="primary" disabled={operation !== 'idle'}>{operation === 'saving' ? '保存中…' : '保存并使用'}</button></div>
      </div>
    </form>
  </Overlay>;
}
