//! Remote synchronization, dirty-worktree lossless rebase, conflict stage extraction,
//! authorized post-merge validation, and fast-forward-only push coordinator.
//! Never invokes `git push --force`, `--force-with-lease`, or `git rebase --autostash`.
use super::{parse::StatusEntry, runner::GitOutput, *};
use crate::{
    foundation::pi::PiResolvedConflictFile,
    i18n::{self, LanguageKind},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet, ffi::OsStr, io::Write, os::unix::ffi::OsStrExt,
    os::unix::fs::PermissionsExt,
};
use tempfile::NamedTempFile;

const SYNC_BACKUP_REL_PATH: &str = "picket/sync-backup.json";
const MAX_CONFLICT_TEXT_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug)]
pub struct UpstreamSpec {
    pub remote: String,
    pub remote_branch: String,
    pub display: String,
    pub tracking_ref: String,
}

#[derive(Clone, Debug)]
pub struct ConflictedFileSummary {
    pub file_id: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct RebasedCommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
}

#[derive(Clone, Debug)]
pub struct LiveRemoteStatus {
    pub upstream: Option<UpstreamSpec>,
    pub ahead: u64,
    pub rebase_in_progress: bool,
    pub merge_or_other_in_progress: bool,
    pub conflicted_files: Vec<ConflictedFileSummary>,
    pub unpushed_commits: Vec<RebasedCommitSummary>,
}

#[derive(Clone, Debug)]
pub enum RebaseSyncOutcome {
    AlreadyUpToDate {
        snapshot: RepositorySnapshot,
    },
    Completed {
        rebased_commits: Vec<RebasedCommitSummary>,
        snapshot: RepositorySnapshot,
    },
    Conflict {
        upstream: String,
        conflicted_files: Vec<ConflictedFileSummary>,
        snapshot: RepositorySnapshot,
    },
    MissingUpstream {
        remotes: Vec<String>,
        snapshot: RepositorySnapshot,
    },
    UnsafeDirtyOverlap {
        upstream: String,
        overlapping_paths: Vec<String>,
        snapshot: RepositorySnapshot,
    },
    UnsafePublishedHistory {
        upstream: String,
        reason: String,
        snapshot: RepositorySnapshot,
    },
}

#[derive(Clone, Debug)]
pub struct ExtractedConflictFile {
    pub file_id: String,
    pub path: String,
    pub base_content: Option<String>,
    pub upstream_content: String,
    pub local_content: String,
    pub conflicted_content: String,
    pub working_hash: String,
}

#[derive(Clone, Debug)]
pub struct ExtractedConflictBatch {
    pub upstream: String,
    pub files: Vec<ExtractedConflictFile>,
}

#[derive(Clone, Debug)]
pub enum RebaseContinueOutcome {
    Completed {
        upstream: String,
        rebased_commits: Vec<RebasedCommitSummary>,
        snapshot: RepositorySnapshot,
    },
    NextConflict {
        upstream: String,
        conflicted_files: Vec<ConflictedFileSummary>,
        snapshot: RepositorySnapshot,
    },
}

#[derive(Clone, Debug)]
pub enum PushOutcome {
    Pushed {
        remote_branch: String,
        snapshot: RepositorySnapshot,
    },
    MissingUpstream {
        remotes: Vec<String>,
        snapshot: RepositorySnapshot,
    },
    Failed {
        remote_branch: String,
        non_fast_forward: bool,
        reason: String,
        snapshot: RepositorySnapshot,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DirtySyncBackup {
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

fn format_git_error(output: &GitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.bytes);
    let combined = match (stderr.trim(), stdout.trim()) {
        ("", "") => format!("Git exit code {:?}", output.code),
        (err, "") => err.to_owned(),
        ("", out) => out.to_owned(),
        (err, out) => format!("{err}; {out}"),
    };
    let clean = combined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = clean.chars();
    let head: String = chars.by_ref().take(280).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn line_has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        let stripped = line.strip_suffix('\r').unwrap_or(line);
        stripped.starts_with("<<<<<<< ")
            || stripped == "<<<<<<<"
            || stripped == "======="
            || stripped.starts_with(">>>>>>> ")
            || stripped == ">>>>>>>"
            || stripped.starts_with("||||||| ")
            || stripped == "|||||||"
    })
}

fn validate_remote_name(remote: &str) -> Result<()> {
    if remote.is_empty()
        || remote.starts_with('-')
        || remote.contains('/')
        || remote.contains("..")
        || remote
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("Invalid Git remote name".into());
    }
    Ok(())
}

impl RepositoryStore {
    pub(super) fn is_remote_verified(&self, repository_id: &str, remote: &str) -> bool {
        if remote.is_empty() {
            return false;
        }
        self.inner
            .verified_remotes
            .lock()
            .map(|set| set.contains(&(repository_id.to_owned(), remote.to_owned())))
            .unwrap_or(false)
    }

    pub(super) fn mark_remote_verified(&self, repository_id: &str, remote: &str) {
        if !remote.is_empty()
            && let Ok(mut set) = self.inner.verified_remotes.lock()
        {
            set.insert((repository_id.to_owned(), remote.to_owned()));
        }
    }

    pub(super) fn clear_remote_verified(&self, repository_id: &str, remote: &str) {
        if !remote.is_empty()
            && let Ok(mut set) = self.inner.verified_remotes.lock()
        {
            set.remove(&(repository_id.to_owned(), remote.to_owned()));
        }
    }

    pub(super) async fn active_rebase_branch(&self, root: &Path) -> Option<String> {
        let reader = &self.inner.reader;
        for marker in ["rebase-merge/head-name", "rebase-apply/head-name"] {
            let Ok(path) = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .await
            else {
                continue;
            };
            if let Ok(raw) = fs::read_to_string(path) {
                let trimmed = raw.trim();
                if let Some(branch) = trimmed.strip_prefix("refs/heads/")
                    && !branch.is_empty()
                {
                    return Some(branch.to_owned());
                }
            }
        }
        None
    }

    pub(super) async fn resolve_branch_upstream_and_divergence(
        &self,
        root: &Path,
        branch: &str,
    ) -> (Option<String>, Option<(u64, u64)>) {
        let Ok(Some(spec)) = self.branch_upstream_spec(root, branch).await else {
            return (None, None);
        };
        let div = self
            .compute_ahead_behind(root, "HEAD", &spec.tracking_ref)
            .await
            .ok();
        (Some(spec.display), div)
    }

    pub(super) async fn list_remotes_unlocked(&self, root: &Path) -> Result<Vec<String>> {
        let output = self.inner.reader.run(root, &["remote"]).await?;
        if !output.success {
            return Ok(Vec::new());
        }
        let text = parse::utf8(&output.bytes)?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }

    async fn branch_upstream_spec(
        &self,
        root: &Path,
        branch: &str,
    ) -> Result<Option<UpstreamSpec>> {
        if branch.is_empty() || branch == "(detached)" {
            return Ok(None);
        }
        let reader = &self.inner.reader;
        let remote_out = reader
            .run(
                root,
                &["config", "--get", &format!("branch.{branch}.remote")],
            )
            .await?;
        let merge_out = reader
            .run(
                root,
                &["config", "--get", &format!("branch.{branch}.merge")],
            )
            .await?;
        if !remote_out.success || !merge_out.success {
            return Ok(None);
        }
        let remote = parse::utf8(&remote_out.bytes)?.trim().to_owned();
        let merge_ref = parse::utf8(&merge_out.bytes)?.trim().to_owned();
        let Some(remote_branch) = merge_ref.strip_prefix("refs/heads/") else {
            return Ok(None);
        };
        if remote.is_empty() || remote == "." || remote_branch.is_empty() {
            return Ok(None);
        }
        Ok(Some(UpstreamSpec {
            display: format!("{remote}/{remote_branch}"),
            tracking_ref: format!("refs/remotes/{remote}/{remote_branch}"),
            remote,
            remote_branch: remote_branch.to_owned(),
        }))
    }

    async fn ref_oid(&self, root: &Path, rev: &str) -> Option<String> {
        let out = self
            .inner
            .reader
            .run(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{rev}^{{commit}}"),
                ],
            )
            .await
            .ok()?;
        if !out.success {
            return None;
        }
        let oid = String::from_utf8_lossy(&out.bytes).trim().to_owned();
        (!oid.is_empty()).then_some(oid)
    }

    async fn is_ancestor(&self, root: &Path, ancestor: &str, descendant: &str) -> bool {
        self.inner
            .reader
            .run(root, &["merge-base", "--is-ancestor", ancestor, descendant])
            .await
            .is_ok_and(|out| out.success)
    }

    async fn compute_ahead_behind(
        &self,
        root: &Path,
        local_rev: &str,
        upstream_ref: &str,
    ) -> Result<(u64, u64)> {
        let text = self
            .inner
            .reader
            .text(
                root,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{local_rev}...{upstream_ref}"),
                ],
            )
            .await?;
        let mut parts = text.split_whitespace();
        let ahead: u64 = parts
            .next()
            .ok_or("Missing ahead commit count")?
            .parse()
            .map_err(|_| "Invalid ahead commit count")?;
        let behind: u64 = parts
            .next()
            .ok_or("Missing behind commit count")?
            .parse()
            .map_err(|_| "Invalid behind commit count")?;
        Ok((ahead, behind))
    }

    async fn git_marker_exists(&self, root: &Path, marker: &str) -> Result<bool> {
        let path = self
            .inner
            .reader
            .text(
                root,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
            )
            .await?;
        Ok(fs::symlink_metadata(path).is_ok())
    }

    async fn rebase_in_progress(&self, root: &Path) -> Result<bool> {
        Ok(self.git_marker_exists(root, "rebase-merge").await?
            || self.git_marker_exists(root, "rebase-apply").await?)
    }

    async fn other_git_op_in_progress(&self, root: &Path) -> Result<Option<&'static str>> {
        for marker in [
            "index.lock",
            "HEAD.lock",
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "BISECT_LOG",
        ] {
            if self.git_marker_exists(root, marker).await? {
                return Ok(Some(marker));
            }
        }
        Ok(None)
    }

    async fn list_unmerged_files(&self, root: &Path) -> Result<Vec<ConflictedFileSummary>> {
        let bytes = self
            .inner
            .reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await?;
        let mut paths = BTreeSet::new();
        for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let Some(tab) = record.iter().position(|b| *b == b'\t') else {
                continue;
            };
            let path = parse::utf8(&record[tab + 1..])?;
            parse::relative(&path)?;
            paths.insert(path);
        }
        Ok(paths
            .into_iter()
            .map(|path| ConflictedFileSummary {
                file_id: format!("file-{}", hash(&path)),
                path,
            })
            .collect())
    }

    async fn list_commits_in_range(
        &self,
        root: &Path,
        branch: &str,
        range: &str,
        locale: LanguageKind,
    ) -> Result<Vec<RebasedCommitSummary>> {
        let out = self
            .inner
            .reader
            .with_locale(locale)
            .run(root, &["log", "--reverse", "--format=%H%x09%s", range])
            .await?;
        if !out.success {
            return Ok(Vec::new());
        }
        let text = parse::utf8_with_locale(&out.bytes, locale)?;
        let fallback_title = i18n::cpush_commit_completed_fallback(locale);
        let mut commits = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let (oid, title) = line.split_once('\t').unwrap_or((line, fallback_title));
            let oid = oid.trim().to_owned();
            if !oid.is_empty() {
                commits.push(RebasedCommitSummary {
                    short_oid: oid.chars().take(7).collect(),
                    oid,
                    title: title.trim().to_owned(),
                    branch: branch.to_owned(),
                });
            }
        }
        Ok(commits)
    }

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

    async fn create_dirty_backup(
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

    async fn restore_dirty_backup(&self, root: &Path) -> Result<bool> {
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

    async fn diff_touched_paths(
        &self,
        root: &Path,
        rev_a: &str,
        rev_b: &str,
    ) -> Result<BTreeSet<String>> {
        let bytes = self
            .inner
            .reader
            .bytes(
                root,
                &["diff", "--name-only", "-z", "--find-renames", rev_a, rev_b],
            )
            .await?;
        let mut set = BTreeSet::new();
        for entry in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            set.insert(parse::utf8(entry)?);
        }
        Ok(set)
    }

    pub async fn inspect_live_remote_status(
        &self,
        target: &ReadTarget,
        verify_remote_network: bool,
        locale: LanguageKind,
    ) -> Result<(LiveRemoteStatus, RepositorySnapshot)> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let reader = self.inner.reader.with_locale(locale);

        let rebase_in_progress = self.rebase_in_progress(&root).await?;
        let merge_or_other_in_progress = self.other_git_op_in_progress(&root).await?.is_some();
        let conflicted_files = self.list_unmerged_files(&root).await?;

        let status_bytes = reader
            .bytes(
                &root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let parsed = parse::status_with_locale(&status_bytes, locale)?;
        let raw_detached = parsed.branch == "(detached)";
        let rebase_branch = if raw_detached {
            self.active_rebase_branch(&root).await
        } else {
            None
        };
        let branch = rebase_branch.unwrap_or_else(|| parsed.branch.clone());
        let upstream = self.branch_upstream_spec(&root, &branch).await?;

        if verify_remote_network
            && !rebase_in_progress
            && !merge_or_other_in_progress
            && conflicted_files.is_empty()
            && let Some(spec) = &upstream
        {
            let fetch_out = reader
                .remote(
                    &root,
                    &["fetch", "--no-tags", "--quiet", "--", &spec.remote],
                )
                .await?;
            if fetch_out.success {
                self.mark_remote_verified(&target.repository_id, &spec.remote);
            } else {
                self.clear_remote_verified(&target.repository_id, &spec.remote);
            }
        }

        if !rebase_in_progress && !merge_or_other_in_progress && conflicted_files.is_empty() {
            let _ = self.restore_dirty_backup(&root).await;
        }

        let (ahead, unpushed_commits) = if let Some(spec) = &upstream
            && parsed.head.is_some()
            && self.ref_oid(&root, &spec.tracking_ref).await.is_some()
        {
            let (a, _) = self
                .compute_ahead_behind(&root, "HEAD", &spec.tracking_ref)
                .await
                .unwrap_or((0, 0));
            let commits = if a > 0 {
                self.list_commits_in_range(
                    &root,
                    &branch,
                    &format!("{}..HEAD", spec.tracking_ref),
                    locale,
                )
                .await?
            } else {
                Vec::new()
            };
            (a, commits)
        } else {
            (0, Vec::new())
        };

        let snapshot = self.read_repository_locked(&root, locale).await?;
        Ok((
            LiveRemoteStatus {
                upstream,
                ahead,
                rebase_in_progress,
                merge_or_other_in_progress,
                conflicted_files,
                unpushed_commits,
            },
            snapshot,
        ))
    }

    #[cfg(test)]
    pub async fn sync_rebase_onto_upstream(
        &self,
        target: &ReadTarget,
    ) -> Result<RebaseSyncOutcome> {
        self.sync_rebase_onto_upstream_with_locale(target, LanguageKind::En)
            .await
    }

    /// Shared safe pull-and-rebase engine used by explicit Pull & Rebase, C-Push, and Commit & Push.
    pub async fn sync_rebase_onto_upstream_with_locale(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<RebaseSyncOutcome> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let reader = self.inner.reader.with_locale(locale);

        if self.rebase_in_progress(&root).await? {
            let conflicted_files = self.list_unmerged_files(&root).await?;
            if !conflicted_files.is_empty() {
                let rebased_branch = self
                    .active_rebase_branch(&root)
                    .await
                    .unwrap_or_else(|| "HEAD".into());
                let upstream = self
                    .branch_upstream_spec(&root, &rebased_branch)
                    .await?
                    .map(|u| u.display)
                    .unwrap_or_else(|| i18n::remote_branch_fallback(locale).into());
                let snapshot = self.read_repository_locked(&root, locale).await?;
                return Ok(RebaseSyncOutcome::Conflict {
                    upstream,
                    conflicted_files,
                    snapshot,
                });
            }
            return Err(
                "An unfinished rebase is active in the current worktree; finish or abort it in Git / IDE first"
                    .into(),
            );
        }
        if let Some(marker) = self.other_git_op_in_progress(&root).await? {
            return Err(i18n::commit_ongoing_op_error(locale, marker));
        }
        if !self.list_unmerged_files(&root).await?.is_empty() {
            return Err(i18n::commit_unresolved_conflict_error(locale).into());
        }

        let worktree_reader = reader.for_worktree(&root).await?;
        let status_bytes = worktree_reader
            .bytes(
                &root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let parsed = parse::status_with_locale(&status_bytes, locale)?;
        if parsed.branch == "(detached)" {
            return Err(i18n::commit_detached_head_error(locale).into());
        }
        let Some(head) = parsed.head.clone() else {
            return Err("Current branch has no initial commit yet; cannot pull and rebase".into());
        };

        let remotes = self.list_remotes_unlocked(&root).await?;
        let Some(spec) = self.branch_upstream_spec(&root, &parsed.branch).await? else {
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(RebaseSyncOutcome::MissingUpstream { remotes, snapshot });
        };

        validate_remote_name(&spec.remote)?;
        let old_upstream_oid = self.ref_oid(&root, &spec.tracking_ref).await;
        let prev_shared_published = if let Some(old_oid) = &old_upstream_oid {
            reader
                .text(&root, &["merge-base", &head, old_oid])
                .await
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        let fetch_out = reader
            .remote(
                &root,
                &["fetch", "--no-tags", "--quiet", "--", &spec.remote],
            )
            .await?;
        if !fetch_out.success {
            self.clear_remote_verified(&target.repository_id, &spec.remote);
            return Err(i18n::remote_fetch_failed_error(
                locale,
                &spec.remote,
                &format_git_error(&fetch_out),
            ));
        }
        self.mark_remote_verified(&target.repository_id, &spec.remote);

        let Some(new_upstream_oid) = self.ref_oid(&root, &spec.tracking_ref).await else {
            return Err(format!(
                "Tracking branch {} does not exist on remote {}; verify the remote branch name",
                spec.remote_branch, spec.remote
            ));
        };

        let (ahead, behind) = self
            .compute_ahead_behind(&root, &head, &spec.tracking_ref)
            .await?;
        if behind == 0 {
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(RebaseSyncOutcome::AlreadyUpToDate { snapshot });
        }

        // Published history safety guard (`R05`):
        if let Some(shared_published) = &prev_shared_published
            && old_upstream_oid.as_ref() != Some(&new_upstream_oid)
            && !self
                .is_ancestor(&root, shared_published, &new_upstream_oid)
                .await
        {
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(RebaseSyncOutcome::UnsafePublishedHistory {
                upstream: spec.display.clone(),
                reason: i18n::remote_rewound_history_reason(locale, &spec.display),
                snapshot,
            });
        }

        if ahead > 0 {
            let merges = reader
                .text(
                    &root,
                    &[
                        "rev-list",
                        "--merges",
                        &format!("{}..{head}", spec.tracking_ref),
                    ],
                )
                .await?;
            if !merges.trim().is_empty() {
                let snapshot = self.read_repository_locked(&root, locale).await?;
                return Ok(RebaseSyncOutcome::UnsafePublishedHistory {
                    upstream: spec.display,
                    reason: i18n::remote_merge_commit_reason(locale).into(),
                    snapshot,
                });
            }

            let local_oids_raw = reader
                .text(
                    &root,
                    &["rev-list", &format!("{}..{head}", spec.tracking_ref)],
                )
                .await?;
            for oid in local_oids_raw
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let containing = reader
                    .text(
                        &root,
                        &[
                            "for-each-ref",
                            "--format=%(refname)",
                            "--contains",
                            oid,
                            "refs/remotes/",
                        ],
                    )
                    .await?;
                let published_on_other: Vec<&str> = containing
                    .lines()
                    .map(str::trim)
                    .filter(|r| !r.is_empty() && *r != spec.tracking_ref && !r.ends_with("/HEAD"))
                    .collect();
                if !published_on_other.is_empty() {
                    let snapshot = self.read_repository_locked(&root, locale).await?;
                    return Ok(RebaseSyncOutcome::UnsafePublishedHistory {
                        upstream: spec.display,
                        reason: i18n::remote_published_on_other_ref_reason(
                            locale,
                            &oid[..oid.len().min(7)],
                            published_on_other[0].trim_start_matches("refs/remotes/"),
                        ),
                        snapshot,
                    });
                }
            }
        }

        // Dirty-worktree safety check (`R06`):
        let merge_base = reader
            .text(&root, &["merge-base", &head, &spec.tracking_ref])
            .await
            .map(|s| s.trim().to_owned())
            .map_err(|_| {
                "Remote branch and current local branch share no common ancestor; paused automatic rebase"
                    .to_string()
            })?;

        let mut incoming_or_replayed = self
            .diff_touched_paths(&root, &merge_base, &spec.tracking_ref)
            .await?;
        if ahead > 0 {
            incoming_or_replayed.extend(self.diff_touched_paths(&root, &merge_base, &head).await?);
        }

        let dirty_paths: BTreeSet<String> = parsed
            .entries
            .iter()
            .flat_map(|e| std::iter::once(e.path.clone()).chain(e.previous.clone()))
            .collect();

        let overlapping: Vec<String> = dirty_paths
            .intersection(&incoming_or_replayed)
            .cloned()
            .collect();
        if !overlapping.is_empty() {
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(RebaseSyncOutcome::UnsafeDirtyOverlap {
                upstream: spec.display,
                overlapping_paths: overlapping,
                snapshot,
            });
        }

        // Disjoint dirty files can be backed up & restored with 100% staged/unstaged fidelity:
        let had_backup = self
            .create_dirty_backup(&root, &parsed.branch, &head, &parsed.entries)
            .await?
            .is_some();

        let rebase_out = reader.write(&root, &["rebase", &spec.tracking_ref]).await?;

        if rebase_out.success {
            if had_backup {
                self.restore_dirty_backup(&root).await?;
            }
            let (new_ahead, _) = self
                .compute_ahead_behind(&root, "HEAD", &spec.tracking_ref)
                .await
                .unwrap_or((0, 0));
            let rebased_commits = if new_ahead > 0 {
                self.list_commits_in_range(
                    &root,
                    &parsed.branch,
                    &format!("{}..HEAD", spec.tracking_ref),
                    locale,
                )
                .await?
            } else {
                Vec::new()
            };
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(RebaseSyncOutcome::Completed {
                rebased_commits,
                snapshot,
            });
        }

        // Check if rebase paused on a merge conflict:
        if self.rebase_in_progress(&root).await? {
            let conflicted_files = self.list_unmerged_files(&root).await?;
            if !conflicted_files.is_empty() {
                let snapshot = self.read_repository_locked(&root, locale).await?;
                return Ok(RebaseSyncOutcome::Conflict {
                    upstream: spec.display,
                    conflicted_files,
                    snapshot,
                });
            }
            let _ = reader.write(&root, &["rebase", "--abort"]).await;
        }
        if had_backup {
            let _ = self.restore_dirty_backup(&root).await;
        }
        Err(format!(
            "Pull and rebase did not complete ({}); original staged changes and drafts were preserved",
            format_git_error(&rebase_out)
        ))
    }

    pub async fn extract_conflict_batch(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<ExtractedConflictBatch> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let reader = self.inner.reader.with_locale(locale);

        if !self.rebase_in_progress(&root).await? {
            return Err(
                "No active rebase conflict available for Agent merge; refresh to inspect latest status"
                    .into(),
            );
        }
        let unmerged = self.list_unmerged_files(&root).await?;
        if unmerged.is_empty() {
            return Err(
                "No unresolved conflicted files detected; refresh to verify rebase status".into(),
            );
        }

        let branch = self
            .active_rebase_branch(&root)
            .await
            .unwrap_or_else(|| "HEAD".into());
        let upstream = self
            .branch_upstream_spec(&root, &branch)
            .await?
            .map(|u| u.display)
            .unwrap_or_else(|| i18n::remote_branch_fallback(locale).into());

        let mut files = Vec::with_capacity(unmerged.len());
        for summary in unmerged {
            let full = root.join(&summary.path);
            let meta = fs::symlink_metadata(&full).map_err(|_| {
                format!(
                    "Conflicted file {} is missing in the working tree (possibly a modify/delete conflict) and cannot be auto-merged safely; choose \"Merge manually\" in your editor.",
                    summary.path
                )
            })?;
            if !meta.is_file() {
                return Err(format!(
                    "Conflicted file {} is not a regular text file and cannot be auto-merged safely; choose \"Merge manually\".",
                    summary.path
                ));
            }
            if meta.len() as usize > MAX_CONFLICT_TEXT_BYTES {
                return Err(format!(
                    "Conflicted file {} exceeds the automatic merge size limit (128 KB); choose \"Merge manually\" in your editor.",
                    summary.path
                ));
            }
            let working_bytes = fs::read(&full)
                .map_err(|_| format!("Cannot read conflicted file {}", summary.path))?;
            if working_bytes.contains(&0) {
                return Err(format!(
                    "Conflicted file {} contains binary data and cannot be auto-merged; choose \"Merge manually\".",
                    summary.path
                ));
            }
            let conflicted_content = String::from_utf8(working_bytes).map_err(|_| {
                format!(
                    "Conflicted file {} is not UTF-8 text and cannot be auto-merged safely; choose \"Merge manually\".",
                    summary.path
                )
            })?;

            let path_str = summary.path.as_str();
            let read_stage_utf8 = |stage: u8| {
                let spec = format!(":{stage}:{path_str}");
                let root_ref = &root;
                let reader_ref = &reader;
                async move {
                    let out = reader_ref
                        .run(root_ref, &["cat-file", "blob", &spec])
                        .await?;
                    if !out.success {
                        return Ok::<Option<String>, String>(None);
                    }
                    if out.bytes.contains(&0) {
                        return Err(format!(
                            "Conflict stage for {path_str} contains binary data; choose \"Merge manually\"."
                        ));
                    }
                    let text = String::from_utf8(out.bytes).map_err(|_| {
                        format!(
                            "Conflict stage for {path_str} is not UTF-8 text; choose \"Merge manually\"."
                        )
                    })?;
                    Ok(Some(text))
                }
            };

            let base_content = read_stage_utf8(1).await?;
            let Some(upstream_content) = read_stage_utf8(2).await? else {
                return Err(format!(
                    "Conflicted file {} is an add/delete conflict and cannot be auto-merged as text; choose \"Merge manually\".",
                    summary.path
                ));
            };
            let Some(local_content) = read_stage_utf8(3).await? else {
                return Err(format!(
                    "Conflicted file {} is an add/delete conflict and cannot be auto-merged as text; choose \"Merge manually\".",
                    summary.path
                ));
            };
            let working_hash = self.working_file(&root, &summary.path)?;
            files.push(ExtractedConflictFile {
                file_id: summary.file_id,
                path: summary.path,
                base_content,
                upstream_content,
                local_content,
                conflicted_content,
                working_hash,
            });
        }

        Ok(ExtractedConflictBatch { upstream, files })
    }

    pub async fn apply_conflict_resolution_and_continue(
        &self,
        target: &ReadTarget,
        expected: &[ExtractedConflictFile],
        resolved: &[PiResolvedConflictFile],
        locale: LanguageKind,
    ) -> Result<RebaseContinueOutcome> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let reader = self.inner.reader.with_locale(locale);

        if !self.rebase_in_progress(&root).await? {
            return Err(
                "Rebase status changed before applying model merge output; refresh to verify live Git status"
                    .into(),
            );
        }
        let live_unmerged = self.list_unmerged_files(&root).await?;
        let expected_paths: BTreeSet<&str> = expected.iter().map(|f| f.path.as_str()).collect();
        let live_paths: BTreeSet<&str> = live_unmerged.iter().map(|f| f.path.as_str()).collect();
        if expected_paths != live_paths || resolved.len() != expected.len() {
            return Err(
                "Set of conflicted files changed; aborted automatic write to protect the working tree"
                    .into(),
            );
        }

        let by_path: HashMap<&str, &ExtractedConflictFile> =
            expected.iter().map(|f| (f.path.as_str(), f)).collect();
        let mut seen = BTreeSet::new();
        for item in resolved {
            parse::relative(&item.path)?;
            let Some(orig) = by_path.get(item.path.as_str()) else {
                return Err(i18n::remote_conflict_unauthorized_file_error(
                    locale, &item.path,
                ));
            };
            if orig.file_id != item.file_id || !seen.insert(item.path.as_str()) {
                return Err("Model returned mismatched conflict file identity".into());
            }
            let current_hash = self.working_file(&root, &item.path)?;
            if current_hash != orig.working_hash {
                return Err(format!(
                    "Conflicted file {} was modified externally while waiting; preserved your latest content",
                    item.path
                ));
            }
            let base_had_markers = orig
                .base_content
                .as_deref()
                .is_some_and(line_has_conflict_markers);
            if !base_had_markers && line_has_conflict_markers(&item.content) {
                return Err(i18n::remote_conflict_leftover_markers_error(
                    locale, &item.path,
                ));
            }
            if item.content.trim().is_empty()
                && !orig.upstream_content.trim().is_empty()
                && !orig.local_content.trim().is_empty()
            {
                return Err(i18n::remote_conflict_empty_result_error(locale, &item.path));
            }
        }

        // Write ONLY the authorized conflicted files & stage ONLY those paths:
        for item in resolved {
            fs::write(root.join(&item.path), item.content.as_bytes()).map_err(|_| {
                format!("Failed to write merged conflict content for {}", item.path)
            })?;
        }
        let mut add_args: Vec<&str> = vec!["add", "--"];
        for item in resolved {
            add_args.push(&item.path);
        }
        let add_out = reader.write(&root, &add_args).await?;
        if !add_out.success {
            return Err(format!(
                "Failed to stage merged conflict files ({})",
                format_git_error(&add_out)
            ));
        }
        if !self.list_unmerged_files(&root).await?.is_empty() {
            return Err(
                "Unmerged Git index entries remain after staging; paused rebase continuation"
                    .into(),
            );
        }

        let branch = self
            .active_rebase_branch(&root)
            .await
            .unwrap_or_else(|| "HEAD".into());
        let spec_opt = self.branch_upstream_spec(&root, &branch).await?;
        let upstream_display = spec_opt
            .as_ref()
            .map(|u| u.display.clone())
            .unwrap_or_else(|| i18n::remote_branch_fallback(locale).into());

        let cont_out = reader.write(&root, &["rebase", "--continue"]).await?;
        if cont_out.success && !self.rebase_in_progress(&root).await? {
            let _ = self.restore_dirty_backup(&root).await?;
            let rebased_commits = if let Some(spec) = &spec_opt {
                let (a, _) = self
                    .compute_ahead_behind(&root, "HEAD", &spec.tracking_ref)
                    .await
                    .unwrap_or((0, 0));
                if a > 0 {
                    self.list_commits_in_range(
                        &root,
                        &branch,
                        &format!("{}..HEAD", spec.tracking_ref),
                        locale,
                    )
                    .await?
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(RebaseContinueOutcome::Completed {
                upstream: upstream_display,
                rebased_commits,
                snapshot,
            });
        }

        if self.rebase_in_progress(&root).await? {
            let next_conflicts = self.list_unmerged_files(&root).await?;
            if !next_conflicts.is_empty() {
                let snapshot = self.read_repository_locked(&root, locale).await?;
                return Ok(RebaseContinueOutcome::NextConflict {
                    upstream: upstream_display,
                    conflicted_files: next_conflicts,
                    snapshot,
                });
            }
        }

        Err(format!(
            "Continuing rebase failed validation ({}); merged file edits were kept for inspection",
            format_git_error(&cont_out)
        ))
    }

    pub async fn abort_active_rebase(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let reader = self.inner.reader.with_locale(locale);
        if self.rebase_in_progress(&root).await? {
            let out = reader.write(&root, &["rebase", "--abort"]).await?;
            if !out.success {
                return Err(format!(
                    "Failed to abort rebase ({})",
                    format_git_error(&out)
                ));
            }
        }
        let _ = self.restore_dirty_backup(&root).await?;
        self.read_repository_locked(&root, locale).await
    }

    /// Fast-forward-only branch push (`R02`, `R05`). Never uses `--force` or `--force-with-lease`.
    pub async fn push_branch(
        &self,
        target: &ReadTarget,
        set_upstream_remote: Option<&str>,
        locale: LanguageKind,
    ) -> Result<PushOutcome> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let reader = self.inner.reader.with_locale(locale);

        if self.rebase_in_progress(&root).await? {
            return Err(
                "Current branch still has an unfinished rebase; finish resolving conflicts before pushing"
                    .into(),
            );
        }
        if let Some(marker) = self.other_git_op_in_progress(&root).await? {
            return Err(format!(
                "Unfinished Git operation ({marker}) in progress; paused push"
            ));
        }
        if !self.list_unmerged_files(&root).await?.is_empty() {
            return Err(
                "Working tree has unresolved conflicted files; resolve conflicts before pushing"
                    .into(),
            );
        }

        let status_bytes = reader
            .bytes(
                &root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let parsed = parse::status_with_locale(&status_bytes, locale)?;
        if parsed.branch == "(detached)" {
            return Err(
                "Currently on detached HEAD and cannot push directly; switch to a local branch first"
                    .into(),
            );
        }
        if parsed.head.is_none() {
            return Err("Current branch has no commits to push yet".into());
        }

        let remotes = self.list_remotes_unlocked(&root).await?;
        let (remote_name, remote_branch, set_u) = if let Some(chosen_remote) = set_upstream_remote {
            validate_remote_name(chosen_remote)?;
            if !remotes.iter().any(|r| r == chosen_remote) {
                return Err(format!(
                    "Remote \"{chosen_remote}\" does not exist in this repository"
                ));
            }
            (chosen_remote.to_owned(), parsed.branch.clone(), true)
        } else if let Some(spec) = self.branch_upstream_spec(&root, &parsed.branch).await? {
            validate_remote_name(&spec.remote)?;
            (spec.remote, spec.remote_branch, false)
        } else {
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(PushOutcome::MissingUpstream { remotes, snapshot });
        };

        let refspec = format!("refs/heads/{}:refs/heads/{}", parsed.branch, remote_branch);
        let push_out = if set_u {
            reader
                .remote(
                    &root,
                    &[
                        "push",
                        "--set-upstream",
                        "--porcelain",
                        "--",
                        &remote_name,
                        &refspec,
                    ],
                )
                .await?
        } else {
            reader
                .remote(
                    &root,
                    &["push", "--porcelain", "--", &remote_name, &refspec],
                )
                .await?
        };

        let remote_display = format!("{remote_name}/{remote_branch}");
        if push_out.success {
            self.mark_remote_verified(&target.repository_id, &remote_name);
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok(PushOutcome::Pushed {
                remote_branch: remote_display,
                snapshot,
            });
        }

        let raw_err = format_git_error(&push_out);
        let full_lower = format!(
            "{}\n{}",
            String::from_utf8_lossy(&push_out.stderr),
            String::from_utf8_lossy(&push_out.bytes)
        )
        .to_ascii_lowercase();
        let non_fast_forward = full_lower.contains("non-fast-forward")
            || full_lower.contains("fetch first")
            || full_lower.contains("remote contains work that you do not")
            || full_lower.contains("[rejected]") && full_lower.contains("behind");
        if !non_fast_forward {
            self.clear_remote_verified(&target.repository_id, &remote_name);
        } else {
            self.mark_remote_verified(&target.repository_id, &remote_name);
        }
        let snapshot = self.read_repository_locked(&root, locale).await?;
        Ok(PushOutcome::Failed {
            remote_branch: remote_display,
            non_fast_forward,
            reason: raw_err,
            snapshot,
        })
    }
}
