//! Read-only `Create with Picket` custom action button generator.

use super::{
    CUSTOM_ACTION_IDLE_TIMEOUT, CustomActionCoordinator, SessionAuthGrants,
    context::format_tool_status_label,
};
use crate::{
    actions::{
        cli_env::check_cli_env,
        executor::exec_controlled_git,
        policy::{CommandDecision, classify_git_command},
        types::*,
    },
    i18n,
    models::store::ModelStore,
    pi_runtime::{
        PiCommand, PiPaths, PiSupervisor, PiTaskOutcome, PiToolExecutionResult, RequestContext,
    },
    repository::{self, RepositoryStore},
    skills::RuntimeSkillSnapshot,
};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_custom_action<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        store: &RepositoryStore,
        request: &GenerateCustomActionRequest,
        on_progress: F,
    ) -> Result<GenerateCustomActionReply, String>
    where
        F: Fn(GenerateCustomActionProgressEvent) + Send + Sync + 'static,
    {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let user_request = request.user_request.trim().to_owned();
        if user_request.is_empty() || user_request.chars().count() > 2_000 {
            return Err(i18n::actions::action_create_empty_request_error(locale).into());
        }

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
        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());
        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let progress_cb = Arc::new(on_progress);

        progress_cb(GenerateCustomActionProgressEvent {
            protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
            run_id: run_id.clone(),
            status: "running",
            status_label: i18n::actions::action_create_inspecting_label(locale).into(),
            pending_call_id: None,
            card: None,
            questionnaire: None,
        });

        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let command = PiCommand::GenerateCustomAction {
            request_id: run_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: active_tree.branch.clone(),
            existing_labels: request.existing_labels.clone(),
            user_request,
            locale,
            skill_overrides: skill_snapshot.catalog_option(),
        };

        let coordinator = self.clone();
        let store_clone = store.clone();
        let target_clone = request.target.clone();
        let run_id_clone = run_id.clone();

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
                    let coordinator = coordinator.clone();
                    let store = store_clone.clone();
                    let target = target_clone.clone();
                    let root = worktree_root.clone();
                    let skills = Arc::clone(&skill_snapshot);
                    let grants = Arc::clone(&grants);
                    let cb = Arc::clone(&progress_cb);
                    let run_id = run_id_clone.clone();
                    async move {
                        let override_dirs = coordinator.override_cli_dirs();
                        let label = format_tool_status_label(locale, &tool_name, &args);
                        if tool_name != "read_skill_override_doc"
                            && tool_name != "message_user"
                        {
                            cb(GenerateCustomActionProgressEvent {
                                protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                run_id: run_id.clone(),
                                status: "running",
                                status_label: label,
                                pending_call_id: None,
                                card: None,
                                questionnaire: None,
                            });
                        }
                        match tool_name.as_str() {
                            "read_skill_override_doc" => {
                                let skill = args.get("skill").and_then(Value::as_str).unwrap_or("");
                                let path = args
                                    .get("path")
                                    .and_then(Value::as_str)
                                    .unwrap_or("SKILL.md");
                                match skills.read_doc(skill, path) {
                                    Ok(Some(content)) => PiToolExecutionResult::success(content),
                                    Ok(None) => {
                                        PiToolExecutionResult::error("Not found in overrides")
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
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
                            "check_cli_env" => {
                                let snap = check_cli_env(&root, override_dirs.as_deref()).await;
                                PiToolExecutionResult::success(
                                    serde_json::to_string_pretty(&snap)
                                        .unwrap_or_else(|_| "{}".into()),
                                )
                            }
                            "run_git" => {
                                let raw_args: Vec<String> = args
                                    .get("args")
                                    .and_then(Value::as_array)
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                // Strictly Tier-0 read-only during Create button drafting!
                                if classify_git_command(&raw_args) != CommandDecision::AllowTier0 {
                                    return PiToolExecutionResult::error(
                                        "Create with Pi is strictly read-only; only Tier-0 Git inspection commands are allowed.",
                                    );
                                }
                                match exec_controlled_git(&root, &raw_args, false, true).await {
                                    Ok(out) => {
                                        let body = format!(
                                            "exitCode: {:?}\nstdout:\n{}\nstderr:\n{}",
                                            out.exit_code, out.stdout, out.stderr
                                        );
                                        if out.success {
                                            PiToolExecutionResult::success(body)
                                        } else {
                                            PiToolExecutionResult::error(body)
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "message_user" => {
                                let raw_req: MessageUserRequest =
                                    match serde_json::from_value(args) {
                                        Ok(parsed) => parsed,
                                        Err(err) => {
                                            return PiToolExecutionResult::error(format!(
                                                "Invalid message_user payload: {err}"
                                            ));
                                        }
                                    };
                                let req = match raw_req.sanitize_and_validate() {
                                    Ok(valid) => valid,
                                    Err(err) => return PiToolExecutionResult::error(err),
                                };
                                if !req.has_questions() {
                                    let card = req.to_card(
                                        i18n::actions::action_create_inspecting_label(locale),
                                    );
                                    cb(GenerateCustomActionProgressEvent {
                                        protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                        run_id: run_id.clone(),
                                        status: "running",
                                        status_label: card.title.clone(),
                                        pending_call_id: None,
                                        card: Some(card),
                                        questionnaire: None,
                                    });
                                    return PiToolExecutionResult {
                                        ok: true,
                                        content: "{\"displayed\":true}".into(),
                                        details: Some(serde_json::json!({ "displayed": true })),
                                        is_error: false,
                                    };
                                }
                                let card = req.card_if_present(
                                    i18n::actions::action_create_needs_input_label(locale),
                                );
                                let waiter = match coordinator.register_pending_question(
                                    &run_id,
                                    &call_id,
                                    &req,
                                    grants,
                                ) {
                                    Ok(w) => w,
                                    Err(err) => return PiToolExecutionResult::error(err),
                                };
                                cb(GenerateCustomActionProgressEvent {
                                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                    run_id: run_id.clone(),
                                    status: "needsDecision",
                                    status_label: i18n::actions::action_create_needs_input_label(
                                        locale,
                                    )
                                    .into(),
                                    pending_call_id: Some(call_id.clone()),
                                    card,
                                    questionnaire: Some(req.clone()),
                                });
                                match waiter.wait().await {
                                    Ok(resp) => {
                                        cb(GenerateCustomActionProgressEvent {
                                            protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                            run_id: run_id.clone(),
                                            status: "running",
                                            status_label:
                                                i18n::actions::action_create_drafting_label(locale)
                                                    .into(),
                                            pending_call_id: None,
                                            card: None,
                                            questionnaire: None,
                                        });
                                        let details = serde_json::to_value(&resp).ok();
                                        let content = serde_json::to_string(&resp)
                                            .unwrap_or_else(|_| "{}".into());
                                        PiToolExecutionResult {
                                            ok: true,
                                            content,
                                            details,
                                            is_error: false,
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            other => PiToolExecutionResult::error(format!(
                                "Tool `{other}` is not available in Create with Picket read-only mode."
                            )),
                        }
                    }
                },
            )
            .await;

        match outcome {
            Ok(PiTaskOutcome::CustomActionDrafted { draft }) => Ok(GenerateCustomActionReply {
                protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                kind: "generateCustomAction",
                request_id: request.request_id.clone(),
                status: "done",
                draft: Some(CustomActionDraft {
                    label: draft.label,
                    prompt: draft.prompt,
                    summary: draft.summary,
                }),
            }),
            Ok(PiTaskOutcome::Cancelled) => Ok(GenerateCustomActionReply {
                protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                kind: "generateCustomAction",
                request_id: request.request_id.clone(),
                status: "cancelled",
                draft: None,
            }),
            Ok(other) => Err(format!(
                "Unexpected Pi outcome for generate_custom_action: {other:?}"
            )),
            Err(err) => Err(err),
        }
    }
}
