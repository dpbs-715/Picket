import type { AppLocale } from './language-protocol';

export const SKILLS_PROTOCOL_VERSION = 1;

export const DEFAULT_SKILLS_SOURCE_REPO = 'https://github.com/dpbs-715/skills';

export const DEFAULT_GIT_SKILLS = [
  'commit',
  'push',
  'cpush',
  'dcr',
  'create-issues',
  'create-pr',
  'review-pr',
  'submit',
] as const;

export type SkillEntrySource = 'bundled' | 'synced';

export interface SkillCatalogItem {
  name: string;
  description: string;
  enabled: boolean;
  builtIn: boolean;
  source: SkillEntrySource;
  fileCount: number;
  referenceCount: number;
}

export interface SkillsStateSnapshot {
  protocolVersion: typeof SKILLS_PROTOCOL_VERSION;
  mode: SkillEntrySource;
  sourceRepo: string;
  defaultSourceRepo: string;
  sourceCommit?: string;
  syncedAt?: string;
  skills: SkillCatalogItem[];
}

export interface GetSkillsStateRequest {
  locale?: AppLocale;
}

export interface SyncSkillsRequest {
  sourceRepo?: string;
  locale?: AppLocale;
}

export interface UpdateEnabledSkillsRequest {
  enabledSkills: string[];
  locale?: AppLocale;
}

export interface ResetSkillsRequest {
  locale?: AppLocale;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function decodeSkillsStateSnapshot(value: unknown): SkillsStateSnapshot {
  if (!isRecord(value)
    || value.protocolVersion !== SKILLS_PROTOCOL_VERSION
    || (value.mode !== 'bundled' && value.mode !== 'synced')
    || typeof value.sourceRepo !== 'string' || !value.sourceRepo.trim()
    || typeof value.defaultSourceRepo !== 'string' || !value.defaultSourceRepo.trim()
    || !(value.sourceCommit === undefined || value.sourceCommit === null || typeof value.sourceCommit === 'string')
    || !(value.syncedAt === undefined || value.syncedAt === null || typeof value.syncedAt === 'string')
    || !Array.isArray(value.skills)) {
    throw new Error('Desktop skills status protocol mismatch.');
  }
  const skills: SkillCatalogItem[] = value.skills.map((raw) => {
    if (!isRecord(raw)
      || typeof raw.name !== 'string' || !raw.name.trim()
      || typeof raw.description !== 'string'
      || typeof raw.enabled !== 'boolean'
      || typeof raw.builtIn !== 'boolean'
      || (raw.source !== 'bundled' && raw.source !== 'synced')
      || typeof raw.fileCount !== 'number' || !Number.isInteger(raw.fileCount) || raw.fileCount < 1
      || typeof raw.referenceCount !== 'number' || !Number.isInteger(raw.referenceCount) || raw.referenceCount < 0) {
      throw new Error('Desktop skill catalog entry protocol mismatch.');
    }
    return {
      name: raw.name.trim(),
      description: raw.description,
      enabled: raw.enabled,
      builtIn: raw.builtIn,
      source: raw.source,
      fileCount: raw.fileCount,
      referenceCount: raw.referenceCount,
    };
  });
  return {
    protocolVersion: SKILLS_PROTOCOL_VERSION,
    mode: value.mode,
    sourceRepo: value.sourceRepo.trim(),
    defaultSourceRepo: value.defaultSourceRepo.trim(),
    ...(typeof value.sourceCommit === 'string' && value.sourceCommit.trim()
      ? { sourceCommit: value.sourceCommit.trim() }
      : {}),
    ...(typeof value.syncedAt === 'string' && value.syncedAt.trim()
      ? { syncedAt: value.syncedAt.trim() }
      : {}),
    skills,
  };
}
