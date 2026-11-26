use super::LiveRemoteStatus;
use crate::{
    i18n::LanguageKind,
    repository::{
        RepositoryStore, Result, parse,
        types::{ReadTarget, RepositorySnapshot},
    },
};

impl RepositoryStore {
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
}
