import { SkillRegistry } from './skills';

const defaultRegistry = new SkillRegistry();

export const COMMIT_SKILL_SPEC = defaultRegistry.getBundledDocSync('commit');
export const PUSH_SKILL_SPEC = defaultRegistry.getBundledDocSync('push');
export const CPUSH_SKILL_SPEC = defaultRegistry.getBundledDocSync('cpush');
export const DCR_SKILL_SPEC = defaultRegistry.getBundledDocSync('dcr');
