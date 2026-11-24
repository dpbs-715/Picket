import { createContext, useContext, useMemo } from 'react';
import type { ReactNode } from 'react';
import { DEFAULT_APP_LOCALE, type AppLocale } from '../../shared/language-protocol';
import { getMessages, LANGUAGE_OPTIONS, type Messages } from './messages';

export { getMessages, LANGUAGE_OPTIONS };
export type { Messages };

interface I18nValue {
  locale: AppLocale;
  t: Messages;
}

const I18nContext = createContext<I18nValue>({
  locale: DEFAULT_APP_LOCALE,
  t: getMessages(DEFAULT_APP_LOCALE),
});

export function I18nProvider({ locale, children }: { locale: AppLocale; children: ReactNode }) {
  const value = useMemo<I18nValue>(() => ({ locale, t: getMessages(locale) }), [locale]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}
