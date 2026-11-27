import { useEffect, useLayoutEffect, useState } from 'react';
import { resolveAppTheme, type AppTheme, type ResolvedTheme } from '../../shared/theme-protocol';

export function readSystemPrefersDark(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return true;
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

export function applyDocumentTheme(resolved: ResolvedTheme): void {
  if (typeof document === 'undefined') return;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.style.colorScheme = resolved;
}

export function useResolvedTheme(
  theme: AppTheme,
  syncNativeWindow?: (theme: AppTheme) => Promise<ResolvedTheme | void>,
): ResolvedTheme {
  const [systemDark, setSystemDark] = useState<boolean>(() => readSystemPrefersDark());
  const [nativeOverride, setNativeOverride] = useState<ResolvedTheme | null>(null);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    const sync = () => {
      setNativeOverride(null);
      setSystemDark(query.matches);
      if (theme === 'system') {
        void syncNativeWindow?.('system');
      }
    };
    sync();
    query.addEventListener('change', sync);
    return () => query.removeEventListener('change', sync);
  }, [theme, syncNativeWindow]);

  useEffect(() => {
    let active = true;
    setNativeOverride(null);
    void syncNativeWindow?.(theme)?.then((reported) => {
      if (!active) return;
      if (reported === 'dark' || reported === 'light') {
        setNativeOverride(reported);
      }
    });
    return () => { active = false; };
  }, [theme, syncNativeWindow]);

  const resolved: ResolvedTheme = theme === 'system'
    ? (nativeOverride ?? resolveAppTheme('system', systemDark))
    : theme;

  useLayoutEffect(() => {
    applyDocumentTheme(resolved);
  }, [resolved]);

  return resolved;
}
