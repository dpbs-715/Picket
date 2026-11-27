import type { ThemeKind } from './generated';

export const APP_THEMES = ['system', 'dark', 'light'] as const satisfies readonly ThemeKind[];

export type AppTheme = ThemeKind;
export type ResolvedTheme = 'dark' | 'light';

export const DEFAULT_APP_THEME: AppTheme = 'system';

export function isAppTheme(value: unknown): value is AppTheme {
  return value === 'system' || value === 'dark' || value === 'light';
}

export function normalizeAppTheme(value: unknown, fallback: AppTheme = DEFAULT_APP_THEME): AppTheme {
  return isAppTheme(value) ? value : fallback;
}

export function resolveAppTheme(theme: AppTheme, systemPrefersDark: boolean): ResolvedTheme {
  if (theme === 'dark' || theme === 'light') return theme;
  return systemPrefersDark ? 'dark' : 'light';
}
