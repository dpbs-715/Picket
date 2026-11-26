use super::{
    RebaseSyncOutcome,
    helpers::{format_git_error, validate_remote_name},
};
use crate::{
    i18n::{self, LanguageKind},
    repository::{
        RepositoryStore, Result, parse,
        types::{ReadTarget, RepositorySnapshot},
    },
};
use std::collections::BTreeSet;

impl RepositoryStore {
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
                    .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale).into());
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
            return Err(i18n::repository::commit_ongoing_op_error(locale, marker));
        }
        if !self.list_unmerged_files(&root).await?.is_empty() {
            return Err(i18n::repository::commit_unresolved_conflict_error(locale).into());
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
        if parsed.entries.iter().any(|entry| entry.mode == "160000") {
            return Err(i18n::repository::commit_submodule_error(locale).into());
        }
        if parsed.branch == "(detached)" {
            return Err(i18n::repository::commit_detached_head_error(locale).into());
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
            return Err(i18n::repository::remote_fetch_failed_error(
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
                reason: i18n::repository::remote_rewound_history_reason(locale, &spec.display),
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
                    reason: i18n::repository::remote_merge_commit_reason(locale).into(),
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
                        reason: i18n::repository::remote_published_on_other_ref_reason(
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

        if self
            .diff_touches_submodule(&root, &merge_base, &spec.tracking_ref)
            .await?
            || (ahead > 0
                && self
                    .diff_touches_submodule(&root, &merge_base, &head)
                    .await?)
        {
            return Err(i18n::repository::remote_rebase_submodule_error(locale).into());
        }

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
}
