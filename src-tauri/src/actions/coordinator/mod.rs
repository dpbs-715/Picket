//! Unified Agent Action & Built-in Workflow coordinator:
//! manages same-repo write exclusion, profile-gated tool dispatch, real-time bidirectional
//! `message_user` cards & oneshot Q&A round-trips, unified `commit_exact_scope` / `sync_and_push`,
//! `agentFirst` verified `handoff_to_recovery`, zero-token Host Recovery, Pull & Rebase, and
//! tool-driven conflict resolution on a single `StoredActionSession` state machine.

mod conflict_flow;
mod context;
mod generator;
mod questions;
mod reconcile;
mod recovery;
mod runner;
pub(super) mod session_helpers;
mod sessions;
mod tools_cli;
mod tools_command;
mod tools_commit;
mod tools_submit;
mod tools_sync;
mod tools_workspace;
#[cfg(test)]
mod workspace_tests;

pub(super) use questions::SessionAuthGrants;

use super::types::*;
use crate::{bounded_cache::BoundedMap, repo_lock::RepoWriteCoordinator};
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
const MAX_PROCESSED_REQUESTS: usize = 128;

pub(super) struct CoordinatorInner {
    pub(super) write_coordinator: RepoWriteCoordinator,
    pub(super) active_operations: AtomicUsize,
    pending_questions: Mutex<HashMap<(String, String), PendingQuestion>>,
    active_pi_requests: Mutex<HashMap<String, String>>,
    override_cli_dirs: Mutex<Option<Vec<PathBuf>>>,
    pub(super) seq: AtomicU64,
    worktree_gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) active_sessions: Mutex<HashMap<String, StoredActionSession>>,
    pub(super) processed_requests: Mutex<BoundedMap<String, ActionCommandReply>>,
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
            active_pi_requests: Mutex::default(),
            override_cli_dirs: Mutex::default(),
            seq: AtomicU64::default(),
            worktree_gates: Mutex::default(),
            active_sessions: Mutex::default(),
            processed_requests: Mutex::new(BoundedMap::with_capacity(MAX_PROCESSED_REQUESTS)),
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

pub(super) struct ActivePiRequestGuard {
    inner: Arc<CoordinatorInner>,
    run_id: String,
    pi_request_id: String,
}

impl Drop for ActivePiRequestGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.inner.active_pi_requests.lock()
            && map.get(&self.run_id).map(String::as_str) == Some(self.pi_request_id.as_str())
        {
            map.remove(&self.run_id);
        }
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

    pub(super) fn register_active_pi_request(
        &self,
        run_id: &str,
        pi_request_id: &str,
    ) -> ActivePiRequestGuard {
        if let Ok(mut map) = self.inner.active_pi_requests.lock() {
            map.insert(run_id.to_owned(), pi_request_id.to_owned());
        }
        ActivePiRequestGuard {
            inner: Arc::clone(&self.inner),
            run_id: run_id.to_owned(),
            pi_request_id: pi_request_id.to_owned(),
        }
    }

    pub fn resolve_active_pi_request_id(&self, run_id: &str) -> String {
        self.inner
            .active_pi_requests
            .lock()
            .ok()
            .and_then(|map| map.get(run_id).cloned())
            .unwrap_or_else(|| run_id.to_owned())
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
}
