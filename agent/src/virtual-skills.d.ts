declare module 'virtual:picket-bundled-skills' {
  export interface BundledSkillEntry {
    name: string;
    description: string;
    entry: string;
    files: string[];
  }
  export interface BundledSkillsManifest {
    schemaVersion: 1;
    sourceRepo: string;
    sourceCommit?: string;
    syncedAt?: string;
    skills: BundledSkillEntry[];
  }
  export const BUNDLED_SKILLS_MANIFEST: BundledSkillsManifest;
  export const BUNDLED_SKILL_FILES: Record<string, string>;
}
