use super::{
    CommandDecision, flag_value, format_preview, has_flag, has_short_flag_char,
    is_protected_ref_arg, is_valid_verified_lease,
};

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
