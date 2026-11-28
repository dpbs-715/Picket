import type { DiscoverRunCommandsCommand, PiDiscoveredRunCommand } from '../../../shared/pi-protocol';
import { RUN_COMMAND_ICON_IDS, normalizeRunIcon } from '../../../shared/run-protocol';
import { extractJsonObject } from './shared';

export function buildDiscoverRunCommandsPrompts(
  command: DiscoverRunCommandsCommand,
): { systemPrompt: string; userPrompt: string } {
  const isZh = command.locale === 'zh';
  const languageRule = isZh
    ? 'Write `summary`, `label`, and `description` in concise Simplified Chinese. Keep `label` crisp and short (include the environment or script target clearly, e.g. `<target> (dev:test)` or `<target> (build:prod)`). Use clean `group` names (keep canonical tool/runtime names such as `Maven`, `Spring Boot`, `Gradle`, `Docker`, or use concise Simplified Chinese group names for Dev/Start, Build/Package, Test & Lint, and Setup when a single-package repo has 6 or more commands).'
    : 'Write `summary`, `label`, and `description` in concise English. Keep `label` crisp and short. Use clean `group` names (e.g. `Maven`, `Spring Boot`, `Gradle`, `Docker`, or for single-package repos with >= 6 commands: `Dev`, `Build`, `Test & Lint`, `Setup`).';
  const isMultiModule = (command.workspaceModules?.length ?? 0) > 0;
  const iconChoices = RUN_COMMAND_ICON_IDS.join('|');

  const multiModuleRules = isMultiModule
    ? [
        '- MULTI-MODULE WORKSPACE DETECTED: Do NOT artificially cap the command count (up to 64 commands are supported; the UI groups and filters them).',
        '- Discover the FULL range of practical local developer commands actually supported by this repository — NOT just a single install command plus service startups:',
        '  1) Root build, lifecycle, and verification commands (`category: "build"` or `"test"`): inspect root manifests (`pom.xml`, `build.gradle*`, `package.json`, `Cargo.toml`, `Makefile`, `README.md`, etc.) to offer useful workspace commands such as compile/build, dependency install, package, unit tests, lint/typecheck, or declared profiles/scripts (for example in Maven: `compile`, `test`, `package -DskipTests`, `install -DskipTests`, or profile-specific commands if defined in `pom.xml`).',
        '  2) Runnable services and applications (`category: "dev"`): include each detected runnable child service/app from `workspaceModules` (e.g. Spring Boot starters via `<wrapper> -pl <module> spring-boot:run`, Gradle subproject `:bootRun` / `:run`, Node workspace `dev`/`start` apps, Cargo bins, Go commands). Skip non-runnable library/common/api modules.',
        '  3) Local infrastructure and repository scripts: if `docker-compose*`, `compose*`, `Makefile`, `justfile`, or `scripts/*` exist, inspect them and include practical local helper commands (e.g. starting local middleware containers or running repo dev/check scripts) in an appropriate group (`Docker`, `Scripts`, etc.).',
        '- Organize commands cleanly with short `group` names matching their tool/runtime/role (e.g. `Maven`, `Spring Boot`, `Gradle`, `Docker`, `Apps`, `Scripts`).',
        '- Rely on the pre-scanned `workspaceModules` list for submodule inventory instead of reading every submodule manifest one by one; spend tool calls on top-level manifests (`pom.xml`, `package.json`, `Makefile`, `docker-compose*`, `scripts/*`, `README.md`) to ensure you do not miss build, test, script, or infrastructure commands.',
      ]
    : [
        '- Do NOT artificially cap the command list at 8 or 10 items (up to 64 commands are supported; the UI supports collapsible groups and instant filtering).',
        '- For `package.json` repositories: read the ENTIRE `"scripts"` section (if `read_repo_file` truncates `package.json`, call `read_repo_file` again with `startLine` so you see every script). Include ALL practical dev/start/serve/preview, build/package, test, lint, and typecheck scripts — INCLUDING ALL environment-specific variants (e.g. `dev:*`, `start:*`, `serve:*`, `build:*`, `test:*`, staging/test/prod/uat and custom environment names such as `hntest`, `tag`, `gray`). Never drop environment variants just to shorten the list.',
        '- Skip only internal lifecycle hooks (`preinstall`, `postinstall`, `prepare`, `prepublishOnly`, `husky`) and destructive/publish/deploy commands.',
        '- Prefer declared manifest scripts (`pnpm run <script>`, `npm run <script>`, `yarn <script>`, `make <target>`, `cargo ...`) over invoking internal helper files inside `scripts/` directly; only add standalone `scripts/*` invocations if they are real standalone developer entrypoints not already wrapped by `package.json` or `Makefile`.',
        '- When the repository yields 6 or more commands across clear categories (e.g. Dev/Start, Build/Package, Test & Lint, Docker/Setup), assign concise `group` names so the user can collapse/expand and batch-select them easily; omit `group` only for tiny repos with 5 or fewer commands.',
      ];

  const systemPrompt = [
    'You are Picket\'s read-only project run-command analyzer.',
    'Your sole job is to objectively inspect project manifests, pre-scanned workspace modules, and scripts in this repository and recommend practical commands that a developer would run locally (development servers, microservices, builds, dependency installs, tests, checks, and local docker/script helpers).',
    'Rules:',
    '- You have read-only tools (`read_repo_file`, `search_repo`). Inspect relevant project manifests first (e.g. `package.json`, `Cargo.toml`, `Makefile`, `pyproject.toml`, `go.mod`, `pom.xml`, `docker-compose*`, `scripts/*`). Do not read unnecessary source files or `.env` files.',
    '- For subdirectories that should execute inside the sub-package (e.g. `apps/web/package.json`), set `cwd` to that relative subdirectory; omit `cwd` when the command runs at the repository root (such as `mvn -pl <module> spring-boot:run`).',
    '- Use exact package manager / build tool wrappers matching the repository (`./mvnw` vs `mvn`, `./gradlew` vs `gradle`, `pnpm`, `cargo`, `npm`, `python3`, etc.). Never invent scripts, targets, profiles, or modules that do not exist in the repository.',
    '- Never recommend destructive, publish, release, deploy, or credential-mutating commands (such as `rm -rf`, `npm publish`, `cargo publish`, `git push`, `mvn deploy`).',
    ...multiModuleRules,
    `- Optionally set \`icon\` to one of: ${iconChoices}.`,
    `- ${languageRule}`,
    '- Return ONLY a valid JSON object matching this exact schema (no markdown fences or commentary outside JSON):',
    '{"summary":"string","commands":[{"label":"short display name (max 48 chars)","command":"exact shell command","cwd":"optional relative subdir","group":"optional short group name (max 48 chars)","icon":"optional built-in icon id","description":"1 concise sentence explaining what it runs or checks","category":"dev|test|build"}]}',
  ].join('\n');

  const manifestList = command.projectFiles.length > 0
    ? command.projectFiles.map((f) => `- ${f}`).join('\n')
    : '(No standard manifest files were found in top-level scan; use search_repo if needed.)';
  const existingList = command.existingCommands.length > 0
    ? command.existingCommands.map((c) => `- ${c}`).join('\n')
    : '(None)';

  const sections = [
    `Repository: ${command.repositoryName}`,
    '',
    'Detected project manifest & script files:',
    manifestList,
  ];

  if (command.workspaceModules && command.workspaceModules.length > 0) {
    sections.push(
      '',
      'Pre-scanned multi-module workspace structure (read-only):',
      command.workspaceModules.map((m) => `- ${m}`).join('\n'),
    );
  }

  sections.push(
    '',
    'Commands already configured by the user (avoid duplicates unless a different cwd applies):',
    existingList,
    '',
    'Inspect any needed top-level manifest files with `read_repo_file` and return the recommended local run commands JSON.',
  );

  return { systemPrompt, userPrompt: sections.join('\n') };
}

export function parseDiscoveredRunCommands(
  text: string,
): { summary: string; commands: PiDiscoveredRunCommand[] } {
  const parsed = extractJsonObject(text);
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('Model returned invalid JSON for run command discovery.');
  }
  const rec = parsed as Record<string, unknown>;
  const summary = typeof rec.summary === 'string' ? rec.summary.trim() : '';
  const rawList = Array.isArray(rec.commands) ? rec.commands : [];
  const commands: PiDiscoveredRunCommand[] = [];
  for (const entry of rawList) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) continue;
    const item = entry as Record<string, unknown>;
    const label = typeof item.label === 'string' ? item.label.trim() : '';
    const cmd = typeof item.command === 'string' ? item.command.trim() : '';
    if (!label || !cmd) continue;
    const cwd = typeof item.cwd === 'string' && item.cwd.trim() && item.cwd.trim() !== '.'
      ? item.cwd.trim()
      : undefined;
    const rawGroup = typeof item.group === 'string'
      ? item.group.replace(/[\r\n\0]+/g, ' ').trim()
      : '';
    const group = rawGroup ? rawGroup.slice(0, 48) : undefined;
    const icon = typeof item.icon === 'string' ? normalizeRunIcon(item.icon) : undefined;
    const description = typeof item.description === 'string' ? item.description.trim() : '';
    const category = typeof item.category === 'string' && item.category.trim()
      ? item.category.trim()
      : 'dev';
    commands.push({
      label: label.slice(0, 48),
      command: cmd.slice(0, 1000),
      ...(cwd ? { cwd } : {}),
      ...(group ? { group } : {}),
      ...(icon ? { icon } : {}),
      description: description.slice(0, 200),
      category: category.slice(0, 24),
    });
  }
  return { summary, commands };
}
