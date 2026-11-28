import {
  BUNDLED_SKILL_FILES,
  BUNDLED_SKILLS_MANIFEST,
  type BundledSkillEntry,
  type BundledSkillsManifest,
} from 'virtual:picket-bundled-skills';

export interface SkillDescriptor {
  name: string;
  description: string;
  source: 'override' | 'bundled';
  files: string[];
}

export interface RuntimeSkillOverrideEntry {
  name: string;
  description: string;
  files: string[];
}

export interface RuntimeSkillOverrideCatalog {
  sourceRepo?: string;
  sourceCommit?: string;
  skills: RuntimeSkillOverrideEntry[];
  disabledSkills?: string[];
}

export type OverrideSkillDocReader = (skill: string, relativePath: string) => Promise<string | null>;

const REPO_ROOT_SKILL_LINK_RE = /\{\{REPO_ROOT\}\}\/skills\/([a-z0-9-]+)\/([a-zA-Z0-9._/-]+)/g;
const OPTIONAL_BODY_PARAGRAPH_RE = /(?:Use a body only when[\s\S]*?(?:\r?\n\r?\n|$)|keep single-line by default[\s\S]*?(?:\r?\n\r?\n|$))/gi;

export function stripFrontmatter(markdown: string): string {
  return markdown.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n*/, '').trim();
}

export function normalizeUpstreamSkillMarkdown(markdown: string, keepFrontmatter = false): string {
  const base = keepFrontmatter ? markdown.trim() : stripFrontmatter(markdown);
  return base.replace(REPO_ROOT_SKILL_LINK_RE, (_match, skillName: string, docPath: string) => (
    `skill://${skillName}/${docPath}`
  ));
}

export function adaptCommitSkillForDesktop(commitMarkdown: string): string {
  return commitMarkdown.replace(
    OPTIONAL_BODY_PARAGRAPH_RE,
    'Always include a blank line followed by 1 to 6 concise `- ` bullet points detailing concrete changes and intent (Picket Desktop two-part commit format).\n\n',
  );
}

export function normalizeRelativeSkillDocPath(rawPath?: string): string {
  const trimmed = (rawPath ?? '').trim().replace(/^\/+/, '');
  const candidate = trimmed || 'SKILL.md';
  if (candidate.includes('\0') || candidate.includes('\\')) {
    throw new Error(`Invalid skill document path: ${candidate}`);
  }
  const segments = candidate.split('/');
  if (segments.some((segment) => !segment || segment === '.' || segment === '..')) {
    throw new Error(`Skill document path must stay inside the skill directory: ${candidate}`);
  }
  return segments.join('/');
}

export const PICKET_DESKTOP_OVERLAY = {
  cpushCommitFormat: [
    'Picket Desktop Commit Overlay (supersedes optional-body and over-splitting interpretations in upstream `commit` skill):',
    '- Always format every `groups[].message` in two parts: `<type>(<optional scope>): <subject>\\n\\n- <detail 1>\\n- <detail 2>` (1 to 6 concise `- ` bullet points detailing concrete changes and intent; never single-line without `- ` bullets).',
    '- Default to cohesive vertical commits (1 commit when changes serve one shared goal); never split a single feature, fix, or refactoring pass horizontally by directory or technical layer (e.g. shared protocol vs agent vs backend vs frontend) or peel off its companion tests, docs, i18n, or lockfiles into separate commits.',
    '- Respect the exact selected scope (`Authorized selected files`); never widen into unselected worktree files or Local Keep files.',
    '- Interactive clarifications and approvals happen through Picket\'s unified dialog rather than terminal prompts.',
  ].join('\n'),
  customActionOverlay: [
    'Picket Desktop Custom Action & Controlled CLI Overlay:',
    '- You have NO general shell/bash tool. Use only the provided controlled tools: `read_skill_doc`, `check_cli_env`, `install_platform_cli`, `run_git`, `run_platform_cli`, `edit_repo_file`, `write_repo_file`, `run_repo_command`, `commit_exact_scope`, `sync_and_push`, `handoff_to_recovery`, `manage_submit_worktree`, `cleanup_submit_duplicates`, `record_action_checkpoint`, `read_scope_diff`, `read_repo_file`, `search_repo`, and `message_user`.',
    '- If the button instruction invokes or matches an available Git skill (`create-issues`, `create-pr`, `review-pr`, `commit`, `push`, `cpush`, `dcr`, `submit`), call `read_skill_doc` FIRST to follow that skill\'s exact procedure and load relevant `references/*.md` docs on demand (`references/cleanup.md`, `references/merge-chain.md`, `references/resume.md`, `references/gitlab.md`, `references/preview.md`).',
    '- Use `message_user` as your primary communication channel with the user in Picket\'s `400px` MessageDock:',
    '  * Call `message_user` WITHOUT `questions` (non-blocking) to publish a clear, scannable structured card for code reviews, PR reviews, pre-release checks, diagnostics, or rich final outcomes. Picket preserves your latest published card as the final outcome view.',
    '  * Keep cards crisp and breathable (typically 1–3 focused `blocks`, never repeating the same facts verbatim in prose): use `findings` (with exact repo-relative `path` and 1-based `line` so clicking opens the user\'s external IDE directly) for `dcr` / `review-pr` issues, `checklist` for verification/CI steps, `facts` for 2–4 key metrics, `callout` for a key caveat or next-step note, `code` (set `collapsed: true` when >8 lines) for error logs or diff excerpts, `files` for clickable repo file chips, and `markdown` only when short prose adds clarity.',
    '  * Call `message_user` WITH `questions` (1–4 items, blocking) whenever user choice, approval, or clarification is needed—include relevant `blocks` right above the options so the user has full context before deciding.',
    '- Before running `gh` or `glab` via `run_platform_cli`, call `check_cli_env` to verify CLI availability, head-SHA guard flags (`--match-head-commit` / `--sha`), and authentication status:',
    '  * If the needed CLI (`gh` or `glab`) is missing and `brew.installed` is true, call `message_user` with `questions` asking explicit user permission with an option having `id: "install-gh"` (preview `"brew install gh"`) or `id: "install-glab"` (preview `"brew install glab"`). Only call `install_platform_cli` AFTER the user selects that option.',
    '  * If the CLI is installed but not authenticated, call `message_user` with `questions` guiding the user to run `gh auth login` or `glab auth login` in their terminal and offer options to retry once signed in or cancel.',
    '- To modify existing repository files (for example bumping versions in manifests or lockfiles), inspect them first with `read_repo_file` / `search_repo` and apply exact unique replacements with `edit_repo_file`. To create a brand-new repository file (for example release notes, changelogs, or config templates), use `write_repo_file` (never overwrites existing files). Both refuse `.git`, sensitive files, `Local Keep`, and pre-existing unselected dirty drafts.',
    '- To run local project checks, formatters, or scripts (for example `node scripts/package-dmg.mjs --check-version-only --tag v0.2.1`, `cargo fmt`, or `pnpm check`), call `run_repo_command` with discrete `command` and `args` (zero shell interpolation; never invoke `sh`/`bash`/`zsh`/`sudo`/`rm`/`curl`/`wget`, and always use `run_git`/`run_platform_cli` for `git`/`gh`/`glab`). Clean or authorized files created/updated by a succeeding `run_repo_command` are automatically reported in `modifiedPaths` and tracked for `commit_exact_scope({ editedPaths })`.',
    '- For single-branch commits on the current checkout, ALWAYS use `commit_exact_scope` (never `run_git(["add", ...])` or `run_git(["commit", ...])`): pass `fileIds` for pre-selected files and/or `editedPaths` for files created/modified via `edit_repo_file`, `write_repo_file`, or `run_repo_command` in this run (or persisted unchanged from the previous run checkpoint), or pass `groups` to split into multiple cohesive commits. When `commit_exact_scope` or `sync_and_push` hits a push failure, missing upstream, or rebase conflict, Rust attaches a verified `recoveryHandle`: decide whether to handle it yourself per the button prompt (for example switching branches or pulling/rebasing) OR call `handoff_to_recovery({ handleId })` to hand off cleanly to Picket\'s zero-token recovery buttons (Retry Push / Set Upstream / Pull & Rebase / AI Conflict Merge / Manual Merge). For multi-group or target-branch `submit` workflows that need isolated temporary worktrees, ALWAYS use `manage_submit_worktree` (`create` -> `commit` -> `list` / `remove`) so Rust transfers ONLY authorized selected files (and ONLY staged content for partially staged files) while recording exact preflight snapshots for Step 5 cleanup.',
    '- Work economically and avoid redundant micro-probes: rely on the patches already included in `Authorized selected files`, call `check_cli_env` at most once per run, and let `manage_submit_worktree` / `cleanup_submit_duplicates` handle exact staging, snapshot verification, and duplicate checks in Rust instead of manually running repetitive `git diff`, `git ls-files`, `git hash-object`, or `git check-ref-format` commands.',
    '- After creating each Issue, PR/MR, merge, Issue closure, or promotion PR/MR, call `record_action_checkpoint` immediately so Picket persists the verified IDs, URLs, branches, and commit SHAs in `~/.picket/action-sessions.json` for crash/restart recovery.',
    '- If `<previous_session_checkpoint>` is supplied, follow `submit/references/resume.md`: reconcile recorded Issues, branches, isolated worktrees (`manage_submit_worktree(list)`), and PRs/MRs against live Git & `gh`/`glab` BEFORE creating any new resources, and continue cleanly from the first unfinished step without duplicating Issues, branches, or PRs.',
    '- For `submit` Step 5 duplicate cleanup, ALWAYS ask the user via `message_user` (with `questions`) first; only if approved, call `cleanup_submit_duplicates({ userConfirmed: true })` (Rust automatically verifies remote tips, creates `<git-dir>/submit-backup/<timestamp>/`, and retains partial-staged or modified files).',
    '- For two-stage promotion merges (`to <first> then <final>`), defer Issue closure and source branch deletion until the final promotion PR/MR is merged; then close resolved Issues, delete the remote feature branch ONLY with `run_git(["push", "<remote>", "--force-with-lease=refs/heads/<branch>:<verified-40-sha>", ":refs/heads/<branch>"])`, and remove temporary Picket worktrees + local branch via `manage_submit_worktree({ op: "remove", ..., deleteLocalBranch: true })`.',
    '- Respect Picket permanent redlines enforced by Rust: never force-push (`-f`/`--force`), never bare `--force-with-lease`, never `reset --hard`/`clean -f`/`checkout -- .`/`worktree remove --force`, never `gh pr merge --admin`, ALWAYS pass `--match-head-commit <40-hex-sha>` to `gh pr merge` and `--sha <40-hex-sha>` to `glab mr merge`/`glab mr approve`, and never pass `--repo` targeting a different repository.',
    '- For multiline Issue/PR/MR descriptions in `run_platform_cli`, pass the Markdown content via `bodyText` + `bodyFlag` (`--body-file` for `gh`, `--description-file` for `glab`) instead of inline `--body` strings.',
    '- When the task finishes, return a concise JSON summary: `{"summary":"Concise outcome","links":[{"label":"PR #12","url":"https://..."}]}`. If you already published a rich `message_user` card for the outcome, keep `summary` to one short line.',
  ].join('\n'),
  createActionOverlay: [
    'Picket Desktop `Create with Picket` Button Generator Overlay:',
    '- You are helping a user create a reusable repository-bound Custom Action Button in Picket.',
    '- You have READ-ONLY tools (`read_skill_doc`, `check_cli_env`, `run_git` Tier-0 only, `read_repo_file`, `search_repo`) plus `message_user`.',
    '- Understand the real runtime capabilities of Picket Custom Action Buttons so every generated `prompt` is 100% executable:',
    '  * Available at runtime: `read_repo_file`, `search_repo`, `edit_repo_file` (exact text edits inside the worktree, blocking `.git`/sensitive/`Local Keep`/pre-existing unselected drafts), `write_repo_file` (create new UTF-8 text files without overwriting existing ones), `run_repo_command` (discrete program + args without shell operators `&&`/`|`/`$()` and blocking `sh`/`bash`/`zsh`/`sudo`/`rm`/`curl`/`wget`; auto-tracks safe modified/created files), `run_git` (controlled Git without `add`/`commit` or destructive flags), `commit_exact_scope` (commits pre-selected `fileIds` and/or `editedPaths` from `edit_repo_file`/`write_repo_file`/`run_repo_command`), `sync_and_push`, `run_platform_cli` (`gh`/`glab`), `manage_submit_worktree`, `cleanup_submit_duplicates`, `record_action_checkpoint`, and `message_user` (displays rich structured cards such as `findings`/`checklist`/`facts`/`code`/`files` in MessageDock and asks interactive `questions`).',
    '  * NEVER generate shell pipelines, `bash -c`, `rm -rf`, `curl | sh`, or raw `git add`/`git commit` instructions that violate Picket\'s controlled tool model.',
    '- Inspect the current repository context thoroughly first: e.g., `check_cli_env` for GitHub vs GitLab remote, `run_git(["branch", "-a"])` for branch conventions, `run_git(["tag", "--sort=-v:refname", "-l"])` for tag naming like `v0.x.y`, AND for release/version-bump/build workflows, read CI workflows (such as `.github/workflows/*.yml`), `package.json`, recent release commits (`run_git(["show", "--stat", "<latest-tag>"])`), and version/packaging scripts so EVERY required version manifest and check command is captured accurately.',
    '- Check the available Git skills index and call `read_skill_doc` if the workflow maps cleanly onto a built-in skill (`submit`, `create-pr`, `review-pr`, `create-issues`, `cpush`, `dcr`, etc.).',
    '- Only call `message_user` with `questions` when genuine ambiguity exists (for example, target branch name, Chinese vs English output, or version-bump vs fixed tag template); batch all clarifying questions into ONE `message_user` call (1–4 questions).',
    '- Return ONLY a single JSON object with schema: `{"label":"Short button label (<= 24 chars)","prompt":"Complete actionable instruction or skill invocation for Picket","summary":"One-sentence explanation of what this button does"}`.',
  ].join('\n'),
} as const;

export class SkillRegistry {
  private readonly descriptors = new Map<string, SkillDescriptor>();
  private readonly overrideReader?: OverrideSkillDocReader;

  constructor(
    overrideCatalog?: RuntimeSkillOverrideCatalog,
    overrideReader?: OverrideSkillDocReader,
  ) {
    this.overrideReader = overrideReader;
    const disabled = new Set(
      (overrideCatalog?.disabledSkills ?? [])
        .map((name) => name.trim())
        .filter(Boolean),
    );
    for (const bundled of BUNDLED_SKILLS_MANIFEST.skills) {
      if (disabled.has(bundled.name)) continue;
      this.descriptors.set(bundled.name, {
        name: bundled.name,
        description: bundled.description,
        source: 'bundled',
        files: bundled.files.map((file) => (
          file.startsWith(`${bundled.name}/`) ? file.slice(bundled.name.length + 1) : file
        )),
      });
    }
    if (overrideCatalog) {
      for (const override of overrideCatalog.skills) {
        const cleanName = override.name.trim();
        if (!cleanName || disabled.has(cleanName)) continue;
        this.descriptors.set(cleanName, {
          name: cleanName,
          description: override.description,
          source: 'override',
          files: override.files.map((file) => (
            file.startsWith(`${cleanName}/`) ? file.slice(cleanName.length + 1) : file
          )),
        });
      }
    }
  }

  listSkills(): SkillDescriptor[] {
    return [...this.descriptors.values()].sort((a, b) => a.name.localeCompare(b.name));
  }

  hasSkill(name: string): boolean {
    return this.descriptors.has(name);
  }

  getBundledDocSync(skill: string, relativePath = 'SKILL.md'): string {
    const cleanPath = normalizeRelativeSkillDocPath(relativePath);
    const key = `${skill}/${cleanPath}`;
    const raw = BUNDLED_SKILL_FILES[key];
    if (raw === undefined) {
      throw new Error(`Bundled skill document not found: ${key}`);
    }
    return normalizeUpstreamSkillMarkdown(raw);
  }

  async readDoc(skill: string, relativePath = 'SKILL.md'): Promise<{
    skill: string;
    path: string;
    source: 'override' | 'bundled';
    content: string;
  }> {
    const trimmedSkill = skill.trim();
    const descriptor = this.descriptors.get(trimmedSkill);
    if (!descriptor) {
      throw new Error(`Unknown skill "${trimmedSkill}". Available skills: ${this.listSkills().map((s) => s.name).join(', ')}`);
    }
    const cleanPath = normalizeRelativeSkillDocPath(relativePath);
    if (descriptor.source === 'override' && this.overrideReader) {
      const overridden = await this.overrideReader(trimmedSkill, cleanPath);
      if (overridden !== null) {
        return {
          skill: trimmedSkill,
          path: cleanPath,
          source: 'override',
          content: normalizeUpstreamSkillMarkdown(overridden),
        };
      }
    }
    const key = `${trimmedSkill}/${cleanPath}`;
    const raw = BUNDLED_SKILL_FILES[key];
    if (raw === undefined) {
      throw new Error(`Document "${cleanPath}" not found in skill "${trimmedSkill}". Available files: ${descriptor.files.join(', ')}`);
    }
    return {
      skill: trimmedSkill,
      path: cleanPath,
      source: 'bundled',
      content: normalizeUpstreamSkillMarkdown(raw),
    };
  }

  formatIndex(): string {
    return this.listSkills()
      .map((skill) => `- \`${skill.name}\`: ${skill.description}`)
      .join('\n');
  }
}

export function getBundledSkillsManifest(): BundledSkillsManifest {
  return BUNDLED_SKILLS_MANIFEST;
}

export function getBundledSkillEntries(): BundledSkillEntry[] {
  return BUNDLED_SKILLS_MANIFEST.skills;
}
