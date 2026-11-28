//! Unified Agent Action & C-Push coordinator:
//! manages same-repo write exclusion, real-time bidirectional `message_user` cards & oneshot
//! Q&A round-trips, unified C-Push / Pull-Rebase / tool-driven conflict resolution,
//! Rust-verified `install_platform_cli` grants, 3-tier controlled CLI policy enforcement,
//! and direct reuse of `RepositoryStore::commit` / `push_branch` / `sync_rebase_onto_upstream`.

mod conflict_flow;
mod context;
mod cpush_commit_exec;
mod cpush_commit_tool;
mod cpush_flow;
pub(super) mod cpush_helpers;
mod cpush_push_retry;
mod cpush_response;
mod cpush_sensitive;
mod cpush_tools;
mod generator;
mod pull_rebase;
mod questions;
mod reconcile;
mod runner;
mod sessions;
mod tools_cli;
mod tools_command;
mod tools_git_ops;
mod tools_submit;
mod tools_workspace;
#[cfg(test)]
mod workspace_tests;

pub(super) use questions::SessionAuthGrants;

use super::types::*;
use crate::{
    bounded_cache::BoundedMap,
    error::AppError,
    i18n::LanguageKind,
    repo_lock::{RepoWriteCoordinator, RepoWriteKind, RepoWriteLease},
};
use questions::PendingQuestion;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(super) const CUSTOM_ACTION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub(super) const CPUSH_AI_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_PROCESSED_CPUSH_REQUESTS: usize = 128;

pub(super) struct CoordinatorInner {
    pub(super) write_coordinator: RepoWriteCoordinator,
    pub(super) active_operations: AtomicUsize,
    pending_questions: Mutex<HashMap<(String, String), PendingQuestion>>,
    override_cli_dirs: Mutex<Option<Vec<PathBuf>>>,
    pub(super) seq: AtomicU64,
    worktree_gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) cpush_sessions: Mutex<HashMap<String, StoredCPushSession>>,
    pub(super) active_action_tray: Mutex<HashMap<String, crate::tray::TrayWorktreeSessionInfo>>,
    pub(super) processed_cpush_requests: Mutex<BoundedMap<String, CPushCommandReply>>,
}

impl Default for CoordinatorInner {
    fn default() -> Self {
        Self::with_write_coordinator(RepoWriteCoordinator::default())
    }
}

impl CoordinatorInner {
    fn with_write_coordinator(write_coordinator: RepoWriteCoordinator) -> Self {
        Self {
            write_coordinator,
            active_operations: AtomicUsize::default(),
            pending_questions: Mutex::default(),
            override_cli_dirs: Mutex::default(),
            seq: AtomicU64::default(),
            worktree_gates: Mutex::default(),
            cpush_sessions: Mutex::default(),
            active_action_tray: Mutex::default(),
            processed_cpush_requests: Mutex::new(BoundedMap::with_capacity(
                MAX_PROCESSED_CPUSH_REQUESTS,
            )),
        }
    }
}

#[derive(Clone, Default)]
pub struct CustomActionCoordinator {
    pub(super) inner: Arc<CoordinatorInner>,
}

pub(crate) struct ActiveOperationGuard {
    inner: Arc<CoordinatorInner>,
}

impl Drop for ActiveOperationGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) struct ActiveWriteRepoGuard {
    inner: Arc<CoordinatorInner>,
    _repo_lease: RepoWriteLease,
}

impl Drop for ActiveWriteRepoGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
    }
}

impl CustomActionCoordinator {
    pub fn with_write_coordinator(write_coordinator: RepoWriteCoordinator) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner::with_write_coordinator(write_coordinator)),
        }
    }

    pub fn has_active_operation(&self) -> bool {
        self.inner.active_operations.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn track_operation(&self) -> ActiveOperationGuard {
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        ActiveOperationGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    pub(super) fn worktree_gate(
        &self,
        worktree_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
        Ok(self
            .inner
            .worktree_gates
            .lock()
            .map_err(|_| "Session coordinator lock is unavailable")?
            .entry(worktree_id.to_owned())
            .or_default()
            .clone())
    }

    pub(super) fn override_cli_dirs(&self) -> Option<Vec<PathBuf>> {
        self.inner
            .override_cli_dirs
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    #[cfg(test)]
    pub(crate) fn set_override_cli_dirs(&self, dirs: Option<Vec<PathBuf>>) {
        if let Ok(mut guard) = self.inner.override_cli_dirs.lock() {
            *guard = dirs;
        }
    }

    pub(super) fn acquire_repo_write(
        &self,
        repository_id: &str,
        locale: LanguageKind,
    ) -> Result<ActiveWriteRepoGuard, AppError> {
        let lease = self.inner.write_coordinator.try_acquire(
            repository_id,
            RepoWriteKind::CustomAction,
            locale,
        )?;
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        Ok(ActiveWriteRepoGuard {
            inner: Arc::clone(&self.inner),
            _repo_lease: lease,
        })
    }
}
