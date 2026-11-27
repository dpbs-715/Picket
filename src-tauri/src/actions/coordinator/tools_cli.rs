//! Read-only inspection, CLI environment, installation, interactive question, and controlled
//! Git / platform CLI tool handlers for Custom Actions.

use super::context::{ActionToolContext, append_unique_links, push_timeline_step};
use crate::{
    actions::{
        cli_env::{check_cli_env, install_platform_cli},
        executor::{exec_controlled_git, exec_controlled_platform_cli},
        policy::{CommandDecision, classify_git_command, classify_platform_cli_command},
        session_store::{self, SubmitSessionLedger},
        types::{CustomActionMessageProgress, CustomActionUiMessage, MessageUserRequest},
    },
    i18n,
    pi_runtime::PiToolExecutionResult,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn resolve_authorized_git_cwd(
    storage_dir: &Path,
    repository_id: &str,
    active_root: &Path,
    ledger: &SubmitSessionLedger,
    requested_worktree: Option<&str>,
) -> Result<PathBuf, String> {
    let Some(wt_raw) = requested_worktree.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(active_root.to_path_buf());
    };
    let wt_path = PathBuf::from(wt_raw);
    if !wt_path.exists() {
        return Err(format!("Requested worktreePath does not exist: `{wt_raw}`"));
    }
    let canon_root =
        std::fs::canonicalize(active_root).unwrap_or_else(|_| active_root.to_path_buf());
    let canon_wt = std::fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
    if canon_wt == canon_root {
        return Ok(canon_wt);
    }
    let owned_in_run = ledger.groups.iter().any(|g| {
        g.worktree_path.as_deref().is_some_and(|p| {
            std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)) == canon_wt
        })
    });
    if owned_in_run
        || session_store::find_owned_worktree_group(storage_dir, repository_id, &canon_wt).is_some()
    {
        return Ok(canon_wt);
    }
    Err(format!(
        "Refused `run_git` outside the active repository worktree or Picket-owned isolated submit worktrees: `{wt_raw}`"
    ))
}

pub(super) fn handle_read_skill_override_doc(
    ctx: &ActionToolContext,
    args: &Value,
) -> PiToolExecutionResult {
    let skill = args.get("skill").and_then(Value::as_str).unwrap_or("");
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("SKILL.md");
    match ctx.skills.read_doc(skill, path) {
        Ok(Some(content)) => PiToolExecutionResult::success(content),
        Ok(None) => PiToolExecutionResult::error("Not found in overrides"),
        Err(err) => PiToolExecutionResult::error(err),
    }
}

pub(super) async fn handle_read_repo_tool(
    ctx: &ActionToolContext,
    tool_name: &str,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    match ctx
        .store
        .execute_read_tool(&ctx.target, tool_name, args, ctx.locale)
        .await
    {
        Ok(content) => {
            if let Ok(mut st) = ctx.run_state.lock() {
                push_timeline_step(&mut st.timeline, "done", status_label);
            }
            PiToolExecutionResult::success(content)
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

pub(super) async fn handle_check_cli_env(
    ctx: &ActionToolContext,
    status_label: String,
) -> PiToolExecutionResult {
    let override_dirs = ctx.coordinator.override_cli_dirs();
    let snapshot = check_cli_env(&ctx.root, override_dirs.as_deref()).await;
    if let Ok(mut st) = ctx.run_state.lock() {
        push_timeline_step(&mut st.timeline, "done", status_label);
    }
    let json_text = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".into());
    PiToolExecutionResult::success(json_text)
}

pub(super) async fn handle_install_platform_cli(
    ctx: &ActionToolContext,
    args: &Value,
) -> PiToolExecutionResult {
    let tool = args
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let authorized = ctx
        .grants
        .lock()
        .map(|g| g.authorized_installs.contains(&tool))
        .unwrap_or(false);
    if !authorized {
        return PiToolExecutionResult::error(format!(
            "Installation of `{tool}` is blocked because the user has not explicitly approved it via `message_user` (with `questions`) in this session. Ask the user with an option whose id is `install-{tool}` and preview `brew install {tool}` first."
        ));
    }
    let override_dirs = ctx.coordinator.override_cli_dirs();
    let emit_stream = Arc::clone(&ctx.emit);
    let run_id_s = ctx.run_id.clone();
    let label_s = ctx.action_label.clone();
    match install_platform_cli(&tool, &ctx.root, override_dirs.as_deref(), move |line| {
        emit_stream(
            "running",
            None,
            Some(CustomActionUiMessage {
                id: format!("progress-{run_id_s}"),
                kind: "progress".into(),
                title: label_s.clone(),
                description: line.clone(),
                scope: None,
                files: None,
                commits: None,
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: Some(CustomActionMessageProgress {
                    completed: 2,
                    total: 3,
                    label: line,
                }),
            }),
            None,
        );
    })
    .await
    {
        Ok(probe) => {
            if let Ok(mut st) = ctx.run_state.lock() {
                push_timeline_step(
                    &mut st.timeline,
                    "done",
                    i18n::actions::action_installed_cli_label(ctx.locale, &tool),
                );
            }
            PiToolExecutionResult::success(
                serde_json::to_string_pretty(&probe).unwrap_or_else(|_| "Installed".into()),
            )
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

pub(super) async fn handle_message_user(
    ctx: &ActionToolContext,
    call_id: &str,
    args: Value,
) -> PiToolExecutionResult {
    let raw_req: MessageUserRequest = match serde_json::from_value(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            return PiToolExecutionResult::error(format!("Invalid message_user payload: {err}"));
        }
    };
    let req = match raw_req.sanitize_and_validate() {
        Ok(valid) => valid,
        Err(err) => return PiToolExecutionResult::error(err),
    };

    let merged_links = req.merged_links();
    if !merged_links.is_empty()
        && let Ok(mut st) = ctx.run_state.lock()
    {
        append_unique_links(&mut st.links, merged_links.clone());
    }

    // Non-blocking rich display / live report card (no `questions`, never grants authorizations).
    if !req.has_questions() {
        let card = req.to_card(&ctx.action_label);
        let subtitle = card.subtitle.clone().unwrap_or_default();
        let all_links = if let Ok(mut st) = ctx.run_state.lock() {
            st.published_card = Some(card.clone());
            push_timeline_step(&mut st.timeline, "done", card.title.clone());
            st.links.clone()
        } else {
            merged_links
        };
        (ctx.emit)(
            "running",
            None,
            Some(CustomActionUiMessage {
                id: format!("progress-{}", ctx.run_id),
                kind: "progress".into(),
                title: card.title.clone(),
                description: subtitle,
                scope: req.context.as_ref().and_then(|c| c.scope.clone()),
                files: None,
                commits: None,
                options: None,
                links: (!all_links.is_empty()).then_some(all_links),
                card: Some(card.clone()),
                questionnaire: None,
                progress: Some(CustomActionMessageProgress {
                    completed: 2,
                    total: 3,
                    label: card.title.clone(),
                }),
            }),
            None,
        );
        ctx.persist("running", card.title).await;
        return PiToolExecutionResult {
            ok: true,
            content: "{\"displayed\":true}".into(),
            details: Some(serde_json::json!({ "displayed": true })),
            is_error: false,
        };
    }

    // Blocking Q&A interaction with optional rich `blocks` rendered right above the choices.
    let first_q = req.questions_slice().first();
    let title = req
        .title
        .clone()
        .or_else(|| first_q.map(|q| q.header.clone()))
        .unwrap_or_else(|| ctx.action_label.clone());
    let desc = req
        .subtitle
        .clone()
        .or_else(|| first_q.map(|q| q.question.clone()))
        .unwrap_or_default();
    let card = req.card_if_present(&title);
    let waiter = match ctx.coordinator.register_pending_question(
        &ctx.run_id,
        call_id,
        &req,
        Arc::clone(&ctx.grants),
    ) {
        Ok(w) => w,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    (ctx.emit)(
        "needsDecision",
        Some(call_id.to_owned()),
        Some(CustomActionUiMessage {
            id: format!("ask-{}-{call_id}", ctx.run_id),
            kind: "question".into(),
            title,
            description: desc.clone(),
            scope: req.context.as_ref().and_then(|c| c.scope.clone()),
            files: None,
            commits: None,
            options: None,
            links: (!merged_links.is_empty()).then_some(merged_links),
            card,
            questionnaire: Some(req.clone()),
            progress: None,
        }),
        None,
    );
    ctx.persist("needsDecision", desc).await;
    match waiter.wait().await {
        Ok(response) => {
            ctx.persist("running", String::new()).await;
            let resuming_label = i18n::actions::action_resuming_label(ctx.locale).to_owned();
            let preserved_card = ctx
                .run_state
                .lock()
                .ok()
                .and_then(|st| st.published_card.clone());
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
                    description: resuming_label.clone(),
                    scope: None,
                    files: None,
                    commits: None,
                    options: None,
                    links: None,
                    card: preserved_card,
                    questionnaire: None,
                    progress: Some(CustomActionMessageProgress {
                        completed: 2,
                        total: 3,
                        label: resuming_label,
                    }),
                }),
                None,
            );
            let details = serde_json::to_value(&response).ok();
            let content = serde_json::to_string(&response).unwrap_or_else(|_| "{}".into());
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

pub(super) async fn handle_run_git(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
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
    let requested_worktree = args.get("worktreePath").and_then(Value::as_str);
    let effective_cwd = {
        let ledger = ctx
            .run_state
            .lock()
            .map(|st| st.submit_ledger.clone())
            .unwrap_or_default();
        match resolve_authorized_git_cwd(
            &ctx.storage_dir,
            &ctx.target.repository_id,
            &ctx.root,
            &ledger,
            requested_worktree,
        ) {
            Ok(cwd) => cwd,
            Err(err) => return PiToolExecutionResult::error(err),
        }
    };
    let decision = classify_git_command(&raw_args);
    let is_read_only = match decision {
        CommandDecision::DenyRedline { code, reason } => {
            return PiToolExecutionResult::error(format!(
                "Blocked by Picket hard redline ({code}): {reason}"
            ));
        }
        CommandDecision::AllowTier0 => true,
        CommandDecision::AllowTier1 => false,
        CommandDecision::RequireConfirmTier2 {
            reason,
            preview,
            allow_dont_ask_again,
        } => {
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    &reason,
                    &preview,
                    allow_dont_ask_again,
                    "User declined to run the write command.",
                )
                .await
            {
                return err_result;
            }
            false
        }
    };
    let allow_remote = raw_args
        .iter()
        .any(|a| matches!(a.as_str(), "fetch" | "push" | "pull" | "ls-remote"));
    match exec_controlled_git(&effective_cwd, &raw_args, allow_remote, is_read_only).await {
        Ok(out) => {
            let should_persist = (!is_read_only && out.success) || !out.extracted_links.is_empty();
            if let Ok(mut st) = ctx.run_state.lock() {
                if !is_read_only && out.success {
                    st.wrote_repo = true;
                }
                append_unique_links(&mut st.links, out.extracted_links.clone());
                push_timeline_step(
                    &mut st.timeline,
                    if out.success { "done" } else { "failed" },
                    status_label,
                );
            }
            if should_persist {
                ctx.persist("running", String::new()).await;
            }
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

pub(super) async fn handle_run_platform_cli(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let cli = args.get("cli").and_then(Value::as_str).unwrap_or("");
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
    let body_text = args.get("bodyText").and_then(Value::as_str);
    let body_flag = args.get("bodyFlag").and_then(Value::as_str);
    let decision = classify_platform_cli_command(cli, &raw_args, &ctx.remotes);
    match decision {
        CommandDecision::DenyRedline { code, reason } => {
            return PiToolExecutionResult::error(format!(
                "Blocked by Picket hard redline ({code}): {reason}"
            ));
        }
        CommandDecision::AllowTier0 | CommandDecision::AllowTier1 => {}
        CommandDecision::RequireConfirmTier2 {
            reason,
            preview,
            allow_dont_ask_again,
        } => {
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    &reason,
                    &preview,
                    allow_dont_ask_again,
                    "User declined to run the platform CLI command.",
                )
                .await
            {
                return err_result;
            }
        }
    }
    let override_dirs = ctx.coordinator.override_cli_dirs();
    match exec_controlled_platform_cli(
        cli,
        &ctx.root,
        &raw_args,
        body_text,
        body_flag,
        override_dirs.as_deref(),
    )
    .await
    {
        Ok(out) => {
            let has_links = !out.extracted_links.is_empty();
            if let Ok(mut st) = ctx.run_state.lock() {
                append_unique_links(&mut st.links, out.extracted_links.clone());
                push_timeline_step(
                    &mut st.timeline,
                    if out.success { "done" } else { "failed" },
                    status_label,
                );
            }
            if out.success && has_links {
                ctx.persist("running", String::new()).await;
            }
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
