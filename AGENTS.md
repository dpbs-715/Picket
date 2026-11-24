# Picket Development Guidelines

This file applies to the entire repository and guides coding assistants developing Picket; it is not the system prompt for Picket's built-in Agent. Any `AGENTS.md` inside a subdirectory applies only to that subtree.

## Getting Started

- Follow the user's current language in conversation. State clearly what was actually completed, verified, and left unverified; never present plans, existing configuration, or simulated output as implemented features.
- Keep all repository files outside `docs/` in English (including code, tests, comments, prompts, configuration files, `AGENTS.md`, and `README.md`), except the runtime Chinese localization catalogs in `src/i18n/messages.ts`, `agent/src/i18n.ts`, and `src-tauri/src/i18n.rs`.
- Read `README.md` and relevant modules first; milestones and acceptance criteria follow `docs/development-plan.md`. M0–M6 feature implementation, release hardening, MVP acceptance, C-Push streamlining, and Pinned Compact Mode are complete (repository reads, IDEA-style Local / Remote collapsible branch menu and remote tracking branch checkout, default auto-selection of normal changes with tri-state Select All, fixed repository-wide `Local Keep` group, internal exact-scope local commits, dynamic detection and controlled launch of five external IDEs, cross-process cold-restart prompt, project settings, and window/compact geometry persistence with silent v1 draft cleanup, five model providers, macOS Keychain storage, structured code review with user prompt context, minimalist on-demand `+ Prompt` toggle and editable pre-generated C-Push commit plans, single-path one-click C-Push split commits and Q&A recovery, pull-with-rebase, retry-push-only on push failure, controlled conflict resolution, pinned `400×~86` 2-row compact bar with expandable project/branch/remote/file drawers, top-left-anchored resize, and unfocused macOS notifications, focus reconciliation, 1-click Model Settings jump on model errors, and bundled Pi Core live `kimi-k2.8` inference verification — see `docs/validation/m6.md`, `docs/validation/local-keep.md`, `docs/validation/cpush-streamline.md`, and `docs/validation/compact-mode.md`). Keep documentation and regression tests synchronized on subsequent changes.
- Product behavior is specified in `docs/development.md`, screen hierarchy in `docs/ui-spec.md`, and the visual baseline in `docs/design/picket-mvp.tsx`. The right-side demo console and simulated auto-fix in the design prototype are not production features; never ship the prototype file directly as the production UI.
- `docs/` is maintained locally under `.gitignore`. If `docs/` is missing, explain what specification is absent and ask the user rather than inventing requirements or removing the `.gitignore` entry.
- Check `git status` first and preserve existing user changes. Committing, pushing, resetting, cleaning, and branch operations require explicit user authorization; Picket's in-product C-Push authorization rules do not authorize coding assistants to mutate this repository's Git history.

## Architecture & Responsibilities

- Initial target platform is macOS; current hardware verification is arm64. Check `package.json` for Node / pnpm versions, `rust-toolchain.toml` for Rust, and the lockfiles for exact dependency versions.
- `src/`: React + TypeScript UI organized by feature; `src/desktop/` wraps typed desktop commands and events; `src/i18n/` provides English (default) and Chinese UI localization.
- `shared/`: Shared protocols and identity contracts between the UI and the Node sidecar; corresponding Rust types must stay synchronized and be validated on both sides whenever protocols change.
- `agent/`: Standalone Pi Core sidecar process; never execute models or store API keys inside the WebView.
- `src-tauri/`: Rust desktop coordinator responsible for Git reads, controlled writes, authorization checks, system integration, process lifecycle, and recovery. `src-tauri/src/repository/`, `src-tauri/src/models/`, and `src-tauri/src/cpush/` currently provide: reading user-selected repositories; switching local branches, checking out remote tracking branches, and jumping to existing worktrees with `branchToken` (manual new-branch and new-worktree creation were intentionally removed in favor of IDEA-style remote branch checkout); internal exact-scope local commits authorized by per-file `commitToken`s; dynamic local detection and controlled launch for five external IDEs (`IntelliJ IDEA`, `RustRover`, `WebStorm`, `VS Code`, `Cursor`); local persistence (`v2`) of UI language, per-project preferred editor and commit message language, canonical-`common_dir`-keyed `Local Keep` paths, window pin/compact geometry preferences, per-worktree extra prompts/selections, model catalog, and C-Push session history; `Security.framework` Keychain credential storage; macOS background attention notifications (`tauri-plugin-notification`) with `RunEvent::Reopen` window focus restoration; connection testing and structured code review via bundled Pi Core; and editable pre-generated C-Push commit plans (`plan_cpush`), verbatim confirmed-plan execution, one-click C-Push split commits, explicit "Pull & Rebase", lossless dirty-worktree backup across rebase, retry-push-only recovery, user-authorized Agent conflict resolution, and external IDE manual-merge reconciliation (zero-model and manual commit UI/IPC paths were intentionally removed so C-Push is the sole commit & push path). `branchToken` never authorizes commits, and UI `scopeVersion` is never accepted as write authorization.
- `scripts/`: Entry points for development, packaging, and verification; never concatenate shell commands in frontend components.
- `src/features/foundation/` and `src-tauri/src/foundation/` are M0 diagnostic probes. Candidate Git executors there run strictly against disposable temporary repositories and reject executable hooks; never expose them directly to user repositories without production validation.
- Split code by responsibility and colocate tests with their owning module; do not add speculative shared layers or empty directories. Never manually edit third-party dependencies, generated files, caches, or build artifacts.

## Non-Negotiable Invariants

- C-Push is the sole commit and push workflow: without a configured model, commits cannot run and the primary footer button guides the user to configure a model. When a model is configured, C-Push runs in a single click (analyzing authorized scope, splitting, generating messages, committing, and pushing) or executes a valid user-edited Preview Plan verbatim without re-planning; pause to ask only when a real decision is required, and never add mandatory confirmation dialogs to every commit. The prompt input carries user extra instructions only (shared with C-Push planning and Code Review), never raw commit messages and never widening authorized scope.
- Checkboxes express UI selection only and never mutate `.git/index`. Partially staged files default to committing only their staged hunks; unselected staged files must never leak into commits, and unstaged working-tree drafts must be preserved byte-for-byte.
- Fixed `Local Keep` paths are excluded from Select All, C-Push, Preview Plan, and Code Review unless explicitly checked by the user; they are persisted locally per repository by canonicalized Git common dir without touching `.git/index`, `.git/info/exclude`, `.gitignore`, `skip-worktree`, or `assume-unchanged`.
- `main` / `master` display a lightweight `Protected` badge inside the branch menu/drawer (without cluttering the outer header row) while allowing direct commits and pushes (if a remote branch protection rule rejects the push, preserve local commits and prompt to switch branches); detached `HEAD` must switch to a local branch before committing; never auto-force-push, discard user modifications, or rewrite published remote history.
- Rust is the sole controlled Git write entry point, and Git is the source of truth. Neither the UI nor the model may declare a commit successful on its own, and `git add -A` must never replace exact scope construction.
- Asynchronous tasks bind `repositoryId`, `worktreeId`, `sessionId`, `requestId`, and `scopeVersion`; switching views, retrying, cancelling, or receiving late responses must never cross targets, widen permissions, overwrite newer prompts/plans, or duplicate commits. Outdated Preview Plans (`baseScopeVersion !== scopeVersion`) must never be committed directly and are discarded on C-Push in favor of fresh planning.
- When pushing fails after local commits succeed, preserve completed commits (and preserve the prompt text) and retry pushing only. Recovery after interruption must reconcile against live Git history first instead of blindly re-running from step one.
- While the user is typing extra instructions or editing a Preview Plan message, incoming messages must never steal focus, auto-expand over the editor, or alter user text. Expanded message cards cover only the lower workspace.
- The Agent may modify conflicted files only after the user explicitly authorizes conflict resolution for the active conflict; code review findings never authorize automatic edits to business source files.
- Store credentials exclusively in the macOS Keychain; never write real API keys into IPC payloads, logs, plain config files, or test fixtures. Never reuse a developer's local Pi login or auto-load external host resources into the product Agent.

## Development & Verification Commands

Run from the repository root with `pnpm` (do not create lockfiles from other package managers):

```bash
pnpm install --frozen-lockfile
pnpm desktop dev          # Desktop development; `pnpm dev` is browser preview only
pnpm desktop fmt          # Format Rust sources
pnpm check                # TypeScript typecheck, Node tests, Rust fmt/clippy, and Rust tests
pnpm ui:verify            # Headless Chrome interaction suite using isolated fixtures (no real Git or live model)
pnpm desktop build        # Build macOS application bundle
pnpm package:verify --ui   # Verify latest .app bundle, disposable Git reads, IDE diff export, branch/worktree switching, exact-scope commits, cold-restart recovery, Keychain model storage, Pi generate/review, C-Push split/push, and conflict merge (requires desktop session; opens and closes window)
```

- `scripts/desktop.mjs` prepends `~/.cargo/bin` explicitly; do not modify the user's shell profile to run checks.
- All Git tests must run against temporary repositories and local `bare` remotes; never run destructive Git tests against this repository or user business repositories.
- After modifying UI, state, Rust, or shared protocols, run relevant tests and `pnpm check`. When changing packaging, process lifecycle, bundled runtimes, or dependencies, also rebuild and verify the macOS `.app` bundle. Explicitly list any check that could not be executed.
- Both Node tests and desktop checks write to the same sidecar staging directory; never run commands that invoke `sidecar:prepare` in parallel.
- Report test fixtures, scripted local providers, and live online model services separately; a single passing command does not imply full MVP acceptance. Historical validation records live in `docs/validation/`.
- Preserve M0 regressions covering scope preservation, idempotency, cancellation timing, and child-process cleanup on exit; never weaken lints, skip Git hooks, or relax acceptance assertions to mask failures.

## Serena & Repository Maintenance

- Serena project configuration lives in `.serena/project.yml`, supporting TypeScript / TSX / JavaScript and Rust. Pi uses `.pi/mcp-adapter.json`, Codex uses `.codex/config.toml`, and Antigravity uses the workspace plugin `.agents/plugins/picket-serena/` (`plugin.json` + `mcp_config.json`); no root-level Serena server entry is defined. Always resolve paths from the active checkout without hard-coding machine usernames or stale directories.
- Pi runs Serena with the `agent` context, Codex with `codex`, and Antigravity with `antigravity`. Each client spawning its own Serena process is expected; do not add duplicate root entries or copy Pi-specific lifecycle fields into Codex or Antigravity configs.
- Prefer Serena for symbol and reference lookup when connected, then read targeted implementations; fall back to `rg` and file reads when unavailable, and never claim Serena is connected merely because config files exist.
- Serena respects `.gitignore`. `docs/` and design prototypes are excluded from symbol indexing — read them directly with file tools rather than treating empty symbol results as missing files.
- Keep caches, logs, memories, and `project.local.yml` local-only; shared configs and this file may be included in future commits when authorized, though tasks do not authorize committing on their own.
- After modifying Serena configs, verify symbol/reference resolution across both TypeScript and Rust (`~/.cargo/bin` must be on `PATH` for indexing and health checks). First-run language server downloads are acceptable, without altering global user preferences or other projects.
- Summarize completed changes, verification outcomes, and remaining limitations when done. Confirm with the user before changing frozen UI specs, permissions, target platforms, or milestone scope.
