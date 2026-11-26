//! Interactive user reply handling for C-Push questions, retries, upstream selection, and merges.

use super::{
    CPushCoordinator,
    helpers::{conflict_chips, enter_push_failed_state, summarize_commits},
    types::*,
};
use crate::{
    foundation::pi::{PiPaths, PiSupervisor},
    i18n,
    models::store::ModelStore,
    repository::{self, RepositoryStore},
};
use std::path::Path;

impl CPushCoordinator {
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
    ) -> Result<CPushCommandReply, String> {
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self.lookup_processed_request(&request.request_id)? {
            return Ok(existing_reply);
        }

        let mut session = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or_else(|| i18n::cpush::cpush_no_pending_session_error(req_locale))?;
        if let Some(loc) = request.locale {
            session.locale = loc;
        }
        let locale = session.locale;

        if session.run_id != request.run_id
            || session.repository_id != request.target.repository_id
            || session.worktree_id != request.target.worktree_id
        {
            return Err(i18n::cpush::cpush_mismatched_reply_target_error(locale).into());
        }
        if !matches!(
            session.session_status,
            CPushSessionStatus::NeedsDecision
                | CPushSessionStatus::Conflict
                | CPushSessionStatus::WaitingManual
                | CPushSessionStatus::PushFailed
        ) {
            return Err(i18n::cpush::cpush_not_interactive_error(locale).into());
        }
        let current_msg = session
            .message
            .clone()
            .ok_or_else(|| i18n::cpush::cpush_missing_active_message_error(locale))?;
        if current_msg.id != request.message_id {
            return Err(i18n::cpush::cpush_stale_message_id_error(locale).into());
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
            .await?
        } else if request.option_id.as_deref() == Some("retry-remaining") {
            if !session.can_retry_remaining
                || session.next_step_index >= session.planned_groups.len()
            {
                return Err(i18n::cpush::cpush_no_retry_remaining_error(locale).into());
            }
            self.execute_planned_groups(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await?
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
            .await?
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
            .await?
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
            .await?
        } else if request.option_id.as_deref() == Some("abort-rebase") {
            self.respond_abort_rebase(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await?
        } else if request.option_id.as_deref() == Some("manual-merge") {
            self.respond_manual_merge(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &current_msg,
                &mut on_progress,
            )
            .await?
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
                return Err(i18n::cpush::cpush_enter_merge_instruction_error(locale).into());
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
            self.respond_replan_decision(
                storage_dir,
                paths,
                supervisor,
                models,
                repo_store,
                request,
                &mut session,
                &current_msg,
                &mut on_progress,
            )
            .await?
        };

        self.record_processed_request(&request.request_id, &reply)?;
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
            self.remove_in_memory(&session.worktree_id)?;
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
                progress: None,
            });
            self.save_in_memory(session)?;
        }
        self.persist_all(storage_dir).await?;
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
            prepared_plan: None,
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
            self.remove_in_memory(&session.worktree_id)?;
        } else {
            let branch_name = session.branch.clone();
            enter_push_failed_state(
                session,
                &branch_name,
                i18n::cpush::cpush_aborted_rebase_reason(locale),
                Some(i18n::cpush::cpush_aborted_rebase_title(locale)),
            );
            self.save_in_memory(session)?;
        }
        self.persist_all(storage_dir).await?;
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
            prepared_plan: None,
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
            progress: None,
        });
        self.save_in_memory(session)?;
        self.persist_all(storage_dir).await?;
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
            prepared_plan: None,
            repository_snapshot: Some(snapshot),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn respond_replan_decision(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &RespondCPushRequest,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        if session.session_status != CPushSessionStatus::NeedsDecision {
            return Err(i18n::cpush::cpush_unsupported_reply_state_error(locale).into());
        }
        let answer_label = if let Some(opt_id) = &request.option_id {
            let matched = current_msg
                .options
                .as_ref()
                .and_then(|opts| opts.iter().find(|o| &o.id == opt_id))
                .ok_or_else(|| i18n::cpush::cpush_invalid_option_error(locale))?;
            matched.label.clone()
        } else if let Some(free_text) = &request.text {
            let trimmed = free_text.trim();
            if trimmed.is_empty() {
                return Err(i18n::cpush::cpush_empty_reply_text_error(locale).into());
            }
            trimmed.to_owned()
        } else {
            return Err(i18n::cpush::cpush_choose_option_or_text_error(locale).into());
        };

        let previous_question_msg = current_msg.clone();
        let previous_timeline = session.timeline.clone();

        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: answer_label.clone(),
            option_id: request.option_id.clone(),
        });
        session.session_status = CPushSessionStatus::Running;
        session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_confirmed_decision(
                    locale,
                    &current_msg.title,
                    &answer_label,
                ),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_replanning(locale).into(),
            },
        ];
        session.message = Some(CPushUiMessage {
            id: format!("progress-replan-{}", request.request_id),
            kind: "progress".into(),
            title: i18n::cpush::cpush_progress_replanning_title(locale).into(),
            description: i18n::cpush::cpush_progress_replanning_desc(locale, &answer_label),
            scope: Some(session.branch.clone()),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 2,
                total: 4,
                label: i18n::cpush::cpush_progress_replanning_steps(locale).into(),
            }),
        });
        self.save_in_memory(session)?;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let spec = match models
            .resolve_runtime_spec_with_locale(storage_dir, &session.model_id, locale)
            .await
        {
            Ok(spec) => spec,
            Err(err) => {
                session.qa_log.pop();
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.timeline = previous_timeline;
                session.message = Some(previous_question_msg);
                self.save_in_memory(session)?;
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: session.to_public(),
                    repository_snapshot: None,
                });
                return Err(err);
            }
        };

        let plan = match self
            .invoke_pi_planner(paths, supervisor, &spec, &request.request_id, session)
            .await
        {
            Ok(plan) => plan,
            Err(err) => {
                session.qa_log.pop();
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.timeline = previous_timeline;
                session.message = Some(previous_question_msg);
                self.save_in_memory(session)?;
                let _ = self.persist_all(storage_dir).await;
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: session.to_public(),
                    repository_snapshot: None,
                });
                return Err(err);
            }
        };

        self.apply_plan_or_execute(
            storage_dir,
            repo_store,
            &request.request_id,
            session,
            plan,
            on_progress,
        )
        .await
    }
}
