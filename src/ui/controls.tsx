import { useEffect, useId, useRef } from 'react';
import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { useI18n } from '../i18n';

const iconPaths = {
  down: 'm6 9 6 6 6-6', up: 'm6 15 6-6 6 6', right: 'm9 6 6 6-6 6',
  close: 'm6 6 12 12M6 18 18 6', check: 'm5 12 4 4L19 6', plus: 'M12 5v14M5 12h14',
  history: 'M3 11a9 9 0 1 1 3 8M3 4v7h7M12 7v5l3 2',
  pin: 'm16 3 5 5-4 1-4 5v4l-3-3-6 6M4 11h4l5-4 1-4',
  expand: 'M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7',
  collapse: 'M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7',
  settings: 'm9 3 1-1h4l1 4 3 2 4 1v5l-4 1-2 3-1 4h-5l-1-4-3-2-4-1v-5l4-1 2-3M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
  branch: 'M6 5v14M6 9c0 5 12 5 12-3M9 4a3 3 0 1 1-6 0 3 3 0 0 1 6 0M9 20a3 3 0 1 1-6 0 3 3 0 0 1 6 0M21 4a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
  tree: 'M4 3v15h5M4 7h5M12 5h8v5h-8zM12 16h8v5h-8z',
  list: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01',
  loader: 'M21 12a9 9 0 1 1-6.219-8.56',
  refresh: 'M20 7a9 9 0 1 0 1 8M20 2v6h-6',
  folder: 'M3 6h6l2 2h10v12H3z',
  external: 'M14 3h7v7M21 3l-9 9M10 3H3v18h18v-7',
  sparkles: 'm12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5z',
  pencil: 'm16 3 5 5-13 13H3v-5zM13 6l5 5',
  copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
  eye: 'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
  eyeOff: 'M3 3l18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M9.9 5.1A10.4 10.4 0 0 1 12 5c6.5 0 10 7 10 7a17.6 17.6 0 0 1-3.2 4.3M6.6 6.6C3.8 8.5 2 12 2 12s3.5 7 10 7a10.6 10.6 0 0 0 5.4-1.4',
  shield: 'M12 2 3 6v6c0 5 9 10 9 10s9-5 9-10V6zM8 12l3 3 5-6',
  send: 'm22 2-7 20-4-9-9-4zM22 2 11 13',
  commit: 'M2 12h5m10 0h5M17 12a5 5 0 1 1-10 0 5 5 0 0 1 10 0',
  upload: 'M12 16V3m-5 5 5-5 5 5M4 16v5h16v-5',
  cpu: 'M6 6h12v12H6zM9 9h6v6H9zM9 2v4m6-4v4M9 18v4m6-4v4M2 9h4m-4 6h4m12-6h4m-4 6h4',
  trash: 'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7',
  alert: 'm12 3 10 18H2zM12 9v5m0 3h.01',
  help: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01',
} as const;
export type IconName = keyof typeof iconPaths;
export function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  return <svg className={`icon ${className}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={iconPaths[name]} /></svg>;
}
export function IconButton({ icon, label, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string }) {
  return <button type="button" {...props} className={`icon-button ${props.className ?? ''}`} aria-label={label} title={label}><Icon name={icon} /></button>;
}

export function HelpTip({
  text,
  label,
  align = 'center',
  placement = 'top',
}: {
  text: string;
  label?: string;
  align?: 'start' | 'center' | 'end';
  placement?: 'top' | 'bottom';
}) {
  const tipId = useId();
  return (
    <span
      className={`help-tip help-tip-${align} help-tip-${placement}`}
      tabIndex={0}
      data-help-tip
      aria-label={label ?? text}
      aria-describedby={tipId}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
    >
      <Icon name="help" />
      <span id={tipId} role="tooltip" className="help-tip-bubble">{text}</span>
    </span>
  );
}

export function useDismiss(onClose: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  const latestClose = useRef(onClose);
  latestClose.current = onClose;
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !ref.current?.parentElement?.contains(event.target)) latestClose.current();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.stopPropagation(); latestClose.current(); }
    };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape); };
  }, []);
  return ref;
}

export function Popover({ title, onClose, children, className = '' }: { title: string; onClose: () => void; children: ReactNode; className?: string }) {
  const ref = useDismiss(onClose);
  return <div ref={ref} role="dialog" aria-label={title} className={`popover ${className}`}>{children}</div>;
}

export function Overlay({ title, children, onClose, covered = false, closeDisabled = false }: { title: string; children: ReactNode; onClose: () => void; covered?: boolean; closeDisabled?: boolean }) {
  const { t } = useI18n();
  const id = useId();
  const ref = useRef<HTMLDivElement>(null);
  const latestClose = useRef(onClose);
  latestClose.current = onClose;
  const latestCloseDisabled = useRef(closeDisabled);
  latestCloseDisabled.current = closeDisabled;
  useEffect(() => {
    if (covered) return;
    const previousFocus = document.activeElement;
    const panel = ref.current!;
    const autofocusTarget = panel.querySelector<HTMLElement>('[data-autofocus]:not(:disabled)');
    const initialTarget = autofocusTarget
      ?? panel.querySelector<HTMLElement>('button:not([data-help-tip]):not(:disabled), input, select, [tabindex="0"]:not([data-help-tip])')
      ?? panel;
    initialTarget.focus();
    if (autofocusTarget instanceof HTMLInputElement) {
      const end = autofocusTarget.value.length;
      try { autofocusTarget.setSelectionRange(end, end); } catch { /* ignore unsupported input types */ }
    }
    const trap = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        if (!latestCloseDisabled.current) latestClose.current();
        return;
      }
      if (event.key !== 'Tab') return;
      const focusable = [...panel.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea, [tabindex="0"]')].filter((element) => element.getClientRects().length);
      const first = focusable[0], last = focusable.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    panel.addEventListener('keydown', trap);
    return () => {
      panel.removeEventListener('keydown', trap);
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
    };
  }, [covered]);
  return <div ref={ref} className="overlay" role="dialog" aria-modal={!covered} aria-labelledby={id} tabIndex={-1} inert={covered}>
    <div className="overlay-heading" data-tauri-drag-region><h1 id={id} data-tauri-drag-region>{title}</h1><IconButton icon="close" label={t.common.closeOverlay(title)} disabled={closeDisabled} onClick={onClose} /></div>
    {children}
  </div>;
}
