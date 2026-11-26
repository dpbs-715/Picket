//! M4 & M5 C-Push, Preview Plan, and Pull-Rebase coordinator:
//! freezes authorized file scope, coordinates Pi Core split planning & authorized
//! conflict resolution, executes user-confirmed Preview Plans verbatim, serializes
//! Git writes per repository, preserves completed commits on partial/push failure
//! without repeating finished steps, reconciles manual IDE merge & terminal push
//! states against live Git + remote, and persists session history in
//! `cpush-sessions.json` (`0600`).

mod conflict;
mod execute;
mod helpers;
#[cfg(test)]
mod m5_tests;
mod plan;
mod reconcile;
mod respond;
mod storage;
mod sync;
#[cfg(test)]
mod tests;
pub mod types;

use crate::repository::types::ReadTarget;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use types::{CPushCommandReply, CPushSessionStatus, StoredCPushSession};

pub(super) const CPUSH_AI_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Default)]
pub struct CPushCoordinator {
    pub(super) inner: Arc<CoordinatorInner>,
}

#[derive(Default)]
pub(super) struct CoordinatorInner {
    pub(super) storage_gate: tokio::sync::Mutex<()>,
    pub(super) worktree_gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) sessions: Mutex<HashMap<String, StoredCPushSession>>,
    pub(super) processed_requests: Mutex<HashMap<String, CPushCommandReply>>,
    pub(super) active_operations: AtomicUsize,
}

pub(crate) struct ActiveOperationGuard {
    inner: Arc<CoordinatorInner>,
}

impl Drop for ActiveOperationGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
    }
}

impl CPushCoordinator {
    pub fn has_active_operation(&self) -> bool {
        self.inner.active_operations.load(Ordering::SeqCst) > 0
    }

    pub fn has_active_operation_for_repo(&self, repository_id: &str) -> bool {
        if self.inner.active_operations.load(Ordering::SeqCst) == 0 {
            return false;
        }
        self.inner
            .sessions
            .lock()
            .map(|sessions| {
                sessions.values().any(|session| {
                    session.repository_id == repository_id
                        && matches!(session.session_status, CPushSessionStatus::Running)
                })
            })
            .unwrap_or(false)
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
            .map_err(|_| "C-Push coordinator lock is unavailable")?
            .entry(worktree_id.to_owned())
            .or_default()
            .clone())
    }

    pub(super) async fn persist_all(&self, storage_dir: &Path) -> Result<(), String> {
        let _guard = self.inner.storage_gate.lock().await;
        let all: Vec<StoredCPushSession> = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .values()
            .cloned()
            .collect();
        storage::write_sessions_file_atomic(storage_dir, &all)
    }

    pub(super) fn save_in_memory(&self, session: &StoredCPushSession) -> Result<(), String> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .insert(session.worktree_id.clone(), session.clone());
        Ok(())
    }

    pub(super) fn remove_in_memory(&self, worktree_id: &str) -> Result<(), String> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .remove(worktree_id);
        Ok(())
    }

    pub(super) fn lookup_processed_request(
        &self,
        request_id: &str,
    ) -> Result<Option<CPushCommandReply>, String> {
        Ok(self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .get(request_id)
            .cloned())
    }

    pub(super) fn record_processed_request(
        &self,
        request_id: &str,
        reply: &CPushCommandReply,
    ) -> Result<(), String> {
        self.inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .insert(request_id.to_owned(), reply.clone());
        Ok(())
    }

    pub(super) async fn ensure_no_busy_session(
        &self,
        storage_dir: &Path,
        worktree_id: &str,
        locale: crate::i18n::LanguageKind,
        for_cpush: bool,
    ) -> Result<(), String> {
        let existing_active = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(worktree_id)
            .cloned();
        if let Some(active) = existing_active
            && helpers::is_active_status(active.session_status)
        {
            if active.plan_only && active.commits.is_empty() {
                let _ = self.remove_in_memory(worktree_id);
                let _ = self.persist_all(storage_dir).await;
            } else {
                return Err(
                    crate::i18n::cpush::cpush_active_session_busy_error(locale, for_cpush).into(),
                );
            }
        }
        Ok(())
    }

    pub(super) fn session_read_target(&self, session: &StoredCPushSession) -> ReadTarget {
        ReadTarget {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        }
    }
}
