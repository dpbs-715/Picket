use super::{
    RepositoryStore, Result, commit, display_with_locale, file_id_for_path, hash, identity,
    ignored_for_review, label,
    parse::{self, DiffAttr, FileAttrs, Stats, WorktreeEntry},
    staged,
    types::{Change, Range, Remote, WorktreeSnapshot},
};
use crate::i18n::{self, LanguageKind};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

const GIT_BINARY_SNIFF_BYTES: usize = 8_000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkingMeta {
    Missing,
    Symlink(Vec<u8>),
    File {
        dev: u64,
        ino: u64,
        size: u64,
        mtime: i64,
        mtime_nsec: i64,
        ctime: i64,
        ctime_nsec: i64,
        exec: bool,
    },
}

fn meta_from_file_metadata(metadata: &fs::Metadata) -> WorkingMeta {
    WorkingMeta::File {
        dev: metadata.dev(),
        ino: metadata.ino(),
        size: metadata.len(),
        mtime: metadata.mtime(),
        mtime_nsec: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_nsec: metadata.ctime_nsec(),
        exec: metadata.permissions().mode() & 0o111 != 0,
    }
}

fn ensure_parent_inside_root(root: &Path, full_path: &Path) -> Result<()> {
    let parent = fs::canonicalize(
        full_path
            .parent()
            .ok_or("File path is missing parent directory")?,
    )
    .map_err(|_| "Cannot access parent directory of changed file")?;
    if !parent.starts_with(root) {
        return Err("File parent directory points outside the worktree; stopped reading".into());
    }
    Ok(())
}

fn collect_working_meta(root: &Path, rel_path: &str) -> Result<WorkingMeta> {
    parse::relative(rel_path)?;
    let full_path = root.join(rel_path);
    let metadata = match fs::symlink_metadata(&full_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorkingMeta::Missing);
        }
        Err(_) => {
            return Err(
                "Cannot read changed file; current draft and selection were preserved".into(),
            );
        }
    };
    ensure_parent_inside_root(root, &full_path)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&full_path).map_err(|_| "Cannot read symbolic link target")?;
        return Ok(WorkingMeta::Symlink(
            target.as_os_str().as_encoded_bytes().to_vec(),
        ));
    }
    if !metadata.is_file() {
        return Err("Changes contain unsupported special files; stopped reading".into());
    }
    Ok(meta_from_file_metadata(&metadata))
}

fn hash_and_measure_working_file(
    root: &Path,
    rel_path: &str,
    expected_meta: Option<&WorkingMeta>,
    untracked_diff_attr: Option<DiffAttr>,
    cancelled: &impl Fn() -> bool,
    locale: LanguageKind,
) -> Result<(String, Option<Stats>)> {
    parse::relative(rel_path)?;
    let full_path = root.join(rel_path);
    let metadata = match fs::symlink_metadata(&full_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(expected) = expected_meta
                && *expected != WorkingMeta::Missing
            {
                return Err(i18n::repository::repo_changed_during_read_error(locale).into());
            }
            if untracked_diff_attr.is_some() {
                return Err(i18n::repository::repo_changed_during_read_error(locale).into());
            }
            return Ok((hash(b"missing\0"), None));
        }
        Err(_) => {
            return Err(
                "Cannot read changed file; current draft and selection were preserved".into(),
            );
        }
    };
    ensure_parent_inside_root(root, &full_path)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&full_path).map_err(|_| "Cannot read symbolic link target")?;
        let target_bytes = target.as_os_str().as_encoded_bytes();
        if let Some(expected) = expected_meta {
            match expected {
                WorkingMeta::Symlink(prev) if prev.as_slice() == target_bytes => {}
                _ => return Err(i18n::repository::repo_changed_during_read_error(locale).into()),
            }
        }
        let mut payload = b"symlink\0".to_vec();
        payload.extend_from_slice(target_bytes);
        let stats = untracked_diff_attr.map(|diff_attr| {
            if diff_attr == DiffAttr::ForceBinary {
                Stats {
                    added: 0,
                    removed: 0,
                    binary: true,
                }
            } else {
                Stats {
                    added: 1,
                    removed: 0,
                    binary: false,
                }
            }
        });
        return Ok((hash(payload), stats));
    }
    if !metadata.is_file() {
        return Err("Changes contain unsupported special files; stopped reading".into());
    }
    let mut file = File::open(&full_path).map_err(|_| "Cannot open changed file")?;
    let opened = file.metadata().map_err(|_| "Cannot stat changed file")?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() || !opened.is_file() {
        return Err("File was replaced while opening; please refresh".into());
    }
    let opened_meta = meta_from_file_metadata(&opened);
    if let Some(expected) = expected_meta
        && *expected != opened_meta
    {
        return Err(i18n::repository::repo_changed_during_read_error(locale).into());
    }

    let mut digest = Sha256::new();
    digest.update(if opened.permissions().mode() & 0o111 != 0 {
        b"file:755\0"
    } else {
        b"file:644\0"
    });
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes: u64 = 0;
    let mut newlines: u64 = 0;
    let mut last_byte: u8 = b'\n';
    let mut detected_binary = false;
    let mut sniffed_bytes: usize = 0;

    loop {
        if cancelled() {
            return Err(i18n::repository::repo_shutting_down_error(locale).into());
        }
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Failed to read changed file")?;
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        digest.update(chunk);
        if let Some(diff_attr) = untracked_diff_attr {
            if diff_attr == DiffAttr::Auto
                && !detected_binary
                && sniffed_bytes < GIT_BINARY_SNIFF_BYTES
            {
                let remaining = GIT_BINARY_SNIFF_BYTES - sniffed_bytes;
                let window = &chunk[..count.min(remaining)];
                if window.contains(&0) {
                    detected_binary = true;
                }
                sniffed_bytes += window.len();
            }
            if diff_attr != DiffAttr::ForceBinary && !detected_binary {
                newlines += chunk.iter().filter(|&&b| b == b'\n').count() as u64;
                last_byte = chunk[count - 1];
            }
            total_bytes += count as u64;
        }
    }

    if expected_meta.is_some() {
        let after_read = file.metadata().map_err(|_| "Cannot stat changed file")?;
        if meta_from_file_metadata(&after_read) != opened_meta {
            return Err(i18n::repository::repo_changed_during_read_error(locale).into());
        }
    }

    let stats = untracked_diff_attr.map(|diff_attr| match diff_attr {
        DiffAttr::ForceBinary => Stats {
            added: 0,
            removed: 0,
            binary: true,
        },
        DiffAttr::Auto if detected_binary => Stats {
            added: 0,
            removed: 0,
            binary: true,
        },
        DiffAttr::Auto | DiffAttr::ForceText => Stats {
            added: if total_bytes == 0 {
                0
            } else {
                newlines + u64::from(last_byte != b'\n')
            },
            removed: 0,
            binary: false,
        },
    });
    Ok((format!("{:x}", digest.finalize()), stats))
}

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

        let unstaged_paths: Vec<(String, bool)> = status
            .entries
            .iter()
            .filter(|file| !staged(file))
            .map(|file| (file.path.clone(), file.xy == "??"))
            .collect();
        let untracked_paths: Vec<String> = unstaged_paths
            .iter()
            .filter(|(_, is_untracked)| *is_untracked)
            .map(|(path, _)| path.clone())
            .collect();

        // Pass 1 (off the Tokio worker thread): record lightweight metadata without reading bytes.
        let initial_metas = if unstaged_paths.is_empty() {
            HashMap::new()
        } else {
            let root_owned = root.to_path_buf();
            let paths_for_meta: Vec<String> =
                unstaged_paths.iter().map(|(p, _)| p.clone()).collect();
            tokio::task::spawn_blocking(move || -> Result<HashMap<String, WorkingMeta>> {
                let mut metas = HashMap::with_capacity(paths_for_meta.len());
                for path in paths_for_meta {
                    let meta = collect_working_meta(&root_owned, &path)?;
                    metas.insert(path, meta);
                }
                Ok(metas)
            })
            .await
            .map_err(|err| format!("Worktree metadata task failed: {err}"))??
        };

        // Run staged diff numstat, unstaged diff numstat, and batched check-attr concurrently.
        let staged_fut = reader.bytes(
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
        );
        let unstaged_fut = reader.bytes(
            root,
            &[
                "diff",
                "--numstat",
                "-z",
                "--no-ext-diff",
                "--no-textconv",
                "--find-renames",
            ],
        );
        let check_attr_fut = async {
            if untracked_paths.is_empty() {
                return Ok::<BTreeMap<String, FileAttrs>, String>(BTreeMap::new());
            }
            let mut stdin = Vec::new();
            for path in &untracked_paths {
                stdin.extend_from_slice(path.as_bytes());
                stdin.push(0);
            }
            let output = reader
                .run_with_input(
                    root,
                    &["check-attr", "-z", "--stdin", "diff", "filter"],
                    &stdin,
                )
                .await?;
            if !output.success {
                return Err("Cannot inspect Git attributes for untracked files".into());
            }
            parse::check_attrs(&output.bytes)
        };

        let (staged_bytes, unstaged_bytes, untracked_attrs) =
            tokio::try_join!(staged_fut, unstaged_fut, check_attr_fut)?;
        let staged_stats = parse::numstat(&staged_bytes)?;
        let unstaged_stats = parse::numstat(&unstaged_bytes)?;

        // Only untracked files bound to an active blocked filter driver fall back to `git diff --no-index`
        // so Git enforces exact external-filter refusal semantics without penalizing normal files.
        let mut filtered_untracked_stats = HashMap::<String, Stats>::new();
        if reader.has_blocked_filters() {
            for path in &untracked_paths {
                if untracked_attrs
                    .get(path)
                    .is_some_and(|attrs| attrs.has_filter)
                {
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
                                path,
                            ],
                        )
                        .await?;
                    if !matches!(output.code, Some(0 | 1)) {
                        return Err("Cannot read Git line statistics for untracked file".into());
                    }
                    filtered_untracked_stats
                        .insert(path.clone(), parse::untracked_numstat(&output.bytes, path)?);
                }
            }
        }

        // Pass 2 (off the Tokio worker thread): single-pass SHA-256 + untracked stats with TOCTOU checks.
        let (working_hashes, mut untracked_computed_stats) = if unstaged_paths.is_empty() {
            (HashMap::new(), HashMap::new())
        } else {
            let root_owned = root.to_path_buf();
            let reader_for_cancel = reader.clone();
            let items: Vec<(String, Option<DiffAttr>)> = unstaged_paths
                .into_iter()
                .map(|(path, is_untracked)| {
                    let attr = if is_untracked && !filtered_untracked_stats.contains_key(&path) {
                        Some(
                            untracked_attrs
                                .get(&path)
                                .map(|a| a.diff)
                                .unwrap_or(DiffAttr::Auto),
                        )
                    } else {
                        None
                    };
                    (path, attr)
                })
                .collect();
            tokio::task::spawn_blocking(
                move || -> Result<(HashMap<String, String>, HashMap<String, Stats>)> {
                    let mut hashes = HashMap::with_capacity(items.len());
                    let mut stats_map = HashMap::new();
                    let is_cancelled = || reader_for_cancel.cancelled();
                    for (path, diff_attr) in items {
                        let expected = initial_metas.get(&path);
                        let (digest, stats) = hash_and_measure_working_file(
                            &root_owned,
                            &path,
                            expected,
                            diff_attr,
                            &is_cancelled,
                            locale,
                        )?;
                        hashes.insert(path.clone(), digest);
                        if let Some(st) = stats {
                            stats_map.insert(path, st);
                        }
                    }
                    Ok((hashes, stats_map))
                },
            )
            .await
            .map_err(|err| format!("Worktree file hashing task failed: {err}"))??
        };
        for (path, stats) in filtered_untracked_stats {
            untracked_computed_stats.insert(path, stats);
        }

        let mut files = Vec::with_capacity(status.entries.len());
        for file in &status.entries {
            let index = staged(file);
            let stats = if file.xy == "??" {
                untracked_computed_stats
                    .get(&file.path)
                    .cloned()
                    .unwrap_or_default()
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
                working_hashes.get(&file.path)
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
                    working.map(String::as_str).unwrap_or("")
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
        let branch_state = self
            .branch_state(root, locale)
            .await
            .map_err(|e| e.to_string())?;
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
        let is_cancelled = || self.inner.reader.cancelled();
        let (digest, _) =
            hash_and_measure_working_file(root, path, None, None, &is_cancelled, LanguageKind::En)?;
        Ok(digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass2_detects_working_file_mutation_between_metadata_and_hash() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let file_path = root.join("draft.txt");
        fs::write(&file_path, "initial content\n").unwrap();

        let meta_before = collect_working_meta(&root, "draft.txt").unwrap();
        fs::write(&file_path, "mutated content with different length\n").unwrap();

        let err = hash_and_measure_working_file(
            &root,
            "draft.txt",
            Some(&meta_before),
            Some(DiffAttr::Auto),
            &|| false,
            LanguageKind::En,
        )
        .unwrap_err();
        assert_eq!(
            err,
            i18n::repository::repo_changed_during_read_error(LanguageKind::En)
        );

        fs::remove_file(&file_path).unwrap();
        let err_missing = hash_and_measure_working_file(
            &root,
            "draft.txt",
            Some(&meta_before),
            Some(DiffAttr::Auto),
            &|| false,
            LanguageKind::En,
        )
        .unwrap_err();
        assert_eq!(
            err_missing,
            i18n::repository::repo_changed_during_read_error(LanguageKind::En)
        );
    }
}
