use crate::repository::{
    RepositoryStore, Result, commit,
    parse::{self, StatusEntry},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

const SYNC_BACKUP_REL_PATH: &str = "picket/sync-backup.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DirtySyncBackup {
    branch: String,
    pre_rebase_head: String,
    tracked_items: Vec<BackedUpTrackedPath>,
    untracked_hashes: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackedUpTrackedPath {
    path: String,
    index_entry: Option<(String, String)>,
    working_blob_oid: Option<String>,
    symlink_target: Option<Vec<u8>>,
    executable: bool,
    working_missing: bool,
    working_sha256: String,
}

impl RepositoryStore {
    async fn sync_backup_file_path(&self, root: &Path) -> Result<PathBuf> {
        let raw = self
            .inner
            .reader
            .text(
                root,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    SYNC_BACKUP_REL_PATH,
                ],
            )
            .await?;
        Ok(PathBuf::from(raw))
    }

    pub(super) async fn create_dirty_backup(
        &self,
        root: &Path,
        branch: &str,
        head: &str,
        entries: &[StatusEntry],
    ) -> Result<Option<DirtySyncBackup>> {
        if entries.is_empty() {
            return Ok(None);
        }
        let reader = &self.inner.reader;
        let index_bytes = reader.bytes(root, &["ls-files", "--stage", "-z"]).await?;
        let stage_map = commit::stage_entries_by_path(&index_bytes)?;

        let mut tracked_paths = BTreeSet::new();
        let mut untracked_hashes = Vec::new();
        for entry in entries {
            if entry.xy == "??" {
                untracked_hashes.push((entry.path.clone(), self.working_file(root, &entry.path)?));
            } else {
                tracked_paths.insert(entry.path.clone());
                if let Some(prev) = &entry.previous {
                    tracked_paths.insert(prev.clone());
                }
            }
        }

        let mut tracked_items = Vec::new();
        for path in &tracked_paths {
            let index_entry = if let Some(records) = stage_map.get(path) {
                let first = records
                    .split(|b| *b == 0)
                    .find(|r| !r.is_empty())
                    .ok_or("Index stage entry is empty")?;
                let tab = first
                    .iter()
                    .position(|b| *b == b'\t')
                    .ok_or("Index stage entry is missing tab separator")?;
                let header = parse::utf8(&first[..tab])?;
                let mut parts = header.split_whitespace();
                let mode = parts.next().ok_or("Missing staged file mode")?.to_owned();
                let oid = parts.next().ok_or("Missing staged object hash")?.to_owned();
                Some((mode, oid))
            } else {
                None
            };

            let full = root.join(path);
            let working_sha256 = self.working_file(root, path)?;
            let mut working_blob_oid = None;
            let mut symlink_target = None;
            let mut executable = false;
            let mut working_missing = false;

            match fs::symlink_metadata(&full) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    working_missing = true;
                }
                Err(_) => return Err("Cannot back up uncommitted worktree file metadata".into()),
                Ok(meta) if meta.file_type().is_symlink() => {
                    let link =
                        fs::read_link(&full).map_err(|_| "Cannot back up worktree symlink")?;
                    symlink_target = Some(link.as_os_str().as_encoded_bytes().to_vec());
                }
                Ok(meta) if meta.is_file() => {
                    executable = meta.permissions().mode() & 0o111 != 0;
                    let file_bytes =
                        fs::read(&full).map_err(|_| "Cannot read worktree draft for backup")?;
                    let hashed = reader
                        .write_with(
                            root,
                            &["hash-object", "-w", "--stdin"],
                            None,
                            Some(&file_bytes),
                        )
                        .await?;
                    if !hashed.success {
                        return Err(
                            "Cannot store worktree draft into Git object database for safe backup"
                                .into(),
                        );
                    }
                    let oid = parse::utf8(&hashed.bytes)?.trim().to_owned();
                    working_blob_oid = Some(oid);
                }
                Ok(_) => {
                    return Err(
                        "Changes include special files; paused automatic rebase to protect the working tree"
                            .into(),
                    );
                }
            }

            tracked_items.push(BackedUpTrackedPath {
                path: path.clone(),
                index_entry,
                working_blob_oid,
                symlink_target,
                executable,
                working_missing,
                working_sha256,
            });
        }

        let backup = DirtySyncBackup {
            branch: branch.to_owned(),
            pre_rebase_head: head.to_owned(),
            tracked_items,
            untracked_hashes,
        };
        let backup_path = self.sync_backup_file_path(root).await?;
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).map_err(|_| "Cannot create sync backup directory")?;
        }
        let bytes = serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())?;
        let mut temp = NamedTempFile::new_in(backup_path.parent().unwrap_or(root))
            .map_err(|_| "Cannot create temporary sync backup file")?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| "Cannot set sync backup file permissions")?;
        temp.write_all(&bytes)
            .map_err(|_| "Cannot write sync backup record")?;
        temp.as_file()
            .sync_all()
            .map_err(|_| "Cannot sync backup record to disk")?;
        temp.persist(&backup_path)
            .map_err(|_| "Cannot persist sync backup record")?;

        // Reset ONLY the backed-up tracked paths to `HEAD` so `git rebase` runs on a clean tracked tree:
        for path in &tracked_paths {
            let ls_head = reader
                .bytes(root, &["ls-tree", "-z", head, "--", path])
                .await?;
            let first_rec = ls_head.split(|b| *b == 0).find(|r| !r.is_empty());
            if let Some(rec) = first_rec {
                let tab = rec
                    .iter()
                    .position(|b| *b == b'\t')
                    .ok_or("Invalid ls-tree entry format")?;
                let header = parse::utf8(&rec[..tab])?;
                let mut parts = header.split_whitespace();
                let mode = parts.next().ok_or("Missing HEAD file mode")?;
                let _kind = parts.next();
                let oid = parts.next().ok_or("Missing HEAD object hash")?;
                let upd = reader
                    .write(
                        root,
                        &[
                            "update-index",
                            "--add",
                            "--cacheinfo",
                            &format!("{mode},{oid},{path}"),
                        ],
                    )
                    .await?;
                if !upd.success {
                    return Err(
                        "Failed to reset staged entry while preparing lossless rebase".into(),
                    );
                }
                let co = reader
                    .write(root, &["checkout-index", "-f", "-q", "--", path])
                    .await?;
                if !co.success {
                    return Err(
                        "Failed to check out baseline file while preparing lossless rebase".into(),
                    );
                }
            } else {
                let rm = reader
                    .write(root, &["update-index", "--force-remove", "--", path])
                    .await?;
                if !rm.success {
                    return Err(
                        "Failed to remove staged addition while preparing lossless rebase".into(),
                    );
                }
                let _ = fs::remove_file(root.join(path));
            }
        }

        Ok(Some(backup))
    }

    pub(super) async fn restore_dirty_backup(&self, root: &Path) -> Result<bool> {
        let backup_path = self.sync_backup_file_path(root).await?;
        let bytes = match fs::read(&backup_path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err("Cannot read pre-sync staging and draft backup record".into()),
        };
        let backup: DirtySyncBackup = serde_json::from_slice(&bytes).map_err(|_| {
            "Pre-sync staging and draft backup record is corrupted; kept in .git/picket for inspection"
        })?;
        let reader = &self.inner.reader;

        for item in &backup.tracked_items {
            if let Some((mode, oid)) = &item.index_entry {
                let out = reader
                    .write(
                        root,
                        &[
                            "update-index",
                            "--add",
                            "--cacheinfo",
                            &format!("{mode},{oid},{}", item.path),
                        ],
                    )
                    .await?;
                if !out.success {
                    return Err(format!("Failed to restore staged state for {}", item.path));
                }
            } else {
                let out = reader
                    .write(root, &["update-index", "--force-remove", "--", &item.path])
                    .await?;
                if !out.success {
                    return Err(format!(
                        "Failed to restore staged deletion for {}",
                        item.path
                    ));
                }
            }

            let full = root.join(&item.path);
            if item.working_missing {
                let _ = fs::remove_file(&full);
            } else if let Some(link_bytes) = &item.symlink_target {
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent).map_err(|_| "Cannot create working directory")?;
                }
                let _ = fs::remove_file(&full);
                let target = Path::new(OsStr::from_bytes(link_bytes));
                std::os::unix::fs::symlink(target, &full)
                    .map_err(|_| "Cannot restore working tree symbolic link")?;
            } else if let Some(blob_oid) = &item.working_blob_oid {
                let content = reader.bytes(root, &["cat-file", "blob", blob_oid]).await?;
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent).map_err(|_| "Cannot create working directory")?;
                }
                let _ = fs::remove_file(&full);
                fs::write(&full, &content)
                    .map_err(|_| "Cannot restore unstaged draft file to working tree")?;
                let perm = if item.executable { 0o755 } else { 0o644 };
                fs::set_permissions(&full, fs::Permissions::from_mode(perm))
                    .map_err(|_| "Cannot restore working tree file permissions")?;
            }

            let restored_hash = self.working_file(root, &item.path)?;
            if restored_hash != item.working_sha256 {
                return Err(format!(
                    "Post-sync working draft hash mismatch for {}; kept backup record for inspection",
                    item.path
                ));
            }
        }

        for (untracked_path, expected_hash) in &backup.untracked_hashes {
            let actual = self.working_file(root, untracked_path)?;
            if &actual != expected_hash {
                return Err(format!(
                    "Untracked file {untracked_path} changed unexpectedly during sync"
                ));
            }
        }

        let _ = fs::remove_file(&backup_path);
        Ok(true)
    }
}
