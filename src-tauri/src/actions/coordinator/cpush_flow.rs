//! New-engine C-Push orchestration (`start_cpush`):
//! runs on the unified `PiCommand::RunCustomAction { mode: Cpush }` tool loop with
//! live `message_user` Q&A, optional `read_scope_diff`/`read_repo_file`/`search_repo`,
//! hot-synced/disabled skills, Rust hard sensitive-file safety interception, batch
//! exact-scope commits, deterministic remote sync/push, and `stopAgent: true` 1-turn fast path.

use super::{
    CPUSH_AI_IDLE_TIMEOUT, CustomActionCoordinator, SessionAuthGrants,
    cpush_helpers::scope_badge,
    cpush_sensitive::{CPushLiveRunState, CPushToolContext, SharedProgressCb, emit_progress_event},
    cpush_tools::dispatch_cpush_tool,
};
use crate::{
    actions::types::*,
    error::{AppError, AppErrorCode},
    i18n,
    models::store::ModelStore,
    pi_runtime::{
        CustomActionRunMode, PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome,
        RequestContext,
    },
    repository::{self, RepositoryStore},
    skills::RuntimeSkillSnapshot,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn start_cpush<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &StartCPushRequest,
        on_progress: F,
    ) -> Result<CPushCommandReply, AppError>
    where
        F: FnMut(CPushProgressEvent) -> Result<(), String> + Send + 'static,
    {
        let map_action = |msg: String| AppError::action(AppErrorCode::ActionFailed, msg);
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::CPush,
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
        self.ensure_no_busy_cpush_session(&request.target.worktree_id, locale, true)
            .map_err(|msg| AppError::action(AppErrorCode::ActionBusy, msg).with_retryable(true))?;

        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let extracted = repo_store
            .extract_cpush_scope(&request.target, &request.selections, locale)
            .await
            .map_err(|msg| AppError::repository(AppErrorCode::GitFailed, msg))?;
        let run_id = request.request_id.clone();

        repo_store
            .preflight_commit_scope(&request.target, &request.selections, locale)
            .await?;

        let live_snapshot = repo_store
            .refresh_with_locale(&request.target, locale)
            .await
            .ok();
        let local_keep_paths: Vec<String> = live_snapshot
            .as_ref()
            .map(|s| s.repository.local_keep_paths.clone())
            .unwrap_or_default();
        let remote_branches: Vec<String> = live_snapshot
            .as_ref()
            .map(|s| {
                s.repository
                    .remote_branches
                    .iter()
                    .map(|b| b.name.clone())
                    .collect()
            })
            .unwrap_or_default();

        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));
        let badge = scope_badge(locale, &extracted.files);
        let initial_timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_scope_frozen(locale, &badge),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_planning(locale).into(),
            },
        ];
        let progress_msg = CPushUiMessage {
            id: format!("progress-{run_id}"),
            kind: "progress".into(),
            title: i18n::cpush::cpush_progress_planning_title(locale).into(),
            description: i18n::cpush::cpush_progress_planning_desc(locale).into(),
            scope: Some(badge),
            files: None,
            commits: None,
            options: None,
            links: None,
            card: None,
            questionnaire: None,
            progress: Some(CPushMessageProgress {
                completed: 1,
                total: 4,
                label: i18n::cpush::cpush_progress_planning_steps(locale).into(),
            }),
        };

        let push_authorized = request.push.unwrap_or(true);
        let session = StoredCPushSession {
            run_id: run_id.clone(),
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            frozen_scope_version: request.target.scope_version.clone(),
            model_id: request.model_id.clone(),
            branch: extracted.branch.clone(),
            initial_head: extracted.initial_head.clone(),
            prompt: request.prompt.clone(),
            recent_commits: extracted.recent_commits.clone(),
            unselected_paths: extracted.unselected_paths.clone(),
            frozen_files: extracted.files.clone(),
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: Vec::new(),
            commits: Vec::new(),
            qa_log: Vec::new(),
            timeline: initial_timeline,
            session_status: CPushSessionStatus::Running,
            pending_call_id: None,
            message: Some(progress_msg),
            can_retry_remaining: false,
            push_authorized,
            auto_collapse: false,
            notice_dismissed: false,
            locale,
            commit_language: request.commit_language,
        };

        let progress_cb: SharedProgressCb = Arc::new(Mutex::new(Box::new(on_progress)));
        self.save_cpush_in_memory(&session).map_err(map_action)?;
        let _ = self.persist_cpush_sessions(storage_dir).await;
        emit_progress_event(
            &progress_cb,
            CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: session.to_public(),
                repository_snapshot: None,
            },
        );

        let context = RequestContext {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        };
        let files_wire: Vec<PiPlanFileEntry> = session
            .frozen_files
            .iter()
            .map(|f| PiPlanFileEntry {
                file_id: f.file_id.clone(),
                path: f.path.clone(),
                previous_path: f.previous_path.clone(),
                status: f.status.clone(),
                range_kind: f.range_kind.clone(),
                added: f.added,
                removed: f.removed,
                ignored_for_review: f.ignored_for_review,
                binary: f.binary,
                patch: f.patch.clone(),
                skip_reason: f.skip_reason.clone(),
            })
            .collect();
        let command = PiCommand::RunCustomAction {
            mode: Some(CustomActionRunMode::Cpush),
            request_id: run_id.clone(),
            context: Box::new(context.clone()),
            model: Box::new(spec.to_pi_spec()),
            branch: session.branch.clone(),
            detached: false,
            remote_branches,
            action_label: "C-Push".into(),
            action_prompt: String::new(),
            extra_prompt: session.prompt.clone(),
            selected_files: files_wire,
            unselected_paths: session.unselected_paths.clone(),
            local_keep_paths,
            recent_commits: Some(session.recent_commits.clone()),
            push_authorized: Some(push_authorized),
            upstream: None,
            commits: None,
            conflict_files: None,
            locale,
            commit_language: request.commit_language,
            previous_checkpoint: None,
            follow_up_prompt: None,
            skill_overrides: skill_snapshot.catalog_option(),
        };

        let run_state = Arc::new(Mutex::new(CPushLiveRunState {
            session: session.clone(),
            qa_confirmed: false,
            user_cancelled: false,
            settled_reply: None,
        }));
        let tool_ctx = Arc::new(CPushToolContext {
            coordinator: self.clone(),
            store: repo_store.clone(),
            storage_dir: storage_dir.to_path_buf(),
            skills: skill_snapshot,
            grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
            run_state: Arc::clone(&run_state),
            progress_cb: Arc::clone(&progress_cb),
            request_id: request.request_id.clone(),
            locale,
        });

        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());

        let outcome = supervisor
            .run_task_with_tools(
                paths,
                run_id.clone(),
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                api_key_for_redaction.as_deref(),
                |_event| Ok(()),
                move |call_id, tool_name, args| {
                    dispatch_cpush_tool(Arc::clone(&tool_ctx), call_id, tool_name, args)
                },
            )
            .await;

        let (settled_reply, user_cancelled, latest_session) = {
            let mut st = run_state
                .lock()
                .map_err(|_| AppError::internal("C-Push run state lock is unavailable"))?;
            (
                st.settled_reply.take(),
                st.user_cancelled,
                st.session.clone(),
            )
        };

        if let Some(reply) = settled_reply {
            self.record_processed_cpush_request(&request.request_id, &reply)
                .map_err(map_action)?;
            return Ok(reply);
        }

        let reset_idle_and_emit = || async {
            let _ = self.remove_cpush_in_memory(&latest_session.worktree_id);
            let _ = self.persist_cpush_sessions(storage_dir).await;
            let idle_snapshot = CPushSessionSnapshot {
                session_status: CPushSessionStatus::Idle,
                pending_call_id: None,
                message: None,
                timeline: Vec::new(),
                ..latest_session.to_public()
            };
            emit_progress_event(
                &progress_cb,
                CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: idle_snapshot.clone(),
                    repository_snapshot: None,
                },
            );
            idle_snapshot
        };

        if user_cancelled || matches!(outcome, Ok(PiTaskOutcome::Cancelled)) {
            let idle_snapshot = reset_idle_and_emit().await;
            let reply = CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request.request_id.clone(),
                session: idle_snapshot,
                repository_snapshot: None,
            };
            self.record_processed_cpush_request(&request.request_id, &reply)
                .map_err(map_action)?;
            return Ok(reply);
        }

        match outcome {
            Ok(PiTaskOutcome::CustomActionCompleted { summary, .. }) => {
                let _ = reset_idle_and_emit().await;
                let detail = if summary.trim().is_empty() {
                    "Model finished without calling commit_exact_scope.".to_owned()
                } else {
                    summary
                };
                Err(AppError::action(AppErrorCode::ActionFailed, detail))
            }
            Ok(other) => {
                let _ = reset_idle_and_emit().await;
                Err(AppError::model(
                    AppErrorCode::PiRuntimeError,
                    format!("C-Push returned an unexpected event type: {other:?}"),
                ))
            }
            Err(err) => {
                let _ = reset_idle_and_emit().await;
                Err(err)
            }
        }
    }
}
