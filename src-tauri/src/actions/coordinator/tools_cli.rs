//! Read-only inspection, CLI environment, installation, interactive question, and controlled
//! Git / platform CLI tool handlers for Custom Actions.

use super::{
    context::{ActionToolContext, Tier2RiskLevel, append_unique_links, push_timeline_step},
    session_helpers::{file_basename, resolve_frozen_file_token, summarize_user_response},
};
use crate::{
    actions::{
        cli_env::{check_cli_env, install_platform_cli},
        executor::{exec_controlled_git, exec_controlled_platform_cli},
        policy::{
            CommandDecision, classify_git_command, classify_platform_cli_command, format_preview,
        },
        profile::ActionProfileKind,
        session_store::{self, SubmitSessionLedger},
        types::{
            CPushQaItem, MessageUserRequest, WorkflowMessageFileChip, WorkflowMessageProgress,
            WorkflowTimelineItem, WorkflowUiMessage,
        },
    },
    i18n,
    pi_runtime::PiToolExecutionResult,
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
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

pub(super) async fn handle_read_scope_diff(
    ctx: &ActionToolContext,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        return PiToolExecutionResult::error("Missing `path` argument for read_scope_diff");
    };
    let start_line = args
        .get("startLine")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    let end_line = args
        .get("endLine")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    let (head, files) = {
        let Ok(st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("Action run state lock is unavailable.");
        };
        (st.initial_head.clone(), st.frozen_files.clone())
    };
    if files.is_empty() {
        return PiToolExecutionResult::error(
            "No authorized selected files are in scope for read_scope_diff.",
        );
    }
    match ctx
        .store
        .read_cpush_scope_diff(
            &ctx.target,
            head.as_deref(),
            &files,
            path,
            start_line,
            end_line,
            ctx.locale,
        )
        .await
    {
        Ok(content) => {
            if ctx.profile.kind != ActionProfileKind::Cpush
                && let Ok(mut st) = ctx.run_state.lock()
            {
                push_timeline_step(&mut st.timeline, "done", status_label);
            }
            PiToolExecutionResult::success(content)
        }
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
            if ctx.profile.kind != ActionProfileKind::Cpush
                && let Ok(mut st) = ctx.run_state.lock()
            {
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
            Some(WorkflowUiMessage {
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
                progress: Some(WorkflowMessageProgress {
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
    let req = match MessageUserRequest::parse_and_validate(args) {
        Ok(valid) => valid,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    let is_cpush = ctx.profile.kind == ActionProfileKind::Cpush;

    let merged_links = req.merged_links();
    if !merged_links.is_empty()
        && let Ok(mut st) = ctx.run_state.lock()
    {
        append_unique_links(&mut st.links, merged_links.clone());
    }

    // Non-blocking rich display / live report card (no `questions`, never grants authorizations).
    if !req.has_questions() {
        let fallback_title = if is_cpush {
            "C-Push"
        } else {
            &ctx.action_label
        };
        let card = req.to_card(fallback_title);
        let subtitle = card.subtitle.clone().unwrap_or_default();
        let (all_links, branch) = if let Ok(mut st) = ctx.run_state.lock() {
            if !is_cpush {
                st.published_card = Some(card.clone());
                push_timeline_step(&mut st.timeline, "done", card.title.clone());
            }
            (st.links.clone(), st.branch.clone())
        } else {
            (merged_links, String::new())
        };
        let msg_id = if is_cpush {
            format!("card-{}-{call_id}", ctx.run_id)
        } else {
            format!("progress-{}", ctx.run_id)
        };
        let scope = if is_cpush {
            Some(branch)
        } else {
            req.context.as_ref().and_then(|c| c.scope.clone())
        };
        let progress = (!is_cpush).then(|| WorkflowMessageProgress {
            completed: 2,
            total: 3,
            label: card.title.clone(),
        });
        (ctx.emit)(
            "running",
            None,
            Some(WorkflowUiMessage {
                id: msg_id,
                kind: "progress".into(),
                title: card.title.clone(),
                description: subtitle,
                scope,
                files: None,
                commits: None,
                options: None,
                links: (!all_links.is_empty()).then_some(all_links),
                card: Some(card.clone()),
                questionnaire: None,
                progress,
            }),
            None,
        );
        if !is_cpush {
            ctx.persist("running", card.title).await;
            return PiToolExecutionResult {
                ok: true,
                content: "{\"displayed\":true}".into(),
                details: Some(json!({ "displayed": true })),
                is_error: false,
            };
        }
        return PiToolExecutionResult::success("Message displayed to user in MessageDock.");
    }

    // Blocking Q&A interaction with optional rich `blocks` rendered right above the choices.
    let first_q = req.questions_slice().first();
    let title = req
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| first_q.map(|q| q.header.clone()))
        .unwrap_or_else(|| ctx.action_label.clone());
    let desc = req
        .subtitle
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| first_q.map(|q| q.question.clone()))
        .unwrap_or_default();
    let card = if is_cpush {
        Some(req.to_card(&title))
    } else {
        req.card_if_present(&title)
    };
    let q_scope = req
        .context
        .as_ref()
        .and_then(|c| c.scope.clone())
        .filter(|s| !s.trim().is_empty());

    let (q_id, affected_chips) = {
        let Ok(mut st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("Action run state lock is unavailable.");
        };
        let chips: Vec<WorkflowMessageFileChip> = req
            .context
            .as_ref()
            .and_then(|c| c.affected_file_ids.as_ref())
            .map(|tokens| {
                let mut seen = HashSet::new();
                tokens
                    .iter()
                    .filter_map(|t| resolve_frozen_file_token(t, &st.frozen_files))
                    .filter(|f| seen.insert(f.file_id.clone()))
                    .map(|file| WorkflowMessageFileChip {
                        id: file.file_id.clone(),
                        label: file_basename(&file.path),
                        badge: Some(if file.range_kind == "partial" {
                            i18n::cpush::cpush_partial_badge(ctx.locale).into()
                        } else {
                            i18n::cpush::cpush_needs_confirm_badge(ctx.locale).into()
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let q_id = if is_cpush {
            st.can_retry_remaining = false;
            st.timeline = vec![
                WorkflowTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush::cpush_timeline_frozen_count(
                        ctx.locale,
                        st.frozen_files.len(),
                    ),
                },
                WorkflowTimelineItem {
                    state: "active".into(),
                    label: i18n::cpush::cpush_timeline_waiting_decision(ctx.locale, &title),
                },
            ];
            format!("q-{}-{}", ctx.run_id, st.qa_log.len() + 1)
        } else {
            format!("ask-{}-{call_id}", ctx.run_id)
        };
        (q_id, chips)
    };

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
        Some(WorkflowUiMessage {
            id: q_id.clone(),
            kind: "question".into(),
            title: title.clone(),
            description: desc.clone(),
            scope: q_scope,
            files: (!affected_chips.is_empty()).then_some(affected_chips),
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
            if is_cpush {
                let chose_dismiss = response.answers.iter().any(|a| {
                    a.selected_option_ids
                        .iter()
                        .any(|id| id == "dismiss-session")
                });
                if response.cancelled || chose_dismiss {
                    if let Ok(mut st) = ctx.run_state.lock() {
                        st.user_cancelled = true;
                        st.pending_call_id = None;
                    }
                    return PiToolExecutionResult {
                        ok: true,
                        content: "User cancelled C-Push.".into(),
                        details: Some(json!({
                            "stopAgent": true,
                            "summary": "",
                            "links": []
                        })),
                        is_error: false,
                    };
                }
            }

            let (answer_label, option_id) = summarize_user_response(&req, &response);
            let (branch, preserved_card) = {
                let Ok(mut st) = ctx.run_state.lock() else {
                    return PiToolExecutionResult::error("Action run state lock is unavailable.");
                };
                st.qa_confirmed = true;
                st.pending_call_id = None;
                st.qa_log.push(CPushQaItem {
                    question_id: q_id,
                    question: title.clone(),
                    answer: answer_label.clone(),
                    option_id,
                });
                if is_cpush {
                    st.timeline = vec![
                        WorkflowTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_confirmed_decision(
                                ctx.locale,
                                &title,
                                &answer_label,
                            ),
                        },
                        WorkflowTimelineItem {
                            state: "active".into(),
                            label: i18n::cpush::cpush_timeline_replanning(ctx.locale).into(),
                        },
                    ];
                }
                (st.branch.clone(), st.published_card.clone())
            };

            if is_cpush {
                (ctx.emit)(
                    "running",
                    None,
                    Some(WorkflowUiMessage {
                        id: format!("progress-replan-{}-{call_id}", ctx.request_id),
                        kind: "progress".into(),
                        title: i18n::cpush::cpush_progress_replanning_title(ctx.locale).into(),
                        description: i18n::cpush::cpush_progress_replanning_desc(
                            ctx.locale,
                            &answer_label,
                        ),
                        scope: Some(branch),
                        files: None,
                        commits: None,
                        options: None,
                        links: None,
                        card: None,
                        questionnaire: None,
                        progress: Some(WorkflowMessageProgress {
                            completed: 2,
                            total: 4,
                            label: i18n::cpush::cpush_progress_replanning_steps(ctx.locale).into(),
                        }),
                    }),
                    None,
                );
            } else {
                ctx.persist("running", String::new()).await;
                let resuming_label = i18n::actions::action_resuming_label(ctx.locale).to_owned();
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
                        description: resuming_label.clone(),
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
                            label: resuming_label,
                        }),
                    }),
                    None,
                );
            }
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
        CommandDecision::AllowTier1 => {
            let preview = format_preview("git", &raw_args);
            let reason = format!("Running Git write command `{preview}`.");
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    Tier2RiskLevel::RoutineWrite,
                    &reason,
                    &preview,
                    "User declined to run the write command.",
                )
                .await
            {
                return err_result;
            }
            false
        }
        CommandDecision::RequireConfirmTier2 {
            reason, preview, ..
        } => {
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    Tier2RiskLevel::HighRiskWrite,
                    &reason,
                    &preview,
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
            let subcommand = raw_args
                .iter()
                .find(|arg| !arg.starts_with('-'))
                .map(String::as_str);
            if let Ok(mut st) = ctx.run_state.lock() {
                if !is_read_only && out.success {
                    st.wrote_repo = true;
                    if matches!(
                        subcommand,
                        Some(
                            "switch" | "checkout" | "branch" | "push" | "rebase" | "pull" | "reset"
                        )
                    ) {
                        st.recovery_handles.clear();
                    }
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
        CommandDecision::AllowTier0 => {}
        CommandDecision::AllowTier1 => {
            let preview = format_preview(cli, &raw_args);
            let reason = format!("Running platform CLI write command `{preview}`.");
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    Tier2RiskLevel::RoutineWrite,
                    &reason,
                    &preview,
                    "User declined to run the platform CLI command.",
                )
                .await
            {
                return err_result;
            }
        }
        CommandDecision::RequireConfirmTier2 {
            reason, preview, ..
        } => {
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    Tier2RiskLevel::HighRiskWrite,
                    &reason,
                    &preview,
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
