//! Controlled isolated worktree engine for Phase 3 `submit` multi-group workflows.
//!
//! Enforces:
//! - Only authorized selected files (never unselected files or `Local Keep`) can be transferred.
//! - Partially staged files transfer ONLY their staged index content, leaving unstaged drafts untouched.
//! - Exact preflight snapshots of original index + working-tree entries are recorded for Step 5 cleanup.
//! - Isolated worktrees live under `<storage_dir>/submit-worktrees/<repo_id>/` (`0700`) so they never
//!   pollute user directories and survive OS restarts for cross-restart reconciliation.
//! - Worktree removal verifies Picket ownership, unlocked state, verified HEAD SHA, absence of active
//!   Git operations, and empty `git status --porcelain=v1 --untracked-files=all --ignored --ignore-submodules=none`
//!   before running `git worktree remove` WITHOUT `--force`.

use super::session_store::{
    SubmitGroupCheckpoint, SubmitPathPreflightSnapshot, SubmitWorktreeEntryState,
    utc_timestamp_compact,
};
use crate::repository::types::Change;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::NamedTempFile;
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

fn sanitize_slug(raw: &str) -> String {
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

pub(crate) async fn capture_path_preflight_snapshot(
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

async fn write_entry_into_worktree(
    dst_root: &Path,
    rel_path: &str,
    entry: &TransferPayload,
) -> Result<(), String> {
    validate_repo_relative_path(rel_path)?;
    let dst = dst_root.join(rel_path);
    match entry {
        TransferPayload::Absent => {
            if let Ok(meta) = fs::symlink_metadata(&dst) {
                if meta.file_type().is_dir() {
                    return Err(format!("Cannot delete directory at `{rel_path}`"));
                }
                fs::remove_file(&dst)
                    .map_err(|err| format!("Failed to remove `{rel_path}` in worktree: {err}"))?;
            }
        }
        TransferPayload::File { mode, bytes } => {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create parent dirs for `{rel_path}`: {err}")
                })?;
            }
            if let Ok(meta) = fs::symlink_metadata(&dst)
                && meta.file_type().is_symlink()
            {
                fs::remove_file(&dst)
                    .map_err(|err| format!("Failed to replace symlink `{rel_path}`: {err}"))?;
            }
            fs::write(&dst, bytes)
                .map_err(|err| format!("Failed to write `{rel_path}` into worktree: {err}"))?;
            let unix_mode = if *mode & 0o111 != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(&dst, fs::Permissions::from_mode(unix_mode))
                .map_err(|err| format!("Failed to set mode on `{rel_path}`: {err}"))?;
        }
        TransferPayload::Symlink { target } => {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create parent dirs for `{rel_path}`: {err}")
                })?;
            }
            if fs::symlink_metadata(&dst).is_ok() {
                fs::remove_file(&dst)
                    .map_err(|err| format!("Failed to replace `{rel_path}` with symlink: {err}"))?;
            }
            std::os::unix::fs::symlink(target, &dst)
                .map_err(|err| format!("Failed to create symlink `{rel_path}`: {err}"))?;
        }
    }
    Ok(())
}

enum TransferPayload {
    File { mode: u32, bytes: Vec<u8> },
    Symlink { target: String },
    Absent,
}

impl TransferPayload {
    fn to_state(&self) -> SubmitWorktreeEntryState {
        match self {
            Self::File { mode, bytes } => SubmitWorktreeEntryState::File {
                mode: if *mode & 0o111 != 0 { 0o755 } else { 0o644 },
                sha256: sha256_hex(bytes),
            },
            Self::Symlink { target } => SubmitWorktreeEntryState::Symlink {
                target: target.clone(),
            },
            Self::Absent => SubmitWorktreeEntryState::Absent,
        }
    }
}

async fn extract_transfer_payload(
    original_root: &Path,
    change: &Change,
) -> Result<TransferPayload, String> {
    let use_staged_index = matches!(change.range.kind.as_str(), "partial" | "staged");
    if use_staged_index {
        let index = inspect_index_stage_entry(original_root, &change.path).await?;
        let Some((mode, oid, _)) = index else {
            return Ok(TransferPayload::Absent);
        };
        let (ok, bytes, err) =
            run_git_bytes(original_root, &["cat-file", "blob", &oid], false, true).await?;
        if !ok {
            return Err(format!(
                "Failed to read staged blob {oid} for `{}`: {err}",
                change.path
            ));
        }
        if mode == "120000" {
            let target = String::from_utf8(bytes)
                .map_err(|_| format!("Non-UTF-8 staged symlink target for `{}`", change.path))?;
            return Ok(TransferPayload::Symlink { target });
        }
        let unix_mode = if mode == "100755" { 0o755 } else { 0o644 };
        return Ok(TransferPayload::File {
            mode: unix_mode,
            bytes,
        });
    }

    let full = original_root.join(&change.path);
    match fs::symlink_metadata(&full) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(TransferPayload::Absent),
        Err(err) => Err(format!("Failed to read `{}`: {err}", change.path)),
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&full)
                    .map_err(|err| format!("Failed to read symlink `{}`: {err}", change.path))?;
                Ok(TransferPayload::Symlink {
                    target: target.to_string_lossy().into_owned(),
                })
            } else if meta.file_type().is_file() {
                let bytes = fs::read(&full)
                    .map_err(|err| format!("Failed to read `{}`: {err}", change.path))?;
                Ok(TransferPayload::File {
                    mode: meta.permissions().mode() & 0o777,
                    bytes,
                })
            } else {
                Err(format!(
                    "Unsupported filesystem entry type for `{}`",
                    change.path
                ))
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedSubmitWorktree {
    pub group_id: String,
    pub worktree_path: String,
    pub branch: String,
    pub base_ref: String,
    pub base_sha: String,
    pub original_branch: String,
    pub original_head: String,
    pub transferred_paths: Vec<String>,
    pub diff_stat: String,
    pub preflight_snapshots: Vec<SubmitPathPreflightSnapshot>,
}

#[allow(clippy::too_many_arguments)]
pub async fn create_isolated_submit_worktree(
    storage_dir: &Path,
    repository_id: &str,
    original_root: &Path,
    group_id: &str,
    branch: &str,
    base_ref: &str,
    reuse_existing_branch: bool,
    selected_changes: &[Change],
) -> Result<CreatedSubmitWorktree, String> {
    let branch_trimmed = branch.trim();
    if branch_trimmed.is_empty()
        || matches!(branch_trimmed, "main" | "master" | "HEAD")
        || branch_trimmed.starts_with('-')
    {
        return Err(format!(
            "Invalid or protected submit development branch name: `{branch_trimmed}`"
        ));
    }
    let (ok_ref, _, err_ref) = run_git_bytes(
        original_root,
        &["check-ref-format", "--branch", branch_trimmed],
        false,
        true,
    )
    .await?;
    if !ok_ref {
        return Err(format!(
            "Invalid Git branch name `{branch_trimmed}`: {err_ref}"
        ));
    }

    let base_trimmed = base_ref.trim();
    if base_trimmed.is_empty() || base_trimmed.starts_with('-') {
        return Err(format!("Invalid base ref `{base_trimmed}`"));
    }
    let base_commit_spec = format!("{base_trimmed}^{{commit}}");
    let (ok_base, base_out, err_base) = run_git_bytes(
        original_root,
        &["rev-parse", "--verify", &base_commit_spec],
        false,
        true,
    )
    .await?;
    if !ok_base {
        return Err(format!(
            "Base ref `{base_trimmed}` could not be resolved to a commit: {err_base}"
        ));
    }
    let base_sha = String::from_utf8_lossy(&base_out).trim().to_owned();

    let (ok_head, head_out, err_head) = run_git_bytes(
        original_root,
        &["rev-parse", "--verify", "HEAD"],
        false,
        true,
    )
    .await?;
    if !ok_head {
        return Err(format!(
            "Original checkout has no valid HEAD commit: {err_head}"
        ));
    }
    let original_head = String::from_utf8_lossy(&head_out).trim().to_owned();
    let (_, branch_out, _) = run_git_bytes(
        original_root,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        false,
        true,
    )
    .await?;
    let original_branch = {
        let s = String::from_utf8_lossy(&branch_out).trim().to_owned();
        if s.is_empty() { "HEAD".into() } else { s }
    };

    // Extract payloads + preflight snapshots BEFORE touching the filesystem/worktree.
    let mut transfers: Vec<(String, Option<String>, TransferPayload)> = Vec::new();
    let mut preflight_snapshots: Vec<SubmitPathPreflightSnapshot> = Vec::new();
    let mut transferred_paths: Vec<String> = Vec::new();
    for change in selected_changes {
        validate_repo_relative_path(&change.path)?;
        if let Some(ref prev) = change.previous_path {
            validate_repo_relative_path(prev)?;
        }
        let payload = extract_transfer_payload(original_root, change).await?;
        let snap = capture_path_preflight_snapshot(
            original_root,
            &change.path,
            change.previous_path.clone(),
            &change.range.kind,
            payload.to_state(),
        )
        .await?;
        preflight_snapshots.push(snap);
        transferred_paths.push(change.path.clone());
        if let Some(ref prev) = change.previous_path {
            let prev_snap = capture_path_preflight_snapshot(
                original_root,
                prev,
                None,
                &change.range.kind,
                SubmitWorktreeEntryState::Absent,
            )
            .await?;
            preflight_snapshots.push(prev_snap);
            transferred_paths.push(prev.clone());
        }
        transfers.push((change.path.clone(), change.previous_path.clone(), payload));
    }

    // Create dedicated isolated worktree directory under `<storage_dir>/submit-worktrees/<repo_id>/`.
    let base_dir = storage_dir
        .join("submit-worktrees")
        .join(sanitize_slug(repository_id));
    fs::create_dir_all(&base_dir)
        .map_err(|err| format!("Failed to create submit-worktrees directory: {err}"))?;
    let _ = fs::set_permissions(&base_dir, fs::Permissions::from_mode(0o700));
    let wt_name = format!(
        "{}-{}",
        utc_timestamp_compact(),
        sanitize_slug(branch_trimmed)
    );
    let mut wt_path = base_dir.join(&wt_name);
    let mut suffix = 1u32;
    while wt_path.exists() {
        wt_path = base_dir.join(format!("{wt_name}-{suffix}"));
        suffix += 1;
    }

    let wt_str = wt_path.to_string_lossy().into_owned();
    let (ok_add, _, err_add) = if reuse_existing_branch {
        run_git_bytes(
            original_root,
            &["worktree", "add", "--", &wt_str, branch_trimmed],
            false,
            true,
        )
        .await?
    } else {
        run_git_bytes(
            original_root,
            &[
                "worktree",
                "add",
                "-b",
                branch_trimmed,
                "--",
                &wt_str,
                &base_sha,
            ],
            false,
            true,
        )
        .await?
    };
    if !ok_add {
        return Err(format!(
            "Failed to create isolated worktree for branch `{branch_trimmed}`: {err_add}"
        ));
    }

    // Apply exact scope transfers into the isolated worktree; roll back temp worktree on error.
    let apply_result: Result<(), String> = async {
        for (path, prev, payload) in &transfers {
            if let Some(prev_path) = prev {
                write_entry_into_worktree(&wt_path, prev_path, &TransferPayload::Absent).await?;
            }
            write_entry_into_worktree(&wt_path, path, payload).await?;
        }
        Ok(())
    }
    .await;
    if let Err(err) = apply_result {
        let _ = run_git_bytes(
            original_root,
            &["worktree", "remove", "--force", "--", &wt_str],
            false,
            true,
        )
        .await;
        if !reuse_existing_branch {
            let _ = run_git_bytes(
                original_root,
                &["branch", "-D", "--", branch_trimmed],
                false,
                true,
            )
            .await;
        }
        return Err(err);
    }

    let (_, stat_out, _) = run_git_bytes(&wt_path, &["status", "--short"], false, true).await?;
    let diff_stat = String::from_utf8_lossy(&stat_out).trim().to_owned();
    let canonical_wt = fs::canonicalize(&wt_path).unwrap_or(wt_path);

    Ok(CreatedSubmitWorktree {
        group_id: group_id.to_owned(),
        worktree_path: canonical_wt.to_string_lossy().into_owned(),
        branch: branch_trimmed.to_owned(),
        base_ref: base_trimmed.to_owned(),
        base_sha,
        original_branch,
        original_head,
        transferred_paths,
        diff_stat,
        preflight_snapshots,
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IsolatedCommitResult {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
    pub pushed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_error: Option<String>,
}

pub async fn commit_in_isolated_worktree(
    worktree_path: &Path,
    allowed_paths: &[String],
    message: &str,
    push: bool,
    remote: Option<&str>,
) -> Result<IsolatedCommitResult, String> {
    if !worktree_path.exists() {
        return Err(format!(
            "Isolated submit worktree does not exist: {}",
            worktree_path.display()
        ));
    }
    let clean_msg = message.trim();
    if clean_msg.is_empty() {
        return Err("Commit message cannot be empty.".into());
    }
    if allowed_paths.is_empty() {
        return Err("No transferred paths recorded for this isolated submit worktree.".into());
    }
    let allowed_set: HashSet<&str> = allowed_paths.iter().map(String::as_str).collect();
    for p in allowed_paths {
        validate_repo_relative_path(p)?;
    }

    // Verify worktree only has changes inside `allowed_paths`.
    let (ok_st, st_out, st_err) = run_git_bytes(
        worktree_path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        false,
        true,
    )
    .await?;
    if !ok_st {
        return Err(format!(
            "Failed to inspect isolated worktree status: {st_err}"
        ));
    }
    let mut dirty_paths = Vec::new();
    let mut iter = st_out.split(|b| *b == 0).filter(|r| !r.is_empty());
    while let Some(rec) = iter.next() {
        if rec.len() < 4 {
            continue;
        }
        let xy = std::str::from_utf8(&rec[..2]).unwrap_or("");
        let path = std::str::from_utf8(&rec[3..])
            .map_err(|_| "Non-UTF-8 path in isolated worktree status")?;
        dirty_paths.push(path.to_owned());
        if (xy.starts_with('R') || xy.starts_with('C'))
            && let Some(prev) = iter.next()
            && let Ok(prev_str) = std::str::from_utf8(prev)
        {
            dirty_paths.push(prev_str.to_owned());
        }
    }
    if dirty_paths.is_empty() {
        return Err("Isolated submit worktree has no changes to commit.".into());
    }
    for dirty in &dirty_paths {
        if !allowed_set.contains(dirty.as_str()) {
            return Err(format!(
                "Isolated worktree contains unexpected path `{dirty}` outside group scope; refusing commit."
            ));
        }
    }

    // Stage ONLY literal allowed paths (`git add -A -- <paths>` stages modifications, additions, and deletions for ONLY those exact paths).
    let mut add_args: Vec<&str> = vec!["add", "-A", "--"];
    for p in allowed_paths {
        add_args.push(p.as_str());
    }
    let (ok_add, _, err_add) = run_git_bytes(worktree_path, &add_args, false, true).await?;
    if !ok_add {
        return Err(format!(
            "Failed to stage exact paths in isolated worktree: {err_add}"
        ));
    }

    // Commit via temp message file with REAL Git hooks enabled (never `--no-verify`!).
    let mut msg_file = NamedTempFile::new()
        .map_err(|err| format!("Failed to create temporary commit message file: {err}"))?;
    msg_file
        .write_all(clean_msg.as_bytes())
        .map_err(|err| format!("Failed to write commit message file: {err}"))?;
    msg_file
        .flush()
        .map_err(|err| format!("Failed to flush commit message file: {err}"))?;
    let msg_path_str = msg_file.path().to_string_lossy().into_owned();

    let (ok_commit, _, err_commit) = run_git_bytes(
        worktree_path,
        &["commit", "-F", &msg_path_str],
        false,
        false, // Run real repository hooks!
    )
    .await?;
    if !ok_commit {
        return Err(format!(
            "Isolated worktree commit failed (or rejected by Git hook): {err_commit}"
        ));
    }

    let (_, oid_out, _) = run_git_bytes(worktree_path, &["rev-parse", "HEAD"], false, true).await?;
    let oid = String::from_utf8_lossy(&oid_out).trim().to_owned();
    let short_oid: String = oid.chars().take(7).collect();
    let (_, br_out, _) = run_git_bytes(
        worktree_path,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        false,
        true,
    )
    .await?;
    let branch = String::from_utf8_lossy(&br_out).trim().to_owned();
    let title = clean_msg
        .lines()
        .next()
        .unwrap_or(clean_msg)
        .trim()
        .to_owned();

    let mut pushed = false;
    let mut remote_branch = None;
    let mut push_error = None;
    if push {
        let remote_name = remote
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("origin");
        let (ok_push, _, err_push) = run_git_bytes(
            worktree_path,
            &["push", "-u", remote_name, &branch],
            true,
            false,
        )
        .await?;
        if ok_push {
            pushed = true;
            remote_branch = Some(format!("{remote_name}/{branch}"));
        } else {
            push_error = Some(err_push);
        }
    }

    Ok(IsolatedCommitResult {
        oid,
        short_oid,
        title,
        branch,
        pushed,
        remote_branch,
        push_error,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedWorktreeEntry {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub is_original_checkout: bool,
    pub owned_by_picket_submit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

pub async fn parse_worktree_list(
    original_root: &Path,
    recorded_groups: &[SubmitGroupCheckpoint],
) -> Result<Vec<ParsedWorktreeEntry>, String> {
    let (ok, stdout, stderr) = run_git_bytes(
        original_root,
        &["worktree", "list", "--porcelain"],
        false,
        true,
    )
    .await?;
    if !ok {
        return Err(format!("Failed to list Git worktrees: {stderr}"));
    }
    let canon_original =
        fs::canonicalize(original_root).unwrap_or_else(|_| original_root.to_path_buf());
    let text = String::from_utf8_lossy(&stdout);
    let mut entries = Vec::new();
    let mut cur_path: Option<String> = None;
    let mut cur_head: Option<String> = None;
    let mut cur_branch: Option<String> = None;
    let mut cur_locked = false;
    let mut cur_prunable = false;

    let flush = |entries: &mut Vec<ParsedWorktreeEntry>,
                 path_opt: &mut Option<String>,
                 head_opt: &mut Option<String>,
                 branch_opt: &mut Option<String>,
                 locked: &mut bool,
                 prunable: &mut bool| {
        if let Some(p) = path_opt.take() {
            let pb = PathBuf::from(&p);
            let canon = fs::canonicalize(&pb).unwrap_or(pb);
            let matched_group = recorded_groups.iter().find(|g| {
                g.worktree_path.as_deref().is_some_and(|gp| {
                    let gpb = PathBuf::from(gp);
                    fs::canonicalize(&gpb).unwrap_or(gpb) == canon
                })
            });
            entries.push(ParsedWorktreeEntry {
                path: canon.to_string_lossy().into_owned(),
                head: head_opt.take(),
                branch: branch_opt.take(),
                locked: *locked,
                prunable: *prunable,
                is_original_checkout: canon == canon_original,
                owned_by_picket_submit: matched_group.is_some(),
                group_id: matched_group.map(|g| g.group_id.clone()),
            });
            *locked = false;
            *prunable = false;
        }
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            flush(
                &mut entries,
                &mut cur_path,
                &mut cur_head,
                &mut cur_branch,
                &mut cur_locked,
                &mut cur_prunable,
            );
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            cur_path = Some(p.trim().to_owned());
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            cur_head = Some(h.trim().to_owned());
        } else if let Some(b) = line.strip_prefix("branch ") {
            let trimmed = b.trim();
            cur_branch = Some(
                trimmed
                    .strip_prefix("refs/heads/")
                    .unwrap_or(trimmed)
                    .to_owned(),
            );
        } else if line == "locked" || line.starts_with("locked ") {
            cur_locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            cur_prunable = true;
        }
    }
    flush(
        &mut entries,
        &mut cur_path,
        &mut cur_head,
        &mut cur_branch,
        &mut cur_locked,
        &mut cur_prunable,
    );
    Ok(entries)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSubmitWorktreeResult {
    pub worktree_path: String,
    pub removed: bool,
    pub already_absent: bool,
    pub branch_deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_note: Option<String>,
}

/// Strictly implements `references/merge-chain.md` temporary worktree & branch cleanup rules:
/// verifies ownership, unlocked status, verified HEAD SHA, zero in-progress Git operations,
/// completely empty status (including untracked & ignored files), non-force `git worktree remove`,
/// post-removal re-check, and optional safe `git branch -d` (never `-D`).
pub async fn remove_verified_submit_worktree(
    original_root: &Path,
    recorded_group: &SubmitGroupCheckpoint,
    expected_head_sha: Option<&str>,
    delete_local_branch: bool,
) -> Result<RemoveSubmitWorktreeResult, String> {
    let Some(ref raw_wt_path) = recorded_group.worktree_path else {
        return Err("No isolated submit worktree path is recorded for this group.".into());
    };
    let wt_path = PathBuf::from(raw_wt_path);
    let canon_original =
        fs::canonicalize(original_root).unwrap_or_else(|_| original_root.to_path_buf());
    let canon_target = fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
    if canon_target == canon_original {
        return Err(
            "Refused to remove the original user checkout; only temporary Picket submit worktrees can be removed."
                .into(),
        );
    }

    let list = parse_worktree_list(original_root, std::slice::from_ref(recorded_group)).await?;
    let matched = list
        .iter()
        .find(|entry| Path::new(&entry.path) == canon_target);

    if matched.is_none() && !wt_path.exists() {
        let (branch_deleted, branch_note) = if delete_local_branch {
            try_safe_delete_local_branch(original_root, recorded_group.branch.as_deref()).await
        } else {
            (false, None)
        };
        return Ok(RemoveSubmitWorktreeResult {
            worktree_path: canon_target.to_string_lossy().into_owned(),
            removed: false,
            already_absent: true,
            branch_deleted,
            branch_note,
        });
    }

    let Some(entry) = matched else {
        return Err(format!(
            "Directory `{}` exists on disk but is not a registered Git worktree; keeping it in place.",
            canon_target.display()
        ));
    };
    if entry.locked {
        return Err(format!(
            "Worktree `{}` is locked; keeping it in place.",
            entry.path
        ));
    }
    if let Some(ref expected_branch) = recorded_group.branch
        && entry.branch.as_deref() != Some(expected_branch.as_str())
    {
        return Err(format!(
            "Worktree `{}` is currently on branch {:?} instead of recorded branch `{expected_branch}`; keeping it in place.",
            entry.path, entry.branch
        ));
    }
    let verified_sha = expected_head_sha.or(recorded_group.verified_head_sha.as_deref());
    if let Some(expected_sha) = verified_sha
        && entry.head.as_deref() != Some(expected_sha)
    {
        return Err(format!(
            "Worktree `{}` HEAD {:?} does not match verified revision `{expected_sha}`; keeping it in place.",
            entry.path, entry.head
        ));
    }

    // Check for in-progress Git operations inside the worktree's git dir.
    let (ok_gd, gd_out, gd_err) = run_git_bytes(
        &canon_target,
        &["rev-parse", "--absolute-git-dir"],
        false,
        true,
    )
    .await?;
    if !ok_gd {
        return Err(format!(
            "Could not resolve git dir for worktree `{}`: {gd_err}",
            canon_target.display()
        ));
    }
    let wt_git_dir = PathBuf::from(String::from_utf8_lossy(&gd_out).trim());
    for sentinel in [
        "MERGE_HEAD",
        "REBASE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
    ] {
        if wt_git_dir.join(sentinel).exists() {
            return Err(format!(
                "Worktree `{}` has an in-progress Git operation (`{sentinel}`); keeping it in place.",
                canon_target.display()
            ));
        }
    }

    // Exact cleanliness check from `references/merge-chain.md` line 98:
    let (ok_st, st_out, st_err) = run_git_bytes(
        &canon_target,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored",
            "--ignore-submodules=none",
        ],
        false,
        true,
    )
    .await?;
    if !ok_st {
        return Err(format!(
            "Failed to verify worktree cleanliness for `{}`: {st_err}",
            canon_target.display()
        ));
    }
    if !st_out.is_empty() {
        return Err(format!(
            "Worktree `{}` is not clean (contains staged, unstaged, untracked, or ignored files); keeping it in place.",
            canon_target.display()
        ));
    }

    // Remove without `--force`:
    let target_str = canon_target.to_string_lossy().into_owned();
    let (ok_rm, _, err_rm) = run_git_bytes(
        original_root,
        &["worktree", "remove", "--", &target_str],
        false,
        true,
    )
    .await?;
    if !ok_rm {
        return Err(format!(
            "Git refused non-force `git worktree remove` for `{target_str}`: {err_rm}"
        ));
    }

    // Recheck worktree list & filesystem path after removal.
    let list_after =
        parse_worktree_list(original_root, std::slice::from_ref(recorded_group)).await?;
    if list_after
        .iter()
        .any(|item| Path::new(&item.path) == canon_target)
        || canon_target.exists()
    {
        return Err(format!(
            "Worktree `{target_str}` still appears present after `git worktree remove`."
        ));
    }

    let (branch_deleted, branch_note) = if delete_local_branch {
        try_safe_delete_local_branch(original_root, recorded_group.branch.as_deref()).await
    } else {
        (false, None)
    };

    Ok(RemoveSubmitWorktreeResult {
        worktree_path: target_str,
        removed: true,
        already_absent: false,
        branch_deleted,
        branch_note,
    })
}

async fn try_safe_delete_local_branch(
    original_root: &Path,
    branch_opt: Option<&str>,
) -> (bool, Option<String>) {
    let Some(branch) = branch_opt.map(str::trim).filter(|b| !b.is_empty()) else {
        return (false, None);
    };
    if matches!(branch, "main" | "master" | "HEAD") {
        return (false, Some(format!("Kept protected branch `{branch}`.")));
    }
    let ref_name = format!("refs/heads/{branch}");
    let Ok((exists, _, _)) = run_git_bytes(
        original_root,
        &["show-ref", "--verify", "--quiet", &ref_name],
        false,
        true,
    )
    .await
    else {
        return (
            false,
            Some(format!("Could not verify local branch `{branch}`.")),
        );
    };
    if !exists {
        return (
            false,
            Some(format!("Local branch `{branch}` already absent.")),
        );
    }
    // Safe merged-only delete (`-d`, NEVER `-D`):
    match run_git_bytes(original_root, &["branch", "-d", "--", branch], false, true).await {
        Ok((true, _, _)) => (true, None),
        Ok((false, _, err)) => (
            false,
            Some(format!(
                "Kept local branch `{branch}` because safe `git branch -d` refused removal: {err}"
            )),
        ),
        Err(err) => (false, Some(err)),
    }
}
