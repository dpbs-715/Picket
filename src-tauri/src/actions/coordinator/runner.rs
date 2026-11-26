//! Custom Action session orchestration and host-tool dispatcher (`start_custom_action`).

use super::{
    CUSTOM_ACTION_IDLE_TIMEOUT, CustomActionCoordinator, SessionAuthGrants,
    context::{
        ActionRunState, ActionToolContext, PersistStateFn, ProgressEmitFn, append_unique_links,
        format_tool_status_label, push_timeline_step,
    },
    tools_cli, tools_git_ops, tools_submit,
};
use crate::{
    actions::{
        cli_env::detect_repo_remotes,
        session_store::{self, PersistedActionSession},
        types::*,
    },
    cpush::CPushCoordinator,
    foundation::pi::{
        PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome, PiToolExecutionResult,
        RequestContext,
    },
    i18n,
    models::store::ModelStore,
    repository::{self, RepositoryStore},
    skills::RuntimeSkillSnapshot,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
};

async fn dispatch_action_tool(
    ctx: Arc<ActionToolContext>,
    call_id: String,
    tool_name: String,
    args: Value,
) -> PiToolExecutionResult {
    let status_label = format_tool_status_label(ctx.locale, &tool_name, &args);
    if tool_name != "read_skill_override_doc" && tool_name != "ask_user_question" {
        if let Ok(mut st) = ctx.run_state.lock() {
            push_timeline_step(&mut st.timeline, "active", status_label.clone());
        }
        (ctx.emit)(
            "running",
            None,
            Some(CustomActionUiMessage {
                id: format!("progress-{}", ctx.run_id),
                kind: "progress".into(),
                title: ctx.action_label.clone(),
                description: status_label.clone(),
                scope: None,
                files: None,
                commits: None,
                links: None,
                questionnaire: None,
                progress: Some(CustomActionMessageProgress {
                    completed: 2,
                    total: 3,
                    label: status_label.clone(),
                }),
            }),
            None,
        );
    }

    match tool_name.as_str() {
        "read_skill_override_doc" => tools_cli::handle_read_skill_override_doc(&ctx, &args),
        "read_repo_file" | "search_repo" => {
            tools_cli::handle_read_repo_tool(&ctx, &tool_name, &args, status_label).await
        }
        "check_cli_env" => tools_cli::handle_check_cli_env(&ctx, status_label).await,
        "install_platform_cli" => tools_cli::handle_install_platform_cli(&ctx, &args).await,
        "ask_user_question" => tools_cli::handle_ask_user_question(&ctx, &call_id, args).await,
        "run_git" => tools_cli::handle_run_git(&ctx, &call_id, &args, status_label).await,
        "run_platform_cli" => {
            tools_cli::handle_run_platform_cli(&ctx, &call_id, &args, status_label).await
        }
        "commit_exact_scope" => tools_git_ops::handle_commit_exact_scope(&ctx, &args).await,
        "sync_and_push" => tools_git_ops::handle_sync_and_push(&ctx, &args).await,
        "manage_submit_worktree" => {
            tools_submit::handle_manage_submit_worktree(&ctx, &args, status_label).await
        }
        "cleanup_submit_duplicates" => {
            tools_submit::handle_cleanup_submit_duplicates(&ctx, &args, status_label).await
        }
        "record_action_checkpoint" => {
            tools_submit::handle_record_action_checkpoint(&ctx, &args, status_label)
        }
        other => PiToolExecutionResult::error(format!(
            "Unsupported tool `{other}` for custom action execution."
        )),
    }
}

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn start_custom_action<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        store: &RepositoryStore,
        cpush: &CPushCoordinator,
        request: &StartCustomActionRequest,
        on_progress: F,
    ) -> Result<CustomActionCommandReply, String>
    where
        F: Fn(CustomActionProgressEvent) + Send + Sync + 'static,
    {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let action_label = request.action.label.trim().to_owned();
        let action_prompt = request
            .action
            .prompt
            .replace("\r\n", "\n")
            .trim()
            .to_owned();
        if action_label.is_empty() || action_prompt.is_empty() {
            return Err(i18n::actions::action_empty_fields_error(locale).into());
        }
        if cpush.has_active_operation_for_repo(&request.target.repository_id) {
            return Err(i18n::actions::action_cpush_active_error(locale).into());
        }
        let _repo_guard = self.acquire_repo_write(&request.target.repository_id, locale)?;

        let run_id = request.request_id.clone();
        let worktree_root = store
            .worktree_root_for_target(&request.target, locale)
            .await?;
        let live_snapshot = store.refresh_with_locale(&request.target, locale).await?;
        let active_tree = live_snapshot
            .worktrees
            .iter()
            .find(|wt| wt.worktree_id == request.target.worktree_id)
            .ok_or_else(|| "Target worktree not found in live repository snapshot".to_string())?;

        // Build optional authorized selected file context without requiring any selection.
        let mut selected_files: Vec<PiPlanFileEntry> = Vec::new();
        let mut unselected_paths: Vec<String> = Vec::new();
        if !request.selections.is_empty() {
            if let Ok(scope) = store
                .extract_cpush_scope(&request.target, &request.selections, locale)
                .await
            {
                selected_files = scope
                    .files
                    .into_iter()
                    .map(|f| PiPlanFileEntry {
                        file_id: f.file_id,
                        path: f.path,
                        previous_path: f.previous_path,
                        status: f.status,
                        range_kind: f.range_kind,
                        added: f.added,
                        removed: f.removed,
                        ignored_for_review: f.ignored_for_review,
                        binary: f.binary,
                        patch: f.patch,
                        skip_reason: f.skip_reason,
                    })
                    .collect();
                unselected_paths = scope.unselected_paths;
            }
        } else {
            unselected_paths = active_tree.files.iter().map(|f| f.path.clone()).collect();
        }

        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());
        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));

        let expected_hash = compute_prompt_hash(&action_prompt);
        let initially_trusted = request.action.skip_write_confirm == Some(true)
            && request.action.trusted_prompt_hash.as_deref() == Some(expected_hash.as_str());

        let prior_session = session_store::find_latest_resumable_session(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            Some(&request.action.id),
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
            matches!(
                prior.status.as_str(),
                "interrupted" | "failed" | "cancelled"
            ) || (prior.status == "done"
                && (active_tree.files.is_empty()
                    || prior
                        .submit_ledger
                        .groups
                        .iter()
                        .any(|g| g.worktree_path.is_some() && !g.worktree_removed)))
        });
        let previous_checkpoint = prior_session
            .as_ref()
            .map(session_store::format_resume_checkpoint_for_prompt);
        let initial_ledger = prior_session
            .as_ref()
            .map(|s| s.submit_ledger.clone())
            .unwrap_or_default();

        let run_state = Arc::new(Mutex::new(ActionRunState {
            timeline: vec![CustomActionTimelineItem {
                state: "active".into(),
                label: i18n::actions::action_preparing_label(locale, &action_label),
            }],
            commits: Vec::new(),
            links: Vec::new(),
            committed_file_ids: HashSet::new(),
            skip_write_confirm: initially_trusted,
            trusted_action_update: None,
            wrote_repo: false,
            submit_ledger: initial_ledger,
        }));
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let progress_cb = Arc::new(on_progress);
        let remotes = detect_repo_remotes(&worktree_root).await;
        let action_id = request.action.id.clone();

        let persist_state: PersistStateFn = Arc::new({
            let run_state = Arc::clone(&run_state);
            let storage_dir = storage_dir.to_path_buf();
            let run_id = run_id.clone();
            let repo_id = request.target.repository_id.clone();
            let wt_id = request.target.worktree_id.clone();
            let action_id = action_id.clone();
            let action_label = action_label.clone();
            let prompt_hash = expected_hash.clone();
            move |status: &str, summary: String| {
                if let Ok(st) = run_state.lock() {
                    let record = PersistedActionSession {
                        run_id: run_id.clone(),
                        repository_id: repo_id.clone(),
                        worktree_id: wt_id.clone(),
                        action_id: action_id.clone(),
                        action_label: action_label.clone(),
                        action_prompt_hash: prompt_hash.clone(),
                        status: status.to_owned(),
                        updated_at_ms: session_store::now_millis(),
                        summary,
                        dismissed_message_id: None,
                        timeline: st.timeline.clone(),
                        commits: st.commits.clone(),
                        links: st.links.clone(),
                        submit_ledger: st.submit_ledger.clone(),
                    };
                    let _ = session_store::upsert_session(&storage_dir, record);
                }
            }
        });

        let emit_progress: ProgressEmitFn = Arc::new({
            let run_state = Arc::clone(&run_state);
            let progress_cb = Arc::clone(&progress_cb);
            let run_id = run_id.clone();
            let repo_id = request.target.repository_id.clone();
            let wt_id = request.target.worktree_id.clone();
            let session_id = request.target.session_id.clone();
            let action_id = action_id.clone();
            let action_label = action_label.clone();
            move |status, pending_call_id, message, repository_snapshot| {
                let (timeline, commits, links, trusted) = run_state
                    .lock()
                    .map(|st| {
                        (
                            st.timeline.clone(),
                            st.commits.clone(),
                            st.links.clone(),
                            st.trusted_action_update.clone(),
                        )
                    })
                    .unwrap_or_default();
                progress_cb(CustomActionProgressEvent {
                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                    run_id: run_id.clone(),
                    repository_id: repo_id.clone(),
                    worktree_id: wt_id.clone(),
                    session_id: session_id.clone(),
                    action_id: action_id.clone(),
                    action_label: action_label.clone(),
                    status,
                    pending_call_id,
                    message,
                    timeline,
                    commits,
                    links,
                    trusted_action_update: trusted,
                    repository_snapshot,
                });
            }
        });

        emit_progress(
            "running",
            None,
            Some(CustomActionUiMessage {
                id: format!("progress-{run_id}"),
                kind: "progress".into(),
                title: action_label.clone(),
                description: i18n::actions::action_analyzing_desc(locale).into(),
                scope: None,
                files: None,
                commits: None,
                links: None,
                questionnaire: None,
                progress: Some(CustomActionMessageProgress {
                    completed: 1,
                    total: 3,
                    label: i18n::actions::action_started_label(locale).into(),
                }),
            }),
            None,
        );
        persist_state("running", String::new());

        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let command = PiCommand::RunCustomAction {
            request_id: run_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: active_tree.branch.clone(),
            detached: active_tree.detached,
            remote_branches: live_snapshot
                .repository
                .remote_branches
                .iter()
                .map(|b| b.name.clone())
                .collect(),
            action_label: action_label.clone(),
            action_prompt: action_prompt.clone(),
            extra_prompt: request.extra_prompt.clone(),
            selected_files,
            unselected_paths,
            local_keep_paths: live_snapshot.repository.local_keep_paths.clone(),
            locale,
            commit_language: request.commit_language,
            skill_overrides: skill_snapshot.catalog_option(),
            previous_checkpoint,
        };

        let tool_ctx = Arc::new(ActionToolContext {
            coordinator: self.clone(),
            store: store.clone(),
            emit: Arc::clone(&emit_progress),
            persist: Arc::clone(&persist_state),
            run_state: Arc::clone(&run_state),
            grants,
            skills: skill_snapshot,
            root: worktree_root,
            storage_dir: storage_dir.to_path_buf(),
            remotes,
            frozen_selections: request.selections.clone(),
            live_changes: active_tree.files.clone(),
            target: request.target.clone(),
            run_id: run_id.clone(),
            action_id: action_id.clone(),
            action_label: action_label.clone(),
            expected_hash,
            locale,
        });

        let outcome = supervisor
            .run_task_with_tools(
                paths,
                run_id.clone(),
                context,
                &command,
                CUSTOM_ACTION_IDLE_TIMEOUT,
                api_key_for_redaction.as_deref(),
                |_event| Ok(()),
                move |call_id, tool_name, args| {
                    dispatch_action_tool(Arc::clone(&tool_ctx), call_id, tool_name, args)
                },
            )
            .await;

        let refreshed_snapshot = store
            .refresh_with_locale(&request.target, locale)
            .await
            .ok();

        match outcome {
            Ok(PiTaskOutcome::CustomActionCompleted { summary, links }) => {
                let (timeline, commits, merged_links, trusted) = {
                    let mut st = run_state.lock().unwrap();
                    append_unique_links(
                        &mut st.links,
                        links
                            .into_iter()
                            .map(|l| CustomActionLink {
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
                    (
                        st.timeline.clone(),
                        st.commits.clone(),
                        st.links.clone(),
                        st.trusted_action_update.clone(),
                    )
                };
                let commit_summaries: Vec<CustomActionMessageCommitSummary> = commits
                    .iter()
                    .map(|c| CustomActionMessageCommitSummary {
                        hash: c.short_oid.clone(),
                        title: c.title.clone(),
                    })
                    .collect();
                let done_msg = CustomActionUiMessage {
                    id: format!("done-action-{run_id}"),
                    kind: "done".into(),
                    title: i18n::actions::action_completed_title(locale, &action_label),
                    description: summary.clone(),
                    scope: None,
                    files: None,
                    commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                    links: (!merged_links.is_empty()).then_some(merged_links.clone()),
                    questionnaire: None,
                    progress: None,
                };
                persist_state("done", summary.clone());
                Ok(CustomActionCommandReply {
                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                    kind: "customAction",
                    request_id: request.request_id.clone(),
                    run_id,
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    action_id,
                    action_label,
                    status: "done",
                    summary,
                    message: Some(done_msg),
                    timeline,
                    commits,
                    links: merged_links,
                    trusted_action_update: trusted,
                    repository_snapshot: refreshed_snapshot,
                })
            }
            Ok(PiTaskOutcome::Cancelled) => {
                let (timeline, commits, merged_links, trusted) = run_state
                    .lock()
                    .map(|st| {
                        (
                            st.timeline.clone(),
                            st.commits.clone(),
                            st.links.clone(),
                            st.trusted_action_update.clone(),
                        )
                    })
                    .unwrap_or_default();
                let cancel_summary: String = i18n::actions::action_cancelled_summary(locale).into();
                persist_state("cancelled", cancel_summary.clone());
                Ok(CustomActionCommandReply {
                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                    kind: "customAction",
                    request_id: request.request_id.clone(),
                    run_id,
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    action_id,
                    action_label,
                    status: "cancelled",
                    summary: cancel_summary,
                    message: None,
                    timeline,
                    commits,
                    links: merged_links,
                    trusted_action_update: trusted,
                    repository_snapshot: refreshed_snapshot,
                })
            }
            Ok(other) => {
                let msg = format!("Unexpected Pi outcome for custom action: {other:?}");
                persist_state("failed", msg.clone());
                Err(msg)
            }
            Err(err) => {
                persist_state("failed", err.clone());
                Err(err)
            }
        }
    }
}
