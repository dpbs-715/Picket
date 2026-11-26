//! Three-tier controlled CLI security policy and permanent hard redlines for `run_git` and
//! `run_platform_cli` (`gh` / `glab`).
//!
//! - Tier 0 (`AllowTier0`): Read-only inspection commands (auto-allowed).
//! - Tier 1 (`AllowTier1`): Routine in-scope write commands that pass every permanent redline.
//! - Tier 2 (`RequireConfirmTier2`): Destructive, irreversible, or unconventional custom write
//!   commands requiring explicit user confirmation via `ask_user_question`.
//! - Hard Redline (`DenyRedline`): Permanently blocked regardless of skills, model output, or
//!   "don't ask again" preferences.

use super::types::RemoteRepoIdentity;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandDecision {
    AllowTier0,
    AllowTier1,
    RequireConfirmTier2 {
        reason: String,
        preview: String,
        allow_dont_ask_again: bool,
    },
    DenyRedline {
        code: &'static str,
        reason: String,
    },
}

fn format_preview(binary: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(binary.to_owned());
    for arg in args {
        if arg.is_empty()
            || arg
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '$' | '`' | ';'))
        {
            parts.push(format!("'{}'", arg.replace('\'', "'\\''")));
        } else {
            parts.push(arg.clone());
        }
    }
    parts.join(" ")
}

fn is_40_hex_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_valid_verified_lease(value: &str) -> bool {
    // Require explicit `<ref>:<40-hex-sha>` (never bare `--force-with-lease`).
    let Some((ref_name, sha)) = value.split_once(':') else {
        return false;
    };
    !ref_name.trim().is_empty()
        && !matches!(
            ref_name.trim(),
            "main" | "master" | "refs/heads/main" | "refs/heads/master"
        )
        && is_40_hex_sha(sha.trim())
}

fn is_protected_ref_arg(arg: &str) -> bool {
    let clean = arg.trim().trim_start_matches('+');
    let target = clean.split_once(':').map(|(_, rhs)| rhs).unwrap_or(clean);
    matches!(
        target,
        "main" | "master" | "refs/heads/main" | "refs/heads/master"
    )
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter()
        .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
}

fn flag_value<'a>(args: &'a [String], names: &[&str]) -> Option<&'a str> {
    for (idx, arg) in args.iter().enumerate() {
        for &name in names {
            if arg == name {
                return args.get(idx + 1).map(String::as_str);
            }
            if let Some(rest) = arg.strip_prefix(&format!("{name}=")) {
                return Some(rest);
            }
        }
    }
    None
}

fn has_short_flag_char(args: &[String], needle: char) -> bool {
    for arg in args {
        if arg == "--" {
            break;
        }
        if arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.len() > 1
            && arg[1..].chars().any(|c| c == needle)
        {
            return true;
        }
    }
    false
}

pub fn classify_git_command(args: &[String]) -> CommandDecision {
    if args.is_empty() {
        return CommandDecision::DenyRedline {
            code: "empty_args",
            reason: "Git command arguments cannot be empty.".into(),
        };
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return CommandDecision::DenyRedline {
            code: "invalid_nul",
            reason: "Git command arguments cannot contain NUL bytes.".into(),
        };
    }

    // Parse leading global git flags before the subcommand.
    let mut idx = 0usize;
    while idx < args.len() && args[idx].starts_with('-') {
        let flag = args[idx].as_str();
        match flag {
            "--no-pager" | "--no-optional-locks" | "--literal-pathspecs" => {
                idx += 1;
            }
            "-c" | "--config-env" | "--git-dir" | "--work-tree" | "--exec-path" | "-C" => {
                return CommandDecision::DenyRedline {
                    code: "forbidden_git_global_flag",
                    reason: format!(
                        "Global Git override flag `{flag}` is forbidden; commands always run inside the active worktree with Picket's security policy."
                    ),
                };
            }
            other
                if other.starts_with("--git-dir=")
                    || other.starts_with("--work-tree=")
                    || other.starts_with("--exec-path=")
                    || other.starts_with("-c")
                    || other.starts_with("-C") =>
            {
                return CommandDecision::DenyRedline {
                    code: "forbidden_git_global_flag",
                    reason: format!(
                        "Global Git override flag `{other}` is forbidden inside controlled `run_git`."
                    ),
                };
            }
            _ => {
                return CommandDecision::DenyRedline {
                    code: "unknown_git_global_flag",
                    reason: format!("Unsupported global Git flag before subcommand: `{flag}`."),
                };
            }
        }
    }

    let Some(subcommand) = args.get(idx).map(String::as_str) else {
        return CommandDecision::DenyRedline {
            code: "missing_subcommand",
            reason: "Missing Git subcommand.".into(),
        };
    };
    let rest = &args[idx + 1..];
    let preview = format_preview("git", args);

    match subcommand {
        // Commits & staging MUST flow through `commit_exact_scope` so partial staging,
        // unselected files, and `Local Keep` stay protected.
        "add" | "commit" | "stage" | "rm" | "mv" => CommandDecision::DenyRedline {
            code: "use_commit_exact_scope",
            reason: format!(
                "`git {subcommand}` is blocked in `run_git` to protect partial staging, unselected files, and Local Keep. Use the `commit_exact_scope` tool for local commits instead."
            ),
        },

        // Permanent destructive redlines:
        "reset" => {
            if has_flag(rest, "--hard") || has_flag(rest, "--merge") || has_flag(rest, "--keep") {
                CommandDecision::DenyRedline {
                    code: "destructive_reset",
                    reason: "`git reset --hard/--merge/--keep` is permanently blocked to prevent discarding user changes.".into(),
                }
            } else {
                CommandDecision::RequireConfirmTier2 {
                    reason: "Resetting Git index/HEAD state requires explicit user confirmation."
                        .into(),
                    preview,
                    allow_dont_ask_again: false,
                }
            }
        }
        "clean" => {
            if has_flag(rest, "--force") || has_short_flag_char(rest, 'f') {
                CommandDecision::DenyRedline {
                    code: "destructive_clean",
                    reason: "`git clean -f` is permanently blocked to prevent deleting untracked user files.".into(),
                }
            } else if has_flag(rest, "--dry-run") || has_short_flag_char(rest, 'n') {
                CommandDecision::AllowTier0
            } else {
                CommandDecision::DenyRedline {
                    code: "destructive_clean",
                    reason: "`git clean` without `--dry-run` is permanently blocked.".into(),
                }
            }
        }
        "restore" => {
            if rest
                .iter()
                .any(|a| a == "." || a == "--worktree" || a == "-W")
            {
                CommandDecision::DenyRedline {
                    code: "destructive_restore",
                    reason:
                        "Discarding working-tree edits via `git restore` is permanently blocked."
                            .into(),
                }
            } else {
                CommandDecision::DenyRedline {
                    code: "destructive_restore",
                    reason: "`git restore` is blocked in `run_git` to preserve working-tree and index edits.".into(),
                }
            }
        }
        "filter-branch" | "filter-repo" | "gc" | "prune" | "reflog" | "credential" => {
            CommandDecision::DenyRedline {
                code: "forbidden_git_maintenance",
                reason: format!(
                    "`git {subcommand}` is permanently blocked in controlled `run_git`."
                ),
            }
        }
        "config" => {
            if has_flag(rest, "--global") || has_flag(rest, "--system") {
                return CommandDecision::DenyRedline {
                    code: "forbidden_global_config",
                    reason: "Modifying global or system Git configuration is permanently blocked."
                        .into(),
                };
            }
            let is_read = has_flag(rest, "--get")
                || has_flag(rest, "--get-all")
                || has_flag(rest, "--get-regexp")
                || has_flag(rest, "--list")
                || has_short_flag_char(rest, 'l');
            if is_read {
                CommandDecision::AllowTier0
            } else {
                CommandDecision::RequireConfirmTier2 {
                    reason: "Modifying local repository Git configuration requires confirmation."
                        .into(),
                    preview,
                    allow_dont_ask_again: true,
                }
            }
        }

        // Tier 0 pure read-only inspection subcommands:
        "status" | "diff" | "log" | "rev-list" | "rev-parse" | "show" | "ls-files"
        | "ls-remote" | "describe" | "shortlog" | "merge-base" | "for-each-ref" | "name-rev"
        | "cat-file" | "diff-tree" | "show-ref" | "symbolic-ref" | "check-ignore" => {
            if subcommand == "symbolic-ref"
                && rest.iter().filter(|a| !a.starts_with('-')).count() > 1
            {
                return CommandDecision::DenyRedline {
                    code: "forbidden_symbolic_ref_write",
                    reason: "Mutating symbolic refs directly is blocked.".into(),
                };
            }
            CommandDecision::AllowTier0
        }

        "fetch" => {
            if has_flag(rest, "--force") || has_short_flag_char(rest, 'f') {
                return CommandDecision::RequireConfirmTier2 {
                    reason: "Force-fetching refs requires explicit confirmation.".into(),
                    preview,
                    allow_dont_ask_again: true,
                };
            }
            CommandDecision::AllowTier0
        }

        "remote" => {
            let sub = rest.first().map(String::as_str).unwrap_or("");
            if sub.is_empty() || matches!(sub, "-v" | "--verbose" | "show" | "get-url") {
                CommandDecision::AllowTier0
            } else {
                CommandDecision::RequireConfirmTier2 {
                    reason: "Modifying Git remotes requires explicit confirmation.".into(),
                    preview,
                    allow_dont_ask_again: false,
                }
            }
        }

        "branch" => {
            let has_delete = has_flag(rest, "--delete")
                || has_short_flag_char(rest, 'd')
                || has_short_flag_char(rest, 'D');
            if has_delete {
                if rest
                    .iter()
                    .filter(|a| !a.starts_with('-'))
                    .any(|a| is_protected_ref_arg(a))
                {
                    return CommandDecision::DenyRedline {
                        code: "protected_branch_delete",
                        reason: "Deleting protected local branch `main`/`master` is permanently blocked.".into(),
                    };
                }
                return CommandDecision::RequireConfirmTier2 {
                    reason: "Deleting a local Git branch requires explicit user confirmation."
                        .into(),
                    preview,
                    allow_dont_ask_again: false,
                };
            }
            let has_rename_or_force = has_flag(rest, "--move")
                || has_flag(rest, "--copy")
                || has_flag(rest, "--force")
                || has_flag(rest, "--set-upstream-to")
                || has_flag(rest, "--unset-upstream")
                || has_short_flag_char(rest, 'm')
                || has_short_flag_char(rest, 'M')
                || has_short_flag_char(rest, 'c')
                || has_short_flag_char(rest, 'C')
                || has_short_flag_char(rest, 'f');
            if has_rename_or_force {
                return CommandDecision::RequireConfirmTier2 {
                    reason: "Renaming, force-updating, or reconfiguring a Git branch requires confirmation.".into(),
                    preview,
                    allow_dont_ask_again: true,
                };
            }
            let is_read_only = rest.is_empty()
                || has_flag(rest, "--list")
                || has_flag(rest, "--all")
                || has_flag(rest, "--remotes")
                || has_flag(rest, "--show-current")
                || has_flag(rest, "--contains")
                || has_flag(rest, "--merged")
                || has_flag(rest, "--no-merged")
                || has_short_flag_char(rest, 'l')
                || has_short_flag_char(rest, 'a')
                || has_short_flag_char(rest, 'r')
                || has_short_flag_char(rest, 'v');
            if is_read_only {
                CommandDecision::AllowTier0
            } else {
                CommandDecision::AllowTier1
            }
        }

        "tag" => {
            let has_delete = has_flag(rest, "--delete") || has_short_flag_char(rest, 'd');
            if has_delete {
                return CommandDecision::RequireConfirmTier2 {
                    reason: "Deleting a Git tag requires explicit user confirmation.".into(),
                    preview,
                    allow_dont_ask_again: false,
                };
            }
            let has_force = has_flag(rest, "--force") || has_short_flag_char(rest, 'f');
            if has_force {
                return CommandDecision::RequireConfirmTier2 {
                    reason: "Overwriting an existing Git tag with `--force` requires confirmation."
                        .into(),
                    preview,
                    allow_dont_ask_again: true,
                };
            }
            let is_read_only = rest.is_empty()
                || has_flag(rest, "--list")
                || has_flag(rest, "--verify")
                || has_flag(rest, "--contains")
                || has_flag(rest, "--points-at")
                || has_flag(rest, "--sort")
                || has_short_flag_char(rest, 'l')
                || has_short_flag_char(rest, 'n')
                || has_short_flag_char(rest, 'v');
            if is_read_only {
                CommandDecision::AllowTier0
            } else {
                // Creating a new lightweight or annotated tag (e.g. `git tag v0.2.0` or `git tag -a v0.2.0 -m "..."`)
                CommandDecision::AllowTier1
            }
        }

        "switch" | "checkout" => {
            if has_flag(rest, "--force")
                || has_flag(rest, "--discard-changes")
                || has_short_flag_char(rest, 'f')
                || rest.iter().any(|a| a == "--" || a == ".")
            {
                return CommandDecision::DenyRedline {
                    code: "destructive_checkout",
                    reason: format!(
                        "`git {subcommand}` with `--force`, `--discard-changes`, or file pathspecs (`--` / `.`) is permanently blocked to prevent discarding user edits."
                    ),
                };
            }
            CommandDecision::AllowTier1
        }

        "push" => {
            if has_flag(rest, "--mirror") || has_flag(rest, "--prune") {
                return CommandDecision::DenyRedline {
                    code: "destructive_push",
                    reason: "`git push --mirror` and `--prune` are permanently blocked.".into(),
                };
            }
            let has_raw_force = has_flag(rest, "--force")
                || has_short_flag_char(rest, 'f')
                || rest.iter().any(|a| a.starts_with('+') && a.len() > 1);
            if has_raw_force {
                return CommandDecision::DenyRedline {
                    code: "forbidden_force_push",
                    reason: "Force-pushing (`--force` / `-f` / `+refspec`) is permanently blocked. Only verified `--force-with-lease=<non-protected-ref>:<40-hex-sha>` is permitted.".into(),
                };
            }
            let mut has_verified_lease = false;
            if has_flag(rest, "--force-with-lease") {
                let Some(lease_val) = flag_value(rest, &["--force-with-lease"]) else {
                    return CommandDecision::DenyRedline {
                        code: "unverified_force_with_lease",
                        reason: "Bare `--force-with-lease` without an explicit `<ref>:<40-hex-sha>` is permanently blocked.".into(),
                    };
                };
                if !is_valid_verified_lease(lease_val) {
                    return CommandDecision::DenyRedline {
                        code: "unverified_force_with_lease",
                        reason: "Refused `--force-with-lease`: requires `<non-protected-ref>:<40-char-hex-sha>`.".into(),
                    };
                }
                has_verified_lease = true;
            }
            let has_delete = has_flag(rest, "--delete")
                || has_short_flag_char(rest, 'd')
                || rest.iter().any(|a| a.starts_with(':') && a.len() > 1);
            if has_delete {
                if rest.iter().any(|a| is_protected_ref_arg(a)) {
                    return CommandDecision::DenyRedline {
                        code: "protected_branch_delete",
                        reason: "Deleting remote `main`/`master` is permanently blocked.".into(),
                    };
                }
                return CommandDecision::RequireConfirmTier2 {
                    reason: if has_verified_lease {
                        "Deleting a verified remote development branch with `--force-with-lease` requires explicit confirmation."
                    } else {
                        "Deleting a remote branch or tag requires explicit user confirmation."
                    }
                    .into(),
                    preview,
                    allow_dont_ask_again: false,
                };
            }
            if has_verified_lease {
                return CommandDecision::RequireConfirmTier2 {
                    reason: "Pushing with `--force-with-lease` rewrites a remote branch and requires explicit confirmation.".into(),
                    preview,
                    allow_dont_ask_again: false,
                };
            }
            // Normal non-force push (branch or tag):
            CommandDecision::AllowTier1
        }

        "worktree" => {
            let sub = rest.first().map(String::as_str).unwrap_or("");
            match sub {
                "" | "list" => CommandDecision::AllowTier0,
                "remove" => {
                    if has_flag(rest, "--force") || has_short_flag_char(rest, 'f') {
                        CommandDecision::DenyRedline {
                            code: "destructive_worktree_remove",
                            reason: "`git worktree remove --force` is permanently blocked. Use `manage_submit_worktree(op=\"remove\")` after verifying cleanliness.".into(),
                        }
                    } else {
                        CommandDecision::RequireConfirmTier2 {
                            reason: "Removing a Git worktree directly requires confirmation (prefer `manage_submit_worktree(op=\"remove\")` for Picket-owned submit worktrees).".into(),
                            preview,
                            allow_dont_ask_again: false,
                        }
                    }
                }
                _ => CommandDecision::RequireConfirmTier2 {
                    reason: format!(
                        "Running `git worktree {sub}` directly requires confirmation (prefer `manage_submit_worktree` so Picket tracks ownership and preflight snapshots)."
                    ),
                    preview,
                    allow_dont_ask_again: false,
                },
            }
        }

        // Unconventional custom Git writes (cherry-pick, revert, merge, notes, etc.) -> Tier 2 confirm with preview:
        "revert" | "cherry-pick" | "merge" | "notes" => CommandDecision::RequireConfirmTier2 {
            reason: format!(
                "Custom Git write command `git {subcommand}` requires confirmation before execution."
            ),
            preview,
            allow_dont_ask_again: true,
        },

        "pull" | "rebase" => CommandDecision::RequireConfirmTier2 {
            reason: format!(
                "Running `git {subcommand}` directly requires confirmation (prefer `sync_and_push` for lossless dirty-worktree rebase)."
            ),
            preview,
            allow_dont_ask_again: true,
        },

        "stash" => {
            let sub = rest.first().map(String::as_str).unwrap_or("");
            if matches!(sub, "list" | "show") {
                CommandDecision::AllowTier0
            } else if matches!(sub, "drop" | "clear") {
                CommandDecision::DenyRedline {
                    code: "destructive_stash",
                    reason: "`git stash drop/clear` is permanently blocked to avoid losing stashed user work.".into(),
                }
            } else {
                CommandDecision::RequireConfirmTier2 {
                    reason: "Mutating Git stash entries requires confirmation.".into(),
                    preview,
                    allow_dont_ask_again: true,
                }
            }
        }

        _ => CommandDecision::RequireConfirmTier2 {
            reason: format!("Unconventional Git command `git {subcommand}` requires confirmation."),
            preview,
            allow_dont_ask_again: true,
        },
    }
}

fn normalize_repo_slug(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .trim_end_matches(".git");
    trimmed.to_ascii_lowercase()
}

fn repo_arg_matches_allowed(repo_arg: &str, allowed_repos: &[RemoteRepoIdentity]) -> bool {
    if allowed_repos.is_empty() {
        return false;
    }
    let normalized = normalize_repo_slug(repo_arg);
    allowed_repos.iter().any(|allowed| {
        let slug = allowed.repo_path.to_ascii_lowercase();
        let host_slug = format!("{}/{}", allowed.host.to_ascii_lowercase(), slug);
        normalized == slug || normalized == host_slug
    })
}

pub fn classify_platform_cli_command(
    cli: &str,
    args: &[String],
    allowed_repos: &[RemoteRepoIdentity],
) -> CommandDecision {
    if cli != "gh" && cli != "glab" {
        return CommandDecision::DenyRedline {
            code: "unsupported_cli",
            reason: format!("Unsupported platform CLI `{cli}`. Only `gh` and `glab` are allowed."),
        };
    }
    if args.is_empty() {
        return CommandDecision::DenyRedline {
            code: "empty_args",
            reason: format!("`{cli}` arguments cannot be empty."),
        };
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return CommandDecision::DenyRedline {
            code: "invalid_nul",
            reason: "CLI arguments cannot contain NUL bytes.".into(),
        };
    }

    let preview = format_preview(cli, args);

    // Block credential-exfiltrating auth commands permanently:
    if args.first().map(String::as_str) == Some("auth") {
        let sub = args.get(1).map(String::as_str).unwrap_or("");
        return match sub {
            "status" => CommandDecision::AllowTier0,
            "token" => CommandDecision::DenyRedline {
                code: "forbidden_auth_token",
                reason: format!(
                    "`{cli} auth token` is permanently blocked to prevent credential exposure."
                ),
            },
            _ => CommandDecision::DenyRedline {
                code: "interactive_auth_blocked",
                reason: format!(
                    "`{cli} auth {sub}` cannot run inside controlled CLI execution. Ask the user via `ask_user_question` to run `{cli} auth login` in their terminal."
                ),
            },
        };
    }

    // Cross-repository hard redline: if `-R` / `--repo` is supplied, it MUST match a remote of the open repo.
    if let Some(repo_arg) = flag_value(args, &["-R", "--repo"])
        && !repo_arg_matches_allowed(repo_arg, allowed_repos)
    {
        return CommandDecision::DenyRedline {
            code: "cross_repo_mismatch",
            reason: format!(
                "Refused `{cli}` targeting repository `{repo_arg}` because it does not match any remote of the currently open repository."
            ),
        };
    }

    // Version / help probes are Tier 0:
    if args
        .iter()
        .any(|a| a == "--version" || a == "--help" || a == "-h")
        && !args.iter().any(|a| a == "merge" || a == "delete")
    {
        return CommandDecision::AllowTier0;
    }

    let first = args[0].as_str();
    let second = args.get(1).map(String::as_str).unwrap_or("");

    // `api` endpoint inspection:
    if first == "api" {
        let method = flag_value(args, &["-X", "--method"])
            .map(str::to_ascii_uppercase)
            .unwrap_or_else(|| {
                if has_flag(args, "-f")
                    || has_flag(args, "-F")
                    || has_flag(args, "--field")
                    || has_flag(args, "--raw-field")
                    || has_flag(args, "--input")
                {
                    "POST".into()
                } else {
                    "GET".into()
                }
            });
        // Check endpoint path for cross-repo access (e.g. `repos/other/repo/...` or `projects/other%2Frepo/...`).
        if let Some(endpoint) = args.iter().skip(1).find(|a| !a.starts_with('-')) {
            let lower = endpoint.trim_start_matches('/').to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("repos/") {
                let mut segs = rest.split('/');
                if let (Some(owner), Some(repo)) = (segs.next(), segs.next()) {
                    let target = format!("{owner}/{repo}");
                    if !repo_arg_matches_allowed(&target, allowed_repos) {
                        return CommandDecision::DenyRedline {
                            code: "cross_repo_api_mismatch",
                            reason: format!(
                                "Refused `{cli} api` targeting `{target}` outside the currently open repository."
                            ),
                        };
                    }
                }
            } else if let Some(rest) = lower.strip_prefix("projects/")
                && let Some(project_seg) = rest.split('/').next()
                && !matches!(project_seg, ":id" | ":fullpath")
            {
                let decoded = project_seg.replace("%2f", "/");
                if decoded.contains('/') && !repo_arg_matches_allowed(&decoded, allowed_repos) {
                    return CommandDecision::DenyRedline {
                        code: "cross_repo_api_mismatch",
                        reason: format!(
                            "Refused `{cli} api` targeting GitLab project `{decoded}` outside the currently open repository."
                        ),
                    };
                }
            }
        }
        if method == "GET" {
            return CommandDecision::AllowTier0;
        }
        if method == "DELETE" {
            return CommandDecision::RequireConfirmTier2 {
                reason: format!(
                    "Destructive `{cli} api -X DELETE` requires explicit user confirmation."
                ),
                preview,
                allow_dont_ask_again: false,
            };
        }
        return CommandDecision::RequireConfirmTier2 {
            reason: format!("Mutating `{cli} api` request ({method}) requires confirmation."),
            preview,
            allow_dont_ask_again: true,
        };
    }

    if cli == "gh" {
        return match (first, second) {
            // Tier 0 read-only:
            ("repo", "view")
            | ("issue", "list" | "view" | "status")
            | ("pr", "list" | "view" | "diff" | "checks" | "status")
            | ("label", "list")
            | ("milestone", "list")
            | ("release", "list" | "view")
            | ("run", "list" | "view")
            | ("search", _) => CommandDecision::AllowTier0,

            // Tier 1 routine writes with permanent redline checks:
            ("pr", "merge") => {
                if has_flag(args, "--admin") {
                    return CommandDecision::DenyRedline {
                        code: "forbidden_admin_merge",
                        reason: "`gh pr merge --admin` is permanently blocked; never bypass branch protection or required checks.".into(),
                    };
                }
                let Some(sha) = flag_value(args, &["--match-head-commit"]) else {
                    return CommandDecision::DenyRedline {
                        code: "missing_head_sha_guard",
                        reason: "`gh pr merge` MUST include `--match-head-commit <40-char-head-sha>` to guard against merging unreviewed new commits.".into(),
                    };
                };
                if !is_40_hex_sha(sha.trim()) {
                    return CommandDecision::DenyRedline {
                        code: "invalid_head_sha_guard",
                        reason: "`--match-head-commit` must be a full 40-character commit SHA."
                            .into(),
                    };
                }
                CommandDecision::AllowTier1
            }
            ("issue", "develop") => {
                if has_flag(args, "--list") || has_short_flag_char(args, 'l') {
                    CommandDecision::AllowTier0
                } else {
                    CommandDecision::AllowTier1
                }
            }
            ("issue", "create" | "comment" | "close" | "reopen" | "edit")
            | ("pr", "create" | "comment" | "review" | "edit" | "ready")
            | ("release", "create") => CommandDecision::AllowTier1,

            // Tier 2 high-risk / destructive:
            ("issue", "delete")
            | ("pr", "close")
            | ("release", "delete")
            | ("repo", "delete" | "archive" | "edit") => CommandDecision::RequireConfirmTier2 {
                reason: format!(
                    "High-risk GitHub action `gh {first} {second}` requires explicit confirmation."
                ),
                preview,
                allow_dont_ask_again: false,
            },
            _ => CommandDecision::RequireConfirmTier2 {
                reason: format!(
                    "Unconventional `gh {first} {second}` command requires confirmation."
                ),
                preview,
                allow_dont_ask_again: true,
            },
        };
    }

    // `glab` rules:
    match (first, second) {
        // Tier 0 read-only:
        ("repo", "view")
        | ("issue", "list" | "view")
        | ("mr", "list" | "view" | "diff" | "issues" | "approvers")
        | ("label", "list")
        | ("release", "list" | "view")
        | ("ci", "status" | "view" | "list" | "get") => CommandDecision::AllowTier0,

        // Tier 1 routine writes with permanent SHA guard redlines:
        ("mr", "merge" | "approve") => {
            let Some(sha) = flag_value(args, &["--sha"]) else {
                return CommandDecision::DenyRedline {
                    code: "missing_head_sha_guard",
                    reason: format!(
                        "`glab mr {second}` MUST include `--sha <40-char-head-sha>` to guard against approving or merging unreviewed commits."
                    ),
                };
            };
            if !is_40_hex_sha(sha.trim()) {
                return CommandDecision::DenyRedline {
                    code: "invalid_head_sha_guard",
                    reason: "`--sha` must be a full 40-character commit SHA.".into(),
                };
            }
            CommandDecision::AllowTier1
        }
        ("issue", "create" | "note" | "close" | "reopen" | "update")
        | ("mr", "create" | "note" | "update" | "reopen")
        | ("release", "create") => CommandDecision::AllowTier1,

        // Tier 2 destructive:
        ("issue", "delete")
        | ("mr", "close" | "delete")
        | ("release", "delete")
        | ("repo", "delete" | "archive") => CommandDecision::RequireConfirmTier2 {
            reason: format!(
                "High-risk GitLab action `glab {first} {second}` requires explicit confirmation."
            ),
            preview,
            allow_dont_ask_again: false,
        },
        _ => CommandDecision::RequireConfirmTier2 {
            reason: format!(
                "Unconventional `glab {first} {second}` command requires confirmation."
            ),
            preview,
            allow_dont_ask_again: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vecs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn sample_github_remotes() -> Vec<RemoteRepoIdentity> {
        vec![RemoteRepoIdentity {
            remote: "origin".into(),
            platform: "github",
            host: "github.com".into(),
            repo_path: "dpbs-715/Picket".into(),
            url: "https://github.com/dpbs-715/Picket.git".into(),
        }]
    }

    #[test]
    fn git_policy_enforces_tier0_tier1_tier2_and_hard_redlines() {
        // Tier 0 read-only:
        assert_eq!(
            classify_git_command(&vecs(&["status", "--short"])),
            CommandDecision::AllowTier0
        );
        assert_eq!(
            classify_git_command(&vecs(&["tag", "-l"])),
            CommandDecision::AllowTier0
        );
        assert_eq!(
            classify_git_command(&vecs(&["branch", "--list"])),
            CommandDecision::AllowTier0
        );
        assert_eq!(
            classify_git_command(&vecs(&["fetch", "origin"])),
            CommandDecision::AllowTier0
        );

        // Tier 1 routine writes:
        assert_eq!(
            classify_git_command(&vecs(&["tag", "v0.2.0"])),
            CommandDecision::AllowTier1
        );
        assert_eq!(
            classify_git_command(&vecs(&["tag", "-a", "v0.2.0", "-m", "Release v0.2.0"])),
            CommandDecision::AllowTier1
        );
        assert_eq!(
            classify_git_command(&vecs(&["push", "origin", "v0.2.0"])),
            CommandDecision::AllowTier1
        );
        assert_eq!(
            classify_git_command(&vecs(&["switch", "-c", "feat/issue-42"])),
            CommandDecision::AllowTier1
        );

        // Tier 2 confirmations:
        assert!(matches!(
            classify_git_command(&vecs(&["tag", "-d", "v0.2.0"])),
            CommandDecision::RequireConfirmTier2 {
                allow_dont_ask_again: false,
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["branch", "-D", "feat/old"])),
            CommandDecision::RequireConfirmTier2 {
                allow_dont_ask_again: false,
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["push", "origin", "--delete", "feat/old"])),
            CommandDecision::RequireConfirmTier2 {
                allow_dont_ask_again: false,
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["tag", "-f", "v0.2.0"])),
            CommandDecision::RequireConfirmTier2 {
                allow_dont_ask_again: true,
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["revert", "HEAD"])),
            CommandDecision::RequireConfirmTier2 {
                allow_dont_ask_again: true,
                ..
            }
        ));

        // Permanent hard redlines:
        assert!(matches!(
            classify_git_command(&vecs(&["push", "--force", "origin", "feat/x"])),
            CommandDecision::DenyRedline {
                code: "forbidden_force_push",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["push", "-f", "origin", "feat/x"])),
            CommandDecision::DenyRedline {
                code: "forbidden_force_push",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["push", "--force-with-lease", "origin", "feat/x"])),
            CommandDecision::DenyRedline {
                code: "unverified_force_with_lease",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&[
                "push",
                "--force-with-lease=main:0123456789abcdef0123456789abcdef01234567",
                "origin",
                "main"
            ])),
            CommandDecision::DenyRedline {
                code: "unverified_force_with_lease",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&[
                "push",
                "--force-with-lease=feat/x:0123456789abcdef0123456789abcdef01234567",
                "origin",
                "feat/x"
            ])),
            CommandDecision::RequireConfirmTier2 {
                allow_dont_ask_again: false,
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["reset", "--hard", "HEAD~1"])),
            CommandDecision::DenyRedline {
                code: "destructive_reset",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["clean", "-fd"])),
            CommandDecision::DenyRedline {
                code: "destructive_clean",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["checkout", "--", "."])),
            CommandDecision::DenyRedline {
                code: "destructive_checkout",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["add", "-A"])),
            CommandDecision::DenyRedline {
                code: "use_commit_exact_scope",
                ..
            }
        ));
        assert!(matches!(
            classify_git_command(&vecs(&["-c", "core.hooksPath=/tmp/evil", "tag", "v1"])),
            CommandDecision::DenyRedline {
                code: "forbidden_git_global_flag",
                ..
            }
        ));
    }

    #[test]
    fn platform_cli_policy_enforces_sha_guards_admin_block_and_repo_scope() {
        let remotes = sample_github_remotes();
        let valid_sha = "0123456789abcdef0123456789abcdef01234567";

        // Tier 0 reads:
        assert_eq!(
            classify_platform_cli_command("gh", &vecs(&["pr", "view", "12"]), &remotes),
            CommandDecision::AllowTier0
        );
        assert_eq!(
            classify_platform_cli_command("gh", &vecs(&["auth", "status"]), &remotes),
            CommandDecision::AllowTier0
        );

        // Tier 1 writes with valid guard & matching repo:
        assert_eq!(
            classify_platform_cli_command(
                "gh",
                &vecs(&[
                    "pr",
                    "merge",
                    "12",
                    "--squash",
                    "--match-head-commit",
                    valid_sha,
                    "-R",
                    "dpbs-715/Picket"
                ]),
                &remotes
            ),
            CommandDecision::AllowTier1
        );
        assert_eq!(
            classify_platform_cli_command(
                "glab",
                &vecs(&["mr", "approve", "9", "--sha", valid_sha]),
                &remotes
            ),
            CommandDecision::AllowTier1
        );
        assert_eq!(
            classify_platform_cli_command(
                "glab",
                &vecs(&["mr", "merge", "9", "--sha", valid_sha]),
                &remotes
            ),
            CommandDecision::AllowTier1
        );

        // Hard redlines:
        assert!(matches!(
            classify_platform_cli_command(
                "gh",
                &vecs(&[
                    "pr",
                    "merge",
                    "12",
                    "--admin",
                    "--match-head-commit",
                    valid_sha
                ]),
                &remotes
            ),
            CommandDecision::DenyRedline {
                code: "forbidden_admin_merge",
                ..
            }
        ));
        assert!(matches!(
            classify_platform_cli_command(
                "gh",
                &vecs(&["pr", "merge", "12", "--squash"]),
                &remotes
            ),
            CommandDecision::DenyRedline {
                code: "missing_head_sha_guard",
                ..
            }
        ));
        assert!(matches!(
            classify_platform_cli_command("glab", &vecs(&["mr", "merge", "9"]), &remotes),
            CommandDecision::DenyRedline {
                code: "missing_head_sha_guard",
                ..
            }
        ));
        assert!(matches!(
            classify_platform_cli_command("glab", &vecs(&["mr", "approve", "9"]), &remotes),
            CommandDecision::DenyRedline {
                code: "missing_head_sha_guard",
                ..
            }
        ));
        assert!(matches!(
            classify_platform_cli_command(
                "gh",
                &vecs(&["issue", "list", "--repo", "someone/other"]),
                &remotes
            ),
            CommandDecision::DenyRedline {
                code: "cross_repo_mismatch",
                ..
            }
        ));
        assert!(matches!(
            classify_platform_cli_command("gh", &vecs(&["auth", "token"]), &remotes),
            CommandDecision::DenyRedline {
                code: "forbidden_auth_token",
                ..
            }
        ));
    }
}
