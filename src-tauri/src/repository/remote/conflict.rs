use super::{
    ExtractedConflictBatch, ExtractedConflictFile, RebaseContinueOutcome,
    helpers::{format_git_error, line_has_conflict_markers},
};
use crate::{
    foundation::pi::PiResolvedConflictFile,
    i18n::{self, LanguageKind},
    repository::{RepositoryStore, Result, parse, types::ReadTarget},
};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
};

const MAX_CONFLICT_TEXT_BYTES: usize = 128 * 1024;

impl RepositoryStore {
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
            .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale).into());

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
                return Err(i18n::repository::remote_conflict_unauthorized_file_error(
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
                return Err(i18n::repository::remote_conflict_leftover_markers_error(
                    locale, &item.path,
                ));
            }
            if item.content.trim().is_empty()
                && !orig.upstream_content.trim().is_empty()
                && !orig.local_content.trim().is_empty()
            {
                return Err(i18n::repository::remote_conflict_empty_result_error(
                    locale, &item.path,
                ));
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
            .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale).into());

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
}
