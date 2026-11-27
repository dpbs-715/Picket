//! Custom Action session orchestration and host-tool dispatcher (`start_custom_action`).

use super::{
    CUSTOM_ACTION_IDLE_TIMEOUT, CustomActionCoordinator, SessionAuthGrants,
    context::{
        ActionRunState, ActionToolContext, ProgressEmitFn, SessionPersistMeta, append_unique_links,
        format_tool_status_label, push_timeline_step, snapshot_persisted_session,
    },
    tools_cli, tools_command, tools_git_ops, tools_submit, tools_workspace,
};
use crate::{
    actions::{cli_env::detect_repo_remotes, session_store, types::*},
    i18n,
    models::store::ModelStore,
    pi_runtime::{
        PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome, PiToolExecutionResult,
        RequestContext,
    },
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
    if tool_name != "read_skill_override_doc" && tool_name != "message_user" {
        let preserved_card = if let Ok(mut st) = ctx.run_state.lock() {
            push_timeline_step(&mut st.timeline, "active", status_label.clone());
            st.published_card.clone()
        } else {
            None
        };
        (ctx.emit)(
            "running",
            None,
            Some(CustomActionUiMessage {
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
                links: None,
                card: preserved_card,
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

    let result = match tool_name.as_str() {
        "read_skill_override_doc" => tools_cli::handle_read_skill_override_doc(&ctx, &args),
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
        "commit_exact_scope" => tools_git_ops::handle_commit_exact_scope(&ctx, &args).await,
        "sync_and_push" => tools_git_ops::handle_sync_and_push(&ctx, &args).await,
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
            "Unsupported tool `{other}` for custom action execution."
        )),
    };

    if tool_name != "read_skill_override_doc"
        && tool_name != "message_user"
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
    pub async fn start_custom_action<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        store: &RepositoryStore,
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

        let follow_up_prompt = request
            .follow_up_prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let has_follow_up = follow_up_prompt.is_some();

        let prior_session = request
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
                    has_follow_up
                        || matches!(
                            prior.status.as_str(),
                            "interrupted" | "failed" | "cancelled"
                        )
                        || (prior.status == "done"
                            && (active_tree.files.is_empty()
                                || !prior.edited_file_hashes.is_empty()
                                || prior
                                    .submit_ledger
                                    .groups
                                    .iter()
                                    .any(|g| g.worktree_path.is_some() && !g.worktree_removed)))
                })
            });
        let previous_checkpoint = prior_session
            .as_ref()
            .map(session_store::format_resume_checkpoint_for_prompt);
        let initial_ledger = prior_session
            .as_ref()
            .map(|s| s.submit_ledger.clone())
            .unwrap_or_default();
        let local_keep_paths_set: HashSet<String> = live_snapshot
            .repository
            .local_keep_paths
            .iter()
            .cloned()
            .collect();
        let initial_edited_hashes = prior_session
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

        let run_state = Arc::new(Mutex::new(ActionRunState {
            timeline: vec![CustomActionTimelineItem {
                state: "active".into(),
                label: i18n::actions::action_preparing_label(locale, &action_label),
            }],
            commits: Vec::new(),
            links: Vec::new(),
            published_card: None,
            committed_file_ids: HashSet::new(),
            action_edited_hashes: initial_edited_hashes,
            committed_edited_paths: HashSet::new(),
            skip_write_confirm: initially_trusted,
            trusted_action_update: None,
            wrote_repo: false,
            submit_ledger: initial_ledger,
        }));
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let progress_cb = Arc::new(on_progress);
        let remotes = detect_repo_remotes(&worktree_root).await;
        let action_id = request.action.id.clone();

        let persist_snapshot = |status: &str, summary: String| {
            let meta = SessionPersistMeta {
                run_id: &run_id,
                repository_id: &request.target.repository_id,
                worktree_id: &request.target.worktree_id,
                action_id: &action_id,
                action_label: &action_label,
                action_prompt_hash: &expected_hash,
            };
            snapshot_persisted_session(&run_state, &meta, status, summary)
        };

        let emit_progress: ProgressEmitFn = Arc::new({
            let coordinator = self.clone();
            let run_state = Arc::clone(&run_state);
            let progress_cb = Arc::clone(&progress_cb);
            let run_id = run_id.clone();
            let repo_id = request.target.repository_id.clone();
            let wt_id = request.target.worktree_id.clone();
            let session_id = request.target.session_id.clone();
            let action_id = action_id.clone();
            let action_label = action_label.clone();
            move |status, pending_call_id, message, repository_snapshot| {
                let tray_status = match status {
                    "running" => crate::tray::TraySessionStatus::Running,
                    "needsDecision" => crate::tray::TraySessionStatus::NeedsDecision,
                    "done" => crate::tray::TraySessionStatus::Done,
                    _ => crate::tray::TraySessionStatus::Idle,
                };
                coordinator.set_action_tray_status(
                    &wt_id,
                    tray_status,
                    message.as_ref().map(|m| m.title.clone()),
                );
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
                card: None,
                questionnaire: None,
                progress: Some(CustomActionMessageProgress {
                    completed: 1,
                    total: 3,
                    label: i18n::actions::action_started_label(locale).into(),
                }),
            }),
            None,
        );
        if let Some(record) = persist_snapshot("running", String::new()) {
            let _ = session_store::upsert_session_async(storage_dir, record).await;
        }

        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let command = PiCommand::RunCustomAction {
            mode: None,
            request_id: run_id.clone(),
            context: Box::new(context.clone()),
            model: Box::new(spec.to_pi_spec()),
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
            follow_up_prompt,
            selected_files,
            unselected_paths,
            local_keep_paths: live_snapshot.repository.local_keep_paths.clone(),
            recent_commits: None,
            push_authorized: None,
            upstream: None,
            commits: None,
            conflict_files: None,
            locale,
            commit_language: request.commit_language,
            skill_overrides: skill_snapshot.catalog_option(),
            previous_checkpoint,
        };

        let tool_ctx = Arc::new(ActionToolContext {
            coordinator: self.clone(),
            store: store.clone(),
            emit: Arc::clone(&emit_progress),
            run_state: Arc::clone(&run_state),
            grants,
            skills: skill_snapshot,
            root: worktree_root,
            storage_dir: storage_dir.to_path_buf(),
            bundled_node_path: paths.node.clone(),
            remotes,
            frozen_selections: request.selections.clone(),
            live_changes: active_tree.files.clone(),
            local_keep_paths: local_keep_paths_set,
            target: request.target.clone(),
            run_id: run_id.clone(),
            action_id: action_id.clone(),
            action_label: action_label.clone(),
            expected_hash: expected_hash.clone(),
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
                let (timeline, commits, merged_links, trusted, published_card) = {
                    let mut st = run_state
                        .lock()
                        .map_err(|_| "Action run state lock is unavailable".to_string())?;
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
                        st.published_card.clone(),
                    )
                };
                let commit_summaries: Vec<CustomActionMessageCommitSummary> = commits
                    .iter()
                    .map(|c| CustomActionMessageCommitSummary {
                        hash: c.short_oid.clone(),
                        title: c.title.clone(),
                    })
                    .collect();
                let (done_title, done_desc) = if let Some(ref card) = published_card {
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
                let done_msg = CustomActionUiMessage {
                    id: format!("done-action-{run_id}"),
                    kind: "done".into(),
                    title: done_title,
                    description: done_desc,
                    scope: None,
                    files: None,
                    commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                    links: (!merged_links.is_empty()).then_some(merged_links.clone()),
                    card: published_card,
                    questionnaire: None,
                    progress: None,
                };
                if let Some(record) = persist_snapshot("done", summary.clone()) {
                    let _ = session_store::upsert_session_async(storage_dir, record).await;
                }
                self.set_action_tray_status(
                    &request.target.worktree_id,
                    crate::tray::TraySessionStatus::Done,
                    Some(done_msg.title.clone()),
                );
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
                if let Some(record) = persist_snapshot("cancelled", cancel_summary.clone()) {
                    let _ = session_store::upsert_session_async(storage_dir, record).await;
                }
                self.set_action_tray_status(
                    &request.target.worktree_id,
                    crate::tray::TraySessionStatus::Idle,
                    None,
                );
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
                self.set_action_tray_status(
                    &request.target.worktree_id,
                    crate::tray::TraySessionStatus::Idle,
                    None,
                );
                let msg = format!("Unexpected Pi outcome for custom action: {other:?}");
                if let Some(record) = persist_snapshot("failed", msg.clone()) {
                    let _ = session_store::upsert_session_async(storage_dir, record).await;
                }
                Err(msg)
            }
            Err(err) => {
                self.set_action_tray_status(
                    &request.target.worktree_id,
                    crate::tray::TraySessionStatus::Idle,
                    None,
                );
                if let Some(record) = persist_snapshot("failed", err.clone()) {
                    let _ = session_store::upsert_session_async(storage_dir, record).await;
                }
                Err(err)
            }
        }
    }
}
