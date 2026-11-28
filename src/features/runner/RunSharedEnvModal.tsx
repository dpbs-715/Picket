import { useState } from 'react';
import type { RunEnvEntry } from '../../../shared/run-protocol';
import { useI18n } from '../../i18n';
import { Icon, Overlay } from '../../ui/controls';
import { appendRunEnvPreset, formatRunEnvEntries, parseRunEnvText } from './runEnv';

interface Props {
  repositoryName: string;
  initialEnv: RunEnvEntry[];
  covered?: boolean;
  onClose(): void;
  onSave(env: RunEnvEntry[]): void;
}

const SHARED_ENV_PRESETS: ReadonlyArray<{ key: string; value: string; label: string }> = [
  { key: 'JAVA_HOME', value: '', label: 'JAVA_HOME' },
  { key: 'SPRING_PROFILES_ACTIVE', value: 'dev', label: 'SPRING_PROFILES_ACTIVE=dev' },
  { key: 'MAVEN_OPTS', value: '-Xmx1024m', label: 'MAVEN_OPTS=-Xmx1024m' },
];

export function RunSharedEnvModal(props: Props) {
  const { t } = useI18n();
  const [envText, setEnvText] = useState(() => formatRunEnvEntries(props.initialEnv));
  const [error, setError] = useState<string | null>(null);

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    const parsed = parseRunEnvText(envText);
    if (!parsed.ok) {
      setError(t.run.validationInvalidEnv);
      return;
    }
    setError(null);
    props.onSave(parsed.entries);
  }

  return (
    <Overlay
      title={t.run.sharedEnvModalTitle}
      covered={props.covered}
      onClose={props.onClose}
    >
      <div className="custom-action-modal">
        <div className="custom-action-shell-head">
          <div className="custom-action-repo-meta">
            <Icon name="folder" />
            <span className="mono truncate">{props.repositoryName}</span>
          </div>
        </div>

        <form className="custom-action-form" onSubmit={handleSubmit}>
          <div className="custom-action-shell-body scroll-area">
            <p className="muted small run-shared-env-note">
              {t.run.sharedEnvDescription}
            </p>

            <div className="run-env-presets" role="group" aria-label={t.run.sharedEnvPresetsLabel}>
              {SHARED_ENV_PRESETS.map((preset) => (
                <button
                  key={preset.key}
                  type="button"
                  className="secondary compact mono run-env-preset-chip"
                  onClick={() => {
                    setEnvText((prev) => appendRunEnvPreset(prev, preset.key, preset.value));
                    setError(null);
                  }}
                >
                  + {preset.label}
                </button>
              ))}
            </div>

            <label className="form-field">
              <span>{t.run.sharedEnvField}</span>
              <textarea
                data-autofocus
                className="mono"
                rows={7}
                value={envText}
                onChange={(e) => setEnvText(e.target.value)}
                placeholder={t.run.sharedEnvPlaceholder}
                spellCheck={false}
              />
            </label>

            {error && <p className="error-text small">{error}</p>}
          </div>

          <div className="overlay-footer">
            <div className="form-actions">
              {envText.trim().length > 0 ? (
                <button
                  type="button"
                  className="secondary"
                  onClick={() => {
                    setEnvText('');
                    setError(null);
                  }}
                >
                  {t.run.sharedEnvClearButton}
                </button>
              ) : <span />}
              <div className="row">
                <button type="button" className="secondary" onClick={props.onClose}>
                  {t.common.cancel}
                </button>
                <button type="submit" className="primary">
                  {t.run.sharedEnvSaveButton}
                </button>
              </div>
            </div>
          </div>
        </form>
      </div>
    </Overlay>
  );
}
