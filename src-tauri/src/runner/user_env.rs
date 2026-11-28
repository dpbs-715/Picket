//! User login-shell environment resolution for Run commands.
//!
//! Strictly uses the user's own local shell and environment (never falls back to Picket's
//! bundled `picket-node` and never injects Picket's model API credentials).

use super::{process_io::ProcessGroupKillGuard, types::UserEnvSummary};
use crate::i18n::{self, LanguageKind};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, sync::Mutex, time::timeout};

const ENV_START_MARKER: &str = "__PICKET_USER_ENV_START_9F2A__";
const ENV_END_MARKER: &str = "__PICKET_USER_ENV_END_9F2A__";
const INTERACTIVE_PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const LOGIN_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ENV_OUTPUT_BYTES: u64 = 512 * 1024;

/// Picket-internal variables that must never bleed into user project processes.
const STRIPPED_PICKET_VARS: &[&str] = &[
    "PICKET_DIST_DIR",
    "PICKET_TS_EXPORT_DIR",
    "PICKET_SMOKE_REPO",
];

#[derive(Clone, Debug)]
pub struct ResolvedUserEnv {
    pub shell: PathBuf,
    pub vars: BTreeMap<String, String>,
    pub loaded_from_login_shell: bool,
}

impl ResolvedUserEnv {
    pub fn summary(&self, locale: LanguageKind) -> UserEnvSummary {
        let shell_str = self.shell.to_string_lossy().into_owned();
        let path_count = self
            .vars
            .get("PATH")
            .map(|p| std::env::split_paths(p).count() as u32)
            .unwrap_or(0);
        let warning = (!self.loaded_from_login_shell)
            .then(|| i18n::runner::user_env_fallback_warning(locale, &shell_str));
        UserEnvSummary {
            shell: shell_str,
            path_count,
            loaded_from_login_shell: self.loaded_from_login_shell,
            warning,
        }
    }
}

#[derive(Clone, Default)]
pub struct UserEnvCache {
    cached: Arc<Mutex<Option<ResolvedUserEnv>>>,
    #[cfg(test)]
    load_barrier: Arc<std::sync::Mutex<Option<Arc<tokio::sync::Notify>>>>,
}

impl UserEnvCache {
    pub async fn get_or_load(&self) -> ResolvedUserEnv {
        #[cfg(test)]
        {
            let barrier = self.load_barrier.lock().ok().and_then(|g| g.clone());
            if let Some(notify) = barrier {
                notify.notified().await;
            }
        }
        let mut guard = self.cached.lock().await;
        if let Some(existing) = guard.as_ref() {
            return existing.clone();
        }
        let resolved = probe_user_login_env().await;
        *guard = Some(resolved.clone());
        resolved
    }

    pub async fn refresh(&self) -> ResolvedUserEnv {
        let resolved = probe_user_login_env().await;
        let mut guard = self.cached.lock().await;
        *guard = Some(resolved.clone());
        resolved
    }

    #[cfg(test)]
    pub async fn set_for_test(&self, env: ResolvedUserEnv) {
        let mut guard = self.cached.lock().await;
        *guard = Some(env);
    }

    #[cfg(test)]
    pub fn set_load_barrier_for_test(&self, barrier: Option<Arc<tokio::sync::Notify>>) {
        if let Ok(mut guard) = self.load_barrier.lock() {
            *guard = barrier;
        }
    }
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

pub fn resolve_user_shell() -> PathBuf {
    if let Some(raw) = std::env::var_os("SHELL") {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() && is_executable_file(&candidate) {
            return candidate;
        }
    }
    for fallback in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        let path = PathBuf::from(fallback);
        if is_executable_file(&path) {
            return path;
        }
    }
    PathBuf::from("/bin/sh")
}

/// Parses NUL-delimited `env -0` output framed by `ENV_START_MARKER` and `ENV_END_MARKER`.
/// Ignores any startup banners/MOTD printed before the start marker or after the end marker.
pub fn parse_marked_env_bytes(bytes: &[u8]) -> Option<BTreeMap<String, String>> {
    let start_bytes = ENV_START_MARKER.as_bytes();
    let end_bytes = ENV_END_MARKER.as_bytes();
    let start_pos = find_subslice(bytes, start_bytes)?;
    let body_start = start_pos + start_bytes.len();
    let rel_end = find_subslice(&bytes[body_start..], end_bytes)?;
    let body = &bytes[body_start..body_start + rel_end];

    let mut vars = BTreeMap::new();
    for entry in body.split(|&b| b == 0) {
        if entry.is_empty() {
            continue;
        }
        let Some(eq_idx) = entry.iter().position(|&b| b == b'=') else {
            continue;
        };
        let Ok(key) = std::str::from_utf8(&entry[..eq_idx]) else {
            continue;
        };
        if key.is_empty() || key == "_" || key == "SHLVL" || key == "PWD" || key == "OLDPWD" {
            continue;
        }
        let value = String::from_utf8_lossy(&entry[eq_idx + 1..]).into_owned();
        vars.insert(key.to_owned(), value);
    }
    if vars.is_empty() {
        return None;
    }
    sanitize_user_env_map(&mut vars);
    Some(vars)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn sanitize_user_env_map(vars: &mut BTreeMap<String, String>) {
    for stripped in STRIPPED_PICKET_VARS {
        vars.remove(*stripped);
    }
}

fn build_fallback_env(shell: &Path) -> ResolvedUserEnv {
    let mut vars: BTreeMap<String, String> = std::env::vars().collect();
    sanitize_user_env_map(&mut vars);
    let mut dirs: Vec<PathBuf> = vars
        .get("PATH")
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    for extra in [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    if let Some(home) = vars.get("HOME").map(PathBuf::from) {
        for rel in [".local/bin", ".cargo/bin"] {
            let dir = home.join(rel);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        vars.insert("PATH".into(), joined.to_string_lossy().into_owned());
    }
    ResolvedUserEnv {
        shell: shell.to_path_buf(),
        vars,
        loaded_from_login_shell: false,
    }
}

async fn run_shell_env_probe(
    shell: &Path,
    flags: &[&str],
    probe_timeout: Duration,
) -> Option<BTreeMap<String, String>> {
    let probe_script =
        format!("printf '{ENV_START_MARKER}'; /usr/bin/env -0; printf '{ENV_END_MARKER}'");
    let mut cmd = Command::new(shell);
    cmd.args(flags)
        .arg(&probe_script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().ok()?;
    let mut pgid_guard = ProcessGroupKillGuard::new(child.id());
    let stdout = child.stdout.take()?;

    let read_fut = async move {
        let mut buf = Vec::new();
        let _ = stdout
            .take(MAX_ENV_OUTPUT_BYTES)
            .read_to_end(&mut buf)
            .await;
        let status = child.wait().await.ok()?;
        pgid_guard.disarm();
        status.success().then_some(buf)
    };

    match timeout(probe_timeout, read_fut).await {
        Ok(Some(bytes)) => parse_marked_env_bytes(&bytes),
        Ok(None) | Err(_) => None,
    }
}

pub async fn probe_user_login_env() -> ResolvedUserEnv {
    let shell = resolve_user_shell();
    // Try interactive login shell first so ~/.zprofile + ~/.zshrc (nvm, fnm, mise, pyenv, etc.)
    // are both initialized; fall back to non-interactive login shell if interactive hooks hang.
    if let Some(vars) =
        run_shell_env_probe(&shell, &["-l", "-i", "-c"], INTERACTIVE_PROBE_TIMEOUT).await
    {
        return ResolvedUserEnv {
            shell,
            vars,
            loaded_from_login_shell: true,
        };
    }
    if let Some(vars) = run_shell_env_probe(&shell, &["-l", "-c"], LOGIN_PROBE_TIMEOUT).await {
        return ResolvedUserEnv {
            shell,
            vars,
            loaded_from_login_shell: true,
        };
    }
    build_fallback_env(&shell)
}
