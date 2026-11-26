use super::{
    CommandDecision, flag_value, format_preview, has_flag, has_short_flag_char, is_40_hex_sha,
};
use crate::actions::types::RemoteRepoIdentity;

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
            | ("workflow", "list" | "view")
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
