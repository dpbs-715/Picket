use super::{
    ExecutionRecord,
    registration::{
        AuthorizedFile, CommitRegistration, collect_unit_stage_entries, ensure_safe_repo_path,
        gitlink_entries_from_stage, path_conflicts_with_submodule, stage_entries_by_path,
    },
};
use crate::{
    error::{AppError, AppErrorCode, CommitGuardError},
    i18n::{self, LanguageKind},
    repository::{
        Registration, RepositoryStore, Result,
        branches::BranchState,
        identity, parse,
        runner::GitReader,
        staged,
        types::{CommitResult, CommitSelection, ReadTarget},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

pub(in crate::repository) async fn is_sparse_checkout_enabled(
    reader: &GitReader,
    root: &Path,
    locale: LanguageKind,
) -> Result<bool> {
    let output = reader
        .run(
            root,
            &["config", "--type=bool", "--get", "core.sparseCheckout"],
        )
        .await?;
    if output.code == Some(1) {
        return Ok(false);
    }
    if !output.success {
        return Err(i18n::repository::commit_sparse_or_hidden_flag_error(locale).into());
    }
    Ok(parse::utf8_with_locale(&output.bytes, locale)?.trim() == "true")
}

pub(in crate::repository) async fn hidden_index_flag_paths(
    reader: &GitReader,
    root: &Path,
    locale: LanguageKind,
) -> Result<BTreeSet<String>> {
    let flags = reader.bytes(root, &["ls-files", "-v", "-z"]).await?;
    let mut hidden = BTreeSet::new();
    for entry in flags.split(|byte| *byte == 0).filter(|e| !e.is_empty()) {
        if entry[0] == b'H' {
            continue;
        }
        if entry.len() <= 2 || entry[1] != b' ' {
            return Err(i18n::repository::commit_sparse_or_hidden_flag_error(locale).into());
        }
        let path = parse::utf8_with_locale(&entry[2..], locale)?;
        hidden.insert(path);
    }
    Ok(hidden)
}

pub(super) fn resolve_authorized_selections<'a>(
    registered: &'a Registration,
    target: &ReadTarget,
    selections: &[CommitSelection],
    locale: LanguageKind,
) -> std::result::Result<(&'a CommitRegistration, Vec<AuthorizedFile>), AppError> {
    let commit_state = registered
        .commit_states
        .get(&target.worktree_id)
        .ok_or_else(|| {
            AppError::git(
                AppErrorCode::CommitTokenInvalid,
                i18n::repository::commit_token_invalid_error(locale),
            )
        })?;
    if commit_state.detached {
        return Err(AppError::git(
            AppErrorCode::CommitDetachedHead,
            i18n::repository::commit_detached_head_error(locale),
        ));
    }
    let mut selected_files = Vec::with_capacity(selections.len());
    for selection in selections {
        let Some(authorized) = commit_state.files.get(&selection.file_id) else {
            return Err(AppError::git(
                AppErrorCode::CommitScopeChanged,
                i18n::repository::commit_scope_changed_error(locale),
            ));
        };
        if authorized.token != selection.commit_token {
            return Err(AppError::git(
                AppErrorCode::CommitTokenInvalid,
                i18n::repository::commit_token_invalid_error(locale),
            ));
        }
        if authorized.conflicted {
            return Err(AppError::git(
                AppErrorCode::CommitConflictUnresolved,
                i18n::repository::commit_unresolved_conflict_error(locale),
            ));
        }
        ensure_safe_repo_path(&authorized.path)
            .map_err(|msg| AppError::git(AppErrorCode::CommitFailed, msg))?;
        if let Some(previous) = &authorized.previous_path {
            ensure_safe_repo_path(previous)
                .map_err(|msg| AppError::git(AppErrorCode::CommitFailed, msg))?;
        }
        selected_files.push(authorized.clone());
    }
    Ok((commit_state, selected_files))
}

impl RepositoryStore {
    pub(crate) async fn worktree_root_for_target(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<PathBuf> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        Ok(root)
    }

    pub(in crate::repository) async fn registered_worktree_root(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<(Registration, PathBuf)> {
        let registered = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?
            .get(&target.repository_id)
            .cloned()
            .ok_or_else(|| i18n::repository::repo_not_opened_error(locale))?;
        let root = registered
            .paths
            .get(&target.worktree_id)
            .cloned()
            .ok_or_else(|| i18n::repository::repo_not_opened_error(locale))?;
        if fs::canonicalize(&root).map_err(|_| "Worktree directory is inaccessible")? != root {
            return Err(
                "Worktree path changed; reopen the directory (draft and selection were preserved)"
                    .into(),
            );
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
        if identity("repo", &common) != target.repository_id
            || fs::canonicalize(toplevel).map_err(|_| "Git working directory is inaccessible")?
                != root
        {
            return Err(i18n::repository::repo_identity_changed_error(locale).into());
        }
        Ok((registered, root))
    }

    pub(super) async fn ensure_commit_write_allowed(
        &self,
        root: &Path,
        selected_unit_paths: &BTreeSet<String>,
        locale: LanguageKind,
    ) -> std::result::Result<(), AppError> {
        let map_git = |msg: String| AppError::git(AppErrorCode::CommitFailed, msg);
        let reader = self.inner.reader.with_locale(locale);
        for lock in ["index.lock", "HEAD.lock"] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", lock],
                )
                .await
                .map_err(map_git)?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(AppError::git(
                    AppErrorCode::CommitLockFilePresent,
                    i18n::repository::commit_lock_file_error(locale, lock),
                )
                .with_retryable(true));
            }
        }
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "BISECT_LOG",
        ] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .await
                .map_err(map_git)?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(AppError::git(
                    AppErrorCode::CommitFailed,
                    i18n::repository::commit_ongoing_op_error(locale, marker),
                ));
            }
        }
        if !reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await
            .map_err(map_git)?
            .is_empty()
        {
            return Err(AppError::git(
                AppErrorCode::CommitConflictUnresolved,
                i18n::repository::commit_unresolved_conflict_error(locale),
            ));
        }
        if is_sparse_checkout_enabled(&reader, root, locale)
            .await
            .map_err(CommitGuardError::SparseOrHiddenFlag)?
        {
            return Err(CommitGuardError::SparseOrHiddenFlag(
                i18n::repository::commit_sparse_or_hidden_flag_error(locale).into(),
            )
            .into());
        }
        let hidden_paths = hidden_index_flag_paths(&reader, root, locale)
            .await
            .map_err(CommitGuardError::SparseOrHiddenFlag)?;
        if hidden_paths.iter().any(|path| path.ends_with('/'))
            || selected_unit_paths
                .iter()
                .any(|path| hidden_paths.contains(path))
        {
            return Err(CommitGuardError::SparseOrHiddenFlag(
                i18n::repository::commit_sparse_or_hidden_flag_error(locale).into(),
            )
            .into());
        }
        let stage_bytes = reader
            .bytes(root, &["ls-files", "--stage", "-z"])
            .await
            .map_err(map_git)?;
        let gitlinks = gitlink_entries_from_stage(&stage_bytes).map_err(map_git)?;
        if selected_unit_paths.iter().any(|path| {
            gitlinks
                .keys()
                .any(|sub| path_conflicts_with_submodule(path, sub))
        }) {
            return Err(CommitGuardError::SubmoduleUnsupported(
                i18n::repository::commit_submodule_error(locale).into(),
            )
            .into());
        }
        Ok(())
    }

    /// Runs static write-eligibility checks for the selected commit scope without mutating Git,
    /// allowing C-Push to fail fast before invoking Pi Core planning.
    pub async fn preflight_commit_scope(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
        locale: LanguageKind,
    ) -> std::result::Result<(), AppError> {
        let map_git = |msg: String| AppError::git(AppErrorCode::CommitFailed, msg);
        if selections.is_empty() {
            return Err(AppError::git(
                AppErrorCode::CommitFailed,
                i18n::repository::commit_selection_required_error(locale),
            ));
        }
        let _guard = self
            .repository_gate(&target.repository_id)
            .map_err(map_git)?
            .lock_owned()
            .await;
        let (registered, root) = self
            .registered_worktree_root(target, locale)
            .await
            .map_err(map_git)?;
        let (commit_state, selected_files) =
            resolve_authorized_selections(&registered, target, selections, locale)?;
        let selected_unit_paths: BTreeSet<String> = selected_files
            .iter()
            .flat_map(AuthorizedFile::unit_paths)
            .collect();
        self.ensure_commit_write_allowed(&root, &selected_unit_paths, locale)
            .await?;
        let unstaged_paths: Vec<String> = selected_files
            .iter()
            .filter(|file| !file.staged)
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.ensure_commit_filters_allowed(&root, &unstaged_paths, locale)
            .await?;
        let live_state = self.branch_state(&root, locale).await?;
        self.verify_live_selected_scope(commit_state, &selected_files, &live_state, locale)?;
        Ok(())
    }

    pub(super) async fn ensure_commit_filters_allowed(
        &self,
        root: &Path,
        unstaged_paths: &[String],
        locale: LanguageKind,
    ) -> std::result::Result<(), AppError> {
        let map_git = |msg: String| AppError::git(AppErrorCode::CommitFailed, msg);
        if unstaged_paths.is_empty() {
            return Ok(());
        }
        let reader = self.inner.reader.with_locale(locale);
        let filters = reader
            .run(
                root,
                &[
                    "config",
                    "--null",
                    "--name-only",
                    "--get-regexp",
                    r"^filter\..*\.(clean|process)$",
                    ".+",
                ],
            )
            .await
            .map_err(map_git)?;
        if filters.code == Some(1) {
            return Ok(());
        }
        if !filters.success {
            return Err(AppError::git(
                AppErrorCode::CommitFailed,
                "Cannot inspect Git file filter configuration; no commit was created (draft and selection were preserved)",
            ));
        }
        let mut drivers = BTreeSet::new();
        for key in filters
            .bytes
            .split(|byte| *byte == 0)
            .filter(|key| !key.is_empty())
        {
            let key = parse::utf8_with_locale(key, locale).map_err(map_git)?;
            let (driver, _) = key
                .strip_prefix("filter.")
                .ok_or_else(|| {
                    AppError::git(
                        AppErrorCode::CommitFailed,
                        "Cannot parse Git filter driver name",
                    )
                })?
                .rsplit_once('.')
                .ok_or_else(|| {
                    AppError::git(
                        AppErrorCode::CommitFailed,
                        "Cannot parse Git filter driver name",
                    )
                })?;
            drivers.insert(driver.to_owned());
        }
        for batch in unstaged_paths.chunks(128) {
            let mut args = vec!["check-attr", "-z", "filter", "--"];
            args.extend(batch.iter().map(String::as_str));
            let attributes = reader.bytes(root, &args).await.map_err(map_git)?;
            let fields: Vec<_> = attributes.split(|byte| *byte == 0).collect();
            for record in fields.chunks(3).filter(|record| record.len() == 3) {
                if drivers.contains(&parse::utf8_with_locale(record[2], locale).map_err(map_git)?) {
                    return Err(CommitGuardError::ExternalFilterBlocked(
                        i18n::repository::runner_external_filter_write_error(locale).into(),
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    pub(super) fn verify_live_selected_scope(
        &self,
        commit_state: &CommitRegistration,
        selected_files: &[AuthorizedFile],
        live_state: &BranchState,
        locale: LanguageKind,
    ) -> std::result::Result<BTreeMap<String, Vec<u8>>, AppError> {
        let map_git = |msg: String| AppError::git(AppErrorCode::CommitFailed, msg);
        let live_status = parse::status_with_locale(&live_state.status, locale).map_err(map_git)?;
        if live_status.branch == "(detached)" {
            return Err(AppError::git(
                AppErrorCode::CommitDetachedHead,
                i18n::repository::commit_detached_head_error(locale),
            ));
        }
        if live_status.branch != commit_state.branch || live_status.head != commit_state.head {
            return Err(AppError::git(
                AppErrorCode::CommitScopeChanged,
                i18n::repository::commit_scope_changed_error(locale),
            ));
        }
        if live_status.entries.iter().any(|entry| entry.conflicted) {
            return Err(AppError::git(
                AppErrorCode::CommitConflictUnresolved,
                i18n::repository::commit_unresolved_conflict_error(locale),
            ));
        }
        if live_status
            .entries
            .iter()
            .any(|entry| entry.mode == "160000")
        {
            return Err(CommitGuardError::SubmoduleUnsupported(
                i18n::repository::commit_submodule_error(locale).into(),
            )
            .into());
        }
        let live_stage_map = stage_entries_by_path(&live_state.index).map_err(map_git)?;
        let selected_paths: HashSet<&str> = selected_files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        for entry in &live_status.entries {
            if !selected_paths.contains(entry.path.as_str())
                && staged(entry)
                && entry.xy.starts_with('R')
                && let Some(previous) = entry.previous.as_deref()
                && selected_paths.contains(previous)
            {
                return Err(AppError::git(
                    AppErrorCode::CommitScopeChanged,
                    i18n::repository::commit_rename_slot_collision_error(locale),
                ));
            }
        }
        let working_map: HashMap<&str, &str> = live_state
            .working
            .iter()
            .map(|(path, digest)| (path.as_str(), digest.as_str()))
            .collect();
        for authorized in selected_files {
            let Some(live_entry) = live_status
                .entries
                .iter()
                .find(|entry| entry.path == authorized.path)
            else {
                return Err(AppError::git(
                    AppErrorCode::CommitScopeChanged,
                    i18n::repository::commit_scope_changed_error(locale),
                ));
            };
            let live_staged = staged(live_entry);
            let live_kind = if live_staged {
                if live_entry
                    .xy
                    .as_bytes()
                    .get(1)
                    .is_some_and(|byte| *byte != b'.')
                {
                    "partial"
                } else {
                    "staged"
                }
            } else {
                "unstaged"
            };
            let live_previous = (live_staged && live_entry.xy.starts_with('R'))
                .then(|| live_entry.previous.clone())
                .flatten();
            let live_stage_entries =
                collect_unit_stage_entries(&live_stage_map, &authorized.unit_paths());
            if live_staged != authorized.staged
                || live_kind != authorized.range_kind
                || live_previous != authorized.previous_path
                || live_stage_entries != authorized.stage_entries
            {
                return Err(AppError::git(
                    AppErrorCode::CommitScopeChanged,
                    i18n::repository::commit_scope_changed_error(locale),
                ));
            }
            if !authorized.staged
                && working_map.get(authorized.path.as_str()).copied()
                    != authorized.working_hash.as_deref()
            {
                return Err(AppError::git(
                    AppErrorCode::CommitScopeChanged,
                    i18n::repository::commit_scope_changed_error(locale),
                ));
            }
        }
        Ok(live_stage_map)
    }

    pub(super) async fn verify_recorded_commit(
        &self,
        root: &Path,
        record: &ExecutionRecord,
        commit: &CommitResult,
    ) -> Result<()> {
        let reader = &self.inner.reader;
        if !reader
            .run(root, &["merge-base", "--is-ancestor", &commit.oid, "HEAD"])
            .await?
            .success
        {
            return Err(
                "Recorded commit is no longer in current branch history; refresh to verify live status (it will not repeat automatically)"
                    .into(),
            );
        }
        if reader
            .text(root, &["rev-parse", &format!("{}^{{tree}}", commit.oid)])
            .await?
            != record.tree
        {
            return Err(
                "Recorded commit tree does not match authorized scope; stop and inspect Git".into(),
            );
        }
        match &record.parent {
            Some(expected_parent) => {
                if reader
                    .text(root, &["rev-parse", &format!("{}^1", commit.oid)])
                    .await?
                    != *expected_parent
                {
                    return Err(
                        "Recorded commit parent does not match authorized base; stop and inspect Git"
                            .into(),
                    );
                }
            }
            None => {
                if reader
                    .run(
                        root,
                        &[
                            "rev-parse",
                            "--verify",
                            "--quiet",
                            &format!("{}^", commit.oid),
                        ],
                    )
                    .await?
                    .success
                {
                    return Err(
                        "Recorded initial commit unexpectedly has a parent; stop and inspect Git"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}
