pub mod ai_context;
#[cfg(test)]
mod branch_tests;
mod branches;
mod commit;
#[cfg(test)]
mod commit_tests;
#[cfg(test)]
mod edge_tests;
pub mod editor;
#[cfg(test)]
mod editor_tests;
mod parse;
pub mod persistence;
#[cfg(test)]
mod persistence_tests;
pub mod remote;
mod runner;
pub mod smoke;
#[cfg(test)]
mod tests;
pub mod types;

use crate::i18n::{self, LanguageKind};
use parse::{StatusEntry, WorktreeEntry};
use runner::GitReader;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
use types::*;

type Result<T> = std::result::Result<T, String>;
pub(crate) const MAX_LOCAL_KEEP_PATHS_PER_REPO: usize = 2048;
pub(crate) struct ExportedLocalKeep {
    pub entries: Vec<(String, Vec<String>)>,
    pub known_common_dirs: HashSet<String>,
}

#[derive(Clone, Default)]
pub struct RepositoryStore {
    inner: Arc<StoreInner>,
}
#[derive(Default)]
struct StoreInner {
    reader: GitReader,
    sequence: AtomicU64,
    active_writes: AtomicUsize,
    repositories: Mutex<HashMap<String, Registration>>,
    opened_order: Mutex<Vec<String>>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    branch_requests: Mutex<HashSet<String>>,
    commit_requests: Mutex<HashSet<String>>,
    verified_remotes: Mutex<HashSet<(String, String)>>,
    local_keep: Mutex<HashMap<PathBuf, BTreeSet<String>>>,
    offline_local_keep: Mutex<HashMap<String, Vec<String>>>,
}

pub(crate) struct ActiveWriteGuard {
    inner: Arc<StoreInner>,
}

impl Drop for ActiveWriteGuard {
    fn drop(&mut self) {
        self.inner.active_writes.fetch_sub(1, Ordering::SeqCst);
    }
}

impl RepositoryStore {
    pub fn has_active_write(&self) -> bool {
        self.inner.active_writes.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn track_write(&self) -> ActiveWriteGuard {
        self.inner.active_writes.fetch_add(1, Ordering::SeqCst);
        ActiveWriteGuard {
            inner: Arc::clone(&self.inner),
        }
    }
}
#[derive(Clone)]
struct Registration {
    sequence: u64,
    common_dir: PathBuf,
    primary_root: PathBuf,
    paths: HashMap<String, PathBuf>,
    branch_tokens: HashMap<String, String>,
    commit_states: HashMap<String, commit::CommitRegistration>,
}
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn identity(prefix: &str, path: &Path) -> String {
    format!("{prefix}-{}", hash(path.as_os_str().as_encoded_bytes()))
}
fn display_with_locale(path: &Path, locale: LanguageKind) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| i18n::non_utf8_directory(locale).into())
}
fn label(path: &Path, locale: LanguageKind) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| i18n::default_worktree_label(locale))
        .to_owned()
}
fn ignored_for_review(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|name| name.to_str()),
        Some(
            "pnpm-lock.yaml"
                | "package-lock.json"
                | "yarn.lock"
                | "Cargo.lock"
                | "bun.lock"
                | "bun.lockb"
        )
    )
}
pub(crate) fn is_valid_local_keep_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') || path.starts_with('/') {
        return false;
    }
    if path.split('/').any(|segment| {
        segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.eq_ignore_ascii_case(".git")
    }) {
        return false;
    }
    parse::relative(path).is_ok()
}
pub(crate) fn file_id_for_path(path: &str) -> String {
    format!("file-{}", hash(path))
}

impl RepositoryStore {
    pub async fn shutdown(&self) {
        self.inner.reader.shutdown().await;
    }
    pub(crate) fn registered_roots(&self) -> Result<Vec<(String, PathBuf)>> {
        let order = self
            .inner
            .opened_order
            .lock()
            .map_err(|_| "Repository order registry is unavailable")?
            .clone();
        let repositories = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?;
        Ok(order
            .into_iter()
            .filter_map(|id| {
                repositories
                    .get(&id)
                    .map(|reg| (id, reg.primary_root.clone()))
            })
            .collect())
    }
    pub async fn open(&self, directory: &Path) -> Result<RepositorySnapshot> {
        self.open_with_locale(directory, LanguageKind::En).await
    }
    pub async fn open_with_locale(
        &self,
        directory: &Path,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        let directory = fs::canonicalize(directory)
            .map_err(|_| i18n::repo_directory_inaccessible(locale).to_string())?;
        if !directory.is_dir() {
            return Err(i18n::repo_must_select_directory(locale).into());
        }
        let dir_name = label(&directory, locale);
        let reader = self.inner.reader.with_locale(locale);
        let root = match reader
            .text(&directory, &["rev-parse", "--show-toplevel"])
            .await
        {
            Ok(root) => root,
            Err(err) => {
                if reader.cancelled() {
                    return Err(err);
                }
                let is_bare = reader
                    .text(&directory, &["rev-parse", "--is-bare-repository"])
                    .await
                    .is_ok_and(|out| out.trim() == "true");
                if is_bare {
                    return Err(i18n::open_bare_repo_error(locale, &dir_name));
                }
                return Err(i18n::open_not_git_repo_error(locale, &dir_name));
            }
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
            .ok_or_else(|| i18n::repo_not_opened_error(locale))?;
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                &path,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "Cannot access Git common directory")?;
        if identity("repo", &common) != target.repository_id {
            return Err(i18n::repo_identity_changed_error(locale).into());
        }
        let snapshot = self.read_repository(&path, locale).await?;
        if snapshot.repository.id != target.repository_id {
            return Err(i18n::repo_identity_changed_error(locale).into());
        }
        Ok(snapshot)
    }
    pub async fn set_local_keep(
        &self,
        target: &ReadTarget,
        paths: &[String],
        keep: bool,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        if paths.is_empty() {
            return Err(i18n::local_keep_empty_paths_error(locale).into());
        }
        if paths.len() > MAX_LOCAL_KEEP_PATHS_PER_REPO {
            return Err(i18n::local_keep_limit_exceeded_error(locale).into());
        }
        if !paths.iter().all(|path| is_valid_local_keep_path(path)) {
            return Err(i18n::local_keep_invalid_path_error(locale).into());
        }
        let _write_guard = self.track_write();
        let path = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?
            .get(&target.repository_id)
            .and_then(|repository| repository.paths.get(&target.worktree_id))
            .cloned()
            .ok_or_else(|| i18n::repo_not_opened_error(locale))?;
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                &path,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common)
            .map_err(|_| "Cannot access Git common directory".to_string())?;
        if identity("repo", &common) != target.repository_id {
            return Err(i18n::repo_identity_changed_error(locale).into());
        }
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        self.resolve_keep_set_for_common(&common)?;
        {
            let mut keep_map = self
                .inner
                .local_keep
                .lock()
                .map_err(|_| "Local Keep registry is unavailable")?;
            let set = keep_map.entry(common.clone()).or_default();
            if keep {
                let added_count = paths
                    .iter()
                    .filter(|candidate| !set.contains(candidate.as_str()))
                    .collect::<HashSet<_>>()
                    .len();
                if set.len() + added_count > MAX_LOCAL_KEEP_PATHS_PER_REPO {
                    return Err(i18n::local_keep_limit_exceeded_error(locale).into());
                }
                for item in paths {
                    set.insert(item.clone());
                }
            } else {
                for item in paths {
                    set.remove(item.as_str());
                }
            }
        }
        let snapshot = self.read_repository_locked(&path, locale).await?;
        if snapshot.repository.id != target.repository_id {
            return Err(i18n::repo_identity_changed_error(locale).into());
        }
        Ok(snapshot)
    }
    pub(crate) fn import_persisted_local_keep(
        &self,
        entries: &[(String, Vec<String>)],
    ) -> Result<()> {
        let mut live = self
            .inner
            .local_keep
            .lock()
            .map_err(|_| "Local Keep registry is unavailable")?;
        let mut offline = self
            .inner
            .offline_local_keep
            .lock()
            .map_err(|_| "Offline Local Keep registry is unavailable")?;
        live.clear();
        offline.clear();
        for (common_dir, raw_paths) in entries {
            if common_dir.is_empty() {
                continue;
            }
            let sanitized: BTreeSet<String> = raw_paths
                .iter()
                .filter(|path| is_valid_local_keep_path(path))
                .take(MAX_LOCAL_KEEP_PATHS_PER_REPO)
                .cloned()
                .collect();
            if sanitized.is_empty() {
                continue;
            }
            match fs::canonicalize(common_dir) {
                Ok(canon) => {
                    let set = live.entry(canon).or_default();
                    for item in sanitized {
                        if set.len() >= MAX_LOCAL_KEEP_PATHS_PER_REPO {
                            break;
                        }
                        set.insert(item);
                    }
                }
                Err(_) => {
                    offline.insert(common_dir.clone(), sanitized.into_iter().collect());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn export_persisted_local_keep(
        &self,
        locale: LanguageKind,
    ) -> Result<ExportedLocalKeep> {
        let repositories = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?;
        let live = self
            .inner
            .local_keep
            .lock()
            .map_err(|_| "Local Keep registry is unavailable")?;
        let offline = self
            .inner
            .offline_local_keep
            .lock()
            .map_err(|_| "Offline Local Keep registry is unavailable")?;
        let mut known = HashSet::with_capacity(repositories.len() + live.len() + offline.len());
        for reg in repositories.values() {
            known.insert(display_with_locale(&reg.common_dir, locale)?);
        }
        let mut out = BTreeMap::<String, Vec<String>>::new();
        for (common_path, set) in live.iter() {
            let key = display_with_locale(common_path, locale)?;
            known.insert(key.clone());
            if !set.is_empty() {
                out.insert(key, set.iter().cloned().collect());
            }
        }
        for (common_str, paths) in offline.iter() {
            if !known.contains(common_str) {
                known.insert(common_str.clone());
                if !paths.is_empty() {
                    out.insert(common_str.clone(), paths.clone());
                }
            }
        }
        Ok(ExportedLocalKeep {
            entries: out.into_iter().collect(),
            known_common_dirs: known,
        })
    }
    pub(crate) fn offline_local_keep_for_repo_id(
        &self,
        repository_id: &str,
    ) -> Option<Vec<String>> {
        let offline = self.inner.offline_local_keep.lock().ok()?;
        offline
            .iter()
            .find(|(common_dir, _)| identity("repo", Path::new(common_dir)) == repository_id)
            .map(|(_, paths)| paths.clone())
    }
    fn resolve_keep_set_for_common(&self, common: &Path) -> Result<BTreeSet<String>> {
        let migrated = if let Some(common_str) = common.to_str() {
            self.inner
                .offline_local_keep
                .lock()
                .map_err(|_| "Offline Local Keep registry is unavailable")?
                .remove(common_str)
        } else {
            None
        };
        let mut keep_map = self
            .inner
            .local_keep
            .lock()
            .map_err(|_| "Local Keep registry is unavailable")?;
        let set = keep_map.entry(common.to_path_buf()).or_default();
        if let Some(paths) = migrated {
            for item in paths {
                if set.len() >= MAX_LOCAL_KEEP_PATHS_PER_REPO {
                    break;
                }
                if is_valid_local_keep_path(&item) {
                    set.insert(item);
                }
            }
        }
        Ok(set.clone())
    }
    fn repository_gate(&self, repository_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>> {
        Ok(self
            .inner
            .gates
            .lock()
            .map_err(|_| "Repository gate is unavailable")?
            .entry(repository_id.into())
            .or_default()
            .clone())
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
    async fn read_repository_locked(
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
    async fn read_worktree(
        &self,
        repository_id: &str,
        entry: &WorktreeEntry,
        keep_set: &BTreeSet<String>,
        locale: LanguageKind,
    ) -> Result<(WorktreeSnapshot, commit::CommitRegistration)> {
        let root = &entry.path;
        let reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(root)
            .await?;
        let before = reader
            .bytes(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let status = parse::status_with_locale(&before, locale)?;
        if status.entries.len() > 10_000 {
            return Err(i18n::repo_too_many_changes_error(locale).into());
        }
        if status.entries.iter().any(|entry| entry.mode == "160000") {
            return Err(i18n::repo_submodule_read_error(locale).into());
        }
        let mut fingerprints = HashMap::new();
        for file in &status.entries {
            if !staged(file) {
                fingerprints.insert(file.path.clone(), self.working_file(root, &file.path)?);
            }
        }
        let staged_stats = parse::numstat(
            &reader
                .bytes(
                    root,
                    &[
                        "diff",
                        "--cached",
                        "--numstat",
                        "-z",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--find-renames",
                    ],
                )
                .await?,
        )?;
        let unstaged_stats = parse::numstat(
            &reader
                .bytes(
                    root,
                    &[
                        "diff",
                        "--numstat",
                        "-z",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--find-renames",
                    ],
                )
                .await?,
        )?;
        let mut files = Vec::new();
        let mut working_hashes = HashMap::new();
        for file in &status.entries {
            let index = staged(file);
            let stats = if file.xy == "??" {
                let output = reader
                    .run(
                        root,
                        &[
                            "diff",
                            "--no-index",
                            "--numstat",
                            "-z",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--",
                            "/dev/null",
                            &file.path,
                        ],
                    )
                    .await?;
                if !matches!(output.code, Some(0 | 1)) {
                    return Err("Cannot read Git line statistics for untracked file".into());
                }
                parse::untracked_numstat(&output.bytes, &file.path)?
            } else {
                if index {
                    &staged_stats
                } else {
                    &unstaged_stats
                }
                .get(&file.path)
                .cloned()
                .unwrap_or_default()
            };
            let working = if !index {
                let current = self.working_file(root, &file.path)?;
                if fingerprints
                    .get(&file.path)
                    .is_some_and(|previous| previous != &current)
                {
                    return Err(i18n::repo_changed_during_read_error(locale).into());
                }
                working_hashes.insert(file.path.clone(), current.clone());
                Some(current)
            } else {
                None
            };
            let conflict = file.conflicted;
            let kind = if index {
                if file.xy.as_bytes().get(1).is_some_and(|byte| *byte != b'.') {
                    "partial"
                } else {
                    "staged"
                }
            } else {
                "unstaged"
            };
            let is_local_keep = keep_set.contains(&file.path);
            let version = format!(
                "{}\0{}\0{}\0{}\0{}\0{}",
                file.path,
                file.previous.as_deref().unwrap_or(""),
                if index { "index" } else { "worktree" },
                if index {
                    &file.index_oid
                } else {
                    working.as_deref().unwrap_or("")
                },
                file.index_oid,
                if is_local_keep {
                    "local-keep"
                } else {
                    "normal"
                },
            );
            let effective = if file.xy == "??" {
                b'A'
            } else {
                file.xy.as_bytes()[usize::from(!index)]
            };
            let status_code = if conflict {
                "U"
            } else {
                match effective {
                    b'A' => "A",
                    b'D' => "D",
                    b'R' => "R",
                    b'C' => "C",
                    b'T' => "T",
                    _ => "M",
                }
            };
            files.push(Change {
                id: file_id_for_path(&file.path),
                path: file.path.clone(),
                previous_path: file.previous.clone(),
                status: status_code.into(),
                git_status: file.xy.clone(),
                range: Range {
                    kind: kind.into(),
                    added: stats.added,
                    removed: stats.removed,
                },
                binary: stats.binary,
                ignored_for_review: ignored_for_review(&file.path),
                local_keep: is_local_keep,
                content_version: hash(version),
                commit_token: String::new(),
            });
        }
        let after = reader
            .bytes(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        if before != after {
            return Err(i18n::repo_changed_during_read_error(locale).into());
        }
        let raw_detached = status.branch == "(detached)";
        let rebase_branch = if raw_detached {
            self.active_rebase_branch(root).await
        } else {
            None
        };
        let display_branch = rebase_branch
            .clone()
            .unwrap_or_else(|| status.branch.clone());
        let detached = raw_detached && rebase_branch.is_none();
        let (effective_upstream, effective_divergence) =
            if status.upstream.is_none() && rebase_branch.is_some() {
                self.resolve_branch_upstream_and_divergence(root, &display_branch)
                    .await
            } else {
                (status.upstream, status.divergence)
            };
        let remote = match (effective_upstream, effective_divergence) {
            (Some(branch), Some((ahead, behind))) => {
                let remote_name = branch.split('/').next().unwrap_or("");
                let verified = self.is_remote_verified(repository_id, remote_name);
                Remote::Tracked {
                    branch,
                    ahead,
                    behind,
                    local_only: !verified,
                }
            }
            (Some(_), None) => Remote::Error {
                reason: i18n::remote_missing_tracking_ref_error(locale).into(),
            },
            (None, _) => {
                let reason = if detached {
                    i18n::remote_detached_head_reason(locale).to_owned()
                } else {
                    let remotes = self.list_remotes_unlocked(root).await.unwrap_or_default();
                    if remotes.is_empty() {
                        i18n::remote_no_remotes_configured_reason(locale).to_owned()
                    } else {
                        i18n::remote_no_upstream_configured_reason(locale).to_owned()
                    }
                };
                Remote::Missing { reason }
            }
        };
        let branch_state = self.branch_state(root, locale).await?;
        if branch_state.status != before {
            return Err(i18n::repo_changed_during_read_error(locale).into());
        }
        let worktree_id = identity("wt", root);
        let commit_state = commit::CommitRegistration::build(
            repository_id,
            &worktree_id,
            &status.branch,
            raw_detached,
            status.head.as_deref(),
            &status.entries,
            &mut files,
            &working_hashes,
            &branch_state.index,
        )?;
        let version = serde_json::to_vec(&(status.head.as_deref(), &display_branch, &files))
            .map_err(|error| error.to_string())?;
        Ok((
            WorktreeSnapshot {
                repository_id: repository_id.into(),
                worktree_id,
                label: label(root, locale),
                path: display_with_locale(root, locale)?,
                branch: display_branch,
                detached,
                head: status.head,
                status_version: hash(version),
                branch_token: Some(branch_state.token()?),
                read_error: None,
                remote,
                files,
            },
            commit_state,
        ))
    }
    fn working_file(&self, root: &Path, path: &str) -> Result<String> {
        parse::relative(path)?;
        let path = root.join(path);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(hash(b"missing\0"));
            }
            Err(_) => {
                return Err(
                    "Cannot read changed file; current draft and selection were preserved".into(),
                );
            }
        };
        let parent = fs::canonicalize(
            path.parent()
                .ok_or("File path is missing parent directory")?,
        )
        .map_err(|_| "Cannot access parent directory of changed file")?;
        if !parent.starts_with(root) {
            return Err(
                "File parent directory points outside the worktree; stopped reading".into(),
            );
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).map_err(|_| "Cannot read symbolic link target")?;
            let mut payload = b"symlink\0".to_vec();
            payload.extend_from_slice(target.as_os_str().as_encoded_bytes());
            return Ok(hash(payload));
        }
        if !metadata.is_file() {
            return Err("Changes contain unsupported special files; stopped reading".into());
        }
        let mut file = File::open(&path).map_err(|_| "Cannot open changed file")?;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let opened = file.metadata().map_err(|_| "Cannot stat changed file")?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() || !opened.is_file() {
            return Err("File was replaced while opening; please refresh".into());
        }
        let mut digest = Sha256::new();
        digest.update(if opened.permissions().mode() & 0o111 != 0 {
            b"file:755\0"
        } else {
            b"file:644\0"
        });
        let mut buffer = [0; 64 * 1024];
        loop {
            if self.inner.reader.cancelled() {
                return Err(i18n::repo_shutting_down_error(LanguageKind::En).into());
            }
            let count = file
                .read(&mut buffer)
                .map_err(|_| "Failed to read changed file")?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }
}
fn staged(file: &StatusEntry) -> bool {
    !file.conflicted
        && file.xy != "??"
        && file.xy.as_bytes().first().is_some_and(|byte| *byte != b'.')
}

#[cfg(test)]
pub fn validate_request_id(request_id: &str) -> Result<()> {
    validate_request_id_with_locale(request_id, LanguageKind::En)
}

pub fn validate_request_id_with_locale(request_id: &str, locale: LanguageKind) -> Result<()> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(i18n::invalid_repo_request_id(locale).into());
    }
    Ok(())
}
