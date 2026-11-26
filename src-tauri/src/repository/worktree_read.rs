use super::{
    RepositoryStore, Result, commit, display_with_locale, file_id_for_path, hash, identity,
    ignored_for_review, label,
    parse::{self, WorktreeEntry},
    staged,
    types::{Change, Range, Remote, WorktreeSnapshot},
};
use crate::i18n::{self, LanguageKind};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

impl RepositoryStore {
    pub(super) async fn read_worktree(
        &self,
        repository_id: &str,
        entry: &WorktreeEntry,
        keep_set: &BTreeSet<String>,
        locale: LanguageKind,
    ) -> Result<(WorktreeSnapshot, commit::CommitRegistration)> {
        let root = &entry.path;
        let reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(root)
            .await?;
        let before = reader
            .bytes(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let status = parse::status_with_locale(&before, locale)?;
        if status.entries.len() > 10_000 {
            return Err(i18n::repository::repo_too_many_changes_error(locale).into());
        }
        if status.entries.iter().any(|entry| entry.mode == "160000") {
            return Err(i18n::repository::repo_submodule_read_error(locale).into());
        }
        let mut fingerprints = HashMap::new();
        for file in &status.entries {
            if !staged(file) {
                fingerprints.insert(file.path.clone(), self.working_file(root, &file.path)?);
            }
        }
        let staged_stats = parse::numstat(
            &reader
                .bytes(
                    root,
                    &[
                        "diff",
                        "--cached",
                        "--numstat",
                        "-z",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--find-renames",
                    ],
                )
                .await?,
        )?;
        let unstaged_stats = parse::numstat(
            &reader
                .bytes(
                    root,
                    &[
                        "diff",
                        "--numstat",
                        "-z",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--find-renames",
                    ],
                )
                .await?,
        )?;
        let mut files = Vec::new();
        let mut working_hashes = HashMap::new();
        for file in &status.entries {
            let index = staged(file);
            let stats = if file.xy == "??" {
                let output = reader
                    .run(
                        root,
                        &[
                            "diff",
                            "--no-index",
                            "--numstat",
                            "-z",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--",
                            "/dev/null",
                            &file.path,
                        ],
                    )
                    .await?;
                if !matches!(output.code, Some(0 | 1)) {
                    return Err("Cannot read Git line statistics for untracked file".into());
                }
                parse::untracked_numstat(&output.bytes, &file.path)?
            } else {
                if index {
                    &staged_stats
                } else {
                    &unstaged_stats
                }
                .get(&file.path)
                .cloned()
                .unwrap_or_default()
            };
            let working = if !index {
                let current = self.working_file(root, &file.path)?;
                if fingerprints
                    .get(&file.path)
                    .is_some_and(|previous| previous != &current)
                {
                    return Err(i18n::repository::repo_changed_during_read_error(locale).into());
                }
                working_hashes.insert(file.path.clone(), current.clone());
                Some(current)
            } else {
                None
            };
            let conflict = file.conflicted;
            let kind = if index {
                if file.xy.as_bytes().get(1).is_some_and(|byte| *byte != b'.') {
                    "partial"
                } else {
                    "staged"
                }
            } else {
                "unstaged"
            };
            let is_local_keep = keep_set.contains(&file.path);
            let version = format!(
                "{}\0{}\0{}\0{}\0{}\0{}",
                file.path,
                file.previous.as_deref().unwrap_or(""),
                if index { "index" } else { "worktree" },
                if index {
                    &file.index_oid
                } else {
                    working.as_deref().unwrap_or("")
                },
                file.index_oid,
                if is_local_keep {
                    "local-keep"
                } else {
                    "normal"
                },
            );
            let effective = if file.xy == "??" {
                b'A'
            } else {
                file.xy.as_bytes()[usize::from(!index)]
            };
            let status_code = if conflict {
                "U"
            } else {
                match effective {
                    b'A' => "A",
                    b'D' => "D",
                    b'R' => "R",
                    b'C' => "C",
                    b'T' => "T",
                    _ => "M",
                }
            };
            files.push(Change {
                id: file_id_for_path(&file.path),
                path: file.path.clone(),
                previous_path: file.previous.clone(),
                status: status_code.into(),
                git_status: file.xy.clone(),
                range: Range {
                    kind: kind.into(),
                    added: stats.added,
                    removed: stats.removed,
                },
                binary: stats.binary,
                ignored_for_review: ignored_for_review(&file.path),
                local_keep: is_local_keep,
                content_version: hash(version),
                commit_token: String::new(),
            });
        }
        let after = reader
            .bytes(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        if before != after {
            return Err(i18n::repository::repo_changed_during_read_error(locale).into());
        }
        let raw_detached = status.branch == "(detached)";
        let rebase_branch = if raw_detached {
            self.active_rebase_branch(root).await
        } else {
            None
        };
        let display_branch = rebase_branch
            .clone()
            .unwrap_or_else(|| status.branch.clone());
        let detached = raw_detached && rebase_branch.is_none();
        let (effective_upstream, effective_divergence) =
            if status.upstream.is_none() && rebase_branch.is_some() {
                self.resolve_branch_upstream_and_divergence(root, &display_branch)
                    .await
            } else {
                (status.upstream, status.divergence)
            };
        let remote = match (effective_upstream, effective_divergence) {
            (Some(branch), Some((ahead, behind))) => {
                let remote_name = branch.split('/').next().unwrap_or("");
                let verified = self.is_remote_verified(repository_id, remote_name);
                Remote::Tracked {
                    branch,
                    ahead,
                    behind,
                    local_only: !verified,
                }
            }
            (Some(_), None) => Remote::Error {
                reason: i18n::repository::remote_missing_tracking_ref_error(locale).into(),
            },
            (None, _) => {
                let reason = if detached {
                    i18n::repository::remote_detached_head_reason(locale).to_owned()
                } else {
                    let remotes = self.list_remotes_unlocked(root).await.unwrap_or_default();
                    if remotes.is_empty() {
                        i18n::repository::remote_no_remotes_configured_reason(locale).to_owned()
                    } else {
                        i18n::repository::remote_no_upstream_configured_reason(locale).to_owned()
                    }
                };
                Remote::Missing { reason }
            }
        };
        let branch_state = self.branch_state(root, locale).await?;
        if branch_state.status != before {
            return Err(i18n::repository::repo_changed_during_read_error(locale).into());
        }
        let worktree_id = identity("wt", root);
        let commit_state = commit::CommitRegistration::build(
            repository_id,
            &worktree_id,
            &status.branch,
            raw_detached,
            status.head.as_deref(),
            &status.entries,
            &mut files,
            &working_hashes,
            &branch_state.index,
        )?;
        let version = serde_json::to_vec(&(status.head.as_deref(), &display_branch, &files))
            .map_err(|error| error.to_string())?;
        Ok((
            WorktreeSnapshot {
                repository_id: repository_id.into(),
                worktree_id,
                label: label(root, locale),
                path: display_with_locale(root, locale)?,
                branch: display_branch,
                detached,
                head: status.head,
                status_version: hash(version),
                branch_token: Some(branch_state.token()?),
                read_error: None,
                remote,
                files,
            },
            commit_state,
        ))
    }

    pub(super) fn working_file(&self, root: &Path, path: &str) -> Result<String> {
        parse::relative(path)?;
        let path = root.join(path);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(hash(b"missing\0"));
            }
            Err(_) => {
                return Err(
                    "Cannot read changed file; current draft and selection were preserved".into(),
                );
            }
        };
        let parent = fs::canonicalize(
            path.parent()
                .ok_or("File path is missing parent directory")?,
        )
        .map_err(|_| "Cannot access parent directory of changed file")?;
        if !parent.starts_with(root) {
            return Err(
                "File parent directory points outside the worktree; stopped reading".into(),
            );
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).map_err(|_| "Cannot read symbolic link target")?;
            let mut payload = b"symlink\0".to_vec();
            payload.extend_from_slice(target.as_os_str().as_encoded_bytes());
            return Ok(hash(payload));
        }
        if !metadata.is_file() {
            return Err("Changes contain unsupported special files; stopped reading".into());
        }
        let mut file = File::open(&path).map_err(|_| "Cannot open changed file")?;
        let opened = file.metadata().map_err(|_| "Cannot stat changed file")?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() || !opened.is_file() {
            return Err("File was replaced while opening; please refresh".into());
        }
        let mut digest = Sha256::new();
        digest.update(if opened.permissions().mode() & 0o111 != 0 {
            b"file:755\0"
        } else {
            b"file:644\0"
        });
        let mut buffer = [0; 64 * 1024];
        loop {
            if self.inner.reader.cancelled() {
                return Err(i18n::repository::repo_shutting_down_error(LanguageKind::En).into());
            }
            let count = file
                .read(&mut buffer)
                .map_err(|_| "Failed to read changed file")?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }
}
