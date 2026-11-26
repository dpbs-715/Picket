//! Controlled policy for `run_repo_command`:
//! Discrete argv execution (zero shell interpolation), blocking shells, destructive/admin
//! binaries, raw network transfer tools, and Picket-guarded CLIs (`git`/`gh`/`glab`/`brew`).
//! Every allowed repository command requires Tier-2 preview confirmation (with optional
//! prompt-hash-bound "don't ask again").

use super::{CommandDecision, format_preview};
use std::path::Path;

const MAX_COMMAND_LEN: usize = 256;
const MAX_COMMAND_ARGS: usize = 64;

const DEDICATED_PICKET_TOOLS: &[&str] = &["git", "gh", "glab", "brew"];

const FORBIDDEN_SHELL_WRAPPERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "csh",
    "tcsh",
    "ksh",
    "mksh",
    "ash",
    "osascript",
    "expect",
    "env",
    "exec",
    "eval",
    "xargs",
    "nohup",
    "screen",
    "tmux",
    "script",
];

const FORBIDDEN_DESTRUCTIVE_BINARIES: &[&str] = &[
    "sudo",
    "su",
    "doas",
    "pkexec",
    "rm",
    "rmdir",
    "unlink",
    "shred",
    "srm",
    "mv",
    "dd",
    "mkfs",
    "fdisk",
    "diskutil",
    "mount",
    "umount",
    "chmod",
    "chown",
    "chgrp",
    "chflags",
    "xattr",
    "ln",
    "launchctl",
    "kill",
    "killall",
    "pkill",
    "reboot",
    "shutdown",
    "halt",
    "csrutil",
    "spctl",
    "dscl",
    "security",
    "defaults",
    "crontab",
    "at",
];

const FORBIDDEN_NETWORK_OR_SYSTEM_BINARIES: &[&str] = &[
    "curl", "wget", "nc", "ncat", "netcat", "telnet", "ftp", "ssh", "scp", "sftp", "rsync", "open",
    "pbcopy", "pbpaste",
];

fn normalize_binary_basename(command: &str) -> Option<String> {
    let raw_name = Path::new(command).file_name()?.to_str()?;
    let lower = raw_name.to_ascii_lowercase();
    let stripped = lower.strip_suffix(".exe").unwrap_or(&lower);
    (!stripped.is_empty()).then(|| stripped.to_owned())
}

pub fn validate_repo_command_target(command: &str) -> Result<String, (&'static str, String)> {
    let trimmed = command.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_COMMAND_LEN {
        return Err((
            "invalid_command",
            "Command must be a non-empty binary name or `./`-relative worktree executable.".into(),
        ));
    }
    if trimmed
        .bytes()
        .any(|b| b <= 0x20 || b == 0x7f || b == b'\\' || b == b'\0')
    {
        return Err((
            "invalid_command",
            "Command cannot contain spaces, control characters, NUL bytes, or backslashes; pass arguments separately in `args`."
                .into(),
        ));
    }
    if trimmed.starts_with('/') || trimmed.starts_with('~') {
        return Err((
            "forbidden_absolute_command",
            "Absolute or home-relative command paths are blocked; pass a bare program name (e.g. `node`, `pnpm`, `cargo`) or a `./`-relative worktree script."
                .into(),
        ));
    }
    if let Some(rel) = trimmed.strip_prefix("./") {
        if rel.is_empty() {
            return Err((
                "invalid_command",
                "Worktree-relative command must specify a script path after `./`.".into(),
            ));
        }
        for seg in rel.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." || seg.eq_ignore_ascii_case(".git") {
                return Err((
                    "forbidden_command_path",
                    format!(
                        "Worktree-relative command `{trimmed}` cannot traverse parent directories or `.git`."
                    ),
                ));
            }
        }
    } else if trimmed.contains('/') {
        return Err((
            "forbidden_command_path",
            format!(
                "Command `{trimmed}` must be either a bare executable name (e.g. `node`) or start with `./` inside the worktree."
            ),
        ));
    } else if trimmed == "."
        || trimmed == ".."
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
    {
        return Err((
            "invalid_command",
            format!("Unsupported characters in command name `{trimmed}`."),
        ));
    }
    Ok(trimmed.to_owned())
}

pub fn classify_repo_command(command: &str, args: &[String]) -> CommandDecision {
    let clean = match validate_repo_command_target(command) {
        Ok(valid) => valid,
        Err((code, reason)) => return CommandDecision::DenyRedline { code, reason },
    };
    if args.len() > MAX_COMMAND_ARGS {
        return CommandDecision::DenyRedline {
            code: "too_many_args",
            reason: format!(
                "Command `{clean}` exceeded the maximum argument count ({MAX_COMMAND_ARGS})."
            ),
        };
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return CommandDecision::DenyRedline {
            code: "invalid_nul",
            reason: "Command arguments cannot contain NUL bytes.".into(),
        };
    }

    let Some(base) = normalize_binary_basename(&clean) else {
        return CommandDecision::DenyRedline {
            code: "invalid_command",
            reason: format!("Cannot determine executable name for `{clean}`."),
        };
    };

    if DEDICATED_PICKET_TOOLS.contains(&base.as_str()) {
        return CommandDecision::DenyRedline {
            code: "use_dedicated_picket_tool",
            reason: format!(
                "`{base}` is blocked in `run_repo_command`; use Picket's dedicated controlled tools (`run_git`, `commit_exact_scope`, `run_platform_cli`, or `install_platform_cli`) instead."
            ),
        };
    }
    if FORBIDDEN_SHELL_WRAPPERS.contains(&base.as_str()) {
        return CommandDecision::DenyRedline {
            code: "forbidden_shell_wrapper",
            reason: format!(
                "Shell or command wrapper `{base}` is permanently blocked; invoke the target program directly with discrete `args`."
            ),
        };
    }
    if FORBIDDEN_DESTRUCTIVE_BINARIES.contains(&base.as_str()) {
        return CommandDecision::DenyRedline {
            code: "forbidden_destructive_binary",
            reason: format!(
                "Destructive or privileged system binary `{base}` is permanently blocked in `run_repo_command`."
            ),
        };
    }
    if FORBIDDEN_NETWORK_OR_SYSTEM_BINARIES.contains(&base.as_str()) {
        return CommandDecision::DenyRedline {
            code: "forbidden_network_or_system_binary",
            reason: format!(
                "Network transfer or desktop launcher binary `{base}` is permanently blocked in `run_repo_command`."
            ),
        };
    }

    let preview = format_preview(&clean, args);
    CommandDecision::RequireConfirmTier2 {
        reason: format!("Running local repository command `{clean}` requires confirmation."),
        preview,
        allow_dont_ask_again: true,
    }
}
