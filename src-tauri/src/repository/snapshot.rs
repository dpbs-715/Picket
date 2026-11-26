use super::{
    Registration, RepositoryStore, Result, branches, display_with_locale, identity, label, parse,
    types::{
        Branch, ReadTarget, Remote, RemoteBranch, RepositoryInfo, RepositorySnapshot,
        WorktreeSnapshot,
    },
};
use crate::i18n::{self, LanguageKind};
use std::{collections::HashMap, fs, path::Path, sync::atomic::Ordering};

impl RepositoryStore {
    pub async fn open(&self, directory: &Path) -> Result<RepositorySnapshot> {
        self.open_with_locale(directory, LanguageKind::En).await
    }

    pub async fn open_with_locale(
        &self,
        directory: &Path,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        let directory = fs::canonicalize(directory)
            .map_err(|_| i18n::repository::repo_directory_inaccessible(locale).to_string())?;
        if !directory.is_dir() {
            return Err(i18n::repository::repo_must_select_directory(locale).into());
        }
        let dir_name = label(&directory, locale);
        let reader = self.inner.reader.with_locale(locale);
        let toplevel = reader
            .run(&directory, &["rev-parse", "--show-toplevel"])
            .await?;
        let root = if toplevel.success {
            let mut text = String::from_utf8(toplevel.bytes)
                .map_err(|_| i18n::repository::parse_non_utf8_path_error(locale).to_string())?;
            if text.ends_with('\n') {
                text.pop();
            }
            text
        } else {
            if toplevel.code == Some(129) {
                let detail = String::from_utf8_lossy(&toplevel.stderr).trim().to_owned();
                return Err(if detail.is_empty() {
                    "Git invocation failed with usage error (exit 129)".into()
                } else {
                    format!("Git invocation failed (exit 129): {detail}")
                });
            }
            let is_bare = reader
                .text(&directory, &["rev-parse", "--is-bare-repository"])
                .await
                .is_ok_and(|out| out.trim() == "true");
            if is_bare {
                return Err(i18n::repository::open_bare_repo_error(locale, &dir_name));
            }
            return Err(i18n::repository::open_not_git_repo_error(locale, &dir_name));
        };
        let root = fs::canonicalize(root)
            .map_err(|_| "Git working directory is inaccessible".to_string())?;
        self.read_repository(&root, locale).await
    }

    #[cfg(test)]
    pub async fn refresh(&self, target: &ReadTarget) -> Result<RepositorySnapshot> {
        self.refresh_with_locale(target, LanguageKind::En).await
    }

    pub async fn refresh_with_locale(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        let path = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?
            .get(&target.repository_id)
            .and_then(|repository| repository.paths.get(&target.worktree_id))
            .cloned()
            .ok_or_else(|| i18n::repository::repo_not_opened_error(locale))?;
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                &path,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "Cannot access Git common directory")?;
        if identity("repo", &common) != target.repository_id {
            return Err(i18n::repository::repo_identity_changed_error(locale).into());
        }
        let snapshot = self.read_repository(&path, locale).await?;
        if snapshot.repository.id != target.repository_id {
            return Err(i18n::repository::repo_identity_changed_error(locale).into());
        }
        Ok(snapshot)
    }

    async fn read_repository(
        &self,
        root: &Path,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "Cannot access Git common directory")?;
        let _guard = self
            .repository_gate(&identity("repo", &common))?
            .lock_owned()
            .await;
        self.read_repository_locked(root, locale).await
    }

    pub(super) async fn read_repository_locked(
        &self,
        root: &Path,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common)
            .map_err(|_| "Cannot access Git common directory".to_string())?;
        let repository_id = identity("repo", &common);
        let active_id = identity("wt", root);
        let keep_set = self.resolve_keep_set_for_common(&common)?;
        let entries = parse::worktrees(
            &reader
                .bytes(root, &["worktree", "list", "--porcelain", "-z"])
                .await?,
        )?;
        let mut worktrees = Vec::new();
        let mut paths = HashMap::new();
        let mut commit_states = HashMap::new();
        for mut entry in entries.into_iter().filter(|entry| !entry.bare) {
            // Canonicalize live paths to keep symlink-opened and worktree-list
            // identities identical; retain missing paths as visibly unavailable.
            if let Ok(path) = fs::canonicalize(&entry.path) {
                entry.path = path;
            }
            let id = identity("wt", &entry.path);
            let tree = match self
                .read_worktree(&repository_id, &entry, &keep_set, locale)
                .await
            {
                Ok((tree, commit_state)) => {
                    commit_states.insert(id.clone(), commit_state);
                    tree
                }
                Err(error) if id == active_id => return Err(error),
                Err(error) => WorktreeSnapshot {
                    repository_id: repository_id.clone(),
                    worktree_id: id.clone(),
                    label: label(&entry.path, locale),
                    path: display_with_locale(&entry.path, locale)?,
                    branch: entry.branch.clone(),
                    detached: entry.branch == "(detached)",
                    head: entry.head.clone(),
                    status_version: String::new(),
                    branch_token: None,
                    read_error: Some(error.clone()),
                    remote: Remote::Error { reason: error },
                    files: vec![],
                },
            };
            paths.insert(id, entry.path);
            worktrees.push(tree);
        }
        if !worktrees.iter().any(|tree| tree.worktree_id == active_id) {
            return Err("Git did not list the selected worktree; please reopen".into());
        }
        let branch_output = reader
            .bytes(
                root,
                &["for-each-ref", "--format=%(refname)%00", "refs/heads/"],
            )
            .await?;
        let mut branches = Vec::new();
        for record in branch_output.split(|byte| *byte == 0) {
            let record = parse::utf8_with_locale(record, locale)?;
            let Some(name) = record.trim_start_matches('\n').strip_prefix("refs/heads/") else {
                continue;
            };
            branches.push(Branch {
                name: name.into(),
                worktree_id: worktrees
                    .iter()
                    .find(|tree| tree.branch == name && !tree.detached)
                    .map(|tree| tree.worktree_id.clone()),
            });
        }
        for tree in &worktrees {
            if !tree.detached && !branches.iter().any(|branch| branch.name == tree.branch) {
                branches.push(Branch {
                    name: tree.branch.clone(),
                    worktree_id: Some(tree.worktree_id.clone()),
                });
            }
        }
        let configured_remotes = self.list_remotes_unlocked(root).await.unwrap_or_default();
        let remote_output = reader
            .bytes(
                root,
                &[
                    "for-each-ref",
                    "--format=%(symref)%09%(refname)%00",
                    "refs/remotes/",
                ],
            )
            .await?;
        let mut remote_branches = Vec::new();
        for record in remote_output.split(|byte| *byte == 0) {
            let record = parse::utf8_with_locale(record, locale)?;
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                continue;
            }
            let Some((symref, full_ref)) = record.split_once('\t') else {
                continue;
            };
            if !symref.is_empty() {
                continue;
            }
            let Some(short_name) = full_ref.strip_prefix("refs/remotes/") else {
                continue;
            };
            let Some((remote_name, local_name)) =
                branches::split_remote_branch(short_name, &configured_remotes)
            else {
                continue;
            };
            remote_branches.push(RemoteBranch {
                name: short_name.to_owned(),
                remote: remote_name,
                local_name,
            });
        }
        let name = if common.file_name().is_some_and(|name| name == ".git") {
            label(common.parent().unwrap_or(root), locale)
        } else {
            label(&common, locale).trim_end_matches(".git").to_owned()
        };
        let snapshot = RepositorySnapshot {
            repository: RepositoryInfo {
                id: repository_id.clone(),
                name,
                branches,
                remote_branches,
                local_keep_paths: keep_set.iter().cloned().collect(),
            },
            worktrees,
            active_worktree_id: active_id,
            read_sequence: sequence,
        };
        let mut registered = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?;
        if registered
            .get(&repository_id)
            .is_none_or(|previous| previous.sequence < sequence)
        {
            let branch_tokens = snapshot
                .worktrees
                .iter()
                .filter_map(|tree| {
                    tree.branch_token
                        .as_ref()
                        .map(|token| (tree.worktree_id.clone(), token.clone()))
                })
                .collect();
            registered.insert(
                repository_id.clone(),
                Registration {
                    sequence,
                    common_dir: common,
                    primary_root: root.to_path_buf(),
                    paths,
                    branch_tokens,
                    commit_states,
                },
            );
            drop(registered);
            let mut order = self
                .inner
                .opened_order
                .lock()
                .map_err(|_| "Repository order registry is unavailable")?;
            if !order.iter().any(|existing| existing == &repository_id) {
                order.push(repository_id);
            }
        }
        Ok(snapshot)
    }
}
