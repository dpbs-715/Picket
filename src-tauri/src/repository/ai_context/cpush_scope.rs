use super::{
    ExtractedCPushScope, FrozenCPushFile,
    patch::{
        CPUSH_MAX_FILE_PATCH_BYTES, CPUSH_MAX_TOTAL_PATCH_BYTES, CPUSH_MIN_PARTIAL_PATCH_BUDGET,
        MAX_REVIEW_UNTRACKED_READ_BYTES, MAX_UNTRACKED_READ_BYTES, RawPatchOutcome,
        is_sensitive_local_file, read_authorized_scope_diff, read_change_raw_patch,
        redact_sensitive_patch, truncate_patch_head_tail,
    },
};
use crate::{
    i18n::{self, LanguageKind},
    repository::{
        RepositoryStore, Result,
        parse::WorktreeEntry,
        read_tools::validate_safe_repo_path,
        types::{Change, CommitSelection, Range, ReadTarget, RepositorySnapshot},
    },
};
use std::collections::{HashMap, HashSet};

impl RepositoryStore {
    pub async fn extract_cpush_scope(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
        locale: LanguageKind,
    ) -> Result<ExtractedCPushScope> {
        if selections.is_empty() {
            return Err(i18n::repository::ai_selection_required_error(locale, true).into());
        }
        let mut seen = HashSet::with_capacity(selections.len());
        for item in selections {
            if item.file_id.is_empty()
                || item.commit_token.is_empty()
                || !seen.insert(item.file_id.as_str())
            {
                return Err(i18n::repository::ai_invalid_token_error(locale, true).into());
            }
        }

        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let (registered, root) = self.registered_worktree_root(target, locale).await?;
        let keep_set = self.resolve_keep_set_for_common(&registered.common_dir)?;
        let entry = WorktreeEntry {
            path: root.clone(),
            head: None,
            branch: String::new(),
            bare: false,
        };
        let (live_tree, live_commit_state) = self
            .read_worktree(&target.repository_id, &entry, &keep_set, locale)
            .await?;
        if live_tree.detached {
            return Err(i18n::repository::cpush_detached_head_error(locale).into());
        }

        let change_by_id: HashMap<&str, &Change> = live_tree
            .files
            .iter()
            .map(|change| (change.id.as_str(), change))
            .collect();

        for item in selections {
            let Some(authorized) = live_commit_state.files.get(&item.file_id) else {
                return Err(i18n::repository::ai_file_missing_error(locale, true).into());
            };
            if authorized.conflicted {
                return Err(i18n::repository::ai_conflict_in_scope_error(locale, true).into());
            }
            if authorized.token != item.commit_token {
                return Err(i18n::repository::ai_scope_stale_error(locale, true).into());
            }
        }

        let unselected_paths = live_tree
            .files
            .iter()
            .filter(|change| !seen.contains(change.id.as_str()))
            .map(|change| change.path.clone())
            .collect();

        let worktree_reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;
        let recent_commits = if live_tree.head.is_some() {
            let output = worktree_reader
                .run(&root, &["log", "-n", "8", "--format=%s%x00", "HEAD"])
                .await?;
            if output.success {
                output
                    .bytes
                    .split(|byte| *byte == 0)
                    .filter_map(|raw| std::str::from_utf8(raw).ok())
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut raw_entries: Vec<(Option<String>, Option<String>)> =
            Vec::with_capacity(selections.len());

        for item in selections {
            let change = change_by_id[item.file_id.as_str()];
            let mut raw_patch_opt = None;
            let mut skip_reason = None;

            if change.ignored_for_review {
                skip_reason = Some(i18n::repository::cpush_skip_lockfile(
                    locale,
                    change.range.added,
                    change.range.removed,
                ));
            } else if change.binary {
                skip_reason = Some(i18n::repository::cpush_skip_binary_patch(locale).into());
            } else {
                match read_change_raw_patch(
                    &worktree_reader,
                    &root,
                    live_tree.head.as_deref(),
                    change,
                    "--unified=1",
                    MAX_UNTRACKED_READ_BYTES,
                )
                .await?
                {
                    RawPatchOutcome::Patch(text) => raw_patch_opt = Some(text),
                    RawPatchOutcome::Binary => {
                        skip_reason = Some(i18n::repository::cpush_skip_binary(locale).into());
                    }
                    RawPatchOutcome::NonUtf8 => {
                        skip_reason = Some(i18n::repository::cpush_skip_non_utf8(locale).into());
                    }
                    RawPatchOutcome::UntrackedTooLarge => {
                        skip_reason =
                            Some(i18n::repository::cpush_skip_large_untracked(locale).into());
                    }
                }
            }

            if is_sensitive_local_file(&change.path) {
                raw_patch_opt =
                    raw_patch_opt.and_then(|patch| redact_sensitive_patch(&change.path, &patch));
                skip_reason = Some(i18n::repository::cpush_skip_sensitive(locale).into());
            }

            raw_entries.push((raw_patch_opt, skip_reason));
        }

        let mut patch_indices: Vec<usize> = raw_entries
            .iter()
            .enumerate()
            .filter_map(|(idx, (patch, _))| patch.as_ref().map(|p| (idx, p.len())))
            .map(|(idx, _)| idx)
            .collect();
        patch_indices.sort_by_key(|&idx| (raw_entries[idx].0.as_ref().map_or(0, String::len), idx));

        let mut remaining_budget = CPUSH_MAX_TOTAL_PATCH_BYTES;
        for (pos, &idx) in patch_indices.iter().enumerate() {
            let files_left = patch_indices.len() - pos;
            let fair_share = (remaining_budget / files_left).min(CPUSH_MAX_FILE_PATCH_BYTES);
            let Some(raw_patch) = raw_entries[idx].0.take() else {
                continue;
            };
            if raw_patch.len() <= fair_share {
                remaining_budget = remaining_budget.saturating_sub(raw_patch.len());
                raw_entries[idx].0 = Some(raw_patch);
            } else if fair_share >= CPUSH_MIN_PARTIAL_PATCH_BUDGET {
                let slimmed = truncate_patch_head_tail(&raw_patch, fair_share, locale);
                remaining_budget = remaining_budget.saturating_sub(slimmed.len());
                raw_entries[idx].0 = Some(slimmed);
                raw_entries[idx].1.get_or_insert_with(|| {
                    i18n::repository::cpush_skip_slimmed_patch(locale).into()
                });
            } else {
                raw_entries[idx].1.get_or_insert_with(|| {
                    i18n::repository::cpush_skip_omitted_budget(locale).into()
                });
            }
        }

        let files = selections
            .iter()
            .zip(raw_entries)
            .map(|(item, (patch, skip_reason))| {
                let change = change_by_id[item.file_id.as_str()];
                FrozenCPushFile {
                    file_id: change.id.clone(),
                    path: change.path.clone(),
                    previous_path: change.previous_path.clone(),
                    status: change.status.clone(),
                    range_kind: change.range.kind.clone(),
                    added: change.range.added,
                    removed: change.range.removed,
                    ignored_for_review: change.ignored_for_review,
                    binary: change.binary,
                    content_version: change.content_version.clone(),
                    patch,
                    skip_reason,
                }
            })
            .collect();

        Ok(ExtractedCPushScope {
            branch: live_tree.branch,
            initial_head: live_tree.head,
            recent_commits,
            unselected_paths,
            files,
        })
    }

    pub async fn resolve_cpush_step_selections(
        &self,
        target: &ReadTarget,
        expected_branch: &str,
        expected_head: Option<&str>,
        step_files: &[FrozenCPushFile],
        locale: LanguageKind,
    ) -> Result<(RepositorySnapshot, Vec<CommitSelection>)> {
        let snapshot = self.refresh_with_locale(target, locale).await?;
        let live_tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.worktree_id == target.worktree_id)
            .ok_or("Current worktree is missing from latest Git snapshot; please reopen")?;
        if let Some(err) = &live_tree.read_error {
            return Err(format!("Worktree status is unavailable: {err}"));
        }
        if live_tree.detached || live_tree.branch != expected_branch {
            return Err(i18n::repository::cpush_step_branch_changed_error(
                locale,
                &live_tree.branch,
                live_tree.detached,
                expected_branch,
            ));
        }
        if live_tree.head.as_deref() != expected_head {
            return Err(
                "Repository HEAD changed outside the current session; paused subsequent commits. Verify latest Git history"
                    .into(),
            );
        }

        let mut selections = Vec::with_capacity(step_files.len());
        for frozen in step_files {
            let Some(live_file) = live_tree
                .files
                .iter()
                .find(|change| change.id == frozen.file_id)
            else {
                return Err(i18n::repository::cpush_step_missing_file_error(
                    locale,
                    &frozen.path,
                ));
            };
            if live_file.content_version != frozen.content_version
                || live_file.range.kind != frozen.range_kind
            {
                return Err(i18n::repository::cpush_step_stale_file_error(
                    locale,
                    &frozen.path,
                ));
            }
            selections.push(CommitSelection {
                file_id: live_file.id.clone(),
                commit_token: live_file.commit_token.clone(),
            });
        }
        Ok((snapshot, selections))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn read_cpush_scope_diff(
        &self,
        target: &ReadTarget,
        initial_head: Option<&str>,
        frozen_files: &[FrozenCPushFile],
        raw_path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
        locale: LanguageKind,
    ) -> Result<String> {
        let path = validate_safe_repo_path(raw_path)?;
        let Some(frozen) = frozen_files.iter().find(|file| file.path == path) else {
            return Err(format!(
                "File `{path}` is not part of the authorized C-Push scope"
            ));
        };
        if is_sensitive_local_file(&path) {
            return Err(format!(
                "Refused to read sensitive local file diff: `{path}`"
            ));
        }
        if frozen.binary {
            return Err(format!("Binary file `{path}` has no text diff"));
        }

        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let worktree_reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;

        let git_status = if frozen.range_kind == "unstaged" && frozen.status == "A" {
            "??".to_owned()
        } else {
            String::new()
        };
        let synthetic = Change {
            id: frozen.file_id.clone(),
            path: frozen.path.clone(),
            previous_path: frozen.previous_path.clone(),
            status: frozen.status.clone(),
            git_status,
            range: Range {
                kind: frozen.range_kind.clone(),
                added: frozen.added,
                removed: frozen.removed,
            },
            binary: frozen.binary,
            ignored_for_review: frozen.ignored_for_review,
            local_keep: false,
            content_version: frozen.content_version.clone(),
            commit_token: String::new(),
        };

        let raw_patch = match read_change_raw_patch(
            &worktree_reader,
            &root,
            initial_head,
            &synthetic,
            "--unified=3",
            MAX_REVIEW_UNTRACKED_READ_BYTES,
        )
        .await?
        {
            RawPatchOutcome::Patch(text) => text,
            RawPatchOutcome::Binary => {
                return Err(format!("Binary file `{path}` has no text diff"));
            }
            RawPatchOutcome::NonUtf8 => {
                return Err(format!("File `{path}` diff is not valid UTF-8 text"));
            }
            RawPatchOutcome::UntrackedTooLarge => {
                return Err(format!(
                    "Untracked file `{path}` exceeds the 512 KB diff read limit"
                ));
            }
        };

        let mut patches = HashMap::with_capacity(1);
        patches.insert(path.clone(), raw_patch);
        let (formatted, _, _, _) =
            read_authorized_scope_diff(&patches, &path, start_line, end_line)?;
        Ok(formatted)
    }
}
