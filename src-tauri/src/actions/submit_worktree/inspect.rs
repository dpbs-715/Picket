use crate::actions::session_store::{SubmitPathPreflightSnapshot, SubmitWorktreeEntryState};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;

const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn validate_repo_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.contains('\0') || path.starts_with('/') || path.contains('\\') {
        return Err(format!("Invalid repository-relative path: `{path}`"));
    }
    for comp in Path::new(path).components() {
        match comp {
            Component::Normal(seg) => {
                if seg.to_string_lossy().eq_ignore_ascii_case(".git") {
                    return Err(format!("Refused unsafe `.git` path: `{path}`"));
                }
            }
            _ => return Err(format!("Path escapes repository root: `{path}`")),
        }
    }
    Ok(())
}

pub(crate) async fn run_git_bytes(
    cwd: &Path,
    args: &[&str],
    allow_remote: bool,
    disable_hooks: bool,
) -> Result<(bool, Vec<u8>, String), String> {
    let mut cmd = Command::new("/usr/bin/git");
    cmd.current_dir(cwd)
        .args(["--no-pager", "--no-optional-locks", "-c", "color.ui=false"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if disable_hooks {
        cmd.args(["-c", "core.hooksPath=/dev/null"]);
    }
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(k);
        }
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", ":")
        .env(
            "GIT_ALLOW_PROTOCOL",
            if allow_remote {
                "file:https:http:ssh:git"
            } else {
                ""
            },
        );
    cmd.args(args);

    let output = tokio::time::timeout(WORKTREE_GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("Git command timed out: git {}", args.join(" ")))?
        .map_err(|err| format!("Failed to run git: {err}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Ok((output.status.success(), output.stdout, stderr))
}

pub(super) fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "group".into()
    } else {
        trimmed.to_owned()
    }
}

pub(crate) async fn inspect_worktree_entry(
    root: &Path,
    rel_path: &str,
) -> Result<SubmitWorktreeEntryState, String> {
    validate_repo_relative_path(rel_path)?;
    let full = root.join(rel_path);
    match fs::symlink_metadata(&full) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(SubmitWorktreeEntryState::Absent)
        }
        Err(err) => Err(format!("Failed to inspect `{rel_path}`: {err}")),
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() {
                let target = fs::read_link(&full)
                    .map_err(|err| format!("Failed to read symlink `{rel_path}`: {err}"))?;
                Ok(SubmitWorktreeEntryState::Symlink {
                    target: target.to_string_lossy().into_owned(),
                })
            } else if ft.is_file() {
                let bytes =
                    fs::read(&full).map_err(|err| format!("Failed to read `{rel_path}`: {err}"))?;
                Ok(SubmitWorktreeEntryState::File {
                    mode: meta.permissions().mode() & 0o777,
                    sha256: sha256_hex(&bytes),
                })
            } else {
                Err(format!(
                    "Unsupported file type (directory/special node) for `{rel_path}`"
                ))
            }
        }
    }
}

pub(crate) async fn inspect_index_stage_entry(
    root: &Path,
    rel_path: &str,
) -> Result<Option<(String, String, String)>, String> {
    validate_repo_relative_path(rel_path)?;
    // Reject special index flags (assume-unchanged / skip-worktree) where `-v` outputs lowercase tag.
    let (ok_v, out_v, err_v) =
        run_git_bytes(root, &["ls-files", "-v", "-z", "--", rel_path], false, true).await?;
    if !ok_v {
        return Err(format!(
            "Failed to inspect index flags for `{rel_path}`: {err_v}"
        ));
    }
    for entry in out_v.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        if let Some(&tag) = entry.first()
            && matches!(tag, b'h' | b'S' | b's' | b'r' | b'c' | b'k')
        {
            return Err(format!(
                "Path `{rel_path}` has special Git index flags (`{}`); keeping it in place.",
                tag as char
            ));
        }
    }

    let (ok, stdout, stderr) = run_git_bytes(
        root,
        &["ls-files", "--stage", "-z", "--", rel_path],
        false,
        true,
    )
    .await?;
    if !ok {
        return Err(format!(
            "Failed to read index entry for `{rel_path}`: {stderr}"
        ));
    }
    let records: Vec<&[u8]> = stdout
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .collect();
    if records.is_empty() {
        return Ok(None);
    }
    if records.len() > 1 {
        return Err(format!(
            "Path `{rel_path}` has unresolved multi-stage conflict entries in index."
        ));
    }
    let rec = records[0];
    let Some(tab) = rec.iter().position(|b| *b == b'\t') else {
        return Err(format!("Unexpected index format for `{rel_path}`"));
    };
    let header = std::str::from_utf8(&rec[..tab])
        .map_err(|_| format!("Non-UTF-8 index header for `{rel_path}`"))?;
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(format!("Malformed index header for `{rel_path}`: {header}"));
    }
    let mode = parts[0].to_owned();
    let oid = parts[1].to_owned();
    let stage = parts[2];
    if stage != "0" {
        return Err(format!(
            "Path `{rel_path}` has non-zero index stage ({stage}); keeping it in place."
        ));
    }
    if mode == "160000" {
        return Err(format!(
            "Path `{rel_path}` is a Git submodule (`160000`); isolated submit transfer does not mutate submodules."
        ));
    }
    let (ok_blob, blob_bytes, err_blob) =
        run_git_bytes(root, &["cat-file", "blob", &oid], false, true).await?;
    if !ok_blob {
        return Err(format!(
            "Failed to read index blob {oid} for `{rel_path}`: {err_blob}"
        ));
    }
    Ok(Some((mode, oid, sha256_hex(&blob_bytes))))
}

async fn resolve_head_blob_oid(root: &Path, rel_path: &str) -> Option<String> {
    let spec = format!("HEAD:{rel_path}");
    let (ok, out, _) = run_git_bytes(root, &["rev-parse", "--verify", &spec], false, true)
        .await
        .ok()?;
    if !ok {
        return None;
    }
    let oid = String::from_utf8_lossy(&out).trim().to_owned();
    (!oid.is_empty()).then_some(oid)
}

pub(super) async fn capture_path_preflight_snapshot(
    original_root: &Path,
    rel_path: &str,
    previous_path: Option<String>,
    range_kind: &str,
    submitted_state: SubmitWorktreeEntryState,
) -> Result<SubmitPathPreflightSnapshot, String> {
    validate_repo_relative_path(rel_path)?;
    if let Some(ref prev) = previous_path {
        validate_repo_relative_path(prev)?;
    }
    let head_oid = resolve_head_blob_oid(original_root, rel_path).await;
    let index = inspect_index_stage_entry(original_root, rel_path).await?;
    let worktree_state = inspect_worktree_entry(original_root, rel_path).await?;
    let (index_mode, index_oid, index_sha256) = match index {
        Some((m, o, s)) => (Some(m), Some(o), Some(s)),
        None => (None, None, None),
    };
    Ok(SubmitPathPreflightSnapshot {
        path: rel_path.to_owned(),
        previous_path,
        range_kind: range_kind.to_owned(),
        head_oid,
        index_mode,
        index_oid,
        index_sha256,
        worktree_state,
        submitted_state,
    })
}
