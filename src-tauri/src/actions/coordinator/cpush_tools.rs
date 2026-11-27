//! Tool dispatcher and `message_user` handler for the C-Push tool loop.

use super::{
    cpush_commit_tool::handle_cpush_commit_exact_scope,
    cpush_helpers::file_basename,
    cpush_sensitive::{
        CPushToolContext, emit_progress_event, resolve_cpush_file_token, summarize_user_response,
    },
};
use crate::{actions::types::*, i18n, pi_runtime::PiToolExecutionResult};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

async fn handle_cpush_message_user(
    ctx: &CPushToolContext,
    call_id: &str,
    args: Value,
) -> PiToolExecutionResult {
    let raw_req: MessageUserRequest = match serde_json::from_value(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            return PiToolExecutionResult::error(format!("Invalid message_user arguments: {err}"));
        }
    };
    let req = match raw_req.sanitize_and_validate() {
        Ok(valid) => valid,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    let fallback_title = req
        .questions_slice()
        .first()
        .map(|q| q.header.as_str())
        .unwrap_or("C-Push");
    let card = req.to_card(fallback_title);

    // Non-blocking informational card:
    if !req.has_questions() {
        let public = {
            let Ok(mut st) = ctx.run_state.lock() else {
                return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
            };
            let msg_id = format!("card-{}-{call_id}", st.session.run_id);
            st.session.message = Some(CPushUiMessage {
                id: msg_id,
                kind: "progress".into(),
                title: card.title.clone(),
                description: card.subtitle.clone().unwrap_or_default(),
                scope: Some(st.session.branch.clone()),
                files: None,
                commits: None,
                options: None,
                links: card.links.clone(),
                card: Some(card),
                questionnaire: None,
                progress: None,
            });
            let _ = ctx.coordinator.save_cpush_in_memory(&st.session);
            st.session.to_public()
        };
        emit_progress_event(
            &ctx.progress_cb,
            CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public,
                repository_snapshot: None,
            },
        );
        return PiToolExecutionResult::success("Message displayed to user in MessageDock.");
    }

    // Blocking interactive questionnaire:
    let (run_id, question_id, question_title, public_snapshot) = {
        let Ok(mut st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
        };
        let first_q = &req.questions_slice()[0];
        let q_title = req
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| first_q.header.clone());
        let q_desc = req
            .subtitle
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| first_q.question.clone());
        let q_scope = req
            .context
            .as_ref()
            .and_then(|c| c.scope.clone())
            .filter(|s| !s.trim().is_empty());

        let affected_chips: Vec<CPushMessageFileChip> = req
            .context
            .as_ref()
            .and_then(|c| c.affected_file_ids.as_ref())
            .map(|tokens| {
                let mut seen = HashSet::new();
                tokens
                    .iter()
                    .filter_map(|t| resolve_cpush_file_token(t, &st.session.frozen_files))
                    .filter(|f| seen.insert(f.file_id.clone()))
                    .map(|file| CPushMessageFileChip {
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

        let q_id = format!("q-{}-{}", st.session.run_id, st.session.qa_log.len() + 1);
        st.session.session_status = CPushSessionStatus::NeedsDecision;
        st.session.pending_call_id = Some(call_id.to_owned());
        st.session.can_retry_remaining = false;
        st.session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_frozen_count(
                    ctx.locale,
                    st.session.frozen_files.len(),
                ),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_waiting_decision(ctx.locale, &q_title),
            },
        ];
        st.session.message = Some(CPushUiMessage {
            id: q_id.clone(),
            kind: "question".into(),
            title: q_title.clone(),
            description: q_desc,
            scope: q_scope,
            files: (!affected_chips.is_empty()).then_some(affected_chips),
            commits: None,
            options: None,
            links: card.links.clone(),
            card: Some(card),
            questionnaire: Some(req.clone()),
            progress: None,
        });
        let _ = ctx.coordinator.save_cpush_in_memory(&st.session);
        (
            st.session.run_id.clone(),
            q_id,
            q_title,
            st.session.to_public(),
        )
    };
    let waiter = match ctx.coordinator.register_pending_question(
        &run_id,
        call_id,
        &req,
        Arc::clone(&ctx.grants),
    ) {
        Ok(w) => w,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    let _ = ctx
        .coordinator
        .persist_cpush_sessions(&ctx.storage_dir)
        .await;
    emit_progress_event(
        &ctx.progress_cb,
        CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public_snapshot,
            repository_snapshot: None,
        },
    );

    match waiter.wait().await {
        Ok(resp) => {
            let chose_dismiss = resp.answers.iter().any(|a| {
                a.selected_option_ids
                    .iter()
                    .any(|id| id == "dismiss-session")
            });
            if resp.cancelled || chose_dismiss {
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.user_cancelled = true;
                    st.session.pending_call_id = None;
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

            let (answer_label, option_id) = summarize_user_response(&req, &resp);
            let resuming_snapshot = {
                let Ok(mut st) = ctx.run_state.lock() else {
                    return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
                };
                st.qa_confirmed = true;
                st.session.pending_call_id = None;
                st.session.qa_log.push(CPushQaItem {
                    question_id,
                    question: question_title.clone(),
                    answer: answer_label.clone(),
                    option_id,
                });
                st.session.session_status = CPushSessionStatus::Running;
                st.session.timeline = vec![
                    CPushTimelineItem {
                        state: "done".into(),
                        label: i18n::cpush::cpush_timeline_confirmed_decision(
                            ctx.locale,
                            &question_title,
                            &answer_label,
                        ),
                    },
                    CPushTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush::cpush_timeline_replanning(ctx.locale).into(),
                    },
                ];
                st.session.message = Some(CPushUiMessage {
                    id: format!("progress-replan-{}-{call_id}", ctx.request_id),
                    kind: "progress".into(),
                    title: i18n::cpush::cpush_progress_replanning_title(ctx.locale).into(),
                    description: i18n::cpush::cpush_progress_replanning_desc(
                        ctx.locale,
                        &answer_label,
                    ),
                    scope: Some(st.session.branch.clone()),
                    files: None,
                    commits: None,
                    options: None,
                    links: None,
                    card: None,
                    questionnaire: None,
                    progress: Some(CPushMessageProgress {
                        completed: 2,
                        total: 4,
                        label: i18n::cpush::cpush_progress_replanning_steps(ctx.locale).into(),
                    }),
                });
                let _ = ctx.coordinator.save_cpush_in_memory(&st.session);
                st.session.to_public()
            };
            emit_progress_event(
                &ctx.progress_cb,
                CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: resuming_snapshot,
                    repository_snapshot: None,
                },
            );
            let details = serde_json::to_value(&resp).ok();
            let content = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".into());
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

pub(super) async fn dispatch_cpush_tool(
    ctx: Arc<CPushToolContext>,
    call_id: String,
    tool_name: String,
    args: Value,
) -> PiToolExecutionResult {
    match tool_name.as_str() {
        "read_skill_override_doc" => {
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
        "read_scope_diff" => {
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
            let (target, head, files) = {
                let Ok(st) = ctx.run_state.lock() else {
                    return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
                };
                (
                    ctx.coordinator.session_read_target(&st.session),
                    st.session.initial_head.clone(),
                    st.session.frozen_files.clone(),
                )
            };
            match ctx
                .store
                .read_cpush_scope_diff(
                    &target,
                    head.as_deref(),
                    &files,
                    path,
                    start_line,
                    end_line,
                    ctx.locale,
                )
                .await
            {
                Ok(content) => PiToolExecutionResult::success(content),
                Err(err) => PiToolExecutionResult::error(err),
            }
        }
        "read_repo_file" | "search_repo" => {
            let target = {
                let Ok(st) = ctx.run_state.lock() else {
                    return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
                };
                ctx.coordinator.session_read_target(&st.session)
            };
            match ctx
                .store
                .execute_read_tool(&target, &tool_name, &args, ctx.locale)
                .await
            {
                Ok(content) => PiToolExecutionResult::success(content),
                Err(err) => PiToolExecutionResult::error(err),
            }
        }
        "message_user" => handle_cpush_message_user(&ctx, &call_id, args).await,
        "commit_exact_scope" => handle_cpush_commit_exact_scope(&ctx, &call_id, &args).await,
        other => PiToolExecutionResult::error(format!(
            "Unsupported tool `{other}` for C-Push execution."
        )),
    }
}
