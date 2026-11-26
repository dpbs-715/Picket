//! Branch and remote-tracking checkout guards.
//! Checks locks, active operations, target-scoped hidden index flags, submodule parity,
//! checkout-triggered hooks (`post-checkout` / `reference-transaction`), and diff-scoped
//! external Git filters without skipping user hooks or executing external filter drivers.

use super::BranchState;
use crate::{
    i18n::{self, LanguageKind},
    repository::{RepositoryStore, Result, commit, parse, runner::GitReader},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

const CHECKOUT_HOOK_NAMES: [&str; 2] = ["post-checkout", "reference-transaction"];

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Resolves symlinks on the deepest existing ancestor of `path` and re-appends
/// any not-yet-existing suffix components so paths inside a canonicalized worktree
/// root compare reliably (e.g. `/var/...` vs `/private/var/...` on macOS).
fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let normalized = normalize_lexical_path(path);
    let mut current = normalized.as_path();
    let mut tail = Vec::new();
    loop {
        if fs::symlink_metadata(current).is_ok() {
            let mut resolved = fs::canonicalize(current)
                .map_err(|_| "Cannot resolve Git hooks path; no branch write was performed")?;
            for segment in tail.into_iter().rev() {
                resolved.push(segment);
            }
            return Ok(resolved);
        }
        let Some(name) = current.file_name() else {
            return Ok(normalized);
        };
        tail.push(name.to_os_string());
        let Some(parent) = current.parent() else {
            return Ok(normalized);
        };
        current = parent;
    }
}

fn check_hook_file_executable(
    path: &Path,
    locale: LanguageKind,
    custom_hooks_path: bool,
) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                return Err(if custom_hooks_path {
                    i18n::repository::branch_custom_hooks_path_error(locale).into()
                } else {
                    i18n::repository::branch_executable_hook_error(locale).into()
                });
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(if custom_hooks_path {
            i18n::repository::branch_custom_hooks_path_error(locale).into()
        } else {
            "Cannot inspect Git hook; no branch write was performed".into()
        }),
    }
}

async fn gitlinks_from_tree(
    reader: &GitReader,
    root: &Path,
    rev: &str,
) -> Result<BTreeMap<String, String>> {
    let bytes = reader.bytes(root, &["ls-tree", "-r", "-z", rev]).await?;
    let mut entries = BTreeMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| record.starts_with(b"160000 "))
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("Git tree entry format mismatch".into());
        };
        let header = parse::utf8(&record[..tab])?;
        let mut parts = header.split_whitespace();
        let mode = parts.next();
        let kind = parts.next();
        let oid = parts.next();
        if mode != Some("160000")
            || kind != Some("commit")
            || oid.is_none()
            || parts.next().is_some()
        {
            return Err("Git tree header format mismatch".into());
        }
        let path = parse::utf8(&record[tab + 1..])?;
        parse::relative(&path)?;
        entries.insert(path, oid.unwrap().to_owned());
    }
    Ok(entries)
}

async fn diff_paths_between_commits(
    reader: &GitReader,
    root: &Path,
    head: &str,
    target_head: &str,
    locale: LanguageKind,
) -> Result<BTreeSet<String>> {
    if head == target_head {
        return Ok(BTreeSet::new());
    }
    let bytes = reader
        .bytes(
            root,
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                head,
                target_head,
            ],
        )
        .await?;
    let mut paths = BTreeSet::new();
    for record in bytes.split(|byte| *byte == 0).filter(|r| !r.is_empty()) {
        let path = parse::utf8_with_locale(record, locale)?;
        parse::relative(&path)?;
        paths.insert(path);
    }
    Ok(paths)
}

impl RepositoryStore {
    pub(super) async fn ensure_branch_base_write_allowed(
        &self,
        root: &Path,
        locale: LanguageKind,
    ) -> Result<()> {
        let reader = self.inner.reader.with_locale(locale);
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "BISECT_LOG",
            "index.lock",
            "HEAD.lock",
        ] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .await?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(i18n::repository::branch_in_progress_or_lock_error(
                    locale, marker,
                ));
            }
        }
        if !reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await?
            .is_empty()
        {
            return Err(i18n::repository::commit_unresolved_conflict_error(locale).into());
        }
        Ok(())
    }

    pub(super) async fn ensure_branch_target_write_allowed(
        &self,
        root: &Path,
        head: &str,
        target_head: &str,
        before: &BranchState,
        locale: LanguageKind,
    ) -> Result<()> {
        let reader = self.inner.reader.with_locale(locale);
        let diff_paths =
            diff_paths_between_commits(&reader, root, head, target_head, locale).await?;

        // 1. Sparse-checkout & target-scoped hidden index flags (`assume-unchanged` / `skip-worktree`):
        if commit::is_sparse_checkout_enabled(&reader, root, locale).await? {
            return Err(i18n::repository::commit_sparse_or_hidden_flag_error(locale).into());
        }
        let hidden_paths = commit::hidden_index_flag_paths(&reader, root, locale).await?;
        if hidden_paths.iter().any(|path| path.ends_with('/'))
            || diff_paths.iter().any(|path| hidden_paths.contains(path))
        {
            return Err(i18n::repository::commit_sparse_or_hidden_flag_error(locale).into());
        }

        // 2. Submodules: allow switching when all submodule pointers match across index, HEAD, and target:
        let parsed_before = parse::status_with_locale(&before.status, locale)?;
        if parsed_before
            .entries
            .iter()
            .any(|entry| entry.mode == "160000")
        {
            return Err(i18n::repository::branch_submodule_diff_error(locale).into());
        }
        let index_gitlinks = commit::gitlink_entries_from_stage(&before.index)?;
        let head_gitlinks = gitlinks_from_tree(&reader, root, head).await?;
        let target_gitlinks = if target_head == head {
            head_gitlinks.clone()
        } else {
            gitlinks_from_tree(&reader, root, target_head).await?
        };
        if index_gitlinks != head_gitlinks || head_gitlinks != target_gitlinks {
            return Err(i18n::repository::branch_submodule_diff_error(locale).into());
        }

        // 3. Checkout-triggered Git hooks (`post-checkout`, `reference-transaction`) on disk and in target tree:
        self.ensure_checkout_hooks_allowed(&reader, root, target_head, locale)
            .await?;

        // 4. External Git filters scoped to paths differing between HEAD and target plus any dirty paths:
        let mut filter_candidates = diff_paths;
        for entry in &parsed_before.entries {
            filter_candidates.insert(entry.path.clone());
            if let Some(prev) = &entry.previous {
                filter_candidates.insert(prev.clone());
            }
        }
        self.ensure_checkout_filters_allowed(root, target_head, &filter_candidates, locale)
            .await?;

        Ok(())
    }

    async fn ensure_checkout_hooks_allowed(
        &self,
        reader: &GitReader,
        root: &Path,
        target_head: &str,
        locale: LanguageKind,
    ) -> Result<()> {
        // Query without the read runner's hooksPath override. The write runner
        // preserves this configuration too; executable checkout hooks are refused, not skipped.
        let configured_hooks = reader
            .write(root, &["config", "--path", "--get", "core.hooksPath"])
            .await?;
        if configured_hooks.success {
            let raw = parse::utf8_with_locale(&configured_hooks.bytes, locale)?;
            let raw = raw.strip_suffix('\n').unwrap_or(&raw);
            if raw.is_empty() {
                return Err(i18n::repository::branch_custom_hooks_path_error(locale).into());
            }
            let raw_path = Path::new(raw);
            let joined = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                root.join(raw_path)
            };
            let resolved = canonicalize_existing_prefix(&joined)
                .map_err(|_| i18n::repository::branch_custom_hooks_path_error(locale).to_owned())?;

            // Fail closed if hooks path exists on disk as a symlink or non-directory.
            if let Ok(meta) = fs::symlink_metadata(&resolved)
                && (meta.file_type().is_symlink() || !meta.is_dir())
            {
                return Err(i18n::repository::branch_custom_hooks_path_error(locale).into());
            }

            // Check live disk for checkout-triggered hooks.
            for hook_name in CHECKOUT_HOOK_NAMES {
                check_hook_file_executable(&resolved.join(hook_name), locale, true)?;
            }

            // If `core.hooksPath` points inside the worktree, also verify `target_head` does not
            // introduce an executable or symlinked checkout hook (or symlinked parent directory).
            if let Ok(rel_hooks) = resolved.strip_prefix(root) {
                let mut rel_parts: Vec<String> = Vec::new();
                for comp in rel_hooks.components() {
                    match comp {
                        Component::Normal(seg) => {
                            let seg = seg.to_str().ok_or_else(|| {
                                i18n::repository::branch_custom_hooks_path_error(locale).to_owned()
                            })?;
                            rel_parts.push(seg.to_owned());
                        }
                        _ => {
                            return Err(
                                i18n::repository::branch_custom_hooks_path_error(locale).into()
                            );
                        }
                    }
                }
                let mut prefix = String::new();
                for part in &rel_parts {
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(part);
                    let prefix_entries = reader
                        .bytes(root, &["ls-tree", "-z", target_head, "--", &prefix])
                        .await?;
                    for record in prefix_entries
                        .split(|byte| *byte == 0)
                        .filter(|r| !r.is_empty())
                    {
                        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
                            return Err(
                                i18n::repository::branch_custom_hooks_path_error(locale).into()
                            );
                        };
                        let header = parse::utf8_with_locale(&record[..tab], locale)?;
                        let mode = header.split_whitespace().next().unwrap_or("");
                        if mode == "120000" {
                            return Err(
                                i18n::repository::branch_custom_hooks_path_error(locale).into()
                            );
                        }
                    }
                }
                let hook_candidates: Vec<String> = CHECKOUT_HOOK_NAMES
                    .iter()
                    .map(|hook_name| {
                        if prefix.is_empty() {
                            (*hook_name).to_owned()
                        } else {
                            format!("{prefix}/{hook_name}")
                        }
                    })
                    .collect();
                let mut ls_args = vec!["ls-tree", "-z", target_head, "--"];
                ls_args.extend(hook_candidates.iter().map(String::as_str));
                let tree_entries = reader.bytes(root, &ls_args).await?;
                for record in tree_entries
                    .split(|byte| *byte == 0)
                    .filter(|r| !r.is_empty())
                {
                    let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
                        return Err(i18n::repository::branch_custom_hooks_path_error(locale).into());
                    };
                    let header = parse::utf8_with_locale(&record[..tab], locale)?;
                    let mode = header.split_whitespace().next().unwrap_or("");
                    if mode == "100755" || mode == "120000" {
                        return Err(i18n::repository::branch_custom_hooks_path_error(locale).into());
                    }
                }
            }
            return Ok(());
        } else if configured_hooks.code != Some(1) {
            return Err("Cannot verify Git core.hooksPath; no branch write was performed".into());
        }

        let hooks = reader
            .write(
                root,
                &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
            )
            .await?;
        if !hooks.success {
            return Err("Cannot verify Git hooks directory; no branch write was performed".into());
        }
        let hooks = parse::utf8_with_locale(&hooks.bytes, locale)?;
        let hooks_dir = PathBuf::from(hooks.strip_suffix('\n').unwrap_or(&hooks));
        for hook_name in CHECKOUT_HOOK_NAMES {
            check_hook_file_executable(&hooks_dir.join(hook_name), locale, false)?;
        }
        Ok(())
    }

    async fn ensure_checkout_filters_allowed(
        &self,
        root: &Path,
        target_head: &str,
        candidate_paths: &BTreeSet<String>,
        locale: LanguageKind,
    ) -> Result<()> {
        if candidate_paths.is_empty() {
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
                    r"^filter\..*\.(clean|smudge|process)$",
                    ".+",
                ],
            )
            .await?;
        if filters.code == Some(1) {
            return Ok(());
        }
        if !filters.success {
            return Err("Cannot inspect Git file filters; no branch write was performed".into());
        }
        let mut drivers = BTreeSet::new();
        for key in filters
            .bytes
            .split(|byte| *byte == 0)
            .filter(|key| !key.is_empty())
        {
            let key = parse::utf8_with_locale(key, locale)?;
            let (driver, _) = key
                .strip_prefix("filter.")
                .ok_or("Cannot parse Git filter driver name")?
                .rsplit_once('.')
                .ok_or("Cannot parse Git filter driver name")?;
            drivers.insert(driver.to_owned());
        }
        let paths: Vec<String> = candidate_paths.iter().cloned().collect();
        // Instead of `git check-attr --source=<tree>` (added in Git 2.40, unsupported on
        // Apple Git 2.39.x shipped with macOS 14/15), load `target_head` into a disposable
        // temporary index and inspect it with `git check-attr --cached`.
        let target_index_dir =
            tempfile::tempdir().map_err(|_| "Cannot create temporary Git attribute index")?;
        let target_index = target_index_dir.path().join("target.index");
        let read_tree = reader
            .run_with_index(root, &["read-tree", target_head], &target_index, None)
            .await?;
        if !read_tree.success {
            return Err(
                "Cannot inspect target branch Git attributes; no branch write was performed".into(),
            );
        }
        let sources: [(bool, Option<&Path>); 3] = [
            (false, None),
            (true, None),
            (true, Some(target_index.as_path())),
        ];
        for (cached, index_override) in sources {
            // Keep argument batches below macOS's command-line size limit.
            for batch in paths.chunks(128) {
                let mut args = vec!["check-attr", "-z"];
                if cached {
                    args.push("--cached");
                }
                args.extend(["filter", "--"]);
                args.extend(batch.iter().map(String::as_str));
                let attributes = match index_override {
                    Some(index_path) => {
                        let output = reader.run_with_index(root, &args, index_path, None).await?;
                        if !output.success {
                            return Err("Git check-attr read failed; check that the repository is accessible (draft and selection were preserved)".into());
                        }
                        output.bytes
                    }
                    None => reader.bytes(root, &args).await?,
                };
                let fields: Vec<_> = attributes.split(|byte| *byte == 0).collect();
                for record in fields.chunks(3).filter(|record| record.len() == 3) {
                    if drivers.contains(&parse::utf8_with_locale(record[2], locale)?) {
                        return Err(i18n::repository::branch_external_filter_error(locale).into());
                    }
                }
            }
        }
        Ok(())
    }
}
