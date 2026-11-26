//! Branch writes are separate from commit authorization and never stage, stash,
//! reset, force checkout, fetch, or retry a partially completed operation.

mod guards;

use super::*;
use crate::i18n::{self, LanguageKind};
use std::collections::BTreeSet;

pub(super) fn split_remote_branch(
    short_name: &str,
    remotes: &[String],
) -> Option<(String, String)> {
    if short_name.is_empty() || short_name.ends_with("/HEAD") {
        return None;
    }
    let mut sorted_remotes: Vec<&str> = remotes.iter().map(String::as_str).collect();
    sorted_remotes.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    for remote in sorted_remotes {
        if let Some(local) = short_name.strip_prefix(&format!("{remote}/"))
            && !local.is_empty()
            && local != "HEAD"
        {
            return Some((remote.to_owned(), local.to_owned()));
        }
    }
    let (remote, local) = short_name.split_once('/')?;
    if remote.is_empty() || local.is_empty() || local == "HEAD" {
        return None;
    }
    Some((remote.to_owned(), local.to_owned()))
}

pub(super) struct BranchState {
    pub status: Vec<u8>,
    pub branch: String,
    pub head: Option<String>,
    pub index: Vec<u8>,
    pub working: Vec<(String, String)>,
}

impl BranchState {
    pub fn token(&self) -> Result<String> {
        Ok(hash(
            serde_json::to_vec(&(&self.status, &self.index, &self.working))
                .map_err(|error| error.to_string())?,
        ))
    }

    fn preserves_changes(&self, next: &Self) -> bool {
        self.index == next.index && self.working == next.working
    }
}

impl RepositoryStore {
    pub(super) async fn branch_state(
        &self,
        root: &Path,
        locale: LanguageKind,
    ) -> Result<BranchState> {
        let reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(root)
            .await?;
        let args = [
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ];
        let status = reader.bytes(root, &args).await?;
        let parsed = parse::status_with_locale(&status, locale)?;
        if parsed.entries.iter().any(|entry| entry.mode == "160000") {
            return Err(i18n::repository::commit_submodule_error(locale).into());
        }
        let index = reader.bytes(root, &["ls-files", "--stage", "-z"]).await?;
        let paths: BTreeSet<_> = parsed
            .entries
            .iter()
            .flat_map(|entry| std::iter::once(entry.path.clone()).chain(entry.previous.clone()))
            .collect();
        let mut working = Vec::new();
        for path in paths {
            working.push((path.clone(), self.working_file(root, &path)?));
        }
        if reader.bytes(root, &args).await? != status {
            return Err(i18n::repository::repo_changed_during_read_error(locale).into());
        }
        Ok(BranchState {
            status,
            index,
            working,
            branch: parsed.branch,
            head: parsed.head,
        })
    }

    async fn registered_branch_path(&self, request: &BranchRequest) -> Result<PathBuf> {
        let locale = request.locale;
        let registered = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?
            .get(&request.target.repository_id)
            .cloned()
            .ok_or_else(|| i18n::repository::repo_not_opened_error(locale))?;
        if registered.branch_tokens.get(&request.target.worktree_id) != Some(&request.branch_token)
        {
            return Err(i18n::repository::branch_token_stale_error(locale).into());
        }
        let root = registered
            .paths
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or_else(|| i18n::repository::repo_not_opened_error(locale))?;
        if fs::canonicalize(&root).map_err(|_| "Worktree directory is inaccessible")? != root {
            return Err("Worktree path changed; please reopen the directory".into());
        }
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                &root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common =
            fs::canonicalize(common).map_err(|_| "Git common directory is inaccessible")?;
        let toplevel = reader
            .text(&root, &["rev-parse", "--show-toplevel"])
            .await?;
        if identity("repo", &common) != request.target.repository_id
            || fs::canonicalize(toplevel).map_err(|_| "Git working directory is inaccessible")?
                != root
        {
            return Err(i18n::repository::repo_identity_changed_error(locale).into());
        }
        Ok(root)
    }

    async fn validate_branch_name(&self, root: &Path, branch: &str) -> Result<()> {
        if branch.is_empty()
            || branch.starts_with('-')
            || branch.trim() != branch
            || branch.chars().any(char::is_control)
        {
            return Err("Invalid branch name; enter a valid local Git branch name".into());
        }
        let checked = self
            .inner
            .reader
            .run(root, &["check-ref-format", "--branch", branch])
            .await?;
        if !checked.success || checked.bytes != format!("{branch}\n").as_bytes() {
            return Err(
                "Invalid branch name; check for spaces, special characters, or path syntax".into(),
            );
        }
        Ok(())
    }

    pub async fn change_branch(&self, request: &BranchRequest) -> Result<RepositorySnapshot> {
        let locale = request.locale;
        validate_request_id_with_locale(&request.request_id, locale)?;
        if !self
            .inner
            .branch_requests
            .lock()
            .map_err(|_| "Branch request registry is unavailable")?
            .insert(request.request_id.clone())
        {
            return Err(i18n::repository::branch_duplicate_request_error(locale).into());
        }
        let _guard = self
            .repository_gate(&request.target.repository_id)?
            .lock_owned()
            .await;
        let _write_guard = self.track_write();
        let root = self.registered_branch_path(request).await?;
        self.ensure_branch_base_write_allowed(&root, locale).await?;
        let before = self.branch_state(&root, locale).await?;
        if before.token()? != request.branch_token {
            return Err(i18n::repository::branch_token_stale_error(locale).into());
        }
        let head = before
            .head
            .as_deref()
            .ok_or_else(|| i18n::repository::branch_unborn_head_error(locale))?;
        let reader = self.inner.reader.with_locale(locale);
        let entries = parse::worktrees(
            &reader
                .bytes(&root, &["worktree", "list", "--porcelain", "-z"])
                .await?,
        )?;
        let (target_branch, checkout_head, switch_args): (String, String, Vec<String>) =
            match request.action {
                BranchAction::SwitchBranch => {
                    self.validate_branch_name(&root, &request.branch).await?;
                    if let Some(entry) = entries.iter().find(|entry| entry.branch == request.branch)
                    {
                        let destination = fs::canonicalize(&entry.path).map_err(|_| {
                            "Worktree occupied by target branch is inaccessible; repair that worktree first"
                        })?;
                        // An occupied branch is a navigation result, never a forced checkout.
                        return self.read_repository_locked(&destination, locale).await;
                    }
                    let oid = reader
                        .text(
                            &root,
                            &[
                                "rev-parse",
                                "--verify",
                                &format!("refs/heads/{}^{{commit}}", request.branch),
                            ],
                        )
                        .await
                        .map_err(
                            |_| "Selected local branch no longer exists; refresh branch list",
                        )?;
                    (
                        request.branch.clone(),
                        oid,
                        vec![
                            "switch".into(),
                            "--no-guess".into(),
                            "--no-overwrite-ignore".into(),
                            request.branch.clone(),
                        ],
                    )
                }
                BranchAction::CheckoutRemoteBranch => {
                    if request.branch.is_empty()
                        || request.branch.starts_with('-')
                        || request.branch.trim() != request.branch
                        || request.branch.chars().any(char::is_control)
                    {
                        return Err(
                            "Invalid remote branch name; select a specific remote branch".into(),
                        );
                    }
                    let remotes = self.list_remotes_unlocked(&root).await.unwrap_or_default();
                    let (remote_name, local_branch) =
                        split_remote_branch(&request.branch, &remotes)
                            .ok_or("Invalid remote branch name; select a specific remote branch")?;
                    self.validate_branch_name(&root, &local_branch).await?;
                    let remote_ref = format!("refs/remotes/{}", request.branch);
                    if reader
                        .run(&root, &["symbolic-ref", "-q", &remote_ref])
                        .await?
                        .success
                    {
                        return Err(
                            "Cannot check out symbolic remote HEAD directly; select a specific remote branch"
                                .into(),
                        );
                    }
                    let remote_head = reader
                        .text(
                            &root,
                            &["rev-parse", "--verify", &format!("{remote_ref}^{{commit}}")],
                        )
                        .await
                        .map_err(|_| {
                            "Selected remote branch does not exist in local tracking refs; sync or refresh first"
                        })?;
                    if let Some(entry) = entries.iter().find(|entry| entry.branch == local_branch) {
                        let destination = fs::canonicalize(&entry.path).map_err(|_| {
                            "Worktree occupied by target branch is inaccessible; repair that worktree first"
                        })?;
                        return self.read_repository_locked(&destination, locale).await;
                    }
                    if let Ok(local_head) = reader
                        .text(
                            &root,
                            &[
                                "rev-parse",
                                "--verify",
                                &format!("refs/heads/{local_branch}^{{commit}}"),
                            ],
                        )
                        .await
                    {
                        (
                            local_branch.clone(),
                            local_head,
                            vec![
                                "switch".into(),
                                "--no-guess".into(),
                                "--no-overwrite-ignore".into(),
                                local_branch,
                            ],
                        )
                    } else {
                        let has_remote_fetch = remotes.contains(&remote_name)
                            && reader
                                .run(
                                    &root,
                                    &[
                                        "config",
                                        "--get-all",
                                        &format!("remote.{remote_name}.fetch"),
                                    ],
                                )
                                .await?
                                .success;
                        let mut args = vec![
                            "switch".to_owned(),
                            "--no-guess".to_owned(),
                            "--no-overwrite-ignore".to_owned(),
                        ];
                        if has_remote_fetch {
                            args.push("--track".to_owned());
                        }
                        args.extend(["-c".to_owned(), local_branch.clone(), remote_ref]);
                        (local_branch, remote_head, args)
                    }
                }
            };
        if !before.working.is_empty() && checkout_head != head {
            return Err(i18n::repository::branch_dirty_cross_commit_error(locale).into());
        }
        // Recheck after all validation and any slow filesystem inspection.
        self.ensure_branch_base_write_allowed(&root, locale).await?;
        self.ensure_branch_target_write_allowed(&root, head, &checkout_head, &before, locale)
            .await?;
        if self.branch_state(&root, locale).await?.token()? != request.branch_token {
            return Err(i18n::repository::branch_token_stale_error(locale).into());
        }
        let writer = reader.for_checkout(&root).await?;
        let args_ref: Vec<&str> = switch_args.iter().map(String::as_str).collect();
        let output = writer
            .write(&root, &args_ref)
            .await
            .map_err(|error| i18n::repository::branch_switch_failed_error(locale, &error))?;
        if !output.success {
            return Err(i18n::repository::branch_switch_failed_error(
                locale,
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        let after = self.branch_state(&root, locale).await?;
        if !before.working.is_empty() && !before.preserves_changes(&after) {
            return Err(
                "Staging area or working draft after Git branch operation did not match expectation; stopped without automatic rollback. Refresh and verify in Git / IDE"
                    .into(),
            );
        }
        if after.branch != target_branch || after.head.as_deref() != Some(checkout_head.as_str()) {
            return Err(
                "Branch or HEAD after Git operation did not match expectation; refresh to verify (no automatic retry)"
                    .into(),
            );
        }
        self.read_repository_locked(&root, locale)
            .await
            .map_err(|error| i18n::repository::branch_switched_read_failed_error(locale, &error))
    }
}
