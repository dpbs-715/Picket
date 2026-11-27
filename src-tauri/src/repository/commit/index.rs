use super::registration::{
    AuthorizedFile, CommitRegistration, gitlink_entries_from_stage, output_detail,
    stage_entries_by_path,
};
use crate::{
    error::{AppError, AppErrorCode, CommitGuardError},
    i18n::{self, LanguageKind},
    repository::{Result, parse, runner::GitReader},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct IndexLock {
    path: PathBuf,
    file: File,
    replaced: bool,
}

impl IndexLock {
    pub(super) fn acquire(index: &Path, locale: LanguageKind) -> Result<Self> {
        let path = index.with_file_name("index.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| i18n::repository::commit_lock_file_error(locale, "index.lock"))?;
        Ok(Self {
            path,
            file,
            replaced: false,
        })
    }

    pub(super) fn replace(&mut self, index: &Path, bytes: &[u8]) -> Result<()> {
        self.file
            .write_all(bytes)
            .map_err(|_| "Failed to write updated staging index".to_string())?;
        self.file
            .sync_all()
            .map_err(|_| "Failed to sync staging index file".to_string())?;
        fs::rename(&self.path, index)
            .map_err(|_| "Failed to atomically replace staging index".to_string())?;
        self.replaced = true;
        Ok(())
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.replaced {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) async fn run_index_checked(
    reader: &GitReader,
    root: &Path,
    args: &[&str],
    index: &Path,
    input: Option<&[u8]>,
    context: &str,
) -> Result<Vec<u8>> {
    let output = reader.run_with_index(root, args, index, input).await?;
    if !output.success {
        return Err(format!(
            "{context} ({}); no commit was created, and draft and selection were preserved",
            output_detail(&output)
        ));
    }
    Ok(output.bytes)
}

pub(super) async fn write_tree(reader: &GitReader, root: &Path, index: &Path) -> Result<String> {
    let bytes = run_index_checked(
        reader,
        root,
        &["write-tree"],
        index,
        None,
        "Failed to build commit tree",
    )
    .await?;
    let tree = parse::utf8(&bytes)?.trim().to_owned();
    if tree.is_empty() {
        return Err("Git did not return a valid tree hash".into());
    }
    Ok(tree)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn build_isolated_commit_tree(
    reader: &GitReader,
    worktree_reader: &GitReader,
    root: &Path,
    commit_state: &CommitRegistration,
    selected_files: &[AuthorizedFile],
    unstaged_paths: &[String],
    initial_live_index: &[u8],
    selected_index: &Path,
    locale: LanguageKind,
) -> std::result::Result<String, AppError> {
    let map_commit_err = |msg: String| AppError::git(AppErrorCode::CommitFailed, msg);
    let baseline_gitlinks = if let Some(parent) = commit_state.head.as_deref() {
        run_index_checked(
            worktree_reader,
            root,
            &["read-tree", parent],
            selected_index,
            None,
            "Failed to initialize temporary commit index",
        )
        .await
        .map_err(map_commit_err)?;
        let base_stage = run_index_checked(
            worktree_reader,
            root,
            &["ls-files", "--stage", "-z"],
            selected_index,
            None,
            "Failed to inspect baseline commit index",
        )
        .await
        .map_err(map_commit_err)?;
        gitlink_entries_from_stage(&base_stage).map_err(map_commit_err)?
    } else {
        run_index_checked(
            worktree_reader,
            root,
            &["read-tree", "--empty"],
            selected_index,
            None,
            "Failed to initialize initial commit index",
        )
        .await
        .map_err(map_commit_err)?;
        BTreeMap::new()
    };
    for batch in unstaged_paths.chunks(128) {
        let mut args = vec!["add", "--all", "--"];
        args.extend(batch.iter().map(String::as_str));
        run_index_checked(
            worktree_reader,
            root,
            &args,
            selected_index,
            None,
            "Failed to stage selected unstaged paths in temporary index",
        )
        .await
        .map_err(map_commit_err)?;
    }
    let mut staged_unit_paths = BTreeSet::new();
    let mut staged_index_info = Vec::new();
    let mut staged_by_unit_path = BTreeMap::<String, Vec<u8>>::new();
    let initial_stage_map = stage_entries_by_path(initial_live_index).map_err(map_commit_err)?;
    for file in selected_files.iter().filter(|file| file.staged) {
        for path in file.unit_paths() {
            if let Some(records) = initial_stage_map.get(&path) {
                staged_by_unit_path.insert(path.clone(), records.clone());
            }
            staged_unit_paths.insert(path);
        }
    }
    if !staged_unit_paths.is_empty() {
        let mut remove_stdin = Vec::new();
        for path in &staged_unit_paths {
            remove_stdin.extend_from_slice(path.as_bytes());
            remove_stdin.push(0);
        }
        run_index_checked(
            worktree_reader,
            root,
            &["update-index", "--force-remove", "-z", "--stdin"],
            selected_index,
            Some(&remove_stdin),
            "Failed to clear old paths in temporary index",
        )
        .await
        .map_err(map_commit_err)?;
        for records in staged_by_unit_path.values() {
            staged_index_info.extend_from_slice(records);
        }
        if !staged_index_info.is_empty() {
            run_index_checked(
                worktree_reader,
                root,
                &["update-index", "-z", "--index-info"],
                selected_index,
                Some(&staged_index_info),
                "Failed to apply selected staged entries",
            )
            .await
            .map_err(map_commit_err)?;
        }
    }
    let selected_stage = run_index_checked(
        worktree_reader,
        root,
        &["ls-files", "--stage", "-z"],
        selected_index,
        None,
        "Failed to inspect temporary commit index",
    )
    .await
    .map_err(map_commit_err)?;
    let selected_gitlinks = gitlink_entries_from_stage(&selected_stage).map_err(map_commit_err)?;
    if selected_gitlinks != baseline_gitlinks {
        return Err(CommitGuardError::SubmoduleUnsupported(
            i18n::repository::commit_submodule_error(locale).into(),
        )
        .into());
    }
    let tree = write_tree(worktree_reader, root, selected_index)
        .await
        .map_err(map_commit_err)?;
    if let Some(parent) = commit_state.head.as_deref() {
        let base_tree = reader
            .text(root, &["rev-parse", &format!("{parent}^{{tree}}")])
            .await
            .map_err(map_commit_err)?;
        if tree == base_tree {
            return Err(AppError::git(
                AppErrorCode::CommitFailed,
                "Selected scope has no effective changes relative to current branch; skipped empty commit (draft and selection were preserved)",
            ));
        }
    } else if selected_stage.is_empty() {
        return Err(AppError::git(
            AppErrorCode::CommitFailed,
            "Selected scope has no files for initial commit; skipped empty commit (draft and selection were preserved)",
        ));
    }
    Ok(tree)
}

pub(super) async fn reconcile_real_index(
    worktree_reader: &GitReader,
    root: &Path,
    temp_dir: &Path,
    index_path: &Path,
    index_lock: &mut IndexLock,
    commit_oid: &str,
    all_unit_paths: &[String],
) -> Result<()> {
    let remaining_index = temp_dir.join("remaining.index");
    if index_path.exists() {
        fs::copy(index_path, &remaining_index).map_err(|_| "Cannot copy current staging index")?;
    } else {
        run_index_checked(
            worktree_reader,
            root,
            &["read-tree", "--empty"],
            &remaining_index,
            None,
            "Failed to initialize remaining staging index",
        )
        .await?;
    }
    for batch in all_unit_paths.chunks(128) {
        let mut reset_args = vec!["reset", "--quiet", commit_oid, "--"];
        reset_args.extend(batch.iter().map(String::as_str));
        run_index_checked(
            worktree_reader,
            root,
            &reset_args,
            &remaining_index,
            None,
            "Failed to reconcile committed paths in staging index",
        )
        .await?;
    }
    let remaining_bytes =
        fs::read(&remaining_index).map_err(|_| "Cannot read updated staging index")?;
    index_lock.replace(index_path, &remaining_bytes)
}
