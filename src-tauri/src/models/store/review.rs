use super::{ModelStore, persistence::AI_TASK_IDLE_TIMEOUT};
use crate::{
    foundation::pi::{
        PiCommand, PiEvent, PiPaths, PiSupervisor, PiTaskOutcome, PiToolExecutionResult,
        RequestContext,
    },
    i18n,
    models::types::*,
    repository::{self, RepositoryStore},
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
};

type DiffReadRangesMap = HashMap<String, Vec<(usize, usize)>>;

impl ModelStore {
    pub async fn review_scope(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        repo_store: &RepositoryStore,
        request: &ReviewScopeRequest,
        mut on_stream: impl FnMut(ModelStreamEvent) -> Result<(), String> + Send,
    ) -> Result<ReviewScopeReply, String> {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        if let Some(changed) = &request.changed_paths
            && changed.iter().any(|path| path.trim().is_empty())
        {
            return Err("Changed paths list for re-review contains an empty path".into());
        }
        let spec = self
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let patch_budget = repository::ai_context::review_patch_budget_for_model(
            spec.context_window,
            spec.max_output_tokens,
        );
        let scope = repo_store
            .extract_ai_scope_with_budget(
                &request.target,
                &request.selections,
                locale,
                patch_budget,
            )
            .await?;
        if scope.files.is_empty() {
            return Ok(ReviewScopeReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                kind: "review",
                request_id: request.request_id.clone(),
                target: request.target.clone(),
                model_id: request.model_id.clone(),
                findings: Vec::new(),
                skipped_paths: scope.skipped_paths,
                reviewed_file_ids: Vec::new(),
            });
        }
        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let skill_snapshot = crate::skills::RuntimeSkillSnapshot::load_from_data_dir(storage_dir);
        let hard_skipped_paths = scope.skipped_paths.clone();
        let deferred_notices = scope.deferred_notices.clone();
        let authorized_patches = Arc::new(scope.authorized_patches);
        let read_diff_ranges: Arc<Mutex<DiffReadRangesMap>> = Arc::new(Mutex::new(HashMap::new()));
        let command = PiCommand::ReviewScope {
            request_id: request.request_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: scope.branch,
            prompt: request.prompt.clone(),
            files: scope.files,
            skipped_paths: hard_skipped_paths.clone(),
            locale,
            enable_tools: Some(true),
            skill_overrides: skill_snapshot.catalog_option(),
        };
        let stream_target = request.target.clone();
        let tool_target = request.target.clone();
        let tool_repo_store = repo_store.clone();
        let tool_patches = Arc::clone(&authorized_patches);
        let tool_ranges = Arc::clone(&read_diff_ranges);
        let outcome = supervisor
            .run_task_with_tools(
                paths,
                request.request_id.clone(),
                context,
                &command,
                AI_TASK_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |event| match event {
                    PiEvent::Started { request_id, .. } => on_stream(ModelStreamEvent::Started {
                        request_id: request_id.clone(),
                        kind: "review",
                        target: stream_target.clone(),
                    }),
                    PiEvent::TextDelta {
                        request_id, delta, ..
                    } => on_stream(ModelStreamEvent::Delta {
                        request_id: request_id.clone(),
                        kind: "review",
                        target: stream_target.clone(),
                        delta: delta.clone(),
                    }),
                    _ => Ok(()),
                },
                |_call_id, tool_name, args| {
                    let repo = tool_repo_store.clone();
                    let target = tool_target.clone();
                    let skills = skill_snapshot.clone();
                    let patches = Arc::clone(&tool_patches);
                    let ranges = Arc::clone(&tool_ranges);
                    async move {
                        if tool_name == "read_scope_diff" {
                            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                            let start_line = args
                                .get("startLine")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as usize);
                            let end_line = args
                                .get("endLine")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as usize);
                            return match repository::ai_context::read_authorized_scope_diff(
                                &patches, path, start_line, end_line,
                            ) {
                                Ok((content, actual_start, actual_end, _total)) => {
                                    if actual_end >= actual_start
                                        && let Ok(mut guard) = ranges.lock()
                                    {
                                        guard
                                            .entry(path.trim().to_owned())
                                            .or_default()
                                            .push((actual_start, actual_end));
                                    }
                                    PiToolExecutionResult::success(content)
                                }
                                Err(err) => PiToolExecutionResult::error(err),
                            };
                        }
                        if tool_name == "read_skill_override_doc" {
                            let skill = args.get("skill").and_then(|v| v.as_str()).unwrap_or("");
                            let path = args
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("SKILL.md");
                            return match skills.read_doc(skill, path) {
                                Ok(Some(doc)) => PiToolExecutionResult::success(doc),
                                Ok(None) => PiToolExecutionResult::error(format!(
                                    "No override skill document found for {skill}/{path}"
                                )),
                                Err(err) => PiToolExecutionResult::error(err),
                            };
                        }
                        match repo
                            .execute_read_tool(&target, &tool_name, &args, locale)
                            .await
                        {
                            Ok(content) => PiToolExecutionResult::success(content),
                            Err(err) => PiToolExecutionResult::error(err),
                        }
                    }
                },
            )
            .await?;

        match outcome {
            PiTaskOutcome::ReviewCompleted { findings } => {
                let mut final_skipped_paths = hard_skipped_paths;
                let mut unreviewed_deferred_ids = HashSet::new();
                let recorded_ranges = read_diff_ranges
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                for deferred in &deferred_notices {
                    let tool_reads = recorded_ranges
                        .get(&deferred.path)
                        .cloned()
                        .unwrap_or_default();
                    let mut combined = deferred.initial_ranges.clone();
                    combined.extend(tool_reads.iter().copied());
                    if repository::ai_context::ranges_cover_all_lines(
                        deferred.total_lines,
                        &combined,
                    ) {
                        continue;
                    }
                    if deferred.deferred_only {
                        if tool_reads.is_empty() {
                            final_skipped_paths.push(deferred.omitted_notice.clone());
                            unreviewed_deferred_ids.insert(deferred.file_id.clone());
                        } else {
                            final_skipped_paths.push(deferred.partial_notice.clone());
                        }
                    } else {
                        final_skipped_paths.push(deferred.partial_notice.clone());
                    }
                }
                let final_reviewed_file_ids: Vec<String> = scope
                    .reviewed_file_ids
                    .into_iter()
                    .filter(|id| !unreviewed_deferred_ids.contains(id))
                    .collect();
                let mapped = findings
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| ReviewFindingWire {
                        id: format!("finding-{}-{index}", request.request_id),
                        file_id: item.file_id,
                        path: item.path,
                        line: item.line.max(1),
                        severity: item.severity,
                        title: item.title,
                        reason: item.reason,
                    })
                    .collect();
                Ok(ReviewScopeReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    kind: "review",
                    request_id: request.request_id.clone(),
                    target: request.target.clone(),
                    model_id: request.model_id.clone(),
                    findings: mapped,
                    skipped_paths: final_skipped_paths,
                    reviewed_file_ids: final_reviewed_file_ids,
                })
            }
            PiTaskOutcome::Cancelled => {
                Err(i18n::models::model_review_cancelled_error(locale).into())
            }
            PiTaskOutcome::Completed { .. }
            | PiTaskOutcome::ModelsListed { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. }
            | PiTaskOutcome::CustomActionCompleted { .. }
            | PiTaskOutcome::CustomActionDrafted { .. } => Err(
                "Code review returned an unexpected event type; draft and selection were preserved."
                    .into(),
            ),
        }
    }
}
