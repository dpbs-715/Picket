use super::{
    DeferredPatchNotice, ExtractedAiScope, ExtractedDiffFile,
    patch::{
        MAX_FILE_PATCH_BYTES, MAX_REVIEW_UNTRACKED_READ_BYTES, MAX_SCALED_FILE_PATCH_BYTES,
        MIN_PARTIAL_PATCH_BUDGET, RawPatchOutcome, is_sensitive_local_file, read_change_raw_patch,
        redact_sensitive_patch, truncate_patch_head_tail_with_ranges,
    },
};
use crate::{
    i18n::{self, LanguageKind},
    repository::{
        RepositoryStore, Result,
        parse::WorktreeEntry,
        types::{Change, CommitSelection, ReadTarget},
    },
};
use std::collections::{HashMap, HashSet};

impl RepositoryStore {
    pub async fn extract_ai_scope_with_budget(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
        locale: LanguageKind,
        max_total_patch_bytes: usize,
    ) -> Result<ExtractedAiScope> {
        if selections.is_empty() {
            return Err(i18n::repository::ai_selection_required_error(locale, false).into());
        }
        let mut seen = HashSet::with_capacity(selections.len());
        for item in selections {
            if item.file_id.is_empty()
                || item.commit_token.is_empty()
                || !seen.insert(&item.file_id)
            {
                return Err(i18n::repository::ai_invalid_token_error(locale, false).into());
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

        let change_by_id: HashMap<&str, &Change> = live_tree
            .files
            .iter()
            .map(|change| (change.id.as_str(), change))
            .collect();

        for item in selections {
            let Some(authorized) = live_commit_state.files.get(&item.file_id) else {
                return Err(i18n::repository::ai_file_missing_error(locale, false).into());
            };
            if authorized.conflicted {
                return Err(i18n::repository::ai_conflict_in_scope_error(locale, false).into());
            }
            if authorized.token != item.commit_token {
                return Err(i18n::repository::ai_scope_stale_error(locale, false).into());
            }
        }

        let worktree_reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;

        let mut skipped_paths = Vec::new();
        let mut authorized_patches = HashMap::new();
        let mut raw_candidates: Vec<(usize, String)> = Vec::new();

        for (selection_idx, item) in selections.iter().enumerate() {
            let change = change_by_id[item.file_id.as_str()];
            if change.ignored_for_review {
                skipped_paths.push(i18n::repository::ai_skipped_lockfile_review(
                    locale,
                    &change.path,
                ));
                continue;
            }
            if change.binary {
                skipped_paths.push(i18n::repository::ai_skipped_binary(locale, &change.path));
                continue;
            }

            let raw_patch = match read_change_raw_patch(
                &worktree_reader,
                &root,
                live_tree.head.as_deref(),
                change,
                "--unified=3",
                MAX_REVIEW_UNTRACKED_READ_BYTES,
            )
            .await?
            {
                RawPatchOutcome::Patch(text) => text,
                RawPatchOutcome::Binary => {
                    skipped_paths.push(i18n::repository::ai_skipped_binary(locale, &change.path));
                    continue;
                }
                RawPatchOutcome::NonUtf8 => {
                    skipped_paths.push(i18n::repository::ai_skipped_non_utf8(locale, &change.path));
                    continue;
                }
                RawPatchOutcome::UntrackedTooLarge => {
                    skipped_paths.push(i18n::repository::ai_skipped_diff_too_large(
                        locale,
                        &change.path,
                    ));
                    continue;
                }
            };

            let raw_patch = if is_sensitive_local_file(&change.path) {
                skipped_paths.push(i18n::repository::ai_skipped_sensitive_redacted(
                    locale,
                    &change.path,
                ));
                match redact_sensitive_patch(&change.path, &raw_patch) {
                    Some(redacted) => redacted,
                    None => continue,
                }
            } else {
                raw_patch
            };

            authorized_patches.insert(change.path.clone(), raw_patch.clone());
            raw_candidates.push((selection_idx, raw_patch));
        }

        // Distribute the inline diff budget smallest-first (water-filling) so early large
        // files never starve smaller files later in alphabetical order.
        let mut order: Vec<usize> = (0..raw_candidates.len()).collect();
        order.sort_by_key(|&pos| (raw_candidates[pos].1.len(), raw_candidates[pos].0));

        let mut remaining_budget = max_total_patch_bytes.max(MIN_PARTIAL_PATCH_BUDGET);
        let max_file_patch_bytes =
            (remaining_budget / 3).clamp(MAX_FILE_PATCH_BYTES, MAX_SCALED_FILE_PATCH_BYTES);
        let mut allocated: Vec<Option<(String, Option<DeferredPatchNotice>)>> =
            vec![None; raw_candidates.len()];

        for (step, &cand_pos) in order.iter().enumerate() {
            let files_left = order.len() - step;
            let fair_share = (remaining_budget / files_left).min(max_file_patch_bytes);
            let (selection_idx, ref raw_patch) = raw_candidates[cand_pos];
            let change = change_by_id[selections[selection_idx].file_id.as_str()];
            let total_lines = raw_patch.lines().count().max(1);

            if raw_patch.len() <= fair_share {
                remaining_budget = remaining_budget.saturating_sub(raw_patch.len());
                allocated[cand_pos] = Some((raw_patch.clone(), None));
            } else if fair_share >= MIN_PARTIAL_PATCH_BUDGET {
                let (mut slimmed, initial_ranges, total_lines) =
                    truncate_patch_head_tail_with_ranges(raw_patch, fair_share, locale);
                let first_omitted = initial_ranges
                    .first()
                    .map(|&(_, end)| end.saturating_add(1))
                    .unwrap_or(1)
                    .min(total_lines);
                let last_omitted = initial_ranges
                    .get(1)
                    .map(|&(start, _)| start.saturating_sub(1))
                    .unwrap_or(total_lines)
                    .max(first_omitted);
                slimmed.push_str(&format!(
                    "\n[Diff truncated ({total_lines} lines total). Call read_scope_diff(path=\"{}\", startLine={first_omitted}, endLine={last_omitted}) to inspect omitted hunks.]",
                    change.path
                ));
                remaining_budget = remaining_budget.saturating_sub(slimmed.len());
                allocated[cand_pos] = Some((
                    slimmed,
                    Some(DeferredPatchNotice {
                        file_id: change.id.clone(),
                        path: change.path.clone(),
                        total_lines,
                        initial_ranges,
                        deferred_only: false,
                        partial_notice: i18n::repository::ai_skipped_large_partial(
                            locale,
                            &change.path,
                        ),
                        omitted_notice: i18n::repository::ai_skipped_total_too_large(
                            locale,
                            &change.path,
                        ),
                    }),
                ));
            } else {
                let stub = format!(
                    "[Inline diff omitted due to large total changeset ({total_lines} lines, {} bytes). Call read_scope_diff(path=\"{}\", startLine=1, endLine=250) to review its exact authorized diff.]",
                    raw_patch.len(),
                    change.path
                );
                allocated[cand_pos] = Some((
                    stub,
                    Some(DeferredPatchNotice {
                        file_id: change.id.clone(),
                        path: change.path.clone(),
                        total_lines,
                        initial_ranges: Vec::new(),
                        deferred_only: true,
                        partial_notice: i18n::repository::ai_skipped_large_partial(
                            locale,
                            &change.path,
                        ),
                        omitted_notice: i18n::repository::ai_skipped_total_too_large(
                            locale,
                            &change.path,
                        ),
                    }),
                ));
            }
        }

        let mut files = Vec::with_capacity(raw_candidates.len());
        let mut reviewed_file_ids = Vec::with_capacity(raw_candidates.len());
        let mut deferred_notices = Vec::new();
        for (cand_pos, (selection_idx, _)) in raw_candidates.into_iter().enumerate() {
            let Some((final_patch, notice)) = allocated[cand_pos].take() else {
                continue;
            };
            let change = change_by_id[selections[selection_idx].file_id.as_str()];
            if let Some(deferred) = notice {
                deferred_notices.push(deferred);
            }
            reviewed_file_ids.push(change.id.clone());
            files.push(ExtractedDiffFile {
                file_id: change.id.clone(),
                path: change.path.clone(),
                previous_path: change.previous_path.clone(),
                status: change.status.clone(),
                range_kind: change.range.kind.clone(),
                added: change.range.added,
                removed: change.range.removed,
                patch: final_patch,
            });
        }

        let branch = if live_tree.detached {
            "(detached)".into()
        } else {
            live_tree.branch
        };
        Ok(ExtractedAiScope {
            branch,
            files,
            skipped_paths,
            reviewed_file_ids,
            authorized_patches,
            deferred_notices,
        })
    }
}
