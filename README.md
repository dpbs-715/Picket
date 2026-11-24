# Picket

Picket is a desktop Git Agent built on Pi Core. Select a repository, worktree, and change scope; Picket inspects external diffs, accepts optional extra instructions, previews editable commit plans, reviews selected code, and splits, commits, and pushes changes in one click via C-Push. It pauses to ask only when a real decision is needed, leaving unselected changes, `Local Keep` files, and unstaged IDE drafts untouched.

## Current Status

**M0–M6 feature development, bundle hardening, MVP acceptance, C-Push streamlining, and Pinned Compact Mode are complete:** live Git repository reads, IDEA-style Local / Remote collapsible branch menus with remote tracking branch checkout, default auto-selection of normal changes with tri-state Select All, fixed repository-wide `Local Keep` group, explicit Pull & Rebase, dynamic local detection and controlled integration for five external IDEs, cross-process cold-restart persistence (`v2`: UI language, per-project editor and commit language, `Local Keep` paths, window pin/compact geometry preferences, extra prompts with automatic v1 draft cleanup, models, and session history), macOS Keychain credential storage, structured code review with user prompt context, minimalist on-demand `+ Prompt` toggle and editable pre-generated commit plans (**Preview Plan**), single-path one-click C-Push split commits, remote pushing, retry-push-only recovery, controlled conflict resolution, pinned `400×~86` 2-row compact bar with expandable project/branch/remote/file drawers and unfocused macOS notifications, 1-click Model Settings jump on model errors, and window-focus reconciliation are implemented and verified.

Picket starts from an empty zero-repository, zero-model state on first launch and restores previously opened repositories, detected preferred editors, saved models, C-Push session history, and per-worktree extra instructions and selections across restarts. Open any local Git working directory, double-click file rows or click the right-side Diff link to open real files, diffs, staged diffs, review line numbers, or conflict merges in an installed IDE (`IntelliJ IDEA`, `RustRover`, `WebStorm`, `VS Code`, `Cursor`), select single or multiple file rows (`Click` / `Cmd+Click` / `Shift+Click`) to batch-move changes into or out of **Local Keep**, switch local branches, check out remote branches, or jump to linked worktrees from the header branch menu, add optional extra instructions, preview and edit a multi-commit plan, run code reviews, or dispatch one-click **C-Push**. Without a configured model, the primary button guides you to **Configure Model**. Controlled Git writes run only when you explicitly trigger branch switching/checkout, remote sync, or C-Push actions.

- Built for macOS with Tauri 2 / Rust + React / TypeScript + an isolated Pi Core sidecar process.
- English UI by default with instant English / Simplified Chinese (`en` / `zh`) switching, plus per-project Preferred Editor and Commit Message Language configuration.
- Streamlined single-path C-Push: optional extra instructions guide C-Push planning and Code Review (never used raw as commit messages and never widening scope); **Preview Plan** generates an editable commit plan below the prompt box whose edited messages are committed verbatim on **C-Push** until scope changes mark it outdated.
- Partially staged files commit only their staged hunks by default; `main` and `master` display a lightweight `Protected` badge inside the branch menu/drawer while allowing direct commits, and detached `HEAD` requires switching to a local branch before committing.
- Push failures retain completed local commits and prompt text, retrying pushing only without re-creating commits.

## Local Documentation & Design

| Document | Purpose |
| --- | --- |
| [Development Spec](docs/development.md) | Confirmed product scope, authorization model, safety rules, and implementation boundaries. |
| [UI Specification](docs/ui-spec.md) | Screen hierarchy, fields, interactions, and boundaries between production UI and demo controls. |
| [Development Plan](docs/development-plan.md) | Milestone breakdown, acceptance checklist, and engineering workflow. |
| [Frozen UI Prototype](docs/design/picket-mvp.tsx) | Unmodified copy of the visual design reference (only the left desktop window represents the product UI). |

## Running Locally

Verified on macOS arm64 with Node 24.17.0, pnpm 10.28.1, `rustup`, and Xcode Command Line Tools. The Rust toolchain is pinned in `rust-toolchain.toml`; desktop scripts prepend `~/.cargo/bin` automatically without modifying global shell profiles.

```bash
pnpm install --frozen-lockfile
pnpm desktop dev
```

Click **Open Local Repository…** to choose a Git working directory; linked worktrees and branches checked out elsewhere can be jumped to directly. Refreshing, switching worktrees, and refocusing the window re-read live Git status. Checkboxes update UI selection only and never touch `.git/index`. Partially staged files display staged line stats, and ahead/behind counts read local tracking refs without implicit network fetches. Files dragged or moved via context menu into the bottom **Local Keep** group stay excluded from Select All, commits, C-Push, and AI scopes across all worktrees of the repository unless explicitly checked.

The second header row exposes a single branch selector that opens an IDEA-style **Local Branches** and **Remote Branches** collapsible menu (plus **Worktrees** when multiple worktrees exist): local branches checked out in another worktree jump directly to that worktree, while clicking a remote branch switches to an existing local branch or checks out a new local tracking branch. Creating brand-new branches or worktrees by custom name stays in your IDE or terminal and is picked up automatically when Picket regains focus. Dirty worktrees may switch directly only to branches pointing at the same commit; switching across commits requires resolving local changes in Git or your IDE first without automatic stashing.

All branch switches, local commits, and remote sync/push operations verify live Git state before and after execution. Detached `HEAD` blocks commits until a local branch is active (`main` and `master` show a `Protected` badge inside the branch menu/drawer and allow direct commits/pushes; automatic force-push is never performed). Explicit **Pull & Rebase** and in-session C-Push sync share a lossless dirty-worktree backup-and-rebase path (non-overlapping staged and unstaged changes are backed up and restored byte-for-byte; overlapping edits or published-history rewrites pause safely). If local commits succeed but pushing fails, **Retry Push** pushes only existing commits; if rebase hits a conflict, you can authorize Agent merge resolution (with conflict-marker and path-boundary validation) or merge manually in your external IDE and return to Picket for automatic reconciliation. Interrupted or crashed sessions reconcile against live Git history before offering user-confirmed recovery.

Native macOS traffic-light window controls share the top header row alongside live window pinning and **Pinned Compact Mode** (collapsing the `main` window into a `400×~86` 2-row bar with fixed top-left coordinates, temporary project-switcher, branch-switcher, remote-details (`Pull & Rebase`), and file-checklist drawers, one-click target-aware expand, and unfocused macOS attention notifications without auto-resizing). App language, per-project editor and commit language preferences, `Local Keep` paths (keyed by canonical Git common dir), window pin/compact geometry preferences (with multi-monitor bounds fallback), opened repositories, model metadata, C-Push session history, and per-worktree extra instructions, selections, and active tabs persist atomically under the local app data directory; model API keys are stored exclusively in the macOS Keychain via `Security.framework`, never in plain config files, frontend state, or error messages, and Picket never loads host environment keys or developer Pi logins.

```bash
pnpm check                # TypeScript, Node tests, Rust fmt/clippy, and Rust integration tests
pnpm ui:verify            # Headless Chrome UI interaction suite using isolated development fixtures
pnpm desktop build        # Build macOS application bundle
pnpm package:verify --ui  # End-to-end bundle verification in isolated temp dirs (opens and closes app window)
```

Built application bundle: `src-tauri/target/release/bundle/macos/Picket.app` (Apple Developer ID signing, notarization, and non-arm64 macOS targets are not configured in this local build).

Validation records: [M0](docs/validation/m0.md), [M1](docs/validation/m1.md), [Titlebar](docs/validation/m1-titlebar.md), [M2 Read](docs/validation/m2-read.md), [M2 Branches](docs/validation/m2-branches.md), [M2 Commit](docs/validation/m2-commit.md), [M2 IDE & Persistence](docs/validation/m2-ide-persistence.md), [M3 Models & AI](docs/validation/m3.md), [M4 C-Push](docs/validation/m4.md), [M5 Sync & Conflict](docs/validation/m5.md), [M6 Release Acceptance](docs/validation/m6.md), [Local Keep](docs/validation/local-keep.md), [C-Push Streamlining](docs/validation/cpush-streamline.md), and [Pinned Compact Mode](docs/validation/compact-mode.md).

### Standalone Browser Preview & Fixtures

Run `pnpm dev` and open `http://127.0.0.1:1420/dev.html?scenario=question` to inspect development scenarios documented in [M1](docs/validation/m1.md); `scenario=foundation` opens the M0 diagnostic harness. Development fixtures are excluded from production builds, and `index.html` always boots from a clean empty state.

`pnpm ui:verify` requires a local Chrome binary (override via `CHROME_PATH`) and launches with an isolated temporary user profile.

## Coding Agents & Serena

Coding assistants should read [AGENTS.md](AGENTS.md) first. Serena is configured in `.serena/project.yml` for TypeScript / TSX / JavaScript and Rust, with per-client project-scoped MCP entries resolved relative to the current checkout:

| Client | Project Config | Serena Context |
| --- | --- | --- |
| Pi MCP Adapter | `.pi/mcp-adapter.json` | `agent` (`lazy-keep-alive` with direct tools enabled). |
| Codex | `.codex/config.toml` | `codex` (symbol navigation paired with Codex native file/shell tools). |
| Antigravity | `.agents/plugins/picket-serena/` | `antigravity` (exposes `picket-serena` via workspace plugin `mcp_config.json`). |

No root-level Serena MCP entry is defined, and global user MCP configs are untouched.

Serena CLI must be installed locally. Rust LSP components are declared in `rust-toolchain.toml`, and Serena manages its TypeScript language server. Because Codex and Antigravity plugin MCP environments do not interpolate shell variables, their launchers invoke `/bin/sh` to prepend `~/.cargo/bin` and `~/.local/bin` at startup without hard-coding machine paths or editing shell profiles.

```bash
# Rebuild symbol index and verify language servers from the repository root
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH" serena project index .
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH" serena project health-check .
```

Serena caches, logs, memories, and machine-local `project.local.yml` stay gitignored and excluded from app bundles. See [Serena Validation](docs/validation/serena.md) for setup and migration details.
