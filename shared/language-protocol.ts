export const APP_LOCALES = ['en', 'zh'] as const;

export type AppLocale = typeof APP_LOCALES[number];
export type CommitLanguage = AppLocale;

export const DEFAULT_APP_LOCALE: AppLocale = 'en';
export const DEFAULT_COMMIT_LANGUAGE: CommitLanguage = 'en';

export function isAppLocale(value: unknown): value is AppLocale {
  return value === 'en' || value === 'zh';
}

export function normalizeAppLocale(value: unknown, fallback: AppLocale = DEFAULT_APP_LOCALE): AppLocale {
  return isAppLocale(value) ? value : fallback;
}
