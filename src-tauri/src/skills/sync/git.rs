//! Controlled `/usr/bin/git` execution and URL validation for skill hot-sync.

use super::{DEFAULT_SKILLS_SOURCE_REPO, MAX_SOURCE_REPO_LEN, SKILL_SYNC_TIMEOUT};
use crate::i18n::{self, LanguageKind};
use std::process::Stdio;
use tokio::process::Command;

/// Strips embedded credentials (`user:pass@`) from HTTP(S)/SSH URLs before storing in manifest.
pub(crate) fn sanitize_display_source_repo(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/').trim_end_matches(".git");
    for scheme in ["https://", "http://", "ssh://"] {
        if let Some(rest) = trimmed.strip_prefix(scheme) {
            let authority_end = rest.find('/').unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            if let Some((_, host_part)) = authority.rsplit_once('@') {
                return format!("{scheme}{host_part}{}", &rest[authority_end..]);
            }
            return trimmed.to_owned();
        }
    }
    trimmed.to_owned()
}

pub(super) fn validate_source_repo(
    raw: Option<&str>,
    locale: LanguageKind,
) -> Result<(String, String), String> {
    let trimmed = raw.map(str::trim).unwrap_or("");
    let candidate = if trimmed.is_empty() {
        DEFAULT_SKILLS_SOURCE_REPO
    } else {
        trimmed
    };
    if candidate.len() > MAX_SOURCE_REPO_LEN
        || candidate.starts_with('-')
        || candidate
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(i18n::skills::skills_invalid_source_url_error(locale).into());
    }
    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("ext::") || lower.starts_with("fd::") {
        return Err(i18n::skills::skills_invalid_source_url_error(locale).into());
    }
    let allowed = lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("ssh://")
        || lower.starts_with("file://")
        || candidate.starts_with("git@")
        || candidate.starts_with('/');
    if !allowed {
        return Err(i18n::skills::skills_invalid_source_url_error(locale).into());
    }
    let display = sanitize_display_source_repo(candidate);
    Ok((candidate.to_owned(), display))
}

fn strip_repo_git_overrides(cmd: &mut Command) {
    crate::git_env::strip_git_overrides(cmd, true);
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
}

pub(super) async fn run_controlled_skill_git(
    args: &[&str],
) -> Result<(bool, String, String), String> {
    let mut cmd = Command::new("/usr/bin/git");
    cmd.args([
        "--no-pager",
        "-c",
        "color.ui=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.symlinks=false",
        "-c",
        "protocol.ext.allow=never",
    ])
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    strip_repo_git_overrides(&mut cmd);

    let output = tokio::time::timeout(SKILL_SYNC_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "Git skill sync timed out after 60s.".to_owned())?
        .map_err(|err| format!("Failed to execute /usr/bin/git: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Ok((output.status.success(), stdout, stderr))
}
