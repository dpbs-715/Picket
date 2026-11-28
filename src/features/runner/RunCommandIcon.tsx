import type { RunCommandIconId } from '../../../shared/run-protocol';

interface Props {
  icon: RunCommandIconId;
  size?: 'sm' | 'compact' | 'md';
  running?: boolean;
  className?: string;
}

function renderGlyph(icon: RunCommandIconId, px: number) {
  switch (icon) {
    case 'node':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinejoin="round">
          <path d="M12 2.6l8 4.6v9.6l-8 4.6-8-4.6V7.2z" />
          <path d="M9.4 14.6V9.4l5.2 5.2V9.4" strokeLinecap="round" />
        </svg>
      );
    case 'pnpm':
      return (
        <svg width={px - 1} height={px - 1} viewBox="0 0 24 24" fill="currentColor">
          <rect x="3" y="3" width="5.2" height="5.2" rx="1" />
          <rect x="9.4" y="3" width="5.2" height="5.2" rx="1" />
          <rect x="15.8" y="3" width="5.2" height="5.2" rx="1" />
          <rect x="15.8" y="9.4" width="5.2" height="5.2" rx="1" />
          <rect x="9.4" y="9.4" width="5.2" height="5.2" rx="1" opacity="0.65" />
          <rect x="9.4" y="15.8" width="5.2" height="5.2" rx="1" opacity="0.65" />
          <rect x="15.8" y="15.8" width="5.2" height="5.2" rx="1" opacity="0.65" />
          <rect x="3" y="15.8" width="5.2" height="5.2" rx="1" opacity="0.65" />
        </svg>
      );
    case 'vite':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
          <path d="M13 2.2L4.6 13.5H12L11 21.8l8.4-11.3H12z" />
        </svg>
      );
    case 'java':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <path d="M5 10h11v4.4a4.4 4.4 0 0 1-4.4 4.4H9.4A4.4 4.4 0 0 1 5 14.4V10z" />
          <path d="M16 11.2h1.7a2.1 2.1 0 0 1 0 4.2H16" />
          <path d="M8.6 6.6c0-1.1 1.1-1.5 1.1-2.7M12.2 6.6c0-1.1 1.1-1.5 1.1-2.7" />
          <path d="M4.2 21h12.6" />
        </svg>
      );
    case 'spring':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
          <path d="M19.8 4.2c-7.8 0-13.6 3.9-14.6 11.6 0 2 1.5 3.9 3.9 3.9 7.3 0 10.7-5.8 10.7-15.5z" />
          <path d="M5.2 18.8c3-3.9 6.8-6.8 11.6-8.8" />
        </svg>
      );
    case 'rust':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="6.8" />
          <circle cx="12" cy="12" r="2.5" />
          <path d="M12 2.6v2.5M12 18.9v2.5M2.6 12h2.5M18.9 12h2.5M5.4 5.4l1.8 1.8M16.8 16.8l1.8 1.8M18.6 5.4l-1.8 1.8M7.2 16.8l-1.8 1.8" />
        </svg>
      );
    case 'python':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 3.2c-3.4 0-3.9 1.4-3.9 3.4V9h4.9v1H6.6C4.6 10 3.2 11.2 3.2 13.9s1.4 3.9 3.4 3.9H8v-2.1c0-1.8 1.4-3.2 3.2-3.2h4.1c1.4 0 2.5-1.1 2.5-2.5V6.6c0-2-1.8-3.4-5.8-3.4z" />
          <path d="M12 20.8c3.4 0 3.9-1.4 3.9-3.4V15H11v-1h6.4c2 0 3.4-1.2 3.4-3.9s-1.4-3.9-3.4-3.9H16" />
          <circle cx="9.8" cy="6.2" r="0.7" fill="currentColor" stroke="none" />
          <circle cx="14.2" cy="17.8" r="0.7" fill="currentColor" stroke="none" />
        </svg>
      );
    case 'go':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 9h4M2 12.4h3.5M4 15.8h3" />
          <circle cx="13" cy="12.2" r="4" />
          <circle cx="19.3" cy="12.2" r="2.3" />
        </svg>
      );
    case 'docker':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
          <rect x="6" y="7" width="3" height="3" />
          <rect x="10" y="7" width="3" height="3" />
          <rect x="14" y="7" width="3" height="3" />
          <rect x="10" y="3.5" width="3" height="3" />
          <path d="M3.2 13h16.2c1.1 0 1.7-.7 2-1.4.7.5 1 .9 1 1.7 0 3.4-3.1 6.6-9.3 6.6-5.4 0-9.3-2.5-9.9-6.9z" />
        </svg>
      );
    case 'desktop':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="4" width="18" height="12.4" rx="2.2" />
          <path d="M8.5 20h7M12 16.4V20" />
        </svg>
      );
    case 'database':
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <ellipse cx="12" cy="6" rx="7.2" ry="2.9" />
          <path d="M4.8 6v12c0 1.6 3.2 2.9 7.2 2.9s7.2-1.3 7.2-2.9V6" />
          <path d="M4.8 12c0 1.6 3.2 2.9 7.2 2.9s7.2-1.3 7.2-2.9" />
        </svg>
      );
    case 'terminal':
    default:
      return (
        <svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4.5 17l5.8-5-5.8-5M12 18.5h7.5" />
        </svg>
      );
  }
}

export function RunCommandIcon({ icon, size = 'md', running = false, className = '' }: Props) {
  const px = size === 'sm' ? 12 : size === 'compact' ? 13 : 16;
  return (
    <span
      className={`run-icon-well size-${size} tone-${icon} ${className}`}
      aria-hidden="true"
    >
      {renderGlyph(icon, px)}
      {running && <span className="run-badge-live-dot" />}
    </span>
  );
}
