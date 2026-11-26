//! Three-tier controlled CLI security policy and permanent hard redlines for `run_git` and
//! `run_platform_cli` (`gh` / `glab`).
//!
//! - Tier 0 (`AllowTier0`): Read-only inspection commands (auto-allowed).
//! - Tier 1 (`AllowTier1`): Routine in-scope write commands that pass every permanent redline.
//! - Tier 2 (`RequireConfirmTier2`): Destructive, irreversible, or unconventional custom write
//!   commands requiring explicit user confirmation via `ask_user_question`.
//! - Hard Redline (`DenyRedline`): Permanently blocked regardless of skills, model output, or
//!   "don't ask again" preferences.

mod git;
mod platform;
#[cfg(test)]
mod tests;

pub use git::classify_git_command;
pub use platform::classify_platform_cli_command;

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

pub(super) fn format_preview(binary: &str, args: &[String]) -> String {
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

pub(super) fn is_40_hex_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn is_valid_verified_lease(value: &str) -> bool {
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

pub(super) fn is_protected_ref_arg(arg: &str) -> bool {
    let clean = arg.trim().trim_start_matches('+');
    let target = clean.split_once(':').map(|(_, rhs)| rhs).unwrap_or(clean);
    matches!(
        target,
        "main" | "master" | "refs/heads/main" | "refs/heads/master"
    )
}

pub(super) fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter()
        .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
}

pub(super) fn flag_value<'a>(args: &'a [String], names: &[&str]) -> Option<&'a str> {
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

pub(super) fn has_short_flag_char(args: &[String], needle: char) -> bool {
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
