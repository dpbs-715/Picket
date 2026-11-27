//! Post-commit, push-retry, upstream selection, and conflict-choice response handlers for C-Push.

use super::{
    CustomActionCoordinator,
    cpush_helpers::{conflict_chips, enter_push_failed_state, summarize_commits},
};
use crate::{
    actions::types::*,
    error::{AppError, AppErrorCode},
    i18n,
    models::store::ModelStore,
    pi_runtime::{PiPaths, PiSupervisor},
    repository::{self, RepositoryStore},
};
use std::path::Path;

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn respond_cpush(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &RespondCPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, AppError> {
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
            .lookup_processed_cpush_request(&request.request_id)
            .map_err(map_action)?
        {
            return Ok(existing_reply);
        }

        let mut session = self
            .inner
            .cpush_sessions
            .lock()
            .map_err(|_| AppError::internal("C-Push session cache lock is unavailable"))?
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

        if session.run_id != request.run_id
            || session.repository_id != request.target.repository_id
            || session.worktree_id != request.target.worktree_id
        {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::cpush::cpush_mismatched_reply_target_error(locale),
            ));
        }
        if !matches!(
            session.session_status,
            CPushSessionStatus::NeedsDecision
                | CPushSessionStatus::Conflict
                | CPushSessionStatus::WaitingManual
                | CPushSessionStatus::PushFailed
        ) {
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
        if current_msg.id != request.message_id {
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

        let reply = if request.option_id.as_deref() == Some("dismiss-session") {
            self.respond_dismiss_session(
                storage_dir,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await
            .map_err(map_action)?
        } else if request.option_id.as_deref() == Some("retry-remaining") {
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
        } else if matches!(
            request.option_id.as_deref(),
            Some("retry-push" | "continue-push")
        ) {
            self.sync_and_push_commits(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                None,
                None,
                &mut on_progress,
            )
            .await
            .map_err(map_action)?
        } else if let Some(chosen_remote) = request
            .option_id
            .as_deref()
            .and_then(|opt| opt.strip_prefix("push-set-upstream:"))
        {
            self.respond_set_upstream_option(
                storage_dir,
                repo_store,
                request,
                &mut session,
                &current_msg,
                chosen_remote,
                &mut on_progress,
            )
            .await
            .map_err(map_action)?
        } else if current_msg.id.starts_with("missing-upstream-")
            && let Some(free_text) = &request.text
        {
            self.respond_set_upstream_free_text(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &current_msg,
                free_text,
                &mut on_progress,
            )
            .await
            .map_err(map_action)?
        } else if request.option_id.as_deref() == Some("abort-rebase") {
            self.respond_abort_rebase(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await
            .map_err(map_action)?
        } else if request.option_id.as_deref() == Some("manual-merge") {
            self.respond_manual_merge(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &current_msg,
                &mut on_progress,
            )
            .await
            .map_err(map_action)?
        } else if session.session_status == CPushSessionStatus::Conflict
            && (request.option_id.as_deref() == Some("agent-merge") || request.text.is_some())
        {
            let instruction = request
                .text
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_owned();
            if request.option_id.is_none() && instruction.is_empty() {
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
                &instruction,
                &mut on_progress,
            )
            .await?
        } else {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::cpush::cpush_unsupported_reply_state_error(locale),
            ));
        };

        self.record_processed_cpush_request(&request.request_id, &reply)
            .map_err(map_action)?;
        Ok(reply)
    }

    async fn respond_dismiss_session(
        &self,
        storage_dir: &Path,
        request_id: &str,
        session: &mut StoredCPushSession,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        if session.commits.is_empty() {
            session.session_status = CPushSessionStatus::Idle;
            session.can_retry_remaining = false;
            session.message = None;
            self.remove_cpush_in_memory(&session.worktree_id)?;
        } else {
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            let commit_summaries = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("done-local-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush::cpush_dismiss_kept_local_title(locale, session.commits.len()),
                description: i18n::cpush::cpush_dismiss_kept_local_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: Some(commit_summaries),
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: None,
            });
            self.save_cpush_in_memory(session)?;
        }
        self.persist_cpush_sessions(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: None,
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            repository_snapshot: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn respond_set_upstream_option(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &RespondCPushRequest,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        chosen_remote: &str,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let chosen = chosen_remote.to_owned();
        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: i18n::cpush::cpush_push_track_label(session.locale, &chosen, &session.branch),
            option_id: request.option_id.clone(),
        });
        self.sync_and_push_commits(
            storage_dir,
            repo_store,
            &request.request_id,
            session,
            Some(&chosen),
            None,
            on_progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn respond_set_upstream_free_text(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        free_text: &str,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
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
                .sync_and_push_commits(
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

    async fn respond_abort_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let read_target = self.session_read_target(session);
        let snapshot = repo_store.abort_active_rebase(&read_target, locale).await?;
        if session.commits.is_empty() {
            session.session_status = CPushSessionStatus::Idle;
            session.message = None;
            self.remove_cpush_in_memory(&session.worktree_id)?;
        } else {
            let branch_name = session.branch.clone();
            enter_push_failed_state(
                session,
                &branch_name,
                i18n::cpush::cpush_aborted_rebase_reason(locale),
                Some(i18n::cpush::cpush_aborted_rebase_title(locale)),
            );
            self.save_cpush_in_memory(session)?;
        }
        self.persist_cpush_sessions(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: Some(snapshot.clone()),
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            repository_snapshot: Some(snapshot),
        })
    }

    async fn respond_manual_merge(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let read_target = self.session_read_target(session);
        let (live, snapshot) = repo_store
            .inspect_live_remote_status(&read_target, false, locale)
            .await?;
        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: i18n::cpush::cpush_option_manual_merge(locale).into(),
            option_id: Some("manual-merge".into()),
        });
        session.session_status = CPushSessionStatus::WaitingManual;
        session.pending_call_id = None;
        session.can_retry_remaining = false;
        session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_chose_manual_merge(locale).into(),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_waiting_return_reconcile(locale).into(),
            },
        ];
        let chips = if !live.conflicted_files.is_empty() {
            Some(conflict_chips(locale, &live.conflicted_files))
        } else {
            current_msg.files.clone()
        };
        session.message = Some(CPushUiMessage {
            id: format!("waiting-manual-{}", session.run_id),
            kind: "waitingManual".into(),
            title: i18n::cpush::cpush_waiting_manual_title(locale).into(),
            description: i18n::cpush::cpush_waiting_manual_desc(locale).into(),
            scope: Some(i18n::cpush::cpush_waiting_manual_scope(locale).into()),
            files: chips,
            commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
            options: Some(vec![CPushMessageOption {
                id: "abort-rebase".into(),
                label: i18n::cpush::cpush_option_abort_rebase(locale, !session.commits.is_empty())
                    .into(),
                recommended: None,
            }]),
            links: None,
            card: None,
            questionnaire: None,
            progress: None,
        });
        self.save_cpush_in_memory(session)?;
        self.persist_cpush_sessions(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: Some(snapshot.clone()),
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            repository_snapshot: Some(snapshot),
        })
    }
}
