//! Step 5 verified local duplicate backup & cleanup engine (`agent/skills/submit/references/cleanup.md`).
//!
//! Guarantees:
//! - Never runs unless every candidate group has BOTH a verified pushed remote tip AND a verified PR/MR URL.
//! - Re-verifies original branch, HEAD SHA, index stage-0 entry, and on-disk bytes/mode/symlink target
//!   against the preflight snapshot recorded before isolated worktree submission.
//! - Retains partially staged files (which still hold unsubmitted unstaged drafts), files edited after
//!   preflight, ambiguous states, or paths whose submitted content cannot be verified in the pushed tip.
//! - Treats rename `(previous_path, path)` pairs as an atomic unit.
//! - Always creates a brand-new, never-overwritten `<git-dir>/submit-backup/<UTC-timestamp>/` directory,
//!   backs up both index blobs (verified via `git hash-object --no-filters`) and working-tree files/symlinks
//!   (verified byte-for-byte & mode), and makes ZERO cleanup writes if backup verification fails.
//! - Never deletes the backup directory and never executes a repository-wide `git reset` or `git clean`.

use super::{
    session_store::{
        RetainedDuplicateEntry, SubmitCleanupRecord, SubmitGroupCheckpoint,
        SubmitPathPreflightSnapshot, SubmitWorktreeEntryState, utc_timestamp_compact,
    },
    submit_worktree::{
        inspect_index_stage_entry, inspect_worktree_entry, run_git_bytes, sha256_hex,
        validate_repo_relative_path,
    },
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupPathManifestEntry {
    id: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_oid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_oid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_blob_file: Option<String>,
    worktree_state: SubmitWorktreeEntryState,
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_copy_file: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubmitBackupManifest {
    version: u8,
    created_at_utc: String,
    original_branch: String,
    original_head: String,
    verified_groups: Vec<VerifiedGroupProof>,
    entries: Vec<BackupPathManifestEntry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedGroupProof {
    group_id: String,
    branch: String,
    remote: String,
    remote_tip_sha: String,
    pr_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupDuplicatesOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_dir: Option<String>,
    pub cleaned_paths: Vec<String>,
    pub retained_paths: Vec<RetainedDuplicateEntry>,
    pub restore_guide: String,
}

impl CleanupDuplicatesOutcome {
    pub fn to_record(&self) -> SubmitCleanupRecord {
        SubmitCleanupRecord {
            backup_dir: self.backup_dir.clone(),
            cleaned_paths: self.cleaned_paths.clone(),
            retained_paths: self.retained_paths.clone(),
        }
    }
}

async fn resolve_verified_remote_tip(
    original_root: &Path,
    remote: &str,
    branch: &str,
) -> Result<String, String> {
    // Refresh tracking ref or inspect remote tip without mutating HEAD/worktree.
    let refspec = format!("refs/heads/{branch}");
    let (ok_ls, ls_out, ls_err) = run_git_bytes(
        original_root,
        &["ls-remote", "--heads", remote, &refspec],
        true,
        true,
    )
    .await?;
    if !ok_ls {
        return Err(format!(
            "Could not verify remote tip for `{remote}/{branch}`: {ls_err}"
        ));
    }
    let ls_text = String::from_utf8_lossy(&ls_out);
    let Some(remote_sha) = ls_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
    else {
        return Err(format!(
            "Remote branch `{remote}/{branch}` does not exist on remote `{remote}`."
        ));
    };

    // Ensure the remote commit object is present locally so we can inspect its tree/blobs.
    let commit_spec = format!("{remote_sha}^{{commit}}");
    let (has_local, _, _) = run_git_bytes(
        original_root,
        &["cat-file", "-e", &commit_spec],
        false,
        true,
    )
    .await?;
    if !has_local {
        let _ = run_git_bytes(
            original_root,
            &["fetch", "--no-tags", remote, &refspec],
            true,
            true,
        )
        .await;
    }
    Ok(remote_sha)
}

async fn verify_path_in_remote_tip(
    original_root: &Path,
    remote_tip_sha: &str,
    snap: &SubmitPathPreflightSnapshot,
) -> Result<bool, String> {
    let (ok_tree, tree_out, _) = run_git_bytes(
        original_root,
        &["ls-tree", "-z", remote_tip_sha, "--", &snap.path],
        false,
        true,
    )
    .await?;
    if !ok_tree {
        return Ok(false);
    }
    let record = tree_out.split(|b| *b == 0).find(|r| !r.is_empty());
    match (&snap.submitted_state, record) {
        (SubmitWorktreeEntryState::Absent, None) => Ok(true),
        (SubmitWorktreeEntryState::Absent, Some(_)) => Ok(false),
        (_, None) => Ok(false),
        (
            SubmitWorktreeEntryState::File {
                mode: expected_mode,
                sha256: expected_sha,
            },
            Some(rec),
        ) => {
            let Some(tab) = rec.iter().position(|b| *b == b'\t') else {
                return Ok(false);
            };
            let header = std::str::from_utf8(&rec[..tab]).unwrap_or("");
            let parts: Vec<&str> = header.split_whitespace().collect();
            if parts.len() != 3 || parts[1] != "blob" {
                return Ok(false);
            }
            let git_mode = parts[0];
            let expected_exec = (*expected_mode & 0o111) != 0;
            let actual_exec = git_mode == "100755";
            if (git_mode != "100644" && git_mode != "100755") || expected_exec != actual_exec {
                return Ok(false);
            }
            let oid = parts[2];
            let (ok_blob, bytes, _) =
                run_git_bytes(original_root, &["cat-file", "blob", oid], false, true).await?;
            Ok(ok_blob && sha256_hex(&bytes) == *expected_sha)
        }
        (
            SubmitWorktreeEntryState::Symlink {
                target: expected_target,
            },
            Some(rec),
        ) => {
            let Some(tab) = rec.iter().position(|b| *b == b'\t') else {
                return Ok(false);
            };
            let header = std::str::from_utf8(&rec[..tab]).unwrap_or("");
            let parts: Vec<&str> = header.split_whitespace().collect();
            if parts.len() != 3 || parts[0] != "120000" {
                return Ok(false);
            }
            let oid = parts[2];
            let (ok_blob, bytes, _) =
                run_git_bytes(original_root, &["cat-file", "blob", oid], false, true).await?;
            Ok(ok_blob && String::from_utf8_lossy(&bytes) == expected_target.as_str())
        }
    }
}

async fn verify_snapshot_unchanged(
    original_root: &Path,
    snap: &SubmitPathPreflightSnapshot,
) -> Result<(), String> {
    let live_index = inspect_index_stage_entry(original_root, &snap.path).await?;
    let expected_index = match (&snap.index_mode, &snap.index_oid, &snap.index_sha256) {
        (Some(m), Some(o), Some(s)) => Some((m.clone(), o.clone(), s.clone())),
        _ => None,
    };
    if live_index != expected_index {
        return Err("Index entry changed after preflight snapshot.".into());
    }
    let live_wt = inspect_worktree_entry(original_root, &snap.path).await?;
    if live_wt != snap.worktree_state {
        return Err("Working-tree file changed after preflight snapshot.".into());
    }
    Ok(())
}

fn create_unique_backup_dir(git_dir: &Path) -> Result<PathBuf, String> {
    let root = git_dir.join("submit-backup");
    fs::create_dir_all(&root)
        .map_err(|err| format!("Failed to create submit-backup directory: {err}"))?;
    let stamp = utc_timestamp_compact();
    for seq in 0..1000u32 {
        let name = if seq == 0 {
            stamp.clone()
        } else {
            format!("{stamp}-{seq}")
        };
        let candidate = root.join(name);
        match fs::create_dir(&candidate) {
            Ok(()) => {
                let _ = fs::set_permissions(&candidate, fs::Permissions::from_mode(0o700));
                fs::create_dir_all(candidate.join("index"))
                    .map_err(|err| format!("Failed to create backup index dir: {err}"))?;
                fs::create_dir_all(candidate.join("worktree"))
                    .map_err(|err| format!("Failed to create backup worktree dir: {err}"))?;
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "Failed to create unique submit-backup directory: {err}"
                ));
            }
        }
    }
    Err("Could not allocate a unique submit-backup directory.".into())
}

pub async fn cleanup_verified_submit_duplicates(
    original_root: &Path,
    recorded_original_branch: &str,
    recorded_original_head: &str,
    groups: &[SubmitGroupCheckpoint],
) -> Result<CleanupDuplicatesOutcome, String> {
    if groups.is_empty() {
        return Err("No isolated submit groups recorded for Step 5 cleanup.".into());
    }

    // 1. Verify original checkout still matches recorded branch & HEAD.
    let (ok_head, head_out, err_head) = run_git_bytes(
        original_root,
        &["rev-parse", "--verify", "HEAD"],
        false,
        true,
    )
    .await?;
    if !ok_head {
        return Err(format!("Failed to read original checkout HEAD: {err_head}"));
    }
    let live_head = String::from_utf8_lossy(&head_out).trim().to_owned();
    let (_, br_out, _) = run_git_bytes(
        original_root,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        false,
        true,
    )
    .await?;
    let live_branch = {
        let s = String::from_utf8_lossy(&br_out).trim().to_owned();
        if s.is_empty() { "HEAD".into() } else { s }
    };
    if live_head != recorded_original_head || live_branch != recorded_original_branch {
        return Err(format!(
            "Original checkout moved from `{recorded_original_branch}` ({recorded_original_head}) to `{live_branch}` ({live_head}); refusing Step 5 duplicate cleanup."
        ));
    }

    // 2. Require EVERY group to have BOTH a pushed remote tip AND a verified PR/MR URL.
    let mut verified_groups = Vec::with_capacity(groups.len());
    let mut group_tips: HashMap<&str, String> = HashMap::new();
    for group in groups {
        let Some(pr_url) = group.pr_url.as_ref().filter(|u| !u.trim().is_empty()) else {
            return Err(format!(
                "Group `{}` does not have a verified PR/MR URL yet; Step 5 cleanup requires every group to have a pushed branch and a verified PR/MR.",
                group.group_id
            ));
        };
        let Some(branch) = group.branch.as_ref().filter(|b| !b.trim().is_empty()) else {
            return Err(format!(
                "Group `{}` has no recorded pushed branch.",
                group.group_id
            ));
        };
        let remote = group
            .remote
            .as_deref()
            .filter(|r| !r.trim().is_empty())
            .unwrap_or("origin");
        let tip = resolve_verified_remote_tip(original_root, remote, branch).await?;
        group_tips.insert(group.group_id.as_str(), tip.clone());
        verified_groups.push(VerifiedGroupProof {
            group_id: group.group_id.clone(),
            branch: branch.clone(),
            remote: remote.to_owned(),
            remote_tip_sha: tip,
            pr_url: pr_url.clone(),
        });
    }

    // 3. Classify every recorded path into verified-cleanable vs retained-with-reason.
    // Treat rename pairs `(previous_path, path)` as one atomic unit.
    let mut verified_snaps: Vec<SubmitPathPreflightSnapshot> = Vec::new();
    let mut retained: Vec<RetainedDuplicateEntry> = Vec::new();
    let mut seen_paths = HashSet::new();

    for group in groups {
        let tip = &group_tips[group.group_id.as_str()];
        // Index companion rename paths by path so we can validate both sides atomically.
        let by_path: HashMap<&str, &SubmitPathPreflightSnapshot> = group
            .preflight_snapshots
            .iter()
            .map(|s| (s.path.as_str(), s))
            .collect();
        let rename_old_paths: HashSet<&str> = group
            .preflight_snapshots
            .iter()
            .filter_map(|s| s.previous_path.as_deref())
            .collect();

        for snap in &group.preflight_snapshots {
            if rename_old_paths.contains(snap.path.as_str()) {
                // Handled atomically together with the destination rename entry.
                continue;
            }
            if !seen_paths.insert(snap.path.clone()) {
                continue;
            }
            let companion = snap
                .previous_path
                .as_deref()
                .and_then(|prev| by_path.get(prev).copied());

            let retain_unit = |retained: &mut Vec<RetainedDuplicateEntry>, reason: String| {
                retained.push(RetainedDuplicateEntry {
                    path: snap.path.clone(),
                    reason: reason.clone(),
                });
                if let Some(comp) = companion {
                    retained.push(RetainedDuplicateEntry {
                        path: comp.path.clone(),
                        reason,
                    });
                }
            };

            // Partial staging carries unsubmitted unstaged draft edits in the working tree -> NEVER discard!
            if snap.range_kind == "partial" {
                retain_unit(
                    &mut retained,
                    "Partially staged file contains unsubmitted working-tree edits; retained safely."
                        .into(),
                );
                continue;
            }
            if let Err(reason) = verify_snapshot_unchanged(original_root, snap).await {
                retain_unit(&mut retained, reason);
                continue;
            }
            if let Some(comp) = companion
                && let Err(reason) = verify_snapshot_unchanged(original_root, comp).await
            {
                retain_unit(&mut retained, format!("Rename source changed: {reason}"));
                continue;
            }

            let covered = verify_path_in_remote_tip(original_root, tip, snap).await?;
            let comp_covered = match companion {
                Some(comp) => verify_path_in_remote_tip(original_root, tip, comp).await?,
                None => true,
            };
            if !covered || !comp_covered {
                retain_unit(
                    &mut retained,
                    format!("Submitted content could not be verified in pushed remote tip {tip}."),
                );
                continue;
            }

            verified_snaps.push(snap.clone());
            if let Some(comp) = companion {
                seen_paths.insert(comp.path.clone());
                verified_snaps.push(comp.clone());
            }
        }
    }

    if verified_snaps.is_empty() {
        return Ok(CleanupDuplicatesOutcome {
            backup_dir: None,
            cleaned_paths: Vec::new(),
            retained_paths: retained,
            restore_guide: "No local files were modified; all candidate paths were retained."
                .into(),
        });
    }

    // 4. Back up every verified path BEFORE any cleanup write, and verify every backed-up byte/OID.
    let (ok_gd, gd_out, gd_err) = run_git_bytes(
        original_root,
        &["rev-parse", "--absolute-git-dir"],
        false,
        true,
    )
    .await?;
    if !ok_gd {
        return Err(format!(
            "Could not resolve original checkout git dir for Step 5 backup: {gd_err}"
        ));
    }
    let git_dir = PathBuf::from(String::from_utf8_lossy(&gd_out).trim());
    let backup_dir = create_unique_backup_dir(&git_dir)?;
    let mut manifest_entries = Vec::with_capacity(verified_snaps.len());

    for (idx, snap) in verified_snaps.iter().enumerate() {
        validate_repo_relative_path(&snap.path)?;
        let entry_id = format!("entry-{idx:04}");
        let mut index_blob_file = None;
        if let Some(ref oid) = snap.index_oid {
            let (ok_blob, blob_bytes, err_blob) =
                run_git_bytes(original_root, &["cat-file", "blob", oid], false, true).await?;
            if !ok_blob {
                return Err(format!(
                    "Step 5 backup aborted before writes: failed to read index blob {oid} for `{}`: {err_blob}",
                    snap.path
                ));
            }
            let rel_blob = format!("index/{entry_id}.blob");
            let abs_blob = backup_dir.join(&rel_blob);
            fs::write(&abs_blob, &blob_bytes)
                .map_err(|err| format!("Step 5 backup aborted before writes: {err}"))?;
            let abs_blob_str = abs_blob.to_string_lossy().into_owned();
            let (ok_hash, hash_out, err_hash) = run_git_bytes(
                original_root,
                &["hash-object", "--no-filters", "--", &abs_blob_str],
                false,
                true,
            )
            .await?;
            let computed_oid = String::from_utf8_lossy(&hash_out).trim().to_owned();
            if !ok_hash || computed_oid != *oid {
                return Err(format!(
                    "Step 5 backup verification failed for `{}` (expected {oid}, got `{computed_oid}`: {err_hash}); zero cleanup writes performed.",
                    snap.path
                ));
            }
            index_blob_file = Some(rel_blob);
        }

        let mut worktree_copy_file = None;
        match &snap.worktree_state {
            SubmitWorktreeEntryState::File { mode, sha256 } => {
                let src = original_root.join(&snap.path);
                let raw = fs::read(&src).map_err(|err| {
                    format!(
                        "Step 5 backup aborted before writes reading `{}`: {err}",
                        snap.path
                    )
                })?;
                let meta = fs::symlink_metadata(&src).map_err(|err| {
                    format!(
                        "Step 5 backup aborted before writes stat `{}`: {err}",
                        snap.path
                    )
                })?;
                let live_mode = meta.permissions().mode() & 0o777;
                if sha256_hex(&raw) != *sha256 || live_mode != *mode {
                    return Err(format!(
                        "Step 5 backup aborted before writes: `{}` changed during backup.",
                        snap.path
                    ));
                }
                let rel_wt = format!("worktree/{entry_id}.file");
                let abs_wt = backup_dir.join(&rel_wt);
                fs::write(&abs_wt, &raw)
                    .map_err(|err| format!("Step 5 backup write failed: {err}"))?;
                fs::set_permissions(&abs_wt, fs::Permissions::from_mode(*mode))
                    .map_err(|err| format!("Step 5 backup chmod failed: {err}"))?;
                // Verify saved file bytes and mode against original.
                let reread = fs::read(&abs_wt)
                    .map_err(|err| format!("Step 5 backup re-read failed: {err}"))?;
                let reread_mode = fs::symlink_metadata(&abs_wt)
                    .map_err(|err| format!("Step 5 backup stat failed: {err}"))?
                    .permissions()
                    .mode()
                    & 0o777;
                if sha256_hex(&reread) != *sha256 || reread_mode != *mode {
                    return Err(format!(
                        "Step 5 backup verification mismatch for `{}`; zero cleanup writes performed.",
                        snap.path
                    ));
                }
                worktree_copy_file = Some(rel_wt);
            }
            SubmitWorktreeEntryState::Symlink { target } => {
                let src = original_root.join(&snap.path);
                let live_target = fs::read_link(&src)
                    .map_err(|err| {
                        format!(
                            "Step 5 backup failed reading symlink `{}`: {err}",
                            snap.path
                        )
                    })?
                    .to_string_lossy()
                    .into_owned();
                if live_target != *target {
                    return Err(format!(
                        "Step 5 backup aborted before writes: symlink `{}` target changed.",
                        snap.path
                    ));
                }
            }
            SubmitWorktreeEntryState::Absent => {}
        }

        manifest_entries.push(BackupPathManifestEntry {
            id: entry_id,
            path: snap.path.clone(),
            previous_path: snap.previous_path.clone(),
            head_oid: snap.head_oid.clone(),
            index_mode: snap.index_mode.clone(),
            index_oid: snap.index_oid.clone(),
            index_blob_file,
            worktree_state: snap.worktree_state.clone(),
            worktree_copy_file,
        });
    }

    let manifest = SubmitBackupManifest {
        version: 1,
        created_at_utc: utc_timestamp_compact(),
        original_branch: recorded_original_branch.to_owned(),
        original_head: recorded_original_head.to_owned(),
        verified_groups,
        entries: manifest_entries,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|err| format!("Failed to serialize Step 5 backup manifest: {err}"))?;
    let manifest_path = backup_dir.join("manifest.json");
    fs::write(&manifest_path, &manifest_bytes)
        .map_err(|err| format!("Failed to write Step 5 backup manifest: {err}"))?;
    let _ = fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o600));

    // 5. Clean ONLY verified literal paths (re-checking right before each write).
    let mut cleaned_paths = Vec::new();
    for snap in &verified_snaps {
        if let Err(reason) = verify_snapshot_unchanged(original_root, snap).await {
            retained.push(RetainedDuplicateEntry {
                path: snap.path.clone(),
                reason: format!("Changed right before cleanup write: {reason}"),
            });
            continue;
        }
        if snap.head_oid.is_some() {
            // Tracked path in HEAD: restore index & working tree for ONLY this literal path.
            let (ok_co, _, err_co) = run_git_bytes(
                original_root,
                &["checkout", recorded_original_head, "--", &snap.path],
                false,
                true,
            )
            .await?;
            if !ok_co {
                retained.push(RetainedDuplicateEntry {
                    path: snap.path.clone(),
                    reason: format!("Failed to restore tracked path to HEAD: {err_co}"),
                });
                continue;
            }
        } else {
            // Newly added / untracked path (absent from HEAD): remove from index if staged, then delete exact file.
            if snap.index_oid.is_some() {
                let (ok_rm, _, err_rm) = run_git_bytes(
                    original_root,
                    &[
                        "rm",
                        "--cached",
                        "--force",
                        "--ignore-unmatch",
                        "--",
                        &snap.path,
                    ],
                    false,
                    true,
                )
                .await?;
                if !ok_rm {
                    retained.push(RetainedDuplicateEntry {
                        path: snap.path.clone(),
                        reason: format!("Failed to remove newly staged path from index: {err_rm}"),
                    });
                    continue;
                }
            }
            let full = original_root.join(&snap.path);
            if fs::symlink_metadata(&full).is_ok()
                && let Err(err) = fs::remove_file(&full)
            {
                retained.push(RetainedDuplicateEntry {
                    path: snap.path.clone(),
                    reason: format!("Failed to remove verified untracked file: {err}"),
                });
                continue;
            }
        }
        cleaned_paths.push(snap.path.clone());
    }

    let backup_dir_str = backup_dir.to_string_lossy().into_owned();
    let restore_guide = format!(
        "Backup saved at `{backup_dir_str}` (manifest.json). To restore any cleaned path at {recorded_original_branch} ({recorded_original_head}): write its saved index blob with `git hash-object -w --no-filters` + `git update-index --add --cacheinfo <mode> <oid> \"<path>\"` (or `git update-index --force-remove` if index was absent), then copy its saved working-tree file or symlink target back independently."
    );

    Ok(CleanupDuplicatesOutcome {
        backup_dir: Some(backup_dir_str),
        cleaned_paths,
        retained_paths: retained,
        restore_guide,
    })
}
