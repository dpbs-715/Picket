use super::{
    index::write_tree,
    registration::{CommitRegistration, output_detail},
};
use crate::{
    i18n::{self, LanguageKind},
    repository::{
        RepositoryStore, Result,
        branches::BranchState,
        display_with_locale, parse,
        runner::{GitOutput, GitReader},
    },
};
use std::{fs, path::Path};

impl RepositoryStore {
    async fn hook_failure_error<T>(
        &self,
        root: &Path,
        hook_name: &str,
        output: &GitOutput,
        pre_hook_state: &BranchState,
        locale: LanguageKind,
    ) -> Result<T> {
        let detail = output_detail(output);
        let mutated = match self.branch_state(root, locale).await {
            Ok(current) => {
                current.status != pre_hook_state.status
                    || current.index != pre_hook_state.index
                    || current.working != pre_hook_state.working
            }
            Err(_) => true,
        };
        Err(i18n::repository::commit_hook_failed_error(
            locale, hook_name, &detail, mutated,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_commit_hooks_and_verify(
        &self,
        reader: &GitReader,
        worktree_reader: &GitReader,
        root: &Path,
        temp_dir: &Path,
        selected_index: &Path,
        commit_state: &CommitRegistration,
        tree: &str,
        raw_message: &str,
        pre_hook_state: &BranchState,
        locale: LanguageKind,
    ) -> Result<String> {
        let pre_commit = reader
            .run_hook(
                root,
                &["hook", "run", "--ignore-missing", "pre-commit"],
                Some(selected_index),
            )
            .await?;
        if !pre_commit.success {
            return self
                .hook_failure_error(root, "pre-commit", &pre_commit, pre_hook_state, locale)
                .await;
        }
        let cleaned = reader
            .write_with(root, &["stripspace"], None, Some(raw_message.as_bytes()))
            .await?;
        if !cleaned.success {
            return Err(
                "Failed to normalize commit message; no commit was created (draft and selection were preserved)"
                    .into(),
            );
        }
        let msg_path = temp_dir.join("COMMIT_EDITMSG");
        fs::write(&msg_path, &cleaned.bytes)
            .map_err(|_| "Cannot write temporary commit message file")?;
        let msg_arg = display_with_locale(&msg_path, locale)?;
        let prepare_msg = reader
            .run_hook(
                root,
                &[
                    "hook",
                    "run",
                    "--ignore-missing",
                    "prepare-commit-msg",
                    "--",
                    &msg_arg,
                    "message",
                ],
                Some(selected_index),
            )
            .await?;
        if !prepare_msg.success {
            return self
                .hook_failure_error(
                    root,
                    "prepare-commit-msg",
                    &prepare_msg,
                    pre_hook_state,
                    locale,
                )
                .await;
        }
        let commit_msg = reader
            .run_hook(
                root,
                &[
                    "hook",
                    "run",
                    "--ignore-missing",
                    "commit-msg",
                    "--",
                    &msg_arg,
                ],
                Some(selected_index),
            )
            .await?;
        if !commit_msg.success {
            return self
                .hook_failure_error(root, "commit-msg", &commit_msg, pre_hook_state, locale)
                .await;
        }
        let hook_msg_bytes =
            fs::read(&msg_path).map_err(|_| "Cannot read commit message after hook validation")?;
        let final_cleaned = reader
            .write_with(root, &["stripspace"], None, Some(&hook_msg_bytes))
            .await?;
        if !final_cleaned.success
            || parse::utf8_with_locale(&final_cleaned.bytes, locale)?
                .trim()
                .is_empty()
        {
            return Err(
                "Commit message became empty after Git hooks; no commit was created (draft and selection were preserved)"
                    .into(),
            );
        }
        fs::write(&msg_path, &final_cleaned.bytes)
            .map_err(|_| "Cannot update temporary commit message file")?;

        let post_hook_tree = write_tree(worktree_reader, root, selected_index).await?;
        let post_hook_state = self.branch_state(root, locale).await?;
        if post_hook_tree != tree
            || post_hook_state.status != pre_hook_state.status
            || post_hook_state.index != pre_hook_state.index
            || post_hook_state.working != pre_hook_state.working
        {
            return Err(i18n::repository::commit_hook_mutated_content_error(locale).into());
        }
        let head_lock = reader
            .text(
                root,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "HEAD.lock",
                ],
            )
            .await?;
        if fs::symlink_metadata(&head_lock).is_ok() {
            return Err(i18n::repository::commit_lock_file_error(
                locale,
                "HEAD.lock",
            ));
        }
        let symbolic_head = reader
            .write(root, &["symbolic-ref", "--quiet", "HEAD"])
            .await?;
        let expected_ref = format!("refs/heads/{}", commit_state.branch);
        if !symbolic_head.success
            || parse::utf8_with_locale(&symbolic_head.bytes, locale)?.trim() != expected_ref
        {
            return Err(i18n::repository::commit_scope_changed_error(locale).into());
        }
        Ok(msg_arg)
    }
}
