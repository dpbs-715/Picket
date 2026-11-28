//! Tool-driven user-authorized Agent conflict resolution during rebase (`mode: resolveConflict`).
//! Sends compact file metadata (never 4x full-file NDJSON blobs) and exposes `read_conflict_file`,
//! `write_conflict_resolution`, `read_repo_file`, and `search_repo` inside the unified agent loop.

use super::{
    CPUSH_AI_IDLE_TIMEOUT, CustomActionCoordinator,
    session_helpers::{apply_rebased_commits, enter_conflict_state, summarize_commits},
};
use crate::{
    actions::types::*,
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
    models::store::{ModelStore, ResolvedRuntimeSpec},
    pi_runtime::{
        CustomActionRunMode, PiCommand, PiConflictFileSummary, PiPaths, PiSupervisor,
        PiTaskOutcome, PiToolExecutionResult, RequestContext,
    },
    repository::{
        RepositoryStore,
        remote::{
            ExtractedConflictFile, RebaseContinueOutcome, ResolvedConflictFile,
            line_has_conflict_markers,
        },
    },
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

const MAX_READ_LINES_PER_CALL: usize = 400;

fn format_paginated_section(
    path: &str,
    section_name: &str,
    text: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total == 0 {
        return format!("Conflicted file `{path}` [{section_name}]: (empty, 0 lines)");
    }
    let start = start_line.unwrap_or(1).clamp(1, total);
    let default_end = (start + MAX_READ_LINES_PER_CALL - 1).min(total);
    let end = end_line
        .unwrap_or(default_end)
        .clamp(start, (start + MAX_READ_LINES_PER_CALL - 1).min(total));
    let mut out =
        format!("Conflicted file `{path}` [{section_name}] (Lines {start}-{end} of {total}):\n");
    for (offset, line) in lines[start - 1..end].iter().enumerate() {
        out.push_str(&format!("{}: {line}\n", start + offset));
    }
    if end < total {
        out.push_str(&format!(
            "... ({} more line(s); call read_conflict_file with startLine={} to continue)\n",
            total - end,
            end + 1
        ));
    }
    out
}

fn find_conflict_file<'a>(
    files: &'a [ExtractedConflictFile],
    token: &str,
) -> Option<&'a ExtractedConflictFile> {
    let clean = token.trim().trim_start_matches("./");
    files.iter().find(|f| f.path == clean || f.file_id == clean)
}

#[derive(Default)]
struct ConflictMergeState {
    resolved: HashMap<String, ResolvedConflictFile>,
    summary: Option<String>,
    last_validation_error: Option<String>,
}

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_agent_conflict_merge(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredActionSession,
        previous_msg: &WorkflowUiMessage,
        instruction: &str,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, AppError> {
        let map_action = |msg: String| AppError::action(AppErrorCode::ActionFailed, msg);
        let locale = session.locale;
        if session.model_id.trim().is_empty() {
            return Err(AppError::model(
                AppErrorCode::ModelNotFound,
                i18n::cpush::cpush_agent_merge_no_model_error(locale),
            ));
        }
        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &session.model_id, locale)
            .await?;
        let read_target = session.read_target();
        let batch = repo_store
            .extract_conflict_batch(&read_target, locale)
            .await
            .map_err(|msg| AppError::git(AppErrorCode::GitFailed, msg))?;

        let answer_text = if instruction.is_empty() {
            i18n::cpush::cpush_option_agent_merge(locale).to_owned()
        } else {
            instruction.to_owned()
        };
        let prev_timeline = session.timeline.clone();
        let prev_msg = previous_msg.clone();

        session.qa_log.push(CPushQaItem {
            question_id: previous_msg.id.clone(),
            question: previous_msg.title.clone(),
            answer: answer_text.clone(),
            option_id: instruction.is_empty().then(|| "agent-merge".into()),
        });
        session.status = ActionSessionStatus::Running;
        session.pending_call_id = None;
        session.timeline = vec![
            WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_authorized_agent_merge(locale, &answer_text),
            },
            WorkflowTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_merging_files(locale, batch.files.len()),
            },
        ];
        session.message = Some(WorkflowUiMessage {
            id: format!("progress-merge-{request_id}"),
            kind: "progress".into(),
            title: i18n::cpush::cpush_progress_merging_title(locale).into(),
            description: i18n::cpush::cpush_progress_merging_desc(locale, instruction),
            scope: Some(i18n::cpush::cpush_merging_scope(locale, &batch.upstream)),
            files: None,
            commits: None,
            options: None,
            links: (!session.links.is_empty()).then_some(session.links.clone()),
            card: None,
            questionnaire: None,
            progress: Some(WorkflowMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush::cpush_progress_merging_steps(locale).into(),
            }),
        });
        self.save_session_in_memory(session).map_err(map_action)?;
        let _ = on_progress(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: session.to_public(),
            trusted_action_update: None,
            repository_snapshot: None,
        });

        let (merge_summary, resolved_files) = match self
            .invoke_conflict_agent(
                paths,
                supervisor,
                repo_store,
                &spec,
                request_id,
                session,
                &batch.upstream,
                instruction,
                &batch.files,
            )
            .await
        {
            Ok(res) => res,
            Err(err) => {
                session.qa_log.pop();
                session.status = ActionSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description =
                    i18n::cpush::cpush_merge_resolver_failed_desc(locale, &err.message);
                session.message = Some(restored_msg);
                self.save_session_in_memory(session).map_err(map_action)?;
                let _ = self.persist_session(storage_dir, session).await;
                let public = session.to_public();
                let _ = on_progress(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: public.clone(),
                    trusted_action_update: None,
                    repository_snapshot: None,
                });
                return Ok(ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request_id.to_owned(),
                    session: public,
                    trusted_action_update: None,
                    repository_snapshot: None,
                });
            }
        };

        let continue_outcome = repo_store
            .apply_conflict_resolution_and_continue(
                &read_target,
                &batch.files,
                &resolved_files,
                locale,
            )
            .await;

        match continue_outcome {
            Err(err) => {
                let refreshed = repo_store
                    .refresh_with_locale(&read_target, locale)
                    .await
                    .ok();
                session.status = ActionSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description = i18n::cpush::cpush_merge_apply_failed_desc(locale, &err);
                session.message = Some(restored_msg);
                self.save_session_in_memory(session).map_err(map_action)?;
                self.persist_session(storage_dir, session)
                    .await
                    .map_err(AppError::storage)?;
                let public = session.to_public();
                let _ = on_progress(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: public.clone(),
                    trusted_action_update: None,
                    repository_snapshot: refreshed.clone(),
                });
                Ok(ActionCommandReply {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    kind: "action",
                    request_id: request_id.to_owned(),
                    session: public,
                    trusted_action_update: None,
                    repository_snapshot: refreshed,
                })
            }
            Ok(RebaseContinueOutcome::NextConflict {
                upstream,
                conflicted_files,
                snapshot,
            }) => {
                enter_conflict_state(
                    session,
                    &upstream,
                    &conflicted_files,
                    true,
                    Some(i18n::cpush::cpush_next_commit_conflict_reason(locale)),
                );
                self.save_session_in_memory(session).map_err(map_action)?;
                self.persist_session(storage_dir, session)
                    .await
                    .map_err(AppError::storage)?;
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
            Ok(RebaseContinueOutcome::Completed {
                upstream,
                rebased_commits,
                snapshot,
                ..
            }) => {
                apply_rebased_commits(session, &rebased_commits);
                if session.push_authorized {
                    self.sync_and_push_session(
                        storage_dir,
                        repo_store,
                        request_id,
                        session,
                        None,
                        Some(snapshot),
                        on_progress,
                    )
                    .await
                    .map_err(map_action)
                } else {
                    session.status = ActionSessionStatus::Done;
                    session.can_retry_remaining = false;
                    session.timeline = vec![
                        WorkflowTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_agent_merge_done(
                                locale,
                                &merge_summary,
                            ),
                        },
                        WorkflowTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_rebase_done_no_push(
                                locale, &upstream,
                            ),
                        },
                    ];
                    let commits = summarize_commits(&session.commits);
                    let title = i18n::cpush::cpush_done_merge_pull_title(locale, &upstream);
                    session.summary = title.clone();
                    session.message = Some(WorkflowUiMessage {
                        id: format!("done-merge-pull-{}", session.run_id),
                        kind: "done".into(),
                        title,
                        description: i18n::cpush::cpush_done_merge_pull_desc(
                            locale,
                            &merge_summary,
                        ),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commits.is_empty()).then_some(commits),
                        options: None,
                        links: (!session.links.is_empty()).then_some(session.links.clone()),
                        card: None,
                        questionnaire: None,
                        progress: None,
                    });
                    self.save_session_in_memory(session).map_err(map_action)?;
                    self.persist_session(storage_dir, session)
                        .await
                        .map_err(AppError::storage)?;
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn invoke_conflict_agent(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        repo_store: &RepositoryStore,
        spec: &ResolvedRuntimeSpec,
        request_id: &str,
        session: &StoredActionSession,
        upstream: &str,
        instruction: &str,
        files: &[ExtractedConflictFile],
    ) -> Result<(String, Vec<ResolvedConflictFile>), AppError> {
        let context = RequestContext {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        };
        let pi_request_id = format!("{request_id}-conflict");
        let conflict_summaries: Vec<PiConflictFileSummary> = files
            .iter()
            .map(|f| PiConflictFileSummary {
                file_id: f.file_id.clone(),
                path: f.path.clone(),
                line_count: f.conflicted_content.lines().count(),
                has_base: f.base_content.is_some().then_some(true),
            })
            .collect();
        let commits_wire: Vec<String> = session
            .commits
            .iter()
            .map(|c| format!("{} {}", c.short_oid, c.title))
            .collect();
        let command = PiCommand::RunCustomAction {
            mode: Some(CustomActionRunMode::ResolveConflict),
            request_id: pi_request_id.clone(),
            context: Box::new(context.clone()),
            model: Box::new(spec.to_pi_spec()),
            branch: session.branch.clone(),
            detached: false,
            remote_branches: Vec::new(),
            action_label: "Resolve Conflict".into(),
            action_prompt: String::new(),
            extra_prompt: instruction.to_owned(),
            selected_files: Vec::new(),
            unselected_paths: Vec::new(),
            local_keep_paths: Vec::new(),
            recent_commits: None,
            push_authorized: None,
            upstream: Some(upstream.to_owned()),
            commits: Some(commits_wire),
            conflict_files: Some(conflict_summaries),
            locale: session.locale,
            commit_language: session.commit_language,
            approval_mode: None,
            previous_checkpoint: None,
            follow_up_prompt: None,
            skill_overrides: None,
        };

        let shared_files = Arc::new(files.to_vec());
        let merge_state = Arc::new(Mutex::new(ConflictMergeState::default()));
        let closure_merge_state = Arc::clone(&merge_state);
        let tool_store = repo_store.clone();
        let tool_target = session.read_target();
        let locale = session.locale;

        let _pi_cancel_guard = self.register_active_pi_request(&session.run_id, &pi_request_id);
        let outcome = supervisor
            .run_task_with_tools(
                paths,
                pi_request_id,
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
                move |_call_id, tool_name, args| {
                    let files_ref = Arc::clone(&shared_files);
                    let state_ref = Arc::clone(&closure_merge_state);
                    let store = tool_store.clone();
                    let target = tool_target.clone();
                    async move {
                        match tool_name.as_str() {
                            "read_conflict_file" => {
                                let Some(path) = args.get("path").and_then(Value::as_str) else {
                                    return PiToolExecutionResult::error(
                                        "Missing required `path` argument for read_conflict_file.",
                                    );
                                };
                                let Some(file) = find_conflict_file(&files_ref, path) else {
                                    return PiToolExecutionResult::error(
                                        i18n::repository::remote_conflict_unauthorized_file_error(
                                            locale, path,
                                        ),
                                    );
                                };
                                let section = args
                                    .get("section")
                                    .and_then(Value::as_str)
                                    .unwrap_or("conflicted");
                                let start_line = args
                                    .get("startLine")
                                    .and_then(Value::as_u64)
                                    .map(|n| n as usize);
                                let end_line = args
                                    .get("endLine")
                                    .and_then(Value::as_u64)
                                    .map(|n| n as usize);
                                let text = match section {
                                    "base" => match file.base_content.as_deref() {
                                        Some(base) => base,
                                        None => {
                                            return PiToolExecutionResult::error(format!(
                                                "No common ancestor (base) stage exists for `{}`.",
                                                file.path
                                            ));
                                        }
                                    },
                                    "upstream" => file.upstream_content.as_str(),
                                    "local" => file.local_content.as_str(),
                                    _ => file.conflicted_content.as_str(),
                                };
                                PiToolExecutionResult::success(format_paginated_section(
                                    &file.path, section, text, start_line, end_line,
                                ))
                            }
                            "write_conflict_resolution" => {
                                let Some(path) = args.get("path").and_then(Value::as_str) else {
                                    return PiToolExecutionResult::error(
                                        "Missing required `path` argument for write_conflict_resolution.",
                                    );
                                };
                                let Some(content) = args.get("content").and_then(Value::as_str)
                                else {
                                    return PiToolExecutionResult::error(
                                        "Missing required string `content` argument for write_conflict_resolution.",
                                    );
                                };
                                let Some(orig) = find_conflict_file(&files_ref, path) else {
                                    return PiToolExecutionResult::error(
                                        i18n::repository::remote_conflict_unauthorized_file_error(
                                            locale, path,
                                        ),
                                    );
                                };
                                let base_had_markers = orig
                                    .base_content
                                    .as_deref()
                                    .is_some_and(line_has_conflict_markers);
                                if !base_had_markers && line_has_conflict_markers(content) {
                                    let err =
                                        i18n::repository::remote_conflict_leftover_markers_error(
                                            locale, &orig.path,
                                        );
                                    if let Ok(mut st) = state_ref.lock() {
                                        st.last_validation_error = Some(err.clone());
                                    }
                                    return PiToolExecutionResult::error(err);
                                }
                                if content.trim().is_empty()
                                    && !orig.upstream_content.trim().is_empty()
                                    && !orig.local_content.trim().is_empty()
                                {
                                    let err = i18n::repository::remote_conflict_empty_result_error(
                                        locale, &orig.path,
                                    );
                                    if let Ok(mut st) = state_ref.lock() {
                                        st.last_validation_error = Some(err.clone());
                                    }
                                    return PiToolExecutionResult::error(err);
                                }
                                let summary_opt = args
                                    .get("summary")
                                    .and_then(Value::as_str)
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.chars().take(240).collect::<String>());
                                let (resolved_count, final_summary) = {
                                    let Ok(mut st) = state_ref.lock() else {
                                        return PiToolExecutionResult::error(
                                            "Conflict merge state lock is unavailable.",
                                        );
                                    };
                                    st.resolved.insert(
                                        orig.file_id.clone(),
                                        ResolvedConflictFile {
                                            file_id: orig.file_id.clone(),
                                            path: orig.path.clone(),
                                            content: content.to_owned(),
                                        },
                                    );
                                    if let Some(s) = summary_opt {
                                        st.summary = Some(s);
                                    }
                                    (st.resolved.len(), st.summary.clone())
                                };
                                let total = files_ref.len();
                                if resolved_count >= total {
                                    let summary = final_summary.unwrap_or_else(|| match locale {
                                        LanguageKind::Zh => {
                                            format!("已完成 {total} 个冲突文件的合并")
                                        }
                                        LanguageKind::En => {
                                            format!("Merged {total} conflicted file(s)")
                                        }
                                    });
                                    PiToolExecutionResult {
                                        ok: true,
                                        content: format!(
                                            "Accepted conflict resolution for `{}` ({resolved_count}/{total}). Applying resolutions and continuing rebase.",
                                            orig.path
                                        ),
                                        details: Some(json!({
                                            "stopAgent": true,
                                            "summary": summary,
                                            "links": []
                                        })),
                                        is_error: false,
                                    }
                                } else {
                                    PiToolExecutionResult::success(format!(
                                        "Accepted conflict resolution for `{}` ({resolved_count}/{total} conflicted files resolved).",
                                        orig.path
                                    ))
                                }
                            }
                            "read_repo_file" | "search_repo" => {
                                match store
                                    .execute_read_tool(&target, &tool_name, &args, locale)
                                    .await
                                {
                                    Ok(content) => PiToolExecutionResult::success(content),
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            other => PiToolExecutionResult::error(format!(
                                "Unsupported tool `{other}` for conflict resolution."
                            )),
                        }
                    }
                },
            )
            .await?;

        match outcome {
            PiTaskOutcome::CustomActionCompleted { summary, .. } => {
                let st = merge_state
                    .lock()
                    .map_err(|_| AppError::internal("Conflict merge state lock is unavailable"))?;
                let mut ordered = Vec::with_capacity(files.len());
                for orig in files {
                    let Some(resolved) = st.resolved.get(&orig.file_id) else {
                        if let Some(ref validation_err) = st.last_validation_error {
                            return Err(AppError::action(
                                AppErrorCode::ActionFailed,
                                validation_err.clone(),
                            ));
                        }
                        return Err(AppError::action(
                            AppErrorCode::ActionFailed,
                            format!(
                                "Model did not submit a verified resolution for conflicted file `{}`.",
                                orig.path
                            ),
                        ));
                    };
                    ordered.push(resolved.clone());
                }
                let effective_summary = st
                    .summary
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(summary);
                Ok((effective_summary, ordered))
            }
            PiTaskOutcome::Cancelled => Err(AppError::action(
                AppErrorCode::ActionCancelled,
                i18n::cpush::cpush_conflict_cancelled_error(session.locale),
            )),
            _ => Err(AppError::model(
                AppErrorCode::PiRuntimeError,
                "Conflict merge returned an unexpected event type",
            )),
        }
    }
}
