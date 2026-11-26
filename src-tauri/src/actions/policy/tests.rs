use super::*;
use crate::actions::types::RemoteRepoIdentity;

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
    assert_eq!(
        classify_platform_cli_command("gh", &vecs(&["workflow", "view", "release.yml"]), &remotes),
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
        classify_platform_cli_command("gh", &vecs(&["pr", "merge", "12", "--squash"]), &remotes),
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

#[test]
fn repo_command_policy_blocks_redlines_and_requires_tier2_for_valid_commands() {
    // Valid discrete-argv commands require Tier-2 preview confirmation with prompt-bound trust:
    assert!(matches!(
        classify_repo_command(
            "node",
            &vecs(&[
                "scripts/package-dmg.mjs",
                "--check-version-only",
                "--tag",
                "v0.2.1"
            ])
        ),
        CommandDecision::RequireConfirmTier2 {
            allow_dont_ask_again: true,
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("pnpm", &vecs(&["check"])),
        CommandDecision::RequireConfirmTier2 {
            allow_dont_ask_again: true,
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("python3.12", &vecs(&["-m", "pytest"])),
        CommandDecision::RequireConfirmTier2 {
            allow_dont_ask_again: true,
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("./scripts/verify.mjs", &vecs(&["--strict"])),
        CommandDecision::RequireConfirmTier2 {
            allow_dont_ask_again: true,
            ..
        }
    ));

    // Picket-dedicated CLIs are permanently redirected to their guarded tools:
    for dedicated in ["git", "gh", "glab", "brew"] {
        assert!(matches!(
            classify_repo_command(dedicated, &vecs(&["--version"])),
            CommandDecision::DenyRedline {
                code: "use_dedicated_picket_tool",
                ..
            }
        ));
    }

    // Shells & wrappers are permanently blocked:
    for shell in [
        "sh",
        "bash",
        "zsh",
        "osascript",
        "env",
        "xargs",
        "./bin/bash",
    ] {
        assert!(matches!(
            classify_repo_command(shell, &vecs(&["-c", "echo hi"])),
            CommandDecision::DenyRedline {
                code: "forbidden_shell_wrapper",
                ..
            }
        ));
    }

    // Destructive & privileged binaries are permanently blocked:
    for destructive in ["sudo", "rm", "mv", "chmod", "kill", "diskutil", "security"] {
        assert!(matches!(
            classify_repo_command(destructive, &vecs(&["-rf", "."])),
            CommandDecision::DenyRedline {
                code: "forbidden_destructive_binary",
                ..
            }
        ));
    }

    // Raw network transfer & desktop launchers are permanently blocked:
    for net_bin in ["curl", "wget", "ssh", "open", "pbcopy"] {
        assert!(matches!(
            classify_repo_command(net_bin, &vecs(&["https://example.com"])),
            CommandDecision::DenyRedline {
                code: "forbidden_network_or_system_binary",
                ..
            }
        ));
    }

    // Absolute paths, traversal, spaces in command, NUL bytes, and excessive args are blocked:
    assert!(matches!(
        classify_repo_command("/usr/bin/node", &vecs(&["-v"])),
        CommandDecision::DenyRedline {
            code: "forbidden_absolute_command",
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("./../outside", &vecs(&[])),
        CommandDecision::DenyRedline {
            code: "forbidden_command_path",
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("./.git/hooks/pre-commit", &vecs(&[])),
        CommandDecision::DenyRedline {
            code: "forbidden_command_path",
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("node -v", &vecs(&[])),
        CommandDecision::DenyRedline {
            code: "invalid_command",
            ..
        }
    ));
    assert!(matches!(
        classify_repo_command("node", &[String::from("bad\0arg")]),
        CommandDecision::DenyRedline {
            code: "invalid_nul",
            ..
        }
    ));
    let many_args: Vec<String> = (0..65).map(|i| i.to_string()).collect();
    assert!(matches!(
        classify_repo_command("node", &many_args),
        CommandDecision::DenyRedline {
            code: "too_many_args",
            ..
        }
    ));
}
