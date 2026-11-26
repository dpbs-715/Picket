//! Controlled local commit coordinator.
//! Freezes and re-verifies selected per-file scopes in Rust, runs real Git hooks without
//! `--no-verify` or `hooksPath` overrides, aborts before writing if hooks mutate content,
//! reconciles only selected paths in `.git/index`, and keeps unstaged/unselected changes intact.

mod guards;
mod hooks;
mod index;
mod registration;

pub(in crate::repository) use guards::{hidden_index_flag_paths, is_sparse_checkout_enabled};
pub(in crate::repository) use registration::{
    CommitRegistration, gitlink_entries_from_stage, stage_entries_by_path,
};

use crate::{
    i18n::{self, LanguageKind},
    repository::{
        RepositoryStore, Result, parse,
        types::{CommitRequest, CommitResult, CommitSelection, RepositorySnapshot},
        validate_request_id_with_locale,
    },
};
use guards::resolve_authorized_selections;
use index::{IndexLock, build_isolated_commit_tree, reconcile_real_index};
use registration::{AuthorizedFile, output_detail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
use tempfile::NamedTempFile;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExecutionRecord {
    pub(super) repository_id: String,
    pub(super) worktree_id: String,
    pub(super) parent: Option<String>,
    pub(super) branch: String,
    pub(super) tree: String,
    pub(super) paths: Vec<String>,
    pub(super) selections: Vec<CommitSelection>,
    pub(super) raw_message: String,
    pub(super) commit: Option<CommitResult>,
}

fn validate_commit_request(request: &CommitRequest, locale: LanguageKind) -> Result<()> {
    validate_request_id_with_locale(&request.request_id, locale)?;
    if request.push {
        return Err(i18n::repository::commit_local_only_no_push_error(locale).into());
    }
    if request.message.trim().is_empty() {
        return Err(i18n::repository::commit_message_required_error(locale).into());
    }
    if request.message.contains('\0') {
        return Err(
            "Commit message cannot contain NUL bytes; current draft and selection were preserved"
                .into(),
        );
    }
    if request.message.len() > 1024 * 1024 {
        return Err(
            "Commit message exceeds the maximum length; current draft and selection were preserved"
                .into(),
        );
    }
    if request.selections.is_empty() {
        return Err(i18n::repository::commit_selection_required_error(locale).into());
    }
    let mut seen_ids = HashSet::with_capacity(request.selections.len());
    for selection in &request.selections {
        if selection.file_id.is_empty()
            || selection.commit_token.is_empty()
            || !seen_ids.insert(selection.file_id.as_str())
        {
            return Err(
                "Commit scope contains invalid or duplicate file entries; refresh and try again (draft and selection were preserved)"
                    .into(),
            );
        }
    }
    Ok(())
}

impl RepositoryStore {
    pub(crate) async fn commit(
        &self,
        request: &CommitRequest,
    ) -> Result<(RepositorySnapshot, CommitResult)> {
        let locale = request.locale;
        validate_commit_request(request, locale)?;

        let _guard = self
            .repository_gate(&request.target.repository_id)?
            .lock_owned()
            .await;
        let _write_guard = self.track_write();
        let (registered, root) = self
            .registered_worktree_root(&request.target, locale)
            .await?;
        let reader = self.inner.reader.with_locale(locale);
        let git_dir = fs::canonicalize(
            reader
                .text(&root, &["rev-parse", "--path-format=absolute", "--git-dir"])
                .await?,
        )
        .map_err(|_| "Cannot access worktree Git metadata directory")?;
        let journal_dir = git_dir.join("picket").join("commits");
        let record_path = journal_dir.join(format!("{}.json", request.request_id));
        if record_path.exists() {
            let previous: ExecutionRecord = serde_json::from_slice(
                &fs::read(&record_path).map_err(|_| "Cannot read commit execution record")?,
            )
            .map_err(
                |_| "Commit execution record is corrupted; inspect actual Git history first",
            )?;
            if previous.repository_id != request.target.repository_id
                || previous.worktree_id != request.target.worktree_id
                || previous.selections != request.selections
                || previous.raw_message != request.message
            {
                return Err(i18n::repository::commit_request_id_reused_error(locale).into());
            }
            let commit = previous
                .commit
                .clone()
                .ok_or_else(|| i18n::repository::commit_interrupted_record_error(locale))?;
            self.verify_recorded_commit(&root, &previous, &commit)
                .await?;
            let snapshot = self.read_repository_locked(&root, locale).await?;
            return Ok((snapshot, commit));
        }
        if !self
            .inner
            .commit_requests
            .lock()
            .map_err(|_| "Commit request registry is unavailable")?
            .insert(request.request_id.clone())
        {
            return Err(
                "This commit request was already accepted and has not finished; refresh to verify live status instead of repeating"
                    .into(),
            );
        }

        let (commit_state, selected_files) = resolve_authorized_selections(
            &registered,
            &request.target,
            &request.selections,
            locale,
        )?;
        let all_unit_paths_set: BTreeSet<String> = selected_files
            .iter()
            .flat_map(AuthorizedFile::unit_paths)
            .collect();

        self.ensure_commit_write_allowed(&root, &all_unit_paths_set, locale)
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

        let initial_live_state = self.branch_state(&root, locale).await?;
        self.verify_live_selected_scope(
            commit_state,
            &selected_files,
            &initial_live_state,
            locale,
        )?;

        let worktree_reader = reader.for_worktree(&root).await?;
        let temp =
            tempfile::tempdir().map_err(|_| "Cannot create temporary directory for commit")?;
        let selected_index = temp.path().join("selected.index");
        let tree = build_isolated_commit_tree(
            &reader,
            &worktree_reader,
            &root,
            commit_state,
            &selected_files,
            &unstaged_paths,
            &initial_live_state.index,
            &selected_index,
            locale,
        )
        .await?;

        let index_path = PathBuf::from(
            reader
                .text(
                    &root,
                    &["rev-parse", "--path-format=absolute", "--git-path", "index"],
                )
                .await?,
        );
        self.ensure_commit_write_allowed(&root, &all_unit_paths_set, locale)
            .await?;
        let mut index_lock = IndexLock::acquire(&index_path, locale)?;
        let pre_hook_state = self.branch_state(&root, locale).await?;
        let pre_stage_map = self.verify_live_selected_scope(
            commit_state,
            &selected_files,
            &pre_hook_state,
            locale,
        )?;

        let msg_arg = self
            .run_commit_hooks_and_verify(
                &reader,
                &worktree_reader,
                &root,
                temp.path(),
                &selected_index,
                commit_state,
                &tree,
                &request.message,
                &pre_hook_state,
                locale,
            )
            .await?;

        let all_unit_paths: Vec<String> = all_unit_paths_set.iter().cloned().collect();
        fs::create_dir_all(&journal_dir)
            .map_err(|_| "Cannot initialize commit coordination journal directory")?;
        let mut record = ExecutionRecord {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            parent: commit_state.head.clone(),
            branch: commit_state.branch.clone(),
            tree: tree.clone(),
            paths: all_unit_paths.clone(),
            selections: request.selections.clone(),
            raw_message: request.message.clone(),
            commit: None,
        };
        let mut record_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&record_path)
            .map_err(|_| {
                "Commit execution record already exists and will not repeat; refresh to verify live status"
            })?;
        record_file
            .write_all(&serde_json::to_vec(&record).map_err(|error| error.to_string())?)
            .map_err(|_| "Cannot write commit execution record")?;
        record_file
            .sync_all()
            .map_err(|_| "Cannot sync commit execution record")?;
        drop(record_file);

        let sign_config = reader
            .write(&root, &["config", "--type=bool", "--get", "commit.gpgSign"])
            .await?;
        let gpg_sign = sign_config.success
            && parse::utf8_with_locale(&sign_config.bytes, locale)?.trim() == "true";
        let mut commit_args = vec!["commit-tree", tree.as_str()];
        if let Some(parent) = commit_state.head.as_deref() {
            commit_args.extend(["-p", parent]);
        }
        if gpg_sign {
            commit_args.push("-S");
        }
        commit_args.extend(["-F", &msg_arg]);
        let created = match reader.write(&root, &commit_args).await {
            Ok(output) => output,
            Err(error) => {
                let _ = fs::remove_file(&record_path);
                return Err(error);
            }
        };
        if !created.success {
            let _ = fs::remove_file(&record_path);
            let detail = output_detail(&created);
            let lower = detail.to_ascii_lowercase();
            if lower.contains("ident")
                || lower.contains("user.name")
                || lower.contains("user.email")
                || lower.contains("email address")
            {
                return Err(i18n::repository::commit_missing_identity_error(
                    locale, &detail,
                ));
            }
            return Err(i18n::repository::commit_object_failed_error(
                locale, &detail,
            ));
        }
        let commit_oid = parse::utf8_with_locale(&created.bytes, locale)?
            .trim()
            .to_owned();
        if commit_oid.is_empty()
            || reader
                .text(&root, &["rev-parse", &format!("{commit_oid}^{{tree}}")])
                .await?
                != tree
        {
            let _ = fs::remove_file(&record_path);
            return Err(
                "Created commit object tree did not match authorized scope; aborted updating branch ref (draft and selection were preserved)"
                    .into(),
            );
        }
        let short_oid = reader
            .text(&root, &["rev-parse", "--short", &commit_oid])
            .await?;
        let title = reader
            .text(&root, &["show", "-s", "--format=%s", &commit_oid])
            .await?;
        let reflog = if commit_state.head.is_some() {
            format!("commit: {title}")
        } else {
            format!("commit (initial): {title}")
        };
        let expected_old = commit_state.head.as_deref().unwrap_or("");
        let updated = reader
            .write(
                &root,
                &[
                    "update-ref",
                    "-m",
                    &reflog,
                    "HEAD",
                    &commit_oid,
                    expected_old,
                ],
            )
            .await?;
        if !updated.success {
            let current_head = reader
                .run(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])
                .await?
                .bytes;
            let unchanged = match commit_state.head.as_deref() {
                Some(parent) => parse::utf8_with_locale(&current_head, locale)?.trim() == parent,
                None => current_head.is_empty(),
            };
            if unchanged {
                let _ = fs::remove_file(&record_path);
            }
            return Err(i18n::repository::commit_update_ref_failed_error(
                locale,
                &output_detail(&updated),
            ));
        }

        let commit_result = CommitResult {
            oid: commit_oid.clone(),
            short_oid: short_oid.clone(),
            title,
            branch: commit_state.branch.clone(),
            parent: commit_state.head.clone(),
        };
        let reconcile = async {
            reconcile_real_index(
                &worktree_reader,
                &root,
                temp.path(),
                &index_path,
                &mut index_lock,
                &commit_oid,
                &all_unit_paths,
            )
            .await?;
            record.commit = Some(commit_result.clone());
            let mut completed = NamedTempFile::new_in(&journal_dir)
                .map_err(|_| "Cannot create completed commit journal file")?;
            completed
                .write_all(&serde_json::to_vec(&record).map_err(|error| error.to_string())?)
                .map_err(|_| "Cannot write completed commit journal file")?;
            completed
                .as_file()
                .sync_all()
                .map_err(|_| "Cannot sync completed commit journal file")?;
            completed
                .persist(&record_path)
                .map_err(|_| "Cannot persist completed commit journal file")?;
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = reconcile {
            return Err(i18n::repository::commit_reconcile_failed_error(
                locale, &short_oid, &error,
            ));
        }

        if reader.text(&root, &["rev-parse", "HEAD"]).await? != commit_oid
            || reader.text(&root, &["rev-parse", "HEAD^{tree}"]).await? != tree
        {
            return Err(i18n::repository::commit_post_verify_tree_error(
                locale, &short_oid,
            ));
        }
        for (path, expected_hash) in &pre_hook_state.working {
            if &self.working_file(&root, path)? != expected_hash {
                return Err(i18n::repository::commit_post_verify_working_error(
                    locale, &short_oid, path,
                ));
            }
        }
        let after_stage_map =
            stage_entries_by_path(&reader.bytes(&root, &["ls-files", "--stage", "-z"]).await?)?;
        let pre_unselected: BTreeMap<_, _> = pre_stage_map
            .iter()
            .filter(|(path, _)| !all_unit_paths_set.contains(*path))
            .collect();
        let after_unselected: BTreeMap<_, _> = after_stage_map
            .iter()
            .filter(|(path, _)| !all_unit_paths_set.contains(*path))
            .collect();
        if pre_unselected != after_unselected {
            return Err(i18n::repository::commit_post_verify_unselected_error(
                locale, &short_oid,
            ));
        }

        let _ = reader
            .run_hook(
                &root,
                &["hook", "run", "--ignore-missing", "post-commit"],
                None,
            )
            .await;

        let snapshot = self
            .read_repository_locked(&root, locale)
            .await
            .map_err(|error| {
                i18n::repository::commit_post_read_failed_error(locale, &short_oid, &error)
            })?;
        Ok((snapshot, commit_result))
    }
}
