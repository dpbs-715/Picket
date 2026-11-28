//! Standalone Pull & Rebase workflow (`push_authorized = false`, `R06`).

use super::{CustomActionCoordinator, cpush_helpers::enter_conflict_state};
use crate::{
    actions::types::*,
    error::{AppError, AppErrorCode},
    i18n,
    repository::{self, RepositoryStore, remote::RebaseSyncOutcome},
};
use std::path::Path;

impl CustomActionCoordinator {
    /// Standalone Pull & Rebase (`push_authorized = false`, `R06`).
    pub async fn pull_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &PullRebaseRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, AppError> {
        let map_action = |msg: String| AppError::action(AppErrorCode::ActionFailed, msg);
        let locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::PullRebase,
            locale,
        )?;
        let wt_gate = self
            .worktree_gate(&request.target.worktree_id)
            .map_err(map_action)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self
            .lookup_processed_cpush_request(&request.request_id)
            .map_err(map_action)?
        {
            return Ok(existing_reply);
        }
        self.ensure_no_busy_cpush_session(&request.target.worktree_id, locale, false)?;

        let outcome = repo_store
            .sync_rebase_onto_upstream_with_locale(&request.target, locale)
            .await
            .map_err(|msg| AppError::git(AppErrorCode::GitFailed, msg))?;
        match outcome {
            RebaseSyncOutcome::MissingUpstream { remotes, .. } => Err(AppError::git(
                AppErrorCode::GitFailed,
                i18n::cpush::pull_rebase_missing_upstream_error(locale, &remotes),
            )),
            RebaseSyncOutcome::UnsafeDirtyOverlap {
                upstream,
                overlapping_paths,
                ..
            } => Err(AppError::git(
                AppErrorCode::GitFailed,
                i18n::cpush::pull_rebase_dirty_overlap_error(locale, &upstream, &overlapping_paths),
            )),
            RebaseSyncOutcome::UnsafePublishedHistory { reason, .. } => {
                Err(AppError::git(AppErrorCode::GitFailed, reason))
            }
            RebaseSyncOutcome::AlreadyUpToDate { snapshot, .. }
            | RebaseSyncOutcome::Completed { snapshot, .. } => {
                let idle_session = CPushSessionSnapshot {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: request.model_id.clone().unwrap_or_default(),
                    frozen_file_ids: Vec::new(),
                    committed_file_ids: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    session_status: CPushSessionStatus::Idle,
                    pending_call_id: None,
                    message: None,
                    timeline: Vec::new(),
                    qa_log: Vec::new(),
                    commits: Vec::new(),
                    can_retry_remaining: false,
                    auto_collapse: None,
                };
                let reply = CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request.request_id.clone(),
                    session: idle_session,
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_cpush_request(&request.request_id, &reply)
                    .map_err(map_action)?;
                Ok(reply)
            }
            RebaseSyncOutcome::Conflict {
                upstream,
                conflicted_files,
                snapshot,
            } => {
                let active_tree = snapshot
                    .worktrees
                    .iter()
                    .find(|w| w.worktree_id == request.target.worktree_id);
                let branch = active_tree
                    .map(|w| w.branch.clone())
                    .unwrap_or_else(|| "HEAD".into());
                let model_id = request.model_id.clone().unwrap_or_default();
                let mut session = StoredCPushSession {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: model_id.clone(),
                    branch,
                    initial_head: active_tree.and_then(|w| w.head.clone()),
                    prompt: String::new(),
                    recent_commits: Vec::new(),
                    unselected_paths: Vec::new(),
                    frozen_files: Vec::new(),
                    planned_groups: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    next_step_index: 0,
                    committed_file_ids: Vec::new(),
                    commits: Vec::new(),
                    qa_log: Vec::new(),
                    timeline: Vec::new(),
                    session_status: CPushSessionStatus::Conflict,
                    pending_call_id: None,
                    message: None,
                    can_retry_remaining: false,
                    push_authorized: false,
                    auto_collapse: false,
                    notice_dismissed: false,
                    locale,
                    commit_language: locale,
                };
                enter_conflict_state(
                    &mut session,
                    &upstream,
                    &conflicted_files,
                    !model_id.is_empty(),
                    None,
                );
                self.save_cpush_in_memory(&session).map_err(map_action)?;
                self.persist_cpush_sessions(storage_dir)
                    .await
                    .map_err(AppError::storage)?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: Some(snapshot.clone()),
                });
                let reply = CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request.request_id.clone(),
                    session: public,
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_cpush_request(&request.request_id, &reply)
                    .map_err(map_action)?;
                Ok(reply)
            }
        }
    }
}
