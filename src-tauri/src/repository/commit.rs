//! Controlled local commit coordinator.
//! Freezes and re-verifies selected per-file scopes in Rust, runs real Git hooks without
//! `--no-verify` or `hooksPath` overrides, aborts before writing if hooks mutate content,
//! reconciles only selected paths in `.git/index`, and keeps unstaged/unselected changes intact.
use super::{branches::BranchState, parse::StatusEntry, runner::GitOutput, *};
use crate::i18n::{self, LanguageKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write,
};
use tempfile::NamedTempFile;

#[derive(Clone)]
pub(super) struct CommitRegistration {
    pub branch: String,
    pub detached: bool,
    pub head: Option<String>,
    pub files: HashMap<String, AuthorizedFile>,
}

#[derive(Clone)]
pub(super) struct AuthorizedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: bool,
    pub range_kind: String,
    pub conflicted: bool,
    pub stage_entries: Vec<u8>,
    pub working_hash: Option<String>,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionRecord {
    repository_id: String,
    worktree_id: String,
    parent: Option<String>,
    branch: String,
    tree: String,
    paths: Vec<String>,
    selections: Vec<CommitSelection>,
    raw_message: String,
    commit: Option<CommitResult>,
}

struct IndexLock {
    path: PathBuf,
    file: File,
    replaced: bool,
}

impl IndexLock {
    fn acquire(index: &Path, locale: LanguageKind) -> Result<Self> {
        let path = index.with_file_name("index.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| i18n::commit_lock_file_error(locale, "index.lock"))?;
        Ok(Self {
            path,
            file,
            replaced: false,
        })
    }

    fn replace(&mut self, index: &Path, bytes: &[u8]) -> Result<()> {
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

fn ensure_safe_repo_path(path: &str) -> Result<()> {
    parse::relative(path)?;
    if path
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err("Commit scope contains an unsafe .git path".into());
    }
    Ok(())
}

pub(super) fn stage_entries_by_path(index_bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    for record in index_bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("Git index entry format mismatch".into());
        };
        let header = parse::utf8(&record[..tab])?;
        if header.split_whitespace().count() != 3 {
            return Err("Git index header format mismatch".into());
        }
        let path = parse::utf8(&record[tab + 1..])?;
        parse::relative(&path)?;
        let slot = entries.entry(path).or_default();
        slot.extend_from_slice(record);
        slot.push(0);
    }
    Ok(entries)
}

fn collect_unit_stage_entries(
    stage_map: &BTreeMap<String, Vec<u8>>,
    unit_paths: &BTreeSet<String>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for path in unit_paths {
        if let Some(records) = stage_map.get(path) {
            bytes.extend_from_slice(records);
        }
    }
    bytes
}

impl AuthorizedFile {
    pub fn unit_paths(&self) -> BTreeSet<String> {
        let mut paths = BTreeSet::new();
        paths.insert(self.path.clone());
        if let Some(previous) = &self.previous_path {
            paths.insert(previous.clone());
        }
        paths
    }
}

impl CommitRegistration {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        repository_id: &str,
        worktree_id: &str,
        branch: &str,
        detached: bool,
        head: Option<&str>,
        entries: &[StatusEntry],
        changes: &mut [Change],
        working_hashes: &HashMap<String, String>,
        index_bytes: &[u8],
    ) -> Result<Self> {
        let stage_map = stage_entries_by_path(index_bytes)?;
        let mut files = HashMap::with_capacity(changes.len());
        for (entry, change) in entries.iter().zip(changes.iter_mut()) {
            let is_staged = staged(entry);
            let previous_path = (is_staged && entry.xy.starts_with('R'))
                .then(|| entry.previous.clone())
                .flatten();
            let mut unit_paths = BTreeSet::new();
            unit_paths.insert(entry.path.clone());
            if let Some(previous) = &previous_path {
                unit_paths.insert(previous.clone());
            }
            let stage_entries = collect_unit_stage_entries(&stage_map, &unit_paths);
            let working_hash = if is_staged {
                None
            } else {
                Some(
                    working_hashes
                        .get(&entry.path)
                        .cloned()
                        .ok_or("Missing content hash for unstaged file")?,
                )
            };
            let payload = serde_json::to_vec(&(
                "picket-commit-v1",
                repository_id,
                worktree_id,
                branch,
                detached,
                head,
                &change.id,
                &entry.path,
                &previous_path,
                is_staged,
                &change.range.kind,
                entry.conflicted,
                change.local_keep,
                &stage_entries,
                &working_hash,
            ))
            .map_err(|error| error.to_string())?;
            let token = hash(payload);
            change.commit_token = token.clone();
            files.insert(
                change.id.clone(),
                AuthorizedFile {
                    path: entry.path.clone(),
                    previous_path,
                    staged: is_staged,
                    range_kind: change.range.kind.clone(),
                    conflicted: entry.conflicted,
                    stage_entries,
                    working_hash,
                    token,
                },
            );
        }
        Ok(Self {
            branch: branch.to_owned(),
            detached,
            head: head.map(str::to_owned),
            files,
        })
    }
}

fn output_detail(output: &GitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.bytes);
    let combined = match (stderr.trim(), stdout.trim()) {
        ("", "") => format!("exit code {:?}", output.code),
        (err, "") => err.to_owned(),
        ("", out) => out.to_owned(),
        (err, out) => format!("{err}; {out}"),
    };
    let single_line = combined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let truncated: String = chars.by_ref().take(320).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

async fn run_index_checked(
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

async fn write_tree(reader: &GitReader, root: &Path, index: &Path) -> Result<String> {
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

impl RepositoryStore {
    pub(crate) async fn worktree_root_for_target(
        &self,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<PathBuf> {
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        Ok(root)
    }

    pub(super) async fn registered_worktree_root(
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
            .ok_or_else(|| i18n::repo_not_opened_error(locale))?;
        let root = registered
            .paths
            .get(&target.worktree_id)
            .cloned()
            .ok_or_else(|| i18n::repo_not_opened_error(locale))?;
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
            return Err(i18n::repo_identity_changed_error(locale).into());
        }
        Ok((registered, root))
    }

    async fn ensure_commit_write_allowed(&self, root: &Path, locale: LanguageKind) -> Result<()> {
        let reader = self.inner.reader.with_locale(locale);
        for lock in ["index.lock", "HEAD.lock"] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", lock],
                )
                .await?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(i18n::commit_lock_file_error(locale, lock));
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
                .await?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(i18n::commit_ongoing_op_error(locale, marker));
            }
        }
        if !reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await?
            .is_empty()
        {
            return Err(i18n::commit_unresolved_conflict_error(locale).into());
        }
        let flags = reader.bytes(root, &["ls-files", "-v", "-z"]).await?;
        if flags
            .split(|byte| *byte == 0)
            .any(|entry| !entry.is_empty() && entry[0] != b'H')
        {
            return Err(i18n::commit_sparse_or_hidden_flag_error(locale).into());
        }
        if reader
            .bytes(root, &["ls-files", "--stage", "-z"])
            .await?
            .split(|byte| *byte == 0)
            .any(|entry| entry.starts_with(b"160000 "))
        {
            return Err(i18n::commit_submodule_error(locale).into());
        }
        Ok(())
    }

    async fn ensure_commit_filters_allowed(
        &self,
        root: &Path,
        unstaged_paths: &[String],
        locale: LanguageKind,
    ) -> Result<()> {
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
            .await?;
        if filters.code == Some(1) {
            return Ok(());
        }
        if !filters.success {
            return Err(
                "Cannot inspect Git file filter configuration; no commit was created (draft and selection were preserved)"
                    .into(),
            );
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
        for batch in unstaged_paths.chunks(128) {
            let mut args = vec!["check-attr", "-z", "filter", "--"];
            args.extend(batch.iter().map(String::as_str));
            let attributes = reader.bytes(root, &args).await?;
            let fields: Vec<_> = attributes.split(|byte| *byte == 0).collect();
            for record in fields.chunks(3).filter(|record| record.len() == 3) {
                if drivers.contains(&parse::utf8_with_locale(record[2], locale)?) {
                    return Err(i18n::runner_external_filter_write_error(locale).into());
                }
            }
        }
        Ok(())
    }

    fn verify_live_selected_scope(
        &self,
        commit_state: &CommitRegistration,
        selected_files: &[AuthorizedFile],
        live_state: &BranchState,
        locale: LanguageKind,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        let live_status = parse::status_with_locale(&live_state.status, locale)?;
        if live_status.branch == "(detached)" {
            return Err(i18n::commit_detached_head_error(locale).into());
        }
        if live_status.branch != commit_state.branch || live_status.head != commit_state.head {
            return Err(i18n::commit_scope_changed_error(locale).into());
        }
        if live_status.entries.iter().any(|entry| entry.conflicted) {
            return Err(i18n::commit_unresolved_conflict_error(locale).into());
        }
        if live_status
            .entries
            .iter()
            .any(|entry| entry.mode == "160000")
        {
            return Err(i18n::commit_submodule_error(locale).into());
        }
        let live_stage_map = stage_entries_by_path(&live_state.index)?;
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
                return Err(i18n::commit_rename_slot_collision_error(locale).into());
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
                return Err(i18n::commit_scope_changed_error(locale).into());
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
                return Err(i18n::commit_scope_changed_error(locale).into());
            }
            if !authorized.staged
                && working_map.get(authorized.path.as_str()).copied()
                    != authorized.working_hash.as_deref()
            {
                return Err(i18n::commit_scope_changed_error(locale).into());
            }
        }
        Ok(live_stage_map)
    }

    async fn hook_failure_error(
        &self,
        root: &Path,
        hook_name: &str,
        output: &GitOutput,
        pre_hook_state: &BranchState,
        locale: LanguageKind,
    ) -> Result<(RepositorySnapshot, CommitResult)> {
        let detail = output_detail(output);
        let mutated = match self.branch_state(root, locale).await {
            Ok(current) => {
                current.status != pre_hook_state.status
                    || current.index != pre_hook_state.index
                    || current.working != pre_hook_state.working
            }
            Err(_) => true,
        };
        Err(i18n::commit_hook_failed_error(
            locale, hook_name, &detail, mutated,
        ))
    }

    async fn verify_recorded_commit(
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

    pub(crate) async fn commit(
        &self,
        request: &CommitRequest,
    ) -> Result<(RepositorySnapshot, CommitResult)> {
        let locale = request.locale;
        validate_request_id_with_locale(&request.request_id, locale)?;
        if request.push {
            return Err(i18n::commit_local_only_no_push_error(locale).into());
        }
        if request.message.trim().is_empty() {
            return Err(i18n::commit_message_required_error(locale).into());
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
            return Err(i18n::commit_selection_required_error(locale).into());
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
                return Err(i18n::commit_request_id_reused_error(locale).into());
            }
            let commit = previous
                .commit
                .clone()
                .ok_or_else(|| i18n::commit_interrupted_record_error(locale))?;
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

        let commit_state = registered
            .commit_states
            .get(&request.target.worktree_id)
            .ok_or_else(|| i18n::commit_token_invalid_error(locale))?;
        if commit_state.detached {
            return Err(i18n::commit_detached_head_error(locale).into());
        }
        let mut selected_files = Vec::with_capacity(request.selections.len());
        for selection in &request.selections {
            let Some(authorized) = commit_state.files.get(&selection.file_id) else {
                return Err(i18n::commit_scope_changed_error(locale).into());
            };
            if authorized.token != selection.commit_token {
                return Err(i18n::commit_token_invalid_error(locale).into());
            }
            if authorized.conflicted {
                return Err(i18n::commit_unresolved_conflict_error(locale).into());
            }
            ensure_safe_repo_path(&authorized.path)?;
            if let Some(previous) = &authorized.previous_path {
                ensure_safe_repo_path(previous)?;
            }
            selected_files.push(authorized.clone());
        }

        self.ensure_commit_write_allowed(&root, locale).await?;
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
        if let Some(parent) = commit_state.head.as_deref() {
            run_index_checked(
                &worktree_reader,
                &root,
                &["read-tree", parent],
                &selected_index,
                None,
                "Failed to initialize temporary commit index",
            )
            .await?;
        } else {
            run_index_checked(
                &worktree_reader,
                &root,
                &["read-tree", "--empty"],
                &selected_index,
                None,
                "Failed to initialize initial commit index",
            )
            .await?;
        }
        for batch in unstaged_paths.chunks(128) {
            let mut args = vec!["add", "--all", "--"];
            args.extend(batch.iter().map(String::as_str));
            run_index_checked(
                &worktree_reader,
                &root,
                &args,
                &selected_index,
                None,
                "Failed to stage selected unstaged paths in temporary index",
            )
            .await?;
        }
        let mut staged_unit_paths = BTreeSet::new();
        let mut staged_index_info = Vec::new();
        let mut staged_by_unit_path = BTreeMap::<String, Vec<u8>>::new();
        let initial_stage_map = stage_entries_by_path(&initial_live_state.index)?;
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
                &worktree_reader,
                &root,
                &["update-index", "--force-remove", "-z", "--stdin"],
                &selected_index,
                Some(&remove_stdin),
                "Failed to clear old paths in temporary index",
            )
            .await?;
            for records in staged_by_unit_path.values() {
                staged_index_info.extend_from_slice(records);
            }
            if !staged_index_info.is_empty() {
                run_index_checked(
                    &worktree_reader,
                    &root,
                    &["update-index", "-z", "--index-info"],
                    &selected_index,
                    Some(&staged_index_info),
                    "Failed to apply selected staged entries",
                )
                .await?;
            }
        }
        let selected_stage = run_index_checked(
            &worktree_reader,
            &root,
            &["ls-files", "--stage", "-z"],
            &selected_index,
            None,
            "Failed to inspect temporary commit index",
        )
        .await?;
        if selected_stage
            .split(|byte| *byte == 0)
            .any(|entry| entry.starts_with(b"160000 "))
        {
            return Err(i18n::commit_submodule_error(locale).into());
        }
        let tree = write_tree(&worktree_reader, &root, &selected_index).await?;
        if let Some(parent) = commit_state.head.as_deref() {
            let base_tree = reader
                .text(&root, &["rev-parse", &format!("{parent}^{{tree}}")])
                .await?;
            if tree == base_tree {
                return Err(
                    "Selected scope has no effective changes relative to current branch; skipped empty commit (draft and selection were preserved)"
                        .into(),
                );
            }
        } else if selected_stage.is_empty() {
            return Err(
                "Selected scope has no files for initial commit; skipped empty commit (draft and selection were preserved)"
                    .into(),
            );
        }

        let index_path = PathBuf::from(
            reader
                .text(
                    &root,
                    &["rev-parse", "--path-format=absolute", "--git-path", "index"],
                )
                .await?,
        );
        self.ensure_commit_write_allowed(&root, locale).await?;
        let mut index_lock = IndexLock::acquire(&index_path, locale)?;
        let pre_hook_state = self.branch_state(&root, locale).await?;
        let pre_stage_map = self.verify_live_selected_scope(
            commit_state,
            &selected_files,
            &pre_hook_state,
            locale,
        )?;

        let pre_commit = reader
            .run_hook(
                &root,
                &["hook", "run", "--ignore-missing", "pre-commit"],
                Some(&selected_index),
            )
            .await?;
        if !pre_commit.success {
            return self
                .hook_failure_error(&root, "pre-commit", &pre_commit, &pre_hook_state, locale)
                .await;
        }
        let cleaned = reader
            .write_with(
                &root,
                &["stripspace"],
                None,
                Some(request.message.as_bytes()),
            )
            .await?;
        if !cleaned.success {
            return Err(
                "Failed to normalize commit message; no commit was created (draft and selection were preserved)"
                    .into(),
            );
        }
        let msg_path = temp.path().join("COMMIT_EDITMSG");
        fs::write(&msg_path, &cleaned.bytes)
            .map_err(|_| "Cannot write temporary commit message file")?;
        let msg_arg = display_with_locale(&msg_path, locale)?;
        let prepare_msg = reader
            .run_hook(
                &root,
                &[
                    "hook",
                    "run",
                    "--ignore-missing",
                    "prepare-commit-msg",
                    "--",
                    &msg_arg,
                    "message",
                ],
                Some(&selected_index),
            )
            .await?;
        if !prepare_msg.success {
            return self
                .hook_failure_error(
                    &root,
                    "prepare-commit-msg",
                    &prepare_msg,
                    &pre_hook_state,
                    locale,
                )
                .await;
        }
        let commit_msg = reader
            .run_hook(
                &root,
                &[
                    "hook",
                    "run",
                    "--ignore-missing",
                    "commit-msg",
                    "--",
                    &msg_arg,
                ],
                Some(&selected_index),
            )
            .await?;
        if !commit_msg.success {
            return self
                .hook_failure_error(&root, "commit-msg", &commit_msg, &pre_hook_state, locale)
                .await;
        }
        let hook_msg_bytes =
            fs::read(&msg_path).map_err(|_| "Cannot read commit message after hook validation")?;
        let final_cleaned = reader
            .write_with(&root, &["stripspace"], None, Some(&hook_msg_bytes))
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

        let post_hook_tree = write_tree(&worktree_reader, &root, &selected_index).await?;
        let post_hook_state = self.branch_state(&root, locale).await?;
        if post_hook_tree != tree
            || post_hook_state.status != pre_hook_state.status
            || post_hook_state.index != pre_hook_state.index
            || post_hook_state.working != pre_hook_state.working
        {
            return Err(i18n::commit_hook_mutated_content_error(locale).into());
        }
        let head_lock = reader
            .text(
                &root,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "HEAD.lock",
                ],
            )
            .await?;
        if fs::symlink_metadata(&head_lock).is_ok() {
            return Err(i18n::commit_lock_file_error(locale, "HEAD.lock"));
        }
        let symbolic_head = reader
            .write(&root, &["symbolic-ref", "--quiet", "HEAD"])
            .await?;
        let expected_ref = format!("refs/heads/{}", commit_state.branch);
        if !symbolic_head.success
            || parse::utf8_with_locale(&symbolic_head.bytes, locale)?.trim() != expected_ref
        {
            return Err(i18n::commit_scope_changed_error(locale).into());
        }

        let all_unit_paths_set: BTreeSet<String> = selected_files
            .iter()
            .flat_map(AuthorizedFile::unit_paths)
            .collect();
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
                return Err(i18n::commit_missing_identity_error(locale, &detail));
            }
            return Err(i18n::commit_object_failed_error(locale, &detail));
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
            return Err(i18n::commit_update_ref_failed_error(
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
            let remaining_index = temp.path().join("remaining.index");
            if index_path.exists() {
                fs::copy(&index_path, &remaining_index)
                    .map_err(|_| "Cannot copy current staging index")?;
            } else {
                run_index_checked(
                    &worktree_reader,
                    &root,
                    &["read-tree", "--empty"],
                    &remaining_index,
                    None,
                    "Failed to initialize remaining staging index",
                )
                .await?;
            }
            for batch in all_unit_paths.chunks(128) {
                let mut reset_args = vec!["reset", "--quiet", commit_oid.as_str(), "--"];
                reset_args.extend(batch.iter().map(String::as_str));
                run_index_checked(
                    &worktree_reader,
                    &root,
                    &reset_args,
                    &remaining_index,
                    None,
                    "Failed to reconcile committed paths in staging index",
                )
                .await?;
            }
            let remaining_bytes =
                fs::read(&remaining_index).map_err(|_| "Cannot read updated staging index")?;
            index_lock.replace(&index_path, &remaining_bytes)?;
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
            return Err(i18n::commit_reconcile_failed_error(
                locale, &short_oid, &error,
            ));
        }

        if reader.text(&root, &["rev-parse", "HEAD"]).await? != commit_oid
            || reader.text(&root, &["rev-parse", "HEAD^{tree}"]).await? != tree
        {
            return Err(i18n::commit_post_verify_tree_error(locale, &short_oid));
        }
        for (path, expected_hash) in &pre_hook_state.working {
            if &self.working_file(&root, path)? != expected_hash {
                return Err(i18n::commit_post_verify_working_error(
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
            return Err(i18n::commit_post_verify_unselected_error(
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
            .map_err(|error| i18n::commit_post_read_failed_error(locale, &short_oid, &error))?;
        Ok((snapshot, commit_result))
    }
}
