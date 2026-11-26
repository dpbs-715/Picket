import { invoke, isTauri } from '@tauri-apps/api/core';
import type { AppLocale } from '../../shared/language-protocol';
import {
  DEFAULT_GIT_SKILLS,
  DEFAULT_SKILLS_SOURCE_REPO,
  SKILLS_PROTOCOL_VERSION,
  decodeSkillsStateSnapshot,
  type SkillsStateSnapshot,
} from '../../shared/skills-protocol';
import { getMessages } from '../i18n/messages';

export function initialBrowserSkillsSnapshot(): SkillsStateSnapshot {
  return {
    protocolVersion: SKILLS_PROTOCOL_VERSION,
    mode: 'bundled',
    sourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
    defaultSourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
    skills: DEFAULT_GIT_SKILLS.map((name) => ({
      name,
      description: '',
      enabled: true,
      builtIn: true,
      source: 'bundled',
      fileCount: name === 'submit' ? 6 : name === 'review-pr' ? 2 : 1,
      referenceCount: name === 'submit' ? 5 : name === 'review-pr' ? 1 : 0,
    })),
  };
}

export async function getDesktopSkillsState(locale?: AppLocale): Promise<SkillsStateSnapshot> {
  if (!isTauri()) {
    return initialBrowserSkillsSnapshot();
  }
  const reply = await invoke<unknown>('get_skills_state', {
    request: {
      ...(locale ? { locale } : {}),
    },
  });
  return decodeSkillsStateSnapshot(reply);
}

export async function syncDesktopSkills(
  sourceRepo: string | undefined,
  locale?: AppLocale,
): Promise<SkillsStateSnapshot> {
  if (!isTauri()) {
    throw new Error(getMessages(locale).desktop.disconnectedSkills);
  }
  const trimmed = sourceRepo?.trim();
  const reply = await invoke<unknown>('sync_skills', {
    request: {
      ...(trimmed ? { sourceRepo: trimmed } : {}),
      ...(locale ? { locale } : {}),
    },
  });
  return decodeSkillsStateSnapshot(reply);
}

export async function setDesktopEnabledSkills(
  enabledSkills: string[],
  locale?: AppLocale,
): Promise<SkillsStateSnapshot> {
  if (!isTauri()) {
    throw new Error(getMessages(locale).desktop.disconnectedSkills);
  }
  const reply = await invoke<unknown>('set_enabled_skills', {
    request: {
      enabledSkills,
      ...(locale ? { locale } : {}),
    },
  });
  return decodeSkillsStateSnapshot(reply);
}

export async function resetDesktopSkills(locale?: AppLocale): Promise<SkillsStateSnapshot> {
  if (!isTauri()) {
    throw new Error(getMessages(locale).desktop.disconnectedSkills);
  }
  const reply = await invoke<unknown>('reset_skills_to_bundled', {
    request: {
      ...(locale ? { locale } : {}),
    },
  });
  return decodeSkillsStateSnapshot(reply);
}
