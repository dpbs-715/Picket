import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { cp, mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const targetRoot = join(root, 'agent/skills');
const DEFAULT_SOURCE_REPO = 'https://github.com/dpbs-715/skills';
const DEFAULT_SOURCE_URL = `${DEFAULT_SOURCE_REPO}.git`;
export const DEFAULT_GIT_SKILLS = [
  'commit',
  'push',
  'cpush',
  'dcr',
  'create-issues',
  'create-pr',
  'review-pr',
  'submit',
];

function parseArgs(argv) {
  let source = DEFAULT_SOURCE_URL;
  let skills = [...DEFAULT_GIT_SKILLS];
  let checkOnly = false;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--check') {
      checkOnly = true;
    } else if (arg === '--source' && argv[i + 1]) {
      source = argv[i + 1];
      i += 1;
    } else if (arg.startsWith('--source=')) {
      source = arg.slice('--source='.length);
    } else if (arg === '--skills' && argv[i + 1]) {
      skills = argv[i + 1].split(',').map((s) => s.trim()).filter(Boolean);
      i += 1;
    } else if (arg.startsWith('--skills=')) {
      skills = arg.slice('--skills='.length).split(',').map((s) => s.trim()).filter(Boolean);
    }
  }
  return { source, skills, checkOnly };
}

function parseFrontmatter(markdown, label) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(markdown);
  if (!match) {
    throw new Error(`Missing YAML frontmatter in ${label}`);
  }
  const block = match[1];
  const fields = {};
  for (const rawLine of block.split(/\r?\n/)) {
    const kv = /^([a-zA-Z0-9_-]+):\s*(.*)$/.exec(rawLine);
    if (!kv) continue;
    let value = kv[2].trim();
    if (
      (value.startsWith('"') && value.endsWith('"'))
      || (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    fields[kv[1]] = value;
  }
  if (!fields.name || !fields.description) {
    throw new Error(`Frontmatter in ${label} must define non-empty name and description`);
  }
  return { name: fields.name, description: fields.description };
}

async function listCleanFiles(dir) {
  const entries = await readdir(dir, { recursive: true, withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && !entry.name.startsWith('.'))
    .map((entry) => relative(dir, join(entry.parentPath, entry.name)).replace(/\\/g, '/'))
    .sort();
}

function validateRelativeLinks(markdown, skillDir, relFile) {
  const baseDir = dirname(join(skillDir, relFile));
  const linkRegex = /\]\(([^)]+)\)/g;
  let match;
  while ((match = linkRegex.exec(markdown)) !== null) {
    const rawTarget = match[1].trim().replace(/^<|>$/g, '');
    if (
      !rawTarget
      || rawTarget.startsWith('#')
      || rawTarget.startsWith('/')
      || /^[a-z][a-z0-9+.-]*:/i.test(rawTarget)
      || rawTarget.includes('{{REPO_ROOT}}')
    ) {
      continue;
    }
    const cleanTarget = rawTarget.split('#')[0];
    if (!cleanTarget) continue;
    const resolved = resolve(baseDir, cleanTarget);
    if (!resolved.startsWith(resolve(skillDir))) {
      throw new Error(`Relative link escapes skill directory in ${relFile}: ${rawTarget}`);
    }
    if (!existsSync(resolved)) {
      throw new Error(`Broken relative link in ${relFile}: ${rawTarget}`);
    }
  }
}

async function verifyBundledSkills() {
  const manifestPath = join(targetRoot, 'skills-manifest.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.skills) || manifest.skills.length === 0) {
    throw new Error('Invalid agent/skills/skills-manifest.json');
  }
  for (const entry of manifest.skills) {
    const skillDir = join(targetRoot, entry.name);
    const skillMdPath = join(targetRoot, entry.entry);
    const content = await readFile(skillMdPath, 'utf8');
    const fm = parseFrontmatter(content, entry.entry);
    if (fm.name !== entry.name) {
      throw new Error(`Skill name mismatch in ${entry.entry}: expected ${entry.name}, got ${fm.name}`);
    }
    for (const relFile of entry.files) {
      const fullPath = join(targetRoot, relFile);
      const fileContent = await readFile(fullPath, 'utf8');
      if (relFile.endsWith('.md')) {
        validateRelativeLinks(fileContent, skillDir, relative(skillDir, fullPath));
      }
    }
  }
  console.log(`Verified ${manifest.skills.length} bundled Git skills (${manifest.sourceCommit ?? 'unknown commit'}).`);
}

function tryGitHead(dir) {
  try {
    return execFileSync('/usr/bin/git', ['-C', dir, 'rev-parse', 'HEAD'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
  } catch {
    return null;
  }
}

function tryGitOrigin(dir) {
  try {
    const url = execFileSync('/usr/bin/git', ['-C', dir, 'remote', 'get-url', 'origin'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    return url.replace(/\.git$/, '');
  } catch {
    return null;
  }
}

async function main() {
  const { source, skills, checkOnly } = parseArgs(process.argv.slice(2));
  if (checkOnly) {
    await verifyBundledSkills();
    return;
  }

  let sourceDir = source;
  let tempClone = null;
  let sourceRepo = DEFAULT_SOURCE_REPO;
  const isRemote = /^(https?:|git@|ssh:|file:)/i.test(source);

  if (isRemote) {
    tempClone = await mkdtemp(join(tmpdir(), 'picket-skills-sync-'));
    execFileSync('/usr/bin/git', ['clone', '--depth', '1', source, tempClone], { stdio: 'inherit' });
    sourceDir = tempClone;
    sourceRepo = source.replace(/\.git$/, '');
  } else {
    sourceDir = resolve(source);
    const info = await stat(sourceDir);
    if (!info.isDirectory()) {
      throw new Error(`Skill source path is not a directory: ${sourceDir}`);
    }
    sourceRepo = tryGitOrigin(sourceDir) ?? DEFAULT_SOURCE_REPO;
  }

  try {
    const skillsBase = existsSync(join(sourceDir, 'skills')) ? join(sourceDir, 'skills') : sourceDir;
    const sourceCommit = tryGitHead(sourceDir);
    const stageDir = await mkdtemp(join(tmpdir(), 'picket-skills-stage-'));
    try {
      const manifestSkills = [];
      for (const name of skills) {
        const srcSkillDir = join(skillsBase, name);
        const srcSkillMd = join(srcSkillDir, 'SKILL.md');
        if (!existsSync(srcSkillMd)) {
          throw new Error(`Skill "${name}" not found at ${srcSkillMd}`);
        }
        const dstSkillDir = join(stageDir, name);
        await cp(srcSkillDir, dstSkillDir, {
          recursive: true,
          filter: (src) => {
            const relParts = relative(srcSkillDir, src).split('/').filter(Boolean);
            return !relParts.some((part) => part === '.DS_Store' || part === '.git' || part === 'evals');
          },
        });
        const files = await listCleanFiles(dstSkillDir);
        const rawSkillMd = await readFile(join(dstSkillDir, 'SKILL.md'), 'utf8');
        const fm = parseFrontmatter(rawSkillMd, `${name}/SKILL.md`);
        for (const relFile of files) {
          if (relFile.endsWith('.md')) {
            const md = await readFile(join(dstSkillDir, relFile), 'utf8');
            validateRelativeLinks(md, dstSkillDir, relFile);
          }
        }
        manifestSkills.push({
          name: fm.name,
          description: fm.description,
          entry: `${name}/SKILL.md`,
          files: files.map((file) => `${name}/${file}`),
        });
      }

      const manifest = {
        schemaVersion: 1,
        sourceRepo,
        ...(sourceCommit ? { sourceCommit } : {}),
        syncedAt: new Date().toISOString(),
        skills: manifestSkills,
      };
      await writeFile(join(stageDir, 'skills-manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);

      await rm(targetRoot, { recursive: true, force: true });
      await cp(stageDir, targetRoot, { recursive: true });
      console.log(`Synced ${manifestSkills.length} skills from ${sourceRepo}${sourceCommit ? ` (${sourceCommit.slice(0, 12)})` : ''} into agent/skills/.`);
    } finally {
      await rm(stageDir, { recursive: true, force: true });
    }
  } finally {
    if (tempClone) {
      await rm(tempClone, { recursive: true, force: true });
    }
  }
}

await main();
