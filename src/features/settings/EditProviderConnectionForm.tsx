import { useEffect, useId, useRef, useState } from 'react';
import type { UpdateProviderConnectionInput } from '../../../shared/model-protocol';
import { useI18n } from '../../i18n';
import { HelpTip, Icon, IconButton, Overlay } from '../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../ui/motion';
import { editProviderConnectionInput, providerFor } from './modelProviders';
import {
  REVEAL_AUTO_HIDE_MS,
  sanitizeModelError,
  type EditProviderConnectionFormProps,
} from './providerConnectionFormUtils';

export function EditProviderConnectionForm({
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
}: EditProviderConnectionFormProps) {
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
