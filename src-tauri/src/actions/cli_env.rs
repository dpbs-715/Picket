//! Local CLI discovery, remote platform detection, capability checks (`--match-head-commit` /
//! `--sha`), auth status probing with strict token redaction, and whitelisted Homebrew installation
//! for `gh` and `glab`.

use super::types::{CliBinaryProbe, CliEnvSnapshot, RemoteRepoIdentity};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_PROBE_OUTPUT: u64 = 32 * 1024;

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

pub fn build_augmented_path(override_dirs: Option<&[PathBuf]>) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(custom) = override_dirs {
        dirs.extend(custom.iter().cloned());
    }
    if let Some(existing) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&existing));
    }
    for extra in [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let local_bin = home.join(".local/bin");
        if !dirs.contains(&local_bin) {
            dirs.push(local_bin);
        }
    }
    std::env::join_paths(dirs)
        .unwrap_or_else(|_| OsString::from("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"))
}

pub fn resolve_cli_binary(name: &str, override_dirs: Option<&[PathBuf]>) -> Option<PathBuf> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    if let Some(dirs) = override_dirs {
        for dir in dirs {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
        return None;
    }
    let mut candidates = Vec::new();
    if let Some(path_env) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path_env).map(|dir| dir.join(name)));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin").join(name));
    candidates.push(PathBuf::from("/usr/local/bin").join(name));
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        candidates.push(home.join(".local/bin").join(name));
    }
    candidates.push(PathBuf::from("/usr/bin").join(name));
    candidates.push(PathBuf::from("/bin").join(name));
    candidates.into_iter().find(|path| is_executable_file(path))
}

/// Redacts credential/token-like values from CLI output so tokens never enter logs,
/// model tool results, error messages, or UI state.
pub fn redact_sensitive_cli_output(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (idx, line) in raw.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let lower = line.to_ascii_lowercase();
        if (lower.contains("token:")
            || lower.contains("oauth_token")
            || lower.contains("authorization:")
            || lower.contains("x-api-key"))
            && let Some((prefix, _)) = line.split_once(':')
        {
            out.push_str(prefix);
            out.push_str(": [REDACTED]");
            continue;
        }
        let mut scrubbed = line.to_owned();
        for prefix in [
            "gho_",
            "ghp_",
            "ghu_",
            "ghs_",
            "ghr_",
            "github_pat_",
            "glpat-",
            "gldt-",
            "glrt-",
            "sk-",
        ] {
            while let Some(pos) = scrubbed.find(prefix) {
                let tail = &scrubbed[pos..];
                let end_rel = tail
                    .find(|ch: char| {
                        ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | ';' | ')' | ']')
                    })
                    .unwrap_or(tail.len());
                scrubbed.replace_range(pos..pos + end_rel, "[REDACTED]");
            }
        }
        out.push_str(&scrubbed);
    }
    out
}

pub fn parse_remote_url(remote: &str, url: &str) -> Option<RemoteRepoIdentity> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (host, raw_path) = if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("ssh://"))
        .or_else(|| trimmed.strip_prefix("git://"))
    {
        let without_auth = rest.split_once('@').map(|(_, after)| after).unwrap_or(rest);
        let (host_port, path) = without_auth.split_once('/')?;
        let host = host_port.split(':').next().unwrap_or(host_port);
        (host.to_ascii_lowercase(), path)
    } else if let Some((left, path)) = trimmed.split_once(':') {
        if left.contains('/') {
            return None;
        }
        let host = left.split_once('@').map(|(_, after)| after).unwrap_or(left);
        (host.to_ascii_lowercase(), path)
    } else {
        return None;
    };

    let clean_path = raw_path
        .trim_start_matches('/')
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_owned();
    if host.is_empty() || clean_path.is_empty() || !clean_path.contains('/') {
        return None;
    }
    let platform =
        if host == "github.com" || host.ends_with(".github.com") || host.contains("github") {
            "github"
        } else if host == "gitlab.com" || host.ends_with(".gitlab.com") || host.contains("gitlab") {
            "gitlab"
        } else {
            "unknown"
        };
    Some(RemoteRepoIdentity {
        remote: remote.to_owned(),
        platform,
        host,
        repo_path: clean_path,
        url: trimmed.to_owned(),
    })
}

pub async fn detect_repo_remotes(worktree_root: &Path) -> Vec<RemoteRepoIdentity> {
    let mut cmd = Command::new("/usr/bin/git");
    cmd.current_dir(worktree_root)
        .args(["--no-pager", "remote", "-v"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(output) = tokio::time::timeout(PROBE_TIMEOUT, cmd.output()).await else {
        return Vec::new();
    };
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut seen = std::collections::HashSet::new();
    let mut remotes = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(url) = parts.next() else { continue };
        let kind = parts.next().unwrap_or("");
        if kind == "(push)" && seen.contains(name) {
            continue;
        }
        if let Some(parsed) = parse_remote_url(name, url)
            && seen.insert(name.to_owned())
        {
            remotes.push(parsed);
        }
    }
    remotes
}

async fn run_quick_probe(
    binary: &Path,
    worktree_root: &Path,
    args: &[&str],
    override_dirs: Option<&[PathBuf]>,
) -> Option<(bool, String)> {
    let mut cmd = Command::new(binary);
    cmd.current_dir(worktree_root)
        .args(args)
        .env("PATH", build_augmented_path(override_dirs))
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let out_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout.take(MAX_PROBE_OUTPUT).read_to_end(&mut buf).await;
        buf
    });
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.take(MAX_PROBE_OUTPUT).read_to_end(&mut buf).await;
        buf
    });
    let status = match tokio::time::timeout(PROBE_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return None;
        }
    };
    let out = out_task.await.unwrap_or_default();
    let err = err_task.await.unwrap_or_default();
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out).trim(),
        String::from_utf8_lossy(&err).trim()
    );
    Some((
        status.success(),
        redact_sensitive_cli_output(combined.trim()),
    ))
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(120).collect())
}

pub async fn check_cli_env(
    worktree_root: &Path,
    override_dirs: Option<&[PathBuf]>,
) -> CliEnvSnapshot {
    let remotes = detect_repo_remotes(worktree_root).await;
    let primary_remote = remotes
        .iter()
        .find(|r| r.remote == "origin")
        .or_else(|| remotes.first());
    let detected_platform = primary_remote.map(|r| r.platform).unwrap_or("unknown");

    let git_path = resolve_cli_binary("git", override_dirs).or_else(|| {
        Path::new("/usr/bin/git")
            .is_file()
            .then(|| PathBuf::from("/usr/bin/git"))
    });
    let git = match git_path {
        Some(path) => {
            let version = run_quick_probe(&path, worktree_root, &["--version"], override_dirs)
                .await
                .and_then(|(_, out)| first_non_empty_line(&out));
            CliBinaryProbe {
                installed: true,
                path: Some(path.to_string_lossy().into_owned()),
                version,
                authenticated: None,
                auth_detail: None,
                supports_head_sha_guard: None,
            }
        }
        None => CliBinaryProbe {
            installed: false,
            path: None,
            version: None,
            authenticated: None,
            auth_detail: None,
            supports_head_sha_guard: None,
        },
    };

    let brew = match resolve_cli_binary("brew", override_dirs) {
        Some(path) => {
            let version = run_quick_probe(&path, worktree_root, &["--version"], override_dirs)
                .await
                .and_then(|(_, out)| first_non_empty_line(&out));
            CliBinaryProbe {
                installed: true,
                path: Some(path.to_string_lossy().into_owned()),
                version,
                authenticated: None,
                auth_detail: None,
                supports_head_sha_guard: None,
            }
        }
        None => CliBinaryProbe {
            installed: false,
            path: None,
            version: None,
            authenticated: None,
            auth_detail: None,
            supports_head_sha_guard: None,
        },
    };

    let gh_host = remotes
        .iter()
        .find(|r| r.platform == "github")
        .map(|r| r.host.as_str());
    let gh = match resolve_cli_binary("gh", override_dirs) {
        Some(path) => {
            let version = run_quick_probe(&path, worktree_root, &["--version"], override_dirs)
                .await
                .and_then(|(_, out)| first_non_empty_line(&out));
            let auth_args: Vec<&str> = match gh_host {
                Some(host) => vec!["auth", "status", "--hostname", host],
                None => vec!["auth", "status"],
            };
            let (authenticated, auth_detail) =
                match run_quick_probe(&path, worktree_root, &auth_args, override_dirs).await {
                    Some((ok, out)) => (Some(ok), first_non_empty_line(&out)),
                    None => (
                        Some(false),
                        Some("Timed out checking gh auth status".into()),
                    ),
                };
            let supports_guard = run_quick_probe(
                &path,
                worktree_root,
                &["pr", "merge", "--help"],
                override_dirs,
            )
            .await
            .map(|(_, out)| out.contains("--match-head-commit"));
            CliBinaryProbe {
                installed: true,
                path: Some(path.to_string_lossy().into_owned()),
                version,
                authenticated,
                auth_detail,
                supports_head_sha_guard: supports_guard,
            }
        }
        None => CliBinaryProbe {
            installed: false,
            path: None,
            version: None,
            authenticated: Some(false),
            auth_detail: None,
            supports_head_sha_guard: None,
        },
    };

    let glab_host = remotes
        .iter()
        .find(|r| r.platform == "gitlab")
        .map(|r| r.host.as_str());
    let glab = match resolve_cli_binary("glab", override_dirs) {
        Some(path) => {
            let version = run_quick_probe(&path, worktree_root, &["--version"], override_dirs)
                .await
                .and_then(|(_, out)| first_non_empty_line(&out));
            let auth_args: Vec<&str> = match glab_host {
                Some(host) => vec!["auth", "status", "--hostname", host],
                None => vec!["auth", "status"],
            };
            let (authenticated, auth_detail) =
                match run_quick_probe(&path, worktree_root, &auth_args, override_dirs).await {
                    Some((ok, out)) => (Some(ok), first_non_empty_line(&out)),
                    None => (
                        Some(false),
                        Some("Timed out checking glab auth status".into()),
                    ),
                };
            let supports_guard = run_quick_probe(
                &path,
                worktree_root,
                &["mr", "merge", "--help"],
                override_dirs,
            )
            .await
            .map(|(_, out)| out.contains("--sha"));
            CliBinaryProbe {
                installed: true,
                path: Some(path.to_string_lossy().into_owned()),
                version,
                authenticated,
                auth_detail,
                supports_head_sha_guard: supports_guard,
            }
        }
        None => CliBinaryProbe {
            installed: false,
            path: None,
            version: None,
            authenticated: Some(false),
            auth_detail: None,
            supports_head_sha_guard: None,
        },
    };

    CliEnvSnapshot {
        git,
        gh,
        glab,
        brew,
        remotes,
        detected_platform,
    }
}

/// Strictly whitelisted Homebrew installation for `gh` or `glab`.
/// Callers MUST verify session user authorization recorded from `ask_user_question` before calling.
pub async fn install_platform_cli<F>(
    tool: &str,
    worktree_root: &Path,
    override_dirs: Option<&[PathBuf]>,
    mut on_progress: F,
) -> Result<CliBinaryProbe, String>
where
    F: FnMut(String) + Send,
{
    let formula = match tool.trim() {
        "gh" => "gh",
        "glab" => "glab",
        other => {
            return Err(format!(
                "Refused to install unsupported CLI `{other}`. Only `gh` and `glab` are whitelisted."
            ));
        }
    };
    let brew = resolve_cli_binary("brew", override_dirs).ok_or_else(|| {
        format!(
            "Homebrew (`brew`) was not found on this Mac. Install `{formula}` manually from its official installer or install Homebrew first."
        )
    })?;
    on_progress(format!("Running `brew install {formula}`..."));
    let mut cmd = Command::new(&brew);
    cmd.current_dir(worktree_root)
        .args(["install", formula])
        .env("PATH", build_augmented_path(override_dirs))
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to start Homebrew (`brew install {formula}`): {err}"))?;
    let stdout = child.stdout.take().ok_or("Missing brew stdout pipe")?;
    let stderr = child.stderr.take().ok_or("Missing brew stderr pipe")?;

    let mut out_lines = BufReader::new(stdout).lines();
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.take(MAX_PROBE_OUTPUT).read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut recent_lines: Vec<String> = Vec::new();
    let status = tokio::time::timeout(INSTALL_TIMEOUT, async {
        while let Ok(Some(line)) = out_lines.next_line().await {
            let clean = redact_sensitive_cli_output(line.trim());
            if !clean.is_empty() {
                on_progress(clean.clone());
                if recent_lines.len() >= 12 {
                    recent_lines.remove(0);
                }
                recent_lines.push(clean);
            }
        }
        child
            .wait()
            .await
            .map_err(|err| format!("Failed waiting for brew install: {err}"))
    })
    .await;

    let status = match status {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!(
                "`brew install {formula}` timed out after 180 seconds."
            ));
        }
    };
    let stderr_text = redact_sensitive_cli_output(&err_task.await.unwrap_or_default());
    if !status.success() {
        let detail = if !stderr_text.trim().is_empty() {
            stderr_text.trim().chars().take(320).collect::<String>()
        } else if !recent_lines.is_empty() {
            recent_lines.join(" | ")
        } else {
            format!("exit code {:?}", status.code())
        };
        return Err(format!("`brew install {formula}` failed: {detail}"));
    }

    let installed_path = resolve_cli_binary(formula, override_dirs);
    let version = match installed_path.as_deref() {
        Some(path) => run_quick_probe(path, worktree_root, &["--version"], override_dirs)
            .await
            .and_then(|(_, out)| first_non_empty_line(&out)),
        None => None,
    };
    Ok(CliBinaryProbe {
        installed: installed_path.is_some(),
        path: installed_path.map(|p| p.to_string_lossy().into_owned()),
        version,
        authenticated: Some(false),
        auth_detail: None,
        supports_head_sha_guard: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_and_gitlab_https_ssh_and_subgroup_remotes() {
        let gh = parse_remote_url("origin", "git@github.com:dpbs-715/Picket.git").unwrap();
        assert_eq!(gh.platform, "github");
        assert_eq!(gh.host, "github.com");
        assert_eq!(gh.repo_path, "dpbs-715/Picket");

        let gh_https =
            parse_remote_url("origin", "https://github.com/dpbs-715/Picket.git").unwrap();
        assert_eq!(gh_https.platform, "github");
        assert_eq!(gh_https.repo_path, "dpbs-715/Picket");

        let gl = parse_remote_url(
            "origin",
            "ssh://git@gitlab.example.com:2222/team/sub/service.git",
        )
        .unwrap();
        assert_eq!(gl.platform, "gitlab");
        assert_eq!(gl.host, "gitlab.example.com");
        assert_eq!(gl.repo_path, "team/sub/service");
    }

    #[test]
    fn redacts_tokens_from_cli_output() {
        let raw =
            "Logged in to github.com\nToken: gho_secretToken123456\nInline glpat-abcdef9999 done";
        let scrubbed = redact_sensitive_cli_output(raw);
        assert!(!scrubbed.contains("gho_secretToken123456"));
        assert!(!scrubbed.contains("glpat-abcdef9999"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn probes_mock_cli_binaries_and_installs_whitelisted_formula() {
        let bin_dir = tempfile::tempdir().unwrap();
        let repo_dir = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("/usr/bin/git")
                .current_dir(repo_dir.path())
                .args(["init", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("/usr/bin/git")
                .current_dir(repo_dir.path())
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/acme/demo.git"
                ])
                .status()
                .unwrap()
                .success()
        );

        let gh_script = bin_dir.path().join("gh");
        fs::write(
            &gh_script,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "gh version 2.65.0"; exit 0; fi
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then echo "Logged in to github.com\nToken: gho_should_never_leak"; exit 0; fi
if [ "$1" = "pr" ] && [ "$2" = "merge" ] && [ "$3" = "--help" ]; then echo "  --match-head-commit string"; exit 0; fi
exit 0
"#,
        )
        .unwrap();
        fs::set_permissions(&gh_script, fs::Permissions::from_mode(0o755)).unwrap();

        let brew_script = bin_dir.path().join("brew");
        let bin_path_str = bin_dir.path().to_string_lossy().into_owned();
        fs::write(
            &brew_script,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "Homebrew 4.4.0"; exit 0; fi
if [ "$1" = "install" ] && [ "$2" = "glab" ]; then
  echo "Downloading glab..."
  cat <<'EOF' > "{bin_path_str}/glab"
#!/bin/sh
echo "glab version 1.50.0"
EOF
  chmod +x "{bin_path_str}/glab"
  echo "Installed glab"
  exit 0
fi
exit 1
"#
            ),
        )
        .unwrap();
        fs::set_permissions(&brew_script, fs::Permissions::from_mode(0o755)).unwrap();

        let dirs = vec![bin_dir.path().to_path_buf()];
        let env = check_cli_env(repo_dir.path(), Some(&dirs)).await;
        assert_eq!(env.detected_platform, "github");
        assert!(env.gh.installed);
        assert_eq!(env.gh.authenticated, Some(true));
        assert_eq!(env.gh.supports_head_sha_guard, Some(true));
        assert!(
            !env.gh
                .auth_detail
                .as_deref()
                .unwrap_or("")
                .contains("gho_should_never_leak")
        );
        assert!(!env.glab.installed);
        assert!(env.brew.installed);

        // Non-whitelisted package is always refused:
        assert!(
            install_platform_cli("curl", repo_dir.path(), Some(&dirs), |_| {})
                .await
                .is_err()
        );

        // Whitelisted `glab` installs via mock brew and streams progress:
        let mut progress = Vec::new();
        let installed = install_platform_cli("glab", repo_dir.path(), Some(&dirs), |line| {
            progress.push(line);
        })
        .await
        .unwrap();
        assert!(installed.installed);
        assert_eq!(installed.version.as_deref(), Some("glab version 1.50.0"));
        assert!(progress.iter().any(|l| l.contains("Downloading glab")));
    }
}
