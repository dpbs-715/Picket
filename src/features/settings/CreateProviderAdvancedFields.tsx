import { useI18n } from '../../i18n';
import { Icon, RotatingChevron } from '../../ui/controls';
import { Collapse } from '../../ui/motion';
import { apiFormats, type ApiFormatId } from './modelProviders';

interface CreateProviderAdvancedFieldsProps {
  open: boolean;
  disabled: boolean;
  effectiveFormat: ApiFormatId;
  contextWindowText: string;
  maxOutputTokensText: string;
  defaultLimits: { contextWindow: number; maxOutputTokens: number };
  onToggle(): void;
  onChangeApiFormat(format: ApiFormatId): void;
  onChangeContextWindow(value: string): void;
  onChangeMaxOutputTokens(value: string): void;
}

export function CreateProviderAdvancedFields({
  open,
  disabled,
  effectiveFormat,
  contextWindowText,
  maxOutputTokensText,
  defaultLimits,
  onToggle,
  onChangeApiFormat,
  onChangeContextWindow,
  onChangeMaxOutputTokens,
}: CreateProviderAdvancedFieldsProps) {
  const { t } = useI18n();

  return (
    <>
      <button
        type="button"
        className="advanced-toggle"
        aria-expanded={open}
        onClick={onToggle}
      >
        <RotatingChevron open={open} />
        <span>{t.modelForm.advancedToggle}</span>
        <span className="muted small mono truncate">
          {t.apiFormats[effectiveFormat].short}
        </span>
      </button>

      <Collapse open={open}>
        <div className="advanced-fields">
          <label>
            {t.modelForm.apiFormatLabel}
            <div className="select-field">
              <select
                aria-label={t.modelForm.apiFormatLabel}
                disabled={disabled}
                value={effectiveFormat}
                onChange={(event) => onChangeApiFormat(event.target.value as ApiFormatId)}
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
                disabled={disabled}
                value={contextWindowText}
                onChange={(event) => onChangeContextWindow(event.target.value)}
                placeholder={t.modelForm.defaultPlaceholder(defaultLimits.contextWindow)}
              />
            </label>
            <label>
              <span>{t.modelForm.maxOutputTokensLabel}</span>
              <input
                className="mono"
                aria-label={t.modelForm.maxOutputTokensLabel}
                inputMode="numeric"
                disabled={disabled}
                value={maxOutputTokensText}
                onChange={(event) => onChangeMaxOutputTokens(event.target.value)}
                placeholder={t.modelForm.defaultPlaceholder(defaultLimits.maxOutputTokens)}
              />
            </label>
          </div>
        </div>
      </Collapse>
    </>
  );
}
