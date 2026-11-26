import { DEFAULT_APP_LOCALE, type AppLocale } from '../../shared/language-protocol';
import { commonEn, commonZh, type CommonMessages } from './catalog/common';
import { desktopEn, desktopZh, type DesktopMessages } from './catalog/desktop';
import { settingsEn, settingsZh, type SettingsMessages } from './catalog/settings';
import { workbenchEn, workbenchZh, type WorkbenchMessages } from './catalog/workbench';
import { workflowsEn, workflowsZh, type WorkflowMessages } from './catalog/workflows';

export const LANGUAGE_OPTIONS: readonly { id: AppLocale; label: string }[] = [
  { id: 'en', label: 'English' },
  { id: 'zh', label: '简体中文' },
];

export type Messages = CommonMessages
  & WorkbenchMessages
  & WorkflowMessages
  & SettingsMessages
  & DesktopMessages;

const en: Messages = {
  ...commonEn,
  ...workbenchEn,
  ...workflowsEn,
  ...settingsEn,
  ...desktopEn,
};

const zh: Messages = {
  ...commonZh,
  ...workbenchZh,
  ...workflowsZh,
  ...settingsZh,
  ...desktopZh,
};

const CATALOG: Record<AppLocale, Messages> = { en, zh };

export function getMessages(locale: AppLocale = DEFAULT_APP_LOCALE): Messages {
  return CATALOG[locale] ?? CATALOG[DEFAULT_APP_LOCALE];
}
