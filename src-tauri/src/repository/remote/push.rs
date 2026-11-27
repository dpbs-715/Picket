use super::{
    PushOutcome,
    helpers::{format_git_error, validate_remote_name},
};
use crate::{
    i18n::LanguageKind,
    repository::{RepositoryStore, Result, parse, types::ReadTarget},
};

impl RepositoryStore {
    /// Fast-forward-only branch push (`R02`, `R05`). Never uses `--force` or `--force-with-lease`.
    pub async fn push_branch(
        &self,
        target: &ReadTarget,
        set_upstream_remote: Option<&str>,
        locale: LanguageKind,
    ) -> Result<PushOutcome> {
        self.inner
            .write_coordinator
            .ensure_not_draining(locale)
            .map_err(|err| err.message)?;
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let _write_guard = self.track_write();
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
