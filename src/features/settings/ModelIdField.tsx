import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import type { RemoteModelOption } from '../../../shared/model-protocol';
import { useI18n } from '../../i18n';
import { Icon, RotatingChevron } from '../../ui/controls';
import { formatTokenCount, resolveModelPresetTokenLimits } from './modelProviders';

interface Props {
  value: string;
  placeholder: string;
  disabled?: boolean;
  autoFocus?: boolean;
  fetchedModels: readonly RemoteModelOption[];
  fetching: boolean;
  fetchError?: string | null;
  fetchStatus?: string | null;
  canFetch: boolean;
  onValueChange(nextValue: string): void;
  onSelectModel(option: RemoteModelOption): void;
  onFetchModels(): void;
}

export function ModelIdField({
  value,
  placeholder,
  disabled = false,
  autoFocus = false,
  fetchedModels,
  fetching,
  fetchError,
  fetchStatus,
  canFetch,
  onValueChange,
  onSelectModel,
  onFetchModels,
}: Props) {
  const { t } = useI18n();
  const inputId = useId();
  const containerRef = useRef<HTMLDivElement>(null);
  const inputWrapRef = useRef<HTMLDivElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [filterActive, setFilterActive] = useState(false);
  const [openUpward, setOpenUpward] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);

  useEffect(() => {
    if (fetchedModels.length > 0) {
      setPickerOpen(true);
      setFilterActive(false);
    } else {
      setPickerOpen(false);
      setFilterActive(false);
    }
  }, [fetchedModels]);

  useEffect(() => {
    if (!pickerOpen) return undefined;
    const handlePointerDown = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setPickerOpen(false);
      }
    };
    document.addEventListener('mousedown', handlePointerDown);
    return () => document.removeEventListener('mousedown', handlePointerDown);
  }, [pickerOpen]);

  useLayoutEffect(() => {
    if (!pickerOpen || !inputWrapRef.current) return;
    const wrapRect = inputWrapRef.current.getBoundingClientRect();
    const scrollHost = inputWrapRef.current.closest('.scroll-area');
    const hostRect = scrollHost?.getBoundingClientRect() ?? { top: 0, bottom: window.innerHeight };
    const spaceBelow = hostRect.bottom - wrapRect.bottom;
    const spaceAbove = wrapRect.top - hostRect.top;
    setOpenUpward(spaceBelow < 168 && spaceAbove > spaceBelow);
  }, [pickerOpen, fetchedModels.length]);

  const query = value.trim().toLowerCase();
  const exactMatched = query.length > 0 && fetchedModels.some((item) => item.id.toLowerCase() === query);
  const filtered = !filterActive || !query || exactMatched
    ? fetchedModels
    : fetchedModels.filter((item) => (
      item.id.toLowerCase().includes(query)
      || (item.displayName?.toLowerCase().includes(query) ?? false)
    ));

  useEffect(() => {
    if (!pickerOpen) {
      setActiveIndex(-1);
      return;
    }
    const selectedIdx = filtered.findIndex((item) => item.id === value.trim());
    setActiveIndex(selectedIdx >= 0 ? selectedIdx : filtered.length > 0 ? 0 : -1);
    if (selectedIdx >= 0 && pickerRef.current) {
      const target = pickerRef.current.querySelector<HTMLElement>('[aria-selected="true"]');
      target?.scrollIntoView?.({ block: 'nearest' });
    }
  }, [pickerOpen, value, filtered.length]);

  return (
    <div className="form-field model-id-field" ref={containerRef}>
      <div className="label-row model-id-label-row">
        <label htmlFor={inputId}>{t.modelForm.modelIdLabel}</label>
        {canFetch && (
          <button
            type="button"
            className="secondary compact model-fetch-button"
            data-action="fetch-models"
            disabled={disabled || fetching}
            onClick={onFetchModels}
          >
            <Icon name="refresh" className={fetching ? 'spinning' : undefined} />
            <span>
              {fetching
                ? t.modelForm.fetchingModels
                : fetchedModels.length > 0
                  ? t.modelForm.refreshModels
                  : t.modelForm.fetchModels}
            </span>
          </button>
        )}
      </div>
      <div
        ref={inputWrapRef}
        className={`model-id-input-wrap ${fetchedModels.length > 0 ? 'has-picker' : ''}`}
      >
        <input
          id={inputId}
          className="mono"
          data-field="model-id"
          data-autofocus={autoFocus ? '' : undefined}
          disabled={disabled}
          value={value}
          onFocus={() => {
            if (fetchedModels.length > 0) setPickerOpen(true);
          }}
          onKeyDown={(event) => {
            if (!pickerOpen || filtered.length === 0) {
              if (event.key === 'ArrowDown' && fetchedModels.length > 0) {
                event.preventDefault();
                setFilterActive(false);
                setPickerOpen(true);
              }
              return;
            }
            if (event.key === 'Escape') {
              event.preventDefault();
              event.stopPropagation();
              setPickerOpen(false);
              return;
            }
            if (event.key === 'ArrowDown') {
              event.preventDefault();
              setActiveIndex((idx) => (idx + 1) % filtered.length);
              return;
            }
            if (event.key === 'ArrowUp') {
              event.preventDefault();
              setActiveIndex((idx) => (idx <= 0 ? filtered.length - 1 : idx - 1));
              return;
            }
            if (event.key === 'Enter' && activeIndex >= 0 && activeIndex < filtered.length) {
              event.preventDefault();
              onSelectModel(filtered[activeIndex]);
              setPickerOpen(false);
            }
          }}
          onChange={(event) => {
            onValueChange(event.target.value);
            if (fetchedModels.length > 0) {
              setPickerOpen(true);
              setFilterActive(true);
            }
          }}
          placeholder={placeholder}
          autoComplete="off"
          spellCheck={false}
          required
        />
        {fetchedModels.length > 0 && (
          <button
            type="button"
            className="model-picker-toggle"
            aria-expanded={pickerOpen}
            aria-label={t.modelForm.toggleFetchedModels(fetchedModels.length)}
            disabled={disabled}
            onClick={() => {
              setFilterActive(false);
              setPickerOpen((open) => !open);
            }}
          >
            <span className="mono small">{fetchedModels.length}</span>
            <RotatingChevron open={pickerOpen} />
          </button>
        )}
        {pickerOpen && fetchedModels.length > 0 && (
          <div
            ref={pickerRef}
            className={`remote-model-picker scroll-area ${openUpward ? 'is-upward' : ''}`}
            role="listbox"
            aria-label={t.modelForm.fetchedModelsListLabel}
          >
            {filtered.length === 0 ? (
              <p className="remote-model-empty muted small">{t.modelForm.noMatchingFetchedModels}</p>
            ) : (
              filtered.map((item, idx) => {
                const selected = item.id === value.trim();
                const highlighted = idx === activeIndex;
                const preset = resolveModelPresetTokenLimits(item.id);
                const badgeCtx = item.contextWindow ?? preset?.contextWindow;
                return (
                  <button
                    key={item.id}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    data-remote-model={item.id}
                    className={`remote-model-option ${selected ? 'is-selected' : ''} ${highlighted ? 'is-highlighted' : ''}`}
                    onMouseEnter={() => setActiveIndex(idx)}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => {
                      onSelectModel(item);
                      setPickerOpen(false);
                    }}
                  >
                    <span className="remote-model-main min-width-zero">
                      <span className="mono truncate">{item.id}</span>
                      {item.displayName && (
                        <span className="muted small truncate">{item.displayName}</span>
                      )}
                    </span>
                    <span className="remote-model-trail">
                      {badgeCtx !== undefined && (
                        <span className="mono small muted remote-model-meta">
                          {formatTokenCount(badgeCtx)}
                        </span>
                      )}
                      {selected && <Icon name="check" className="remote-model-check" />}
                    </span>
                  </button>
                );
              })
            )}
          </div>
        )}
      </div>
      {fetchError && (
        <p className="danger small model-fetch-note" role="alert">{fetchError}</p>
      )}
      {!fetchError && fetchStatus && (
        <p className="muted small model-fetch-note">{fetchStatus}</p>
      )}
    </div>
  );
}
