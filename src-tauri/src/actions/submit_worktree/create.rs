use super::inspect::{
    capture_path_preflight_snapshot, inspect_index_stage_entry, run_git_bytes, sanitize_slug,
    sha256_hex, validate_repo_relative_path,
};
use crate::{
    actions::session_store::{
        SubmitPathPreflightSnapshot, SubmitWorktreeEntryState, utc_timestamp_compact,
    },
    repository::types::Change,
};
use serde::Serialize;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

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
