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
mod local_keep;
mod parse;
pub mod persistence;
#[cfg(test)]
mod persistence_tests;
pub mod read_tools;
pub mod remote;
mod runner;
pub mod smoke;
mod snapshot;
#[cfg(test)]
mod tests;
pub mod types;
mod worktree_read;

use crate::i18n::{self, LanguageKind};
#[allow(unused_imports)]
use parse::{StatusEntry, WorktreeEntry};
use runner::GitReader;
use sha2::{Digest, Sha256};
#[allow(unused_imports)]
use std::fs;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
#[allow(unused_imports)]
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
        .ok_or_else(|| i18n::repository::non_utf8_directory(locale).into())
}

fn label(path: &Path, locale: LanguageKind) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| i18n::repository::default_worktree_label(locale))
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
        return Err(i18n::repository::invalid_repo_request_id(locale).into());
    }
    Ok(())
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
}
