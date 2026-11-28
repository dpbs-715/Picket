//! Unified Action session orchestration (`start_action`) and profile-gated host-tool dispatcher.
//!
//! Serves both built-in C-Push (`ActionProfileKind::Cpush`) and user-defined Custom Action buttons
//! (`ActionProfileKind::CustomAction`) through one coordinator pipeline, one tool dispatcher, and
//! one `StoredActionSession` state machine.

use super::{
    CustomActionCoordinator, SessionAuthGrants,
    context::{
        ActionRunState, ActionToolContext, ProgressEmitFn, RawEventEmitFn, SessionPersistMeta,
        append_unique_links, format_tool_status_label, parse_status_str, push_timeline_step,
    },
    session_helpers::{
        enter_conflict_state, enter_push_failed_state, scope_badge, summarize_commits,
    },
    tools_cli, tools_command, tools_commit, tools_submit, tools_sync, tools_workspace,
};
use crate::{
    actions::{
        cli_env::detect_repo_remotes,
        profile::{ActionProfileKind, BUILTIN_CPUSH_ACTION_ID, RecoveryHandoffPolicy},
        session_store,
        types::*,
    },
    error::{AppError, AppErrorCode},
    i18n,
    models::store::ModelStore,
    pi_runtime::{
        CustomActionRunMode, PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome,
        PiToolExecutionResult, RequestContext,
    },
    repository::{self, RepositoryStore, ai_context::FrozenCPushFile, types::Remote},
    skills::RuntimeSkillSnapshot,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
};

fn format_upstream_summary(remote: &Remote) -> Option<String> {
    match remote {
        Remote::Tracked {
            branch,
            ahead,
            behind,
            local_only,
        } => {
            if *local_only {
                Some(format!(
                    "{branch} (ahead {ahead}, behind {behind}, local cache only)"
                ))
            } else {
                Some(format!("{branch} (ahead {ahead}, behind {behind})"))
            }
        }
        Remote::Missing { reason } => Some(format!("no upstream ({reason})")),
        Remote::Error { reason } => Some(format!("remote error ({reason})")),
    }
}

async fn dispatch_action_tool(
    ctx: Arc<ActionToolContext>,
    call_id: String,
    tool_name: String,
    args: Value,
) -> PiToolExecutionResult {
    if let Ok(st) = ctx.run_state.lock()
        && (st.settled_reply.is_some() || st.user_cancelled)
    {
        return PiToolExecutionResult::error("Action already settled or cancelled by user.");
    }
    if !ctx.profile.allows_tool(&tool_name) {
        return PiToolExecutionResult::error(format!(
            "Tool `{tool_name}` is not permitted for execution profile `{:?}`.",
            ctx.profile.kind
        ));
    }

    let is_cpush = ctx.profile.kind == ActionProfileKind::Cpush;
    let status_label = format_tool_status_label(ctx.locale, &tool_name, &args);
    let track_timeline = !is_cpush
        && tool_name != "read_skill_override_doc"
        && tool_name != "message_user"
        && tool_name != "handoff_to_recovery";

    if track_timeline {
        let preserved_card = if let Ok(mut st) = ctx.run_state.lock() {
            push_timeline_step(&mut st.timeline, "active", status_label.clone());
            st.published_card.clone()
        } else {
            None
        };
        (ctx.emit)(
            "running",
            None,
            Some(WorkflowUiMessage {
                id: format!("progress-{}", ctx.run_id),
                kind: "progress".into(),
                title: preserved_card
                    .as_ref()
                    .map(|c| c.title.clone())
                    .unwrap_or_else(|| ctx.action_label.clone()),
                description: status_label.clone(),
                scope: None,
                files: None,
                commits: None,
                options: None,
                links: None,
                card: preserved_card,
                questionnaire: None,
                progress: Some(WorkflowMessageProgress {
                    completed: 2,
                    total: 3,
                    label: status_label.clone(),
                }),
            }),
            None,
        );
    }

    let result = match tool_name.as_str() {
        "read_skill_override_doc" => tools_cli::handle_read_skill_override_doc(&ctx, &args),
        "read_scope_diff" => tools_cli::handle_read_scope_diff(&ctx, &args, status_label).await,
        "read_repo_file" | "search_repo" => {
            tools_cli::handle_read_repo_tool(&ctx, &tool_name, &args, status_label).await
        }
        "check_cli_env" => tools_cli::handle_check_cli_env(&ctx, status_label).await,
        "install_platform_cli" => tools_cli::handle_install_platform_cli(&ctx, &args).await,
        "message_user" => tools_cli::handle_message_user(&ctx, &call_id, args).await,
        "run_git" => tools_cli::handle_run_git(&ctx, &call_id, &args, status_label).await,
        "run_platform_cli" => {
            tools_cli::handle_run_platform_cli(&ctx, &call_id, &args, status_label).await
        }
        "edit_repo_file" => tools_workspace::handle_edit_repo_file(&ctx, &call_id, &args).await,
        "write_repo_file" => tools_workspace::handle_write_repo_file(&ctx, &call_id, &args).await,
        "run_repo_command" => {
            tools_command::handle_run_repo_command(&ctx, &call_id, &args, status_label).await
        }
        "commit_exact_scope" => {
            tools_commit::handle_commit_exact_scope(&ctx, &call_id, &args).await
        }
        "sync_and_push" => tools_sync::handle_sync_and_push(&ctx, &args).await,
        "handoff_to_recovery" => tools_sync::handle_handoff_to_recovery(&ctx, &args).await,
        "manage_submit_worktree" => {
            tools_submit::handle_manage_submit_worktree(&ctx, &args, status_label).await
        }
        "cleanup_submit_duplicates" => {
            tools_submit::handle_cleanup_submit_duplicates(&ctx, &args, status_label).await
        }
        "record_action_checkpoint" => {
            tools_submit::handle_record_action_checkpoint(&ctx, &args, status_label).await
        }
        other => PiToolExecutionResult::error(format!(
            "Unsupported tool `{other}` for action execution."
        )),
    };

    if track_timeline
        && let Ok(mut st) = ctx.run_state.lock()
        && let Some(last) = st.timeline.last_mut()
        && last.state == "active"
    {
        last.state = if result.ok && !result.is_error {
            "done".into()
        } else {
            "failed".into()
        };
    }

    result
}

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn start_action<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        store: &RepositoryStore,
        request: &StartActionRequest,
        on_progress: F,
    ) -> Result<ActionCommandReply, AppError>
    where
        F: Fn(ActionProgressEvent) + Send + Sync + 'static,
    {
        let map_action = |msg: String| AppError::action(AppErrorCode::ActionFailed, msg);
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;

        if !matches!(
            request.profile,
            ActionProfileKind::Cpush | ActionProfileKind::CustomAction
        ) {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                format!(
                    "Unsupported execution profile `{:?}` for start_action.",
                    request.profile
                ),
            ));
        }

        let profile = request.profile.profile();
        let is_cpush = profile.kind == ActionProfileKind::Cpush;

        let (action_id, action_label, action_prompt, expected_hash) = if is_cpush {
            (
                BUILTIN_CPUSH_ACTION_ID.to_owned(),
                "C-Push".to_owned(),
                String::new(),
                String::new(),
            )
        } else {
            let Some(ref cfg) = request.action else {
                return Err(AppError::action(
                    AppErrorCode::ActionInvalidInput,
                    i18n::actions::action_empty_fields_error(locale),
                ));
            };
            let label = cfg.label.trim().to_owned();
            let prompt = cfg.prompt.replace("\r\n", "\n").trim().to_owned();
            if label.is_empty() || prompt.is_empty() {
                return Err(AppError::action(
                    AppErrorCode::ActionInvalidInput,
                    i18n::actions::action_empty_fields_error(locale),
                ));
            }
            let hash = compute_prompt_hash(&prompt);
            (cfg.id.clone(), label, prompt, hash)
        };

        let write_kind = if is_cpush {
            crate::repo_lock::RepoWriteKind::CPush
        } else {
            crate::repo_lock::RepoWriteKind::CustomAction
        };
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            write_kind,
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
        self.ensure_no_busy_session(&request.target.worktree_id, locale, is_cpush)?;

        let run_id = request.request_id.clone();
        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());

        let worktree_root = store
            .worktree_root_for_target(&request.target, locale)
            .await
            .map_err(|msg| AppError::repository(AppErrorCode::RepoInvalidTarget, msg))?;

        let (
            branch,
            detached,
            initial_head,
            upstream_summary,
            remote_branches,
            local_keep_paths,
            live_changes,
            frozen_files,
            effective_frozen_selections,
            recent_commits,
            unselected_paths,
        ) = if profile.require_selected_scope {
            let extracted = store
                .extract_cpush_scope(&request.target, &request.selections, locale)
                .await
                .map_err(|msg| AppError::repository(AppErrorCode::GitFailed, msg))?;
            store
                .preflight_commit_scope(&request.target, &request.selections, locale)
                .await?;
            let live_snap = store
                .refresh_with_locale(&request.target, locale)
                .await
                .ok();
            let active_wt = live_snap.as_ref().and_then(|s| {
                s.worktrees
                    .iter()
                    .find(|w| w.worktree_id == request.target.worktree_id)
            });
            let local_keep = live_snap
                .as_ref()
                .map(|s| s.repository.local_keep_paths.clone())
                .unwrap_or_default();
            let r_branches = live_snap
                .as_ref()
                .map(|s| {
                    s.repository
                        .remote_branches
                        .iter()
                        .map(|b| b.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            let changes = active_wt.map(|w| w.files.clone()).unwrap_or_default();
            let upstream = active_wt.and_then(|w| format_upstream_summary(&w.remote));
            (
                extracted.branch,
                false,
                extracted.initial_head,
                upstream,
                r_branches,
                local_keep,
                changes,
                extracted.files,
                request.selections.clone(),
                extracted.recent_commits,
                extracted.unselected_paths,
            )
        } else {
            let live_snap = store
                .refresh_with_locale(&request.target, locale)
                .await
                .map_err(|msg| AppError::repository(AppErrorCode::GitFailed, msg))?;
            let active_wt = live_snap
                .worktrees
                .iter()
                .find(|wt| wt.worktree_id == request.target.worktree_id)
                .ok_or_else(|| {
                    AppError::repository(
                        AppErrorCode::RepoInvalidTarget,
                        "Target worktree not found in live repository snapshot",
                    )
                })?;
            let mut frozen_files: Vec<FrozenCPushFile> = Vec::new();
            let mut effective_selections: Vec<crate::repository::types::CommitSelection> =
                Vec::new();
            let mut recent_commits: Vec<String> = Vec::new();
            let unselected_paths: Vec<String>;
            if !request.selections.is_empty() {
                if let Ok(scope) = store
                    .extract_cpush_scope(&request.target, &request.selections, locale)
                    .await
                {
                    frozen_files = scope.files;
                    effective_selections = request.selections.clone();
                    recent_commits = scope.recent_commits;
                    unselected_paths = scope.unselected_paths;
                } else {
                    // Selection tokens were stale or HEAD was detached: allow read-only custom
                    // actions to proceed while treating all worktree changes as unselected so
                    // write/commit tools strictly reject unvalidated files.
                    unselected_paths = active_wt.files.iter().map(|f| f.path.clone()).collect();
                }
            } else {
                unselected_paths = active_wt.files.iter().map(|f| f.path.clone()).collect();
            }
            let r_branches = live_snap
                .repository
                .remote_branches
                .iter()
                .map(|b| b.name.clone())
                .collect();
            (
                active_wt.branch.clone(),
                active_wt.detached,
                active_wt.head.clone(),
                format_upstream_summary(&active_wt.remote),
                r_branches,
                live_snap.repository.local_keep_paths.clone(),
                active_wt.files.clone(),
                frozen_files,
                effective_selections,
                recent_commits,
                unselected_paths,
            )
        };

        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));
        let local_keep_paths_set: HashSet<String> = local_keep_paths.iter().cloned().collect();

        let follow_up_prompt = request
            .follow_up_prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let has_follow_up = follow_up_prompt.is_some();

        let (previous_checkpoint, initial_ledger, initial_edited_hashes) = if is_cpush {
            (None, Default::default(), HashMap::new())
        } else {
            let prior_session =
                request
                    .resume_from_run_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .and_then(|rid| {
                        session_store::find_session_by_run_id(
                            storage_dir,
                            &request.target.repository_id,
                            &request.target.worktree_id,
                            rid,
                        )
                    })
                    .or_else(|| {
                        session_store::find_latest_resumable_session(
                            storage_dir,
                            &request.target.repository_id,
                            &request.target.worktree_id,
                            Some(&action_id),
                        )
                        .or_else(|| {
                            session_store::find_latest_resumable_session(
                                storage_dir,
                                &request.target.repository_id,
                                &request.target.worktree_id,
                                None,
                            )
                        })
                        .filter(|prior| {
                            has_follow_up
                                || matches!(
                                    prior.status,
                                    ActionSessionStatus::Interrupted
                                        | ActionSessionStatus::Failed
                                        | ActionSessionStatus::Cancelled
                                )
                                || (prior.status == ActionSessionStatus::Done
                                    && (live_changes.is_empty()
                                        || !prior.edited_file_hashes.is_empty()
                                        || prior.submit_ledger.groups.iter().any(|g| {
                                            g.worktree_path.is_some() && !g.worktree_removed
                                        })))
                        })
                    });
            let checkpoint = prior_session
                .as_ref()
                .map(session_store::format_resume_checkpoint_for_prompt);
            let ledger = prior_session
                .as_ref()
                .map(|s| s.submit_ledger.clone())
                .unwrap_or_default();
            let edited = prior_session
                .as_ref()
                .filter(|s| !s.edited_file_hashes.is_empty())
                .map(|s| {
                    tools_workspace::hydrate_verified_edited_hashes(
                        &worktree_root,
                        &s.edited_file_hashes,
                        &local_keep_paths_set,
                    )
                })
                .unwrap_or_default();
            (checkpoint, ledger, edited)
        };

        let push_authorized = request.push.unwrap_or(profile.commit_auto_push_default);
        let (initial_timeline, initial_msg) = if is_cpush {
            let badge = scope_badge(locale, &frozen_files);
            let tl = vec![
                WorkflowTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush::cpush_timeline_scope_frozen(locale, &badge),
                },
                WorkflowTimelineItem {
                    state: "active".into(),
                    label: i18n::cpush::cpush_timeline_planning(locale).into(),
                },
            ];
            let msg = WorkflowUiMessage {
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
                progress: Some(WorkflowMessageProgress {
                    completed: 1,
                    total: 4,
                    label: i18n::cpush::cpush_progress_planning_steps(locale).into(),
                }),
            };
            (tl, msg)
        } else {
            let tl = vec![WorkflowTimelineItem {
                state: "active".into(),
                label: i18n::actions::action_preparing_label(locale, &action_label),
            }];
            let msg = WorkflowUiMessage {
                id: format!("progress-{run_id}"),
                kind: "progress".into(),
                title: action_label.clone(),
                description: i18n::actions::action_analyzing_desc(locale).into(),
                scope: None,
                files: None,
                commits: None,
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: Some(WorkflowMessageProgress {
                    completed: 1,
                    total: 3,
                    label: i18n::actions::action_started_label(locale).into(),
                }),
            };
            (tl, msg)
        };

        let run_state = Arc::new(Mutex::new(ActionRunState {
            status: ActionSessionStatus::Running,
            pending_call_id: None,
            message: Some(initial_msg.clone()),
            summary: String::new(),
            timeline: initial_timeline,
            qa_log: Vec::new(),
            commits: Vec::new(),
            links: Vec::new(),
            published_card: None,
            frozen_files: frozen_files.clone(),
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: HashSet::new(),
            committed_file_order: Vec::new(),
            action_edited_hashes: initial_edited_hashes,
            committed_edited_paths: HashSet::new(),
            trusted_action_update: None,
            wrote_repo: false,
            submit_ledger: initial_ledger,
            branch: branch.clone(),
            initial_head,
            prompt: request.extra_prompt.clone(),
            recent_commits: recent_commits.clone(),
            unselected_paths: unselected_paths.clone(),
            model_id: request.model_id.clone(),
            push_authorized,
            can_retry_remaining: false,
            auto_collapse: false,
            qa_confirmed: false,
            user_cancelled: false,
            settled_reply: None,
            recovery_handles: HashMap::new(),
            next_recovery_seq: 0,
        }));

        let raw_emit: RawEventEmitFn = Arc::new(on_progress);
        let persist_meta_owned = (
            run_id.clone(),
            request.target.repository_id.clone(),
            request.target.worktree_id.clone(),
            request.target.session_id.clone(),
            action_id.clone(),
            action_label.clone(),
            profile.kind,
            expected_hash.clone(),
            request.target.scope_version.clone(),
            locale,
            request.commit_language,
        );

        let build_stored = {
            let run_state = Arc::clone(&run_state);
            let meta_owned = persist_meta_owned.clone();
            move || -> Option<StoredActionSession> {
                let meta = SessionPersistMeta {
                    run_id: &meta_owned.0,
                    repository_id: &meta_owned.1,
                    worktree_id: &meta_owned.2,
                    session_id: &meta_owned.3,
                    action_id: &meta_owned.4,
                    action_label: &meta_owned.5,
                    profile: meta_owned.6,
                    action_prompt_hash: &meta_owned.7,
                    frozen_scope_version: &meta_owned.8,
                    locale: meta_owned.9,
                    commit_language: meta_owned.10,
                };
                run_state.lock().ok().map(|st| st.to_stored_session(&meta))
            }
        };

        let emit_progress: ProgressEmitFn = Arc::new({
            let coordinator = self.clone();
            let run_state = Arc::clone(&run_state);
            let raw_emit = Arc::clone(&raw_emit);
            let meta_owned = persist_meta_owned.clone();
            move |status_str, pending_call_id, message, repository_snapshot| {
                let parsed_status = parse_status_str(status_str);
                let (stored, trusted) = {
                    let meta = SessionPersistMeta {
                        run_id: &meta_owned.0,
                        repository_id: &meta_owned.1,
                        worktree_id: &meta_owned.2,
                        session_id: &meta_owned.3,
                        action_id: &meta_owned.4,
                        action_label: &meta_owned.5,
                        profile: meta_owned.6,
                        action_prompt_hash: &meta_owned.7,
                        frozen_scope_version: &meta_owned.8,
                        locale: meta_owned.9,
                        commit_language: meta_owned.10,
                    };
                    let Ok(mut st) = run_state.lock() else {
                        return;
                    };
                    st.status = parsed_status;
                    st.pending_call_id = pending_call_id;
                    if message.is_some() {
                        st.message = message;
                    }
                    (
                        st.to_stored_session(&meta),
                        st.trusted_action_update.clone(),
                    )
                };
                let _ = coordinator.save_session_in_memory(&stored);
                raw_emit(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: stored.to_public(),
                    trusted_action_update: trusted,
                    repository_snapshot,
                });
            }
        });

        // Save initial running session and emit initial progress event:
        if let Some(initial_stored) = build_stored() {
            self.save_session_in_memory(&initial_stored)
                .map_err(map_action)?;
            let _ = self.persist_session(storage_dir, &initial_stored).await;
            raw_emit(ActionProgressEvent {
                protocol_version: ACTION_PROTOCOL_VERSION,
                session: initial_stored.to_public(),
                trusted_action_update: None,
                repository_snapshot: None,
            });
        }

        let files_wire: Vec<PiPlanFileEntry> = frozen_files
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

        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };

        let command = PiCommand::RunCustomAction {
            mode: if is_cpush {
                Some(CustomActionRunMode::Cpush)
            } else {
                None
            },
            request_id: run_id.clone(),
            context: Box::new(context.clone()),
            model: Box::new(spec.to_pi_spec()),
            branch: branch.clone(),
            detached,
            remote_branches,
            action_label: action_label.clone(),
            action_prompt,
            extra_prompt: request.extra_prompt.clone(),
            selected_files: files_wire,
            unselected_paths,
            local_keep_paths,
            recent_commits: (!recent_commits.is_empty()).then_some(recent_commits),
            push_authorized: is_cpush.then_some(push_authorized),
            upstream: if is_cpush { None } else { upstream_summary },
            commits: None,
            conflict_files: None,
            locale,
            commit_language: request.commit_language,
            approval_mode: (!is_cpush).then_some(request.approval_mode),
            previous_checkpoint,
            follow_up_prompt,
            skill_overrides: skill_snapshot.catalog_option(),
        };

        let remotes = if is_cpush {
            Vec::new()
        } else {
            detect_repo_remotes(&worktree_root).await
        };
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let tool_ctx = Arc::new(ActionToolContext {
            coordinator: self.clone(),
            store: store.clone(),
            profile,
            emit: Arc::clone(&emit_progress),
            raw_emit: Arc::clone(&raw_emit),
            run_state: Arc::clone(&run_state),
            grants,
            skills: skill_snapshot,
            root: worktree_root,
            storage_dir: storage_dir.to_path_buf(),
            bundled_node_path: paths.node.clone(),
            remotes,
            frozen_selections: effective_frozen_selections,
            live_changes,
            local_keep_paths: local_keep_paths_set,
            target: request.target.clone(),
            request_id: request.request_id.clone(),
            run_id: run_id.clone(),
            action_id: action_id.clone(),
            action_label: action_label.clone(),
            expected_hash: expected_hash.clone(),
            locale,
            commit_language: request.commit_language,
            approval_mode: request.approval_mode,
        });

        let _pi_cancel_guard = self.register_active_pi_request(&run_id, &run_id);
        let outcome = supervisor
            .run_task_with_tools(
                paths,
                run_id.clone(),
                context,
                &command,
                profile.idle_timeout,
                api_key_for_redaction.as_deref(),
                |_event| Ok(()),
                move |call_id, tool_name, args| {
                    dispatch_action_tool(Arc::clone(&tool_ctx), call_id, tool_name, args)
                },
            )
            .await;

        let (settled_reply, user_cancelled) = {
            let mut st = run_state
                .lock()
                .map_err(|_| AppError::internal("Action run state lock is unavailable"))?;
            (st.settled_reply.take(), st.user_cancelled)
        };

        if let Some(reply) = settled_reply {
            self.record_processed_request(&request.request_id, &reply)
                .map_err(map_action)?;
            return Ok(reply);
        }

        if is_cpush {
            let reset_idle = || async {
                let _ = self.remove_session_in_memory(&request.target.worktree_id);
                let _ = self
                    .remove_persisted_session(
                        storage_dir,
                        &request.target.worktree_id,
                        Some(&run_id),
                    )
                    .await;
                let mut idle = build_stored().map(|s| s.to_public()).unwrap_or_else(|| {
                    ActionSessionSnapshot {
                        run_id: run_id.clone(),
                        repository_id: request.target.repository_id.clone(),
                        worktree_id: request.target.worktree_id.clone(),
                        session_id: request.target.session_id.clone(),
                        action_id: action_id.clone(),
                        action_label: action_label.clone(),
                        profile: ActionProfileKind::Cpush,
                        frozen_scope_version: request.target.scope_version.clone(),
                        model_id: request.model_id.clone(),
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
                    }
                });
                idle.status = ActionSessionStatus::Idle;
                idle.pending_call_id = None;
                idle.message = None;
                idle.timeline.clear();
                raw_emit(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: idle.clone(),
                    trusted_action_update: None,
                    repository_snapshot: None,
                });
                idle
            };

            if user_cancelled || matches!(outcome, Ok(PiTaskOutcome::Cancelled)) {
                // Never delete or hide landed commits or an active rebase conflict on cancel.
                if let Some(mut existing) = build_stored()
                    && let Ok((live, live_snap)) = store
                        .inspect_live_remote_status(&request.target, false, locale)
                        .await
                {
                    if live.rebase_in_progress && !live.conflicted_files.is_empty() {
                        let upstream = live
                            .upstream
                            .as_ref()
                            .map(|u| u.display.as_str())
                            .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale));
                        let has_model = !existing.model_id.is_empty();
                        enter_conflict_state(
                            &mut existing,
                            upstream,
                            &live.conflicted_files,
                            has_model,
                            None,
                        );
                        let _ = self.save_session_in_memory(&existing);
                        let _ = self.persist_session(storage_dir, &existing).await;
                        let public = existing.to_public();
                        raw_emit(ActionProgressEvent {
                            protocol_version: ACTION_PROTOCOL_VERSION,
                            session: public.clone(),
                            trusted_action_update: None,
                            repository_snapshot: Some(live_snap.clone()),
                        });
                        let reply = ActionCommandReply {
                            protocol_version: ACTION_PROTOCOL_VERSION,
                            kind: "action",
                            request_id: request.request_id.clone(),
                            session: public,
                            trusted_action_update: None,
                            repository_snapshot: Some(live_snap),
                        };
                        self.record_processed_request(&request.request_id, &reply)
                            .map_err(map_action)?;
                        return Ok(reply);
                    }
                    if !existing.commits.is_empty() {
                        let remote_target = live
                            .upstream
                            .as_ref()
                            .map(|u| u.display.clone())
                            .unwrap_or_else(|| existing.branch.clone());
                        enter_push_failed_state(
                            &mut existing,
                            &remote_target,
                            i18n::actions::action_cancelled_summary(locale),
                            None,
                        );
                        let _ = self.save_session_in_memory(&existing);
                        let _ = self.persist_session(storage_dir, &existing).await;
                        let public = existing.to_public();
                        raw_emit(ActionProgressEvent {
                            protocol_version: ACTION_PROTOCOL_VERSION,
                            session: public.clone(),
                            trusted_action_update: None,
                            repository_snapshot: Some(live_snap.clone()),
                        });
                        let reply = ActionCommandReply {
                            protocol_version: ACTION_PROTOCOL_VERSION,
                            kind: "action",
                            request_id: request.request_id.clone(),
                            session: public,
                            trusted_action_update: None,
                            repository_snapshot: Some(live_snap),
                        };
                        self.record_processed_request(&request.request_id, &reply)
                            .map_err(map_action)?;
                        return Ok(reply);
                    }
                }
                let idle_snapshot = reset_idle().await;
                let reply = ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request.request_id.clone(),
                    session: idle_snapshot,
                    trusted_action_update: None,
                    repository_snapshot: None,
                };
                self.record_processed_request(&request.request_id, &reply)
                    .map_err(map_action)?;
                return Ok(reply);
            }

            return match outcome {
                Ok(PiTaskOutcome::CustomActionCompleted { summary, .. }) => {
                    let _ = reset_idle().await;
                    let detail = if summary.trim().is_empty() {
                        "Model finished without calling commit_exact_scope.".to_owned()
                    } else {
                        summary
                    };
                    Err(AppError::action(AppErrorCode::ActionFailed, detail))
                }
                Ok(other) => {
                    let _ = reset_idle().await;
                    Err(AppError::model(
                        AppErrorCode::PiRuntimeError,
                        format!("C-Push returned an unexpected event type: {other:?}"),
                    ))
                }
                Err(err) => {
                    let _ = reset_idle().await;
                    Err(err)
                }
            };
        }

        // Custom Action settlement:
        let refreshed_snapshot = store
            .refresh_with_locale(&request.target, locale)
            .await
            .ok();

        // Final-turn live Git rebase-conflict safety net for `AgentFirst` custom actions across
        // ALL endings (completed, cancelled, error, unexpected): if this run mutated the repo or
        // issued a conflict recovery handle AND live Git currently has an active rebase conflict,
        // automatically surface `Conflict` recovery controls so the repository is never stranded.
        if profile.recovery_mode == RecoveryHandoffPolicy::AgentFirst
            && let Some(conflict_reply) = self
                .maybe_surface_dangling_conflict_recovery(
                    storage_dir,
                    store,
                    &request.request_id,
                    &request.target,
                    &run_state,
                    &build_stored,
                    &raw_emit,
                    locale,
                )
                .await
        {
            let _ = self.record_processed_request(&request.request_id, &conflict_reply);
            return Ok(conflict_reply);
        }

        match outcome {
            Ok(PiTaskOutcome::CustomActionCompleted { summary, links }) => {
                let (stored, trusted) = {
                    let meta = SessionPersistMeta {
                        run_id: &persist_meta_owned.0,
                        repository_id: &persist_meta_owned.1,
                        worktree_id: &persist_meta_owned.2,
                        session_id: &persist_meta_owned.3,
                        action_id: &persist_meta_owned.4,
                        action_label: &persist_meta_owned.5,
                        profile: persist_meta_owned.6,
                        action_prompt_hash: &persist_meta_owned.7,
                        frozen_scope_version: &persist_meta_owned.8,
                        locale: persist_meta_owned.9,
                        commit_language: persist_meta_owned.10,
                    };
                    let mut st = run_state
                        .lock()
                        .map_err(|_| AppError::internal("Action run state lock is unavailable"))?;
                    append_unique_links(
                        &mut st.links,
                        links
                            .into_iter()
                            .map(|l| WorkflowLink {
                                label: l.label,
                                url: l.url,
                            })
                            .collect(),
                    );
                    for step in &mut st.timeline {
                        if step.state == "active" {
                            step.state = "done".into();
                        }
                    }
                    let commit_summaries = summarize_commits(&st.commits);
                    let (done_title, done_desc) = if let Some(ref card) = st.published_card {
                        (
                            card.title.clone(),
                            card.subtitle.clone().unwrap_or_else(|| summary.clone()),
                        )
                    } else {
                        (
                            i18n::actions::action_completed_title(locale, &action_label),
                            summary.clone(),
                        )
                    };
                    st.status = ActionSessionStatus::Done;
                    st.pending_call_id = None;
                    st.summary = summary.clone();
                    st.message = Some(WorkflowUiMessage {
                        id: format!("done-action-{run_id}"),
                        kind: "done".into(),
                        title: done_title,
                        description: done_desc,
                        scope: None,
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: None,
                        links: (!st.links.is_empty()).then_some(st.links.clone()),
                        card: st.published_card.clone(),
                        questionnaire: None,
                        progress: None,
                    });
                    (
                        st.to_stored_session(&meta),
                        st.trusted_action_update.clone(),
                    )
                };
                let _ = self.save_session_in_memory(&stored);
                let _ = self.persist_session(storage_dir, &stored).await;
                let reply = ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request.request_id.clone(),
                    session: stored.to_public(),
                    trusted_action_update: trusted,
                    repository_snapshot: refreshed_snapshot,
                };
                let _ = self.record_processed_request(&request.request_id, &reply);
                Ok(reply)
            }
            Ok(PiTaskOutcome::Cancelled) => {
                let cancel_summary: String = i18n::actions::action_cancelled_summary(locale).into();
                let (stored, trusted) = {
                    let meta = SessionPersistMeta {
                        run_id: &persist_meta_owned.0,
                        repository_id: &persist_meta_owned.1,
                        worktree_id: &persist_meta_owned.2,
                        session_id: &persist_meta_owned.3,
                        action_id: &persist_meta_owned.4,
                        action_label: &persist_meta_owned.5,
                        profile: persist_meta_owned.6,
                        action_prompt_hash: &persist_meta_owned.7,
                        frozen_scope_version: &persist_meta_owned.8,
                        locale: persist_meta_owned.9,
                        commit_language: persist_meta_owned.10,
                    };
                    let mut st = run_state
                        .lock()
                        .map_err(|_| AppError::internal("Action run state lock is unavailable"))?;
                    st.status = ActionSessionStatus::Cancelled;
                    st.pending_call_id = None;
                    st.summary = cancel_summary;
                    st.message = None;
                    (
                        st.to_stored_session(&meta),
                        st.trusted_action_update.clone(),
                    )
                };
                let _ = self.remove_session_in_memory(&request.target.worktree_id);
                let _ = self.persist_session(storage_dir, &stored).await;
                let reply = ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request.request_id.clone(),
                    session: stored.to_public(),
                    trusted_action_update: trusted,
                    repository_snapshot: refreshed_snapshot,
                };
                Ok(reply)
            }
            Ok(other) => {
                let msg = format!("Unexpected Pi outcome for action: {other:?}");
                self.mark_action_failed(
                    storage_dir,
                    &request.target.worktree_id,
                    &run_state,
                    &build_stored,
                    msg.clone(),
                )
                .await;
                Err(AppError::model(AppErrorCode::PiRuntimeError, msg))
            }
            Err(err) => {
                self.mark_action_failed(
                    storage_dir,
                    &request.target.worktree_id,
                    &run_state,
                    &build_stored,
                    err.message.clone(),
                )
                .await;
                Err(err)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn maybe_surface_dangling_conflict_recovery(
        &self,
        storage_dir: &Path,
        store: &RepositoryStore,
        request_id: &str,
        target: &crate::repository::types::ReadTarget,
        run_state: &Mutex<ActionRunState>,
        build_stored: &impl Fn() -> Option<StoredActionSession>,
        raw_emit: &RawEventEmitFn,
        locale: i18n::LanguageKind,
    ) -> Option<ActionCommandReply> {
        let may_have_caused_rebase_conflict = run_state.lock().ok().is_some_and(|st| {
            st.wrote_repo
                || st.recovery_handles.values().any(|payload| {
                    matches!(
                        payload,
                        super::context::RecoveryHandlePayload::Conflict { .. }
                    )
                })
        });
        if !may_have_caused_rebase_conflict {
            return None;
        }
        let (live, snapshot) = store
            .inspect_live_remote_status(target, false, locale)
            .await
            .ok()?;
        if !live.rebase_in_progress || live.conflicted_files.is_empty() {
            return None;
        }
        let upstream = live
            .upstream
            .as_ref()
            .map(|u| u.display.as_str())
            .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale));
        let mut session = build_stored()?;
        let trusted = run_state
            .lock()
            .ok()
            .and_then(|st| st.trusted_action_update.clone());
        let has_model = !session.model_id.is_empty();
        enter_conflict_state(
            &mut session,
            upstream,
            &live.conflicted_files,
            has_model,
            None,
        );
        let _ = self.save_session_in_memory(&session);
        let _ = self.persist_session(storage_dir, &session).await;
        let public = session.to_public();
        raw_emit(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: public.clone(),
            trusted_action_update: trusted.clone(),
            repository_snapshot: Some(snapshot.clone()),
        });
        Some(ActionCommandReply {
            protocol_version: ACTION_PROTOCOL_VERSION,
            kind: "action",
            request_id: request_id.to_owned(),
            session: public,
            trusted_action_update: trusted,
            repository_snapshot: Some(snapshot),
        })
    }

    async fn mark_action_failed(
        &self,
        storage_dir: &Path,
        worktree_id: &str,
        run_state: &Mutex<ActionRunState>,
        build_stored: &impl Fn() -> Option<StoredActionSession>,
        message: String,
    ) {
        if let Ok(mut st) = run_state.lock() {
            st.status = ActionSessionStatus::Failed;
            st.pending_call_id = None;
            st.summary = message;
            st.message = None;
        }
        let _ = self.remove_session_in_memory(worktree_id);
        if let Some(record) = build_stored() {
            let _ = self.persist_session(storage_dir, &record).await;
        }
    }
}
