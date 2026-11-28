//! Unified zero-token Host Recovery & standalone Pull-Rebase orchestration (`run_session_recovery`).
//!
//! Serves ALL workflow sessions (built-in C-Push, standalone Pull & Rebase, and Custom Actions
//! handed off via `handoff_to_recovery` or final-turn Git conflict safety net).

use super::{
    CustomActionCoordinator,
    session_helpers::{
        conflict_chips, enter_conflict_state, enter_push_failed_state, summarize_commits,
    },
};
use crate::{
    actions::{session_store, types::*},
    error::{AppError, AppErrorCode},
    i18n,
    models::store::ModelStore,
    pi_runtime::{PiPaths, PiSupervisor},
    repository::{self, RepositoryStore, remote::RebaseSyncOutcome},
};
use std::path::Path;

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn run_session_recovery(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &SessionRecoveryRequest,
        mut on_progress: impl FnMut(ActionProgressEvent) -> Result<(), String> + Send,
    ) -> Result<ActionCommandReply, AppError> {
        if matches!(request.action, SessionRecoveryAction::PullRebase) {
            return self
                .execute_standalone_pull_rebase(storage_dir, repo_store, request, &mut on_progress)
                .await;
        }

        let map_action = |msg: String| AppError::action(AppErrorCode::ActionFailed, msg);
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::CPush,
            req_locale,
        )?;
        let wt_gate = self
            .worktree_gate(&request.target.worktree_id)
            .map_err(map_action)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self
            .lookup_processed_request(&request.request_id)
            .map_err(map_action)?
        {
            return Ok(existing_reply);
        }

        let mut session = self
            .inner
            .active_sessions
            .lock()
            .map_err(|_| AppError::internal("Session cache lock is unavailable"))?
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or_else(|| {
                AppError::action(
                    AppErrorCode::ActionInvalidInput,
                    i18n::cpush::cpush_no_pending_session_error(req_locale),
                )
            })?;
        if let Some(loc) = request.locale {
            session.locale = loc;
        }
        let locale = session.locale;

        let req_run_id = request.run_id.as_deref().map(str::trim).unwrap_or("");
        if req_run_id.is_empty()
            || session.run_id != req_run_id
            || session.repository_id != request.target.repository_id
            || session.worktree_id != request.target.worktree_id
        {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::cpush::cpush_mismatched_reply_target_error(locale),
            ));
        }
        if !session.status.is_interactive_recovery() {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::cpush::cpush_not_interactive_error(locale),
            ));
        }
        let current_msg = session.message.clone().ok_or_else(|| {
            AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::cpush::cpush_missing_active_message_error(locale),
            )
        })?;
        let req_msg_id = request.message_id.as_deref().map(str::trim).unwrap_or("");
        if req_msg_id.is_empty() || current_msg.id != req_msg_id {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::cpush::cpush_stale_message_id_error(locale),
            ));
        }
        session.auto_collapse = false;
        if let Some(req_model) = &request.model_id
            && !req_model.trim().is_empty()
            && session.model_id.is_empty()
        {
            session.model_id = req_model.trim().to_owned();
        }

        let reply = match &request.action {
            SessionRecoveryAction::DismissSession => self
                .recover_dismiss_session(
                    storage_dir,
                    &request.request_id,
                    &mut session,
                    &mut on_progress,
                )
                .await
                .map_err(map_action)?,
            SessionRecoveryAction::RetryRemainingCommits => {
                if !session.can_retry_remaining
                    || session.next_step_index >= session.planned_groups.len()
                {
                    return Err(AppError::action(
                        AppErrorCode::ActionInvalidInput,
                        i18n::cpush::cpush_no_retry_remaining_error(locale),
                    ));
                }
                self.execute_planned_groups(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &mut on_progress,
                )
                .await
                .map_err(map_action)?
            }
            SessionRecoveryAction::RetryPush {
                set_upstream_remote,
            } => {
                if let Some(chosen_remote) = set_upstream_remote.as_deref() {
                    session.qa_log.push(CPushQaItem {
                        question_id: current_msg.id.clone(),
                        question: current_msg.title.clone(),
                        answer: i18n::cpush::cpush_push_track_label(
                            session.locale,
                            chosen_remote,
                            &session.branch,
                        ),
                        option_id: Some(format!("push-set-upstream:{chosen_remote}")),
                    });
                }
                self.sync_and_push_session(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    set_upstream_remote.as_deref(),
                    None,
                    &mut on_progress,
                )
                .await
                .map_err(map_action)?
            }
            SessionRecoveryAction::SetUpstreamFreeText { text } => {
                if !current_msg.id.starts_with("missing-upstream-") {
                    return Err(AppError::action(
                        AppErrorCode::ActionInvalidInput,
                        i18n::cpush::cpush_unsupported_reply_state_error(locale),
                    ));
                }
                self.recover_set_upstream_free_text(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &current_msg,
                    text,
                    &mut on_progress,
                )
                .await
                .map_err(map_action)?
            }
            SessionRecoveryAction::AbortRebase => self
                .recover_abort_rebase(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &mut on_progress,
                )
                .await
                .map_err(map_action)?,
            SessionRecoveryAction::ManualMerge => self
                .recover_manual_merge(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &current_msg,
                    &mut on_progress,
                )
                .await
                .map_err(map_action)?,
            SessionRecoveryAction::AgentConflictMerge { instruction } => {
                if session.status != ActionSessionStatus::Conflict {
                    return Err(AppError::action(
                        AppErrorCode::ActionInvalidInput,
                        i18n::cpush::cpush_unsupported_reply_state_error(locale),
                    ));
                }
                let trimmed = instruction
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .to_owned();
                if instruction.is_some() && trimmed.is_empty() {
                    return Err(AppError::action(
                        AppErrorCode::ActionInvalidInput,
                        i18n::cpush::cpush_enter_merge_instruction_error(locale),
                    ));
                }
                self.execute_agent_conflict_merge(
                    storage_dir,
                    paths,
                    supervisor,
                    models,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &current_msg,
                    &trimmed,
                    &mut on_progress,
                )
                .await?
            }
            SessionRecoveryAction::PullRebase => unreachable!(),
        };

        self.record_processed_request(&request.request_id, &reply)
            .map_err(map_action)?;
        Ok(reply)
    }

    async fn execute_standalone_pull_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &SessionRecoveryRequest,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, AppError> {
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
            .lookup_processed_request(&request.request_id)
            .map_err(map_action)?
        {
            return Ok(existing_reply);
        }
        self.ensure_no_busy_session(&request.target.worktree_id, locale, false)?;

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
                let idle_session = ActionSessionSnapshot {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    action_id: BUILTIN_PULL_REBASE_ACTION_ID.into(),
                    action_label: "Pull & Rebase".into(),
                    profile: ActionProfileKind::PullRebase,
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: request.model_id.clone().unwrap_or_default(),
                    frozen_file_ids: Vec::new(),
                    committed_file_ids: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    status: ActionSessionStatus::Idle,
                    pending_call_id: None,
                    summary: String::new(),
                    message: None,
                    timeline: Vec::new(),
                    qa_log: Vec::new(),
                    commits: Vec::new(),
                    links: Vec::new(),
                    can_retry_remaining: false,
                    auto_collapse: None,
                };
                let reply = ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request.request_id.clone(),
                    session: idle_session,
                    trusted_action_update: None,
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_request(&request.request_id, &reply)
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
                let mut session = StoredActionSession {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    action_id: BUILTIN_PULL_REBASE_ACTION_ID.into(),
                    action_label: "Pull & Rebase".into(),
                    profile: ActionProfileKind::PullRebase,
                    action_prompt_hash: String::new(),
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
                    status: ActionSessionStatus::Conflict,
                    pending_call_id: None,
                    updated_at_ms: session_store::now_millis(),
                    summary: String::new(),
                    card: None,
                    message: None,
                    timeline: Vec::new(),
                    qa_log: Vec::new(),
                    commits: Vec::new(),
                    links: Vec::new(),
                    can_retry_remaining: false,
                    push_authorized: false,
                    auto_collapse: false,
                    notice_dismissed: false,
                    dismissed_message_id: None,
                    edited_file_hashes: Default::default(),
                    submit_ledger: Default::default(),
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
                self.save_session_in_memory(&session).map_err(map_action)?;
                self.persist_session(storage_dir, &session)
                    .await
                    .map_err(AppError::storage)?;
                let public = session.to_public();
                let _ = on_progress(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: public.clone(),
                    trusted_action_update: None,
                    repository_snapshot: Some(snapshot.clone()),
                });
                let reply = ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request.request_id.clone(),
                    session: public,
                    trusted_action_update: None,
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_request(&request.request_id, &reply)
                    .map_err(map_action)?;
                Ok(reply)
            }
        }
    }

    async fn recover_dismiss_session(
        &self,
        storage_dir: &Path,
        request_id: &str,
        session: &mut StoredActionSession,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, String> {
        let locale = session.locale;
        if session.commits.is_empty() {
            session.status = ActionSessionStatus::Idle;
            session.can_retry_remaining = false;
            session.message = None;
            self.remove_session_in_memory(&session.worktree_id)?;
            self.remove_persisted_session(storage_dir, &session.worktree_id, Some(&session.run_id))
                .await?;
        } else {
            session.status = ActionSessionStatus::Done;
            session.can_retry_remaining = false;
            let commit_summaries = summarize_commits(&session.commits);
            let title = i18n::cpush::cpush_dismiss_kept_local_title(locale, session.commits.len());
            session.summary = title.clone();
            session.message = Some(WorkflowUiMessage {
                id: format!("done-local-{}", session.run_id),
                kind: "done".into(),
                title,
                description: i18n::cpush::cpush_dismiss_kept_local_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: Some(commit_summaries),
                options: None,
                links: (!session.links.is_empty()).then_some(session.links.clone()),
                card: None,
                questionnaire: None,
                progress: None,
            });
            self.save_session_in_memory(session)?;
            self.persist_session(storage_dir, session).await?;
        }
        let public = session.to_public();
        let _ = on_progress(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: public.clone(),
            trusted_action_update: None,
            repository_snapshot: None,
        });
        Ok(ActionCommandReply {
            protocol_version: ACTION_PROTOCOL_VERSION,
            kind: "action",
            request_id: request_id.to_owned(),
            session: public,
            trusted_action_update: None,
            repository_snapshot: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn recover_set_upstream_free_text(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredActionSession,
        current_msg: &WorkflowUiMessage,
        free_text: &str,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, String> {
        let locale = session.locale;
        let trimmed = free_text.trim();
        if trimmed.is_empty() {
            return Err(i18n::cpush::cpush_enter_remote_or_select_error(locale).into());
        }
        let available: Vec<String> = current_msg
            .options
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|o| o.id.strip_prefix("push-set-upstream:").map(str::to_owned))
            .collect();
        let matched: Vec<&String> = available
            .iter()
            .filter(|r| trimmed.contains(r.as_str()))
            .collect();
        if matched.len() == 1 {
            let chosen = matched[0].clone();
            session.qa_log.push(CPushQaItem {
                question_id: current_msg.id.clone(),
                question: current_msg.title.clone(),
                answer: trimmed.to_owned(),
                option_id: None,
            });
            return self
                .sync_and_push_session(
                    storage_dir,
                    repo_store,
                    request_id,
                    session,
                    Some(&chosen),
                    None,
                    on_progress,
                )
                .await;
        }
        Err(i18n::cpush::cpush_ambiguous_remote_input_error(
            locale, &available,
        ))
    }

    async fn recover_abort_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredActionSession,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, String> {
        let locale = session.locale;
        let read_target = session.read_target();
        let snapshot = repo_store.abort_active_rebase(&read_target, locale).await?;
        if session.commits.is_empty() {
            session.status = ActionSessionStatus::Idle;
            session.message = None;
            self.remove_session_in_memory(&session.worktree_id)?;
            self.remove_persisted_session(storage_dir, &session.worktree_id, Some(&session.run_id))
                .await?;
        } else {
            let branch_name = session.branch.clone();
            enter_push_failed_state(
                session,
                &branch_name,
                i18n::cpush::cpush_aborted_rebase_reason(locale),
                Some(i18n::cpush::cpush_aborted_rebase_title(locale)),
            );
            self.save_session_in_memory(session)?;
            self.persist_session(storage_dir, session).await?;
        }
        let public = session.to_public();
        let _ = on_progress(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: public.clone(),
            trusted_action_update: None,
            repository_snapshot: Some(snapshot.clone()),
        });
        Ok(ActionCommandReply {
            protocol_version: ACTION_PROTOCOL_VERSION,
            kind: "action",
            request_id: request_id.to_owned(),
            session: public,
            trusted_action_update: None,
            repository_snapshot: Some(snapshot),
        })
    }

    async fn recover_manual_merge(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredActionSession,
        current_msg: &WorkflowUiMessage,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, String> {
        let locale = session.locale;
        let read_target = session.read_target();
        let (live, snapshot) = repo_store
            .inspect_live_remote_status(&read_target, false, locale)
            .await?;
        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: i18n::cpush::cpush_option_manual_merge(locale).into(),
            option_id: Some("manual-merge".into()),
        });
        session.status = ActionSessionStatus::WaitingManual;
        session.pending_call_id = None;
        session.can_retry_remaining = false;
        session.timeline = vec![
            WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_chose_manual_merge(locale).into(),
            },
            WorkflowTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_waiting_return_reconcile(locale).into(),
            },
        ];
        let chips = if !live.conflicted_files.is_empty() {
            Some(conflict_chips(locale, &live.conflicted_files))
        } else {
            current_msg.files.clone()
        };
        let title: String = i18n::cpush::cpush_waiting_manual_title(locale).into();
        session.summary = title.clone();
        session.message = Some(WorkflowUiMessage {
            id: format!("waiting-manual-{}", session.run_id),
            kind: "waitingManual".into(),
            title,
            description: i18n::cpush::cpush_waiting_manual_desc(locale).into(),
            scope: Some(i18n::cpush::cpush_waiting_manual_scope(locale).into()),
            files: chips,
            commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
            options: Some(vec![WorkflowMessageOption {
                id: "abort-rebase".into(),
                label: i18n::cpush::cpush_option_abort_rebase(locale, !session.commits.is_empty())
                    .into(),
                recommended: None,
            }]),
            links: (!session.links.is_empty()).then_some(session.links.clone()),
            card: None,
            questionnaire: None,
            progress: None,
        });
        self.save_session_in_memory(session)?;
        self.persist_session(storage_dir, session).await?;
        let public = session.to_public();
        let _ = on_progress(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: public.clone(),
            trusted_action_update: None,
            repository_snapshot: Some(snapshot.clone()),
        });
        Ok(ActionCommandReply {
            protocol_version: ACTION_PROTOCOL_VERSION,
            kind: "action",
            request_id: request_id.to_owned(),
            session: public,
            trusted_action_update: None,
            repository_snapshot: Some(snapshot),
        })
    }
}
