import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vitest/config';

const rootDir = dirname(fileURLToPath(import.meta.url));
const VIRTUAL_SKILLS_ID = 'virtual:picket-bundled-skills';
const RESOLVED_VIRTUAL_SKILLS_ID = `\0${VIRTUAL_SKILLS_ID}`;

function picketBundledSkillsPlugin(): Plugin {
  return {
    name: 'picket-bundled-skills',
    resolveId(id) {
      if (id === VIRTUAL_SKILLS_ID) return RESOLVED_VIRTUAL_SKILLS_ID;
      return null;
    },
    load(id) {
      if (id !== RESOLVED_VIRTUAL_SKILLS_ID) return null;
      const skillsRoot = join(rootDir, 'agent/skills');
      const manifest = JSON.parse(readFileSync(join(skillsRoot, 'skills-manifest.json'), 'utf8')) as {
        skills: { files: string[] }[];
      };
      const files: Record<string, string> = {};
      for (const skill of manifest.skills) {
        for (const relPath of skill.files) {
          files[relPath] = readFileSync(join(skillsRoot, relPath), 'utf8');
        }
      }
      return [
        `export const BUNDLED_SKILLS_MANIFEST = ${JSON.stringify(manifest)};`,
        `export const BUNDLED_SKILL_FILES = ${JSON.stringify(files)};`,
      ].join('\n');
    },
  };
}

export default defineConfig({
  plugins: [react(), picketBundledSkillsPlugin()],
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  clearScreen: false,
  build: {
    outDir: process.env.PICKET_DIST_DIR || 'dist',
    rollupOptions: {
      input: {
        main: resolve(rootDir, 'index.html'),
        splash: resolve(rootDir, 'splash.html'),
        tray: resolve(rootDir, 'tray.html'),
      },
    },
  },
  test: {
    include: ['src/**/*.test.ts', 'agent/**/*.test.ts', 'shared/**/*.test.ts'],
    testTimeout: 15_000,
  },
});
