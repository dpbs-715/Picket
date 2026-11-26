import { describe, expect, it } from 'vitest';
import {
  DEFAULT_GIT_SKILLS,
  DEFAULT_SKILLS_SOURCE_REPO,
  SKILLS_PROTOCOL_VERSION,
  decodeSkillsStateSnapshot,
} from '../../shared/skills-protocol';
import { initialBrowserSkillsSnapshot } from './skills';

describe('skills protocol & browser fallback', () => {
  it('decodes a valid synced skills snapshot including optional commit and syncedAt', () => {
    const decoded = decodeSkillsStateSnapshot({
      protocolVersion: SKILLS_PROTOCOL_VERSION,
      mode: 'synced',
      sourceRepo: 'https://github.com/dpbs-715/skills',
      defaultSourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
      sourceCommit: 'abc1234def5678',
      syncedAt: '2026-11-26T05:00:00Z',
      skills: [
        {
          name: 'commit',
          description: 'Create focused git commits.',
          enabled: true,
          builtIn: true,
          source: 'synced',
          fileCount: 1,
          referenceCount: 0,
        },
        {
          name: 'release-tag',
          description: 'Tag release versions.',
          enabled: false,
          builtIn: false,
          source: 'synced',
          fileCount: 2,
          referenceCount: 1,
        },
      ],
    });

    expect(decoded.mode).toBe('synced');
    expect(decoded.sourceCommit).toBe('abc1234def5678');
    expect(decoded.syncedAt).toBe('2026-11-26T05:00:00Z');
    expect(decoded.skills).toHaveLength(2);
    expect(decoded.skills[1].referenceCount).toBe(1);
    expect(decoded.skills[1].enabled).toBe(false);
  });

  it('rejects malformed snapshot payloads', () => {
    expect(() =>
      decodeSkillsStateSnapshot({
        protocolVersion: 99,
        mode: 'bundled',
        sourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
        defaultSourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
        skills: [],
      }),
    ).toThrow(/protocol mismatch/i);

    expect(() =>
      decodeSkillsStateSnapshot({
        protocolVersion: SKILLS_PROTOCOL_VERSION,
        mode: 'synced',
        sourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
        defaultSourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
        skills: [
          {
            name: 'commit',
            description: 'ok',
            enabled: true,
            builtIn: true,
            source: 'synced',
            fileCount: 0,
            referenceCount: 0,
          },
        ],
      }),
    ).toThrow(/catalog entry protocol mismatch/i);
  });

  it('produces a valid initial browser bundled snapshot covering all 8 default git skills', () => {
    const initial = initialBrowserSkillsSnapshot();
    const roundTrip = decodeSkillsStateSnapshot(initial);
    expect(roundTrip.mode).toBe('bundled');
    expect(roundTrip.sourceRepo).toBe(DEFAULT_SKILLS_SOURCE_REPO);
    expect(roundTrip.skills.map((item) => item.name)).toEqual([...DEFAULT_GIT_SKILLS]);
    expect(roundTrip.skills.every((item) => item.builtIn && item.enabled && item.source === 'bundled')).toBe(true);
  });
});
