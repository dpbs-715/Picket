import commitSkillRaw from '../skills/commit/SKILL.md?raw';
import cpushSkillRaw from '../skills/cpush/SKILL.md?raw';
import pushSkillRaw from '../skills/push/SKILL.md?raw';

function stripFrontmatter(markdown: string): string {
  return markdown.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n*/, '').trim();
}

export const COMMIT_SKILL_SPEC = stripFrontmatter(commitSkillRaw);
export const PUSH_SKILL_SPEC = stripFrontmatter(pushSkillRaw);
export const CPUSH_SKILL_SPEC = stripFrontmatter(cpushSkillRaw);
