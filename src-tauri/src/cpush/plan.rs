//! C-Push planning, Rust-side plan validation, and Start C-Push entry.

use super::{
    CPUSH_AI_IDLE_TIMEOUT, CPushCoordinator,
    helpers::{file_basename, scope_badge},
    types::*,
};
use crate::{
    foundation::pi::{
        PiCPushPlan, PiCPushQaTurn, PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor,
        PiTaskOutcome, PiToolExecutionResult, RequestContext,
    },
    i18n::{self, LanguageKind},
    models::store::{ModelStore, ResolvedRuntimeSpec},
    repository::{self, RepositoryStore, ai_context::FrozenCPushFile},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

pub(super) fn validate_plan_in_rust(
    plan: PiCPushPlan,
    frozen_files: &[FrozenCPushFile],
    allow_skips: bool,
    locale: LanguageKind,
) -> Result<PiCPushPlan, String> {
    let authorized_ids: HashSet<&str> = frozen_files.iter().map(|f| f.file_id.as_str()).collect();
    match plan {
        PiCPushPlan::Question {
            title,
            scope,
            description,
            affected_file_ids,
            options,
        } => {
            if title.trim().is_empty() || description.trim().is_empty() || options.is_empty() {
                return Err(i18n::cpush::cpush_plan_incomplete_question_error(locale).into());
            }
            let filtered_affected: Vec<String> = affected_file_ids
                .into_iter()
                .filter(|id| authorized_ids.contains(id.as_str()))
                .collect();
            Ok(PiCPushPlan::Question {
                title,
                scope,
                description,
                affected_file_ids: filtered_affected,
                options,
            })
        }
        PiCPushPlan::Ready {
            summary,
            groups,
            skipped_file_ids,
            skip_push,
        } => {
            if !allow_skips && !skipped_file_ids.is_empty() {
                return Err(i18n::cpush::cpush_plan_unconfirmed_skip_error(locale).into());
            }
            let mut seen = HashSet::with_capacity(frozen_files.len());
            for skipped in &skipped_file_ids {
                if !authorized_ids.contains(skipped.as_str()) {
                    return Err(i18n::cpush::cpush_plan_unauthorized_skip_error(locale).into());
                }
                if !seen.insert(skipped.as_str()) {
                    return Err(i18n::cpush::cpush_plan_duplicate_file_error(locale).into());
                }
            }
            for group in &groups {
                if group.file_ids.is_empty() || group.message.trim().is_empty() {
                    return Err(i18n::cpush::cpush_plan_empty_group_error(locale).into());
                }
                for id in &group.file_ids {
                    if !authorized_ids.contains(id.as_str()) {
                        return Err(i18n::cpush::cpush_plan_unselected_file_error(locale).into());
                    }
                    if !seen.insert(id.as_str()) {
                        return Err(i18n::cpush::cpush_plan_duplicate_file_error(locale).into());
                    }
                }
            }
            if seen.len() != authorized_ids.len() {
                return Err(i18n::cpush::cpush_plan_incomplete_coverage_error(locale).into());
            }
            if !allow_skips {
                let sensitive: Vec<&FrozenCPushFile> = frozen_files
                    .iter()
                    .filter(|f| crate::repository::ai_context::is_sensitive_local_file(&f.path))
                    .collect();
                if !sensitive.is_empty() {
                    let affected_file_ids: Vec<String> =
                        sensitive.iter().map(|f| f.file_id.clone()).collect();
                    let names: Vec<String> =
                        sensitive.iter().map(|f| file_basename(&f.path)).collect();
                    let has_other_files = sensitive.len() < frozen_files.len();
                    let (title, scope, description, options) =
                        i18n::cpush::cpush_sensitive_question(locale, &names, has_other_files);
                    return Ok(PiCPushPlan::Question {
                        title,
                        scope,
                        description,
                        affected_file_ids,
                        options,
                    });
                }
            }
            Ok(PiCPushPlan::Ready {
                summary,
                groups,
                skipped_file_ids,
                skip_push,
            })
        }
    }
}

impl CPushCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn start_cpush(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &StartCPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self.lookup_processed_request(&request.request_id)? {
            return Ok(existing_reply);
        }
        self.ensure_no_busy_session(storage_dir, &request.target.worktree_id, locale, true)
            .await?;

        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let extracted = repo_store
            .extract_cpush_scope(&request.target, &request.selections, locale)
            .await?;

        let run_id = request.request_id.clone();

        repo_store
            .preflight_commit_scope(&request.target, &request.selections, locale)
            .await?;

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
            progress: Some(CPushMessageProgress {
                completed: 1,
                total: 4,
                label: i18n::cpush::cpush_progress_planning_steps(locale).into(),
            }),
        };

        let mut session = StoredCPushSession {
            run_id: run_id.clone(),
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            frozen_scope_version: request.target.scope_version.clone(),
            model_id: request.model_id.clone(),
            branch: extracted.branch,
            initial_head: extracted.initial_head,
            prompt: request.prompt.clone(),
            recent_commits: extracted.recent_commits,
            unselected_paths: extracted.unselected_paths,
            frozen_files: extracted.files,
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: Vec::new(),
            commits: Vec::new(),
            qa_log: Vec::new(),
            timeline: initial_timeline,
            session_status: CPushSessionStatus::Running,
            message: Some(progress_msg),
            can_retry_remaining: false,
            push_authorized: request.push.unwrap_or(true),
            plan_only: false,
            auto_collapse: false,
            notice_dismissed: false,
            locale,
            commit_language: request.commit_language,
        };
        self.save_in_memory(&session)?;
        let _ = self.persist_all(storage_dir).await;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let plan = match self
            .invoke_pi_planner(paths, supervisor, repo_store, &spec, &run_id, &session)
            .await
        {
            Ok(plan) => plan,
            Err(err) => {
                let _ = self.remove_in_memory(&session.worktree_id);
                let _ = self.persist_all(storage_dir).await;
                let idle_snapshot = CPushSessionSnapshot {
                    session_status: CPushSessionStatus::Idle,
                    message: None,
                    timeline: Vec::new(),
                    ..session.to_public()
                };
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: idle_snapshot,
                    repository_snapshot: None,
                });
                return Err(err);
            }
        };

        let reply = self
            .apply_plan_or_execute(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                plan,
                &mut on_progress,
            )
            .await?;

        self.record_processed_request(&request.request_id, &reply)?;
        Ok(reply)
    }

    pub(super) async fn invoke_pi_planner(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        repo_store: &RepositoryStore,
        spec: &ResolvedRuntimeSpec,
        request_id: &str,
        session: &StoredCPushSession,
    ) -> Result<PiCPushPlan, String> {
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
        let qa_wire: Vec<PiCPushQaTurn> = session
            .qa_log
            .iter()
            .map(|qa| PiCPushQaTurn {
                question_id: qa.question_id.clone(),
                question: qa.question.clone(),
                answer: qa.answer.clone(),
                option_id: qa.option_id.clone(),
            })
            .collect();
        let command = PiCommand::PlanCPush {
            request_id: request_id.to_owned(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: session.branch.clone(),
            recent_commits: session.recent_commits.clone(),
            prompt: session.prompt.clone(),
            files: files_wire,
            unselected_paths: session.unselected_paths.clone(),
            qa_history: qa_wire,
            locale: session.locale,
            commit_language: session.commit_language,
            enable_tools: Some(true),
            skill_overrides: None,
        };

        let tool_store = repo_store.clone();
        let tool_target = self.session_read_target(session);
        let tool_head = Arc::new(session.initial_head.clone());
        let tool_files = Arc::new(session.frozen_files.clone());
        let tool_locale = session.locale;

        let outcome = supervisor
            .run_task_with_tools(
                paths,
                request_id.to_owned(),
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
                move |_call_id, tool_name, args| {
                    let store = tool_store.clone();
                    let target = tool_target.clone();
                    let head = Arc::clone(&tool_head);
                    let files = Arc::clone(&tool_files);
                    async move {
                        match tool_name.as_str() {
                            "read_scope_diff" => {
                                let Some(path) = args.get("path").and_then(Value::as_str) else {
                                    return PiToolExecutionResult::error(
                                        "Missing `path` argument for read_scope_diff",
                                    );
                                };
                                let start_line = args
                                    .get("startLine")
                                    .and_then(Value::as_u64)
                                    .map(|n| n as usize);
                                let end_line = args
                                    .get("endLine")
                                    .and_then(Value::as_u64)
                                    .map(|n| n as usize);
                                match store
                                    .read_cpush_scope_diff(
                                        &target,
                                        head.as_deref(),
                                        &files,
                                        path,
                                        start_line,
                                        end_line,
                                        tool_locale,
                                    )
                                    .await
                                {
                                    Ok(content) => PiToolExecutionResult::success(content),
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "read_repo_file" | "search_repo" => {
                                match store
                                    .execute_read_tool(&target, &tool_name, &args, tool_locale)
                                    .await
                                {
                                    Ok(content) => PiToolExecutionResult::success(content),
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            other => PiToolExecutionResult::error(format!(
                                "Unsupported read-only tool `{other}` for C-Push planning"
                            )),
                        }
                    }
                },
            )
            .await?;

        match outcome {
            PiTaskOutcome::CPushPlanCompleted { plan } => validate_plan_in_rust(
                plan,
                &session.frozen_files,
                !session.qa_log.is_empty(),
                session.locale,
            ),
            PiTaskOutcome::Cancelled => {
                Err(i18n::cpush::cpush_plan_cancelled_error(session.locale).into())
            }
            _ => Err(
                "C-Push planning returned an unexpected event type; prompt and selection were preserved."
                    .into(),
            ),
        }
    }

    pub(super) async fn apply_plan_or_execute(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        plan: PiCPushPlan,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        match plan {
            PiCPushPlan::Question {
                title,
                scope,
                description,
                affected_file_ids,
                options,
            } => {
                let by_id: HashMap<&str, &FrozenCPushFile> = session
                    .frozen_files
                    .iter()
                    .map(|f| (f.file_id.as_str(), f))
                    .collect();
                let chips: Vec<CPushMessageFileChip> = affected_file_ids
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()).copied())
                    .map(|file| CPushMessageFileChip {
                        id: file.file_id.clone(),
                        label: file_basename(&file.path),
                        badge: Some(if file.range_kind == "partial" {
                            i18n::cpush::cpush_partial_badge(locale).into()
                        } else {
                            i18n::cpush::cpush_needs_confirm_badge(locale).into()
                        }),
                    })
                    .collect();
                let question_id = format!("q-{}-{}", session.run_id, session.qa_log.len() + 1);
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.can_retry_remaining = false;
                session.timeline = vec![
                    CPushTimelineItem {
                        state: "done".into(),
                        label: i18n::cpush::cpush_timeline_frozen_count(
                            locale,
                            session.frozen_files.len(),
                        ),
                    },
                    CPushTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush::cpush_timeline_waiting_decision(locale, &title),
                    },
                ];
                session.message = Some(CPushUiMessage {
                    id: question_id,
                    kind: "question".into(),
                    title,
                    description,
                    scope: (!scope.trim().is_empty()).then_some(scope),
                    files: (!chips.is_empty()).then_some(chips),
                    commits: None,
                    options: Some(
                        options
                            .into_iter()
                            .map(|o| CPushMessageOption {
                                id: o.id,
                                label: o.label,
                                recommended: o.recommended,
                            })
                            .collect(),
                    ),
                    progress: None,
                });
                self.save_in_memory(session)?;
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
                    repository_snapshot: None,
                })
            }
            PiCPushPlan::Ready {
                summary: _,
                groups,
                skipped_file_ids,
                skip_push,
            } => {
                if skip_push {
                    session.push_authorized = false;
                }
                session.planned_groups = groups;
                session.skipped_file_ids = skipped_file_ids;
                session.next_step_index = 0;
                self.execute_planned_groups(
                    storage_dir,
                    repo_store,
                    request_id,
                    session,
                    on_progress,
                )
                .await
            }
        }
    }
}
