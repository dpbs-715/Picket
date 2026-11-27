//! `commit_exact_scope` tool handler for C-Push:
//! validates group coverage, intercepts sensitive files with a hard Rust safety check,
//! and executes exact-scope commits + remote sync/push.

use super::cpush_sensitive::{
    CPushToolContext, build_sensitive_questionnaire, emit_progress_event, resolve_cpush_file_token,
    summarize_user_response,
};
use crate::{
    actions::types::*, i18n, pi_runtime::PiToolExecutionResult,
    repository::ai_context::FrozenCPushFile,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub(super) async fn handle_cpush_commit_exact_scope(
    ctx: &CPushToolContext,
    call_id: &str,
    args: &Value,
) -> PiToolExecutionResult {
    let locale = ctx.locale;
    let (frozen_files, mut qa_confirmed) = {
        let Ok(st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
        };
        if st.settled_reply.is_some() {
            return PiToolExecutionResult::error(
                "C-Push commits were already settled in this run.",
            );
        }
        (st.session.frozen_files.clone(), st.qa_confirmed)
    };

    let raw_skipped: Vec<String> = args
        .get("skippedFileIds")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    if !qa_confirmed && !raw_skipped.is_empty() {
        return PiToolExecutionResult::error(format!(
            "Validation error: `skippedFileIds` MUST be empty before the user confirms via `message_user`. Either include ALL {} authorized files (@1..@{}) in `groups[].fileIds`, OR call `message_user` with `questions` first.",
            frozen_files.len(),
            frozen_files.len()
        ));
    }

    // Parse either `groups: [...]` or defensive single-group `{ fileIds, message }`:
    let raw_groups: Vec<(String, Vec<String>, String)> = if let Some(groups_arr) =
        args.get("groups").and_then(Value::as_array)
    {
        let mut parsed = Vec::with_capacity(groups_arr.len());
        for (idx, item) in groups_arr.iter().enumerate() {
            let Some(obj) = item.as_object() else {
                return PiToolExecutionResult::error(format!(
                    "Validation error: groups[{idx}] must be an object with `fileIds` and `message`."
                ));
            };
            let g_id = obj
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(64).collect())
                .unwrap_or_else(|| format!("group-{}", idx + 1));
            let g_files: Vec<String> = obj
                .get("fileIds")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let g_msg = obj
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if g_files.is_empty() || g_msg.is_empty() {
                return PiToolExecutionResult::error(i18n::cpush::cpush_plan_empty_group_error(
                    locale,
                ));
            }
            parsed.push((g_id, g_files, g_msg));
        }
        parsed
    } else {
        let g_files: Vec<String> = args
            .get("fileIds")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let g_msg = args
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        if g_files.is_empty() || g_msg.is_empty() {
            return PiToolExecutionResult::error(i18n::cpush::cpush_plan_empty_group_error(locale));
        }
        vec![("group-1".into(), g_files, g_msg)]
    };

    let mut seen_ids: HashSet<String> = HashSet::with_capacity(frozen_files.len());
    let mut skipped_file_ids: Vec<String> = Vec::with_capacity(raw_skipped.len());
    for token in &raw_skipped {
        let Some(matched) = resolve_cpush_file_token(token, &frozen_files) else {
            return PiToolExecutionResult::error(format!(
                "Validation error: unauthorized skipped file reference `{token}`."
            ));
        };
        if !seen_ids.insert(matched.file_id.clone()) {
            return PiToolExecutionResult::error(format!(
                "Validation error: file `{}` appeared more than once across skippedFileIds/groups.",
                matched.path
            ));
        }
        skipped_file_ids.push(matched.file_id.clone());
    }

    let mut groups: Vec<CPushCommitGroup> = Vec::with_capacity(raw_groups.len());
    for (g_id, g_tokens, g_msg) in raw_groups {
        let mut canonical_ids = Vec::with_capacity(g_tokens.len());
        for token in &g_tokens {
            let Some(matched) = resolve_cpush_file_token(token, &frozen_files) else {
                return PiToolExecutionResult::error(format!(
                    "Validation error: unauthorized file reference `{token}` is outside the user's selected scope. Only @1..@{} are authorized.",
                    frozen_files.len()
                ));
            };
            if !seen_ids.insert(matched.file_id.clone()) {
                return PiToolExecutionResult::error(format!(
                    "Validation error: authorized file `{}` (fileId={}) was assigned to multiple commit groups. Each file must appear in exactly one group.",
                    matched.path, matched.file_id
                ));
            }
            canonical_ids.push(matched.file_id.clone());
        }
        groups.push(CPushCommitGroup {
            id: g_id,
            message: g_msg,
            file_ids: canonical_ids,
        });
    }

    if groups.is_empty() && skipped_file_ids.is_empty() {
        return PiToolExecutionResult::error(i18n::cpush::cpush_plan_empty_group_error(locale));
    }

    let missing: Vec<String> = frozen_files
        .iter()
        .enumerate()
        .filter(|(_, f)| !seen_ids.contains(&f.file_id))
        .map(|(idx, f)| {
            format!(
                "- @{} | fileId={} | status={} | path={}",
                idx + 1,
                f.file_id,
                f.status,
                f.path
            )
        })
        .collect();
    if !missing.is_empty() {
        return PiToolExecutionResult::error(format!(
            "Validation error: commit_exact_scope omitted {} authorized selected file(s):\n{}\nEvery authorized file (@1..@{}) must appear once in groups[].fileIds (or confirmed skippedFileIds). Re-call commit_exact_scope with all missing files included.",
            missing.len(),
            missing.join("\n"),
            frozen_files.len()
        ));
    }

    // Hard Rust sensitive-file safety net when not yet confirmed by the user:
    if !qa_confirmed {
        let by_id: HashMap<&str, &FrozenCPushFile> = frozen_files
            .iter()
            .map(|f| (f.file_id.as_str(), f))
            .collect();
        let sensitive_files: Vec<&FrozenCPushFile> = groups
            .iter()
            .flat_map(|g| g.file_ids.iter())
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|f| crate::repository::ai_context::is_sensitive_local_file(&f.path))
            .collect();
        if !sensitive_files.is_empty() {
            let has_other = sensitive_files.len() < frozen_files.len();
            let (q_req, chips, q_title, q_scope, q_desc) =
                build_sensitive_questionnaire(locale, &sensitive_files, has_other);
            let sensitive_call_id = format!("sensitive-{call_id}");
            let (run_id, question_id, public_snap) = {
                let Ok(mut st) = ctx.run_state.lock() else {
                    return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
                };
                let q_id = format!("q-{}-{}", st.session.run_id, st.session.qa_log.len() + 1);
                let card = q_req.to_card(&q_title);
                st.session.session_status = CPushSessionStatus::NeedsDecision;
                st.session.pending_call_id = Some(sensitive_call_id.clone());
                st.session.can_retry_remaining = false;
                st.session.timeline = vec![
                    CPushTimelineItem {
                        state: "done".into(),
                        label: i18n::cpush::cpush_timeline_frozen_count(
                            locale,
                            st.session.frozen_files.len(),
                        ),
                    },
                    CPushTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush::cpush_timeline_waiting_decision(locale, &q_title),
                    },
                ];
                st.session.message = Some(CPushUiMessage {
                    id: q_id.clone(),
                    kind: "question".into(),
                    title: q_title.clone(),
                    description: q_desc,
                    scope: Some(q_scope),
                    files: Some(chips),
                    commits: None,
                    options: None,
                    links: None,
                    card: Some(card),
                    questionnaire: Some(q_req.clone()),
                    progress: None,
                });
                let _ = ctx.coordinator.save_cpush_in_memory(&st.session);
                (st.session.run_id.clone(), q_id, st.session.to_public())
            };
            let waiter = match ctx.coordinator.register_pending_question(
                &run_id,
                &sensitive_call_id,
                &q_req,
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
                    session: public_snap,
                    repository_snapshot: None,
                },
            );

            match waiter.wait().await {
                Ok(resp) => {
                    if resp.cancelled {
                        if let Ok(mut st) = ctx.run_state.lock() {
                            st.user_cancelled = true;
                            st.session.pending_call_id = None;
                        }
                        return PiToolExecutionResult {
                            ok: true,
                            content: "User cancelled sensitive-file confirmation.".into(),
                            details: Some(json!({
                                "stopAgent": true,
                                "summary": "",
                                "links": []
                            })),
                            is_error: false,
                        };
                    }
                    let (answer_label, option_id) = summarize_user_response(&q_req, &resp);
                    let chose_skip = option_id.as_deref() == Some("skip")
                        || resp.answers.iter().any(|a| {
                            a.selected_option_ids.iter().any(|id| id == "skip")
                                || a.custom_text.as_deref().is_some_and(|t| {
                                    let low = t.to_ascii_lowercase();
                                    low.contains("skip")
                                        || low.contains("exclude")
                                        || t.contains("排除")
                                        || t.contains("跳过")
                                })
                        });
                    if chose_skip {
                        let sensitive_id_set: HashSet<String> =
                            sensitive_files.iter().map(|f| f.file_id.clone()).collect();
                        for group in &mut groups {
                            group.file_ids.retain(|id| !sensitive_id_set.contains(id));
                        }
                        groups.retain(|g| !g.file_ids.is_empty());
                        for f in &sensitive_files {
                            if !skipped_file_ids.contains(&f.file_id) {
                                skipped_file_ids.push(f.file_id.clone());
                            }
                        }
                    }
                    qa_confirmed = true;
                    if let Ok(mut st) = ctx.run_state.lock() {
                        st.qa_confirmed = true;
                        st.session.pending_call_id = None;
                        st.session.qa_log.push(CPushQaItem {
                            question_id,
                            question: q_title,
                            answer: answer_label,
                            option_id,
                        });
                    }
                }
                Err(err) => return PiToolExecutionResult::error(err),
            }
        }
    }

    let _ = qa_confirmed;
    let requested_push = args.get("push").and_then(Value::as_bool);
    let summary_hint = args
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let mut session = {
        let Ok(mut st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("C-Push run state lock is unavailable.");
        };
        if requested_push == Some(false) {
            st.session.push_authorized = false;
        }
        st.session.pending_call_id = None;
        st.session.planned_groups = groups;
        st.session.skipped_file_ids = skipped_file_ids;
        st.session.next_step_index = 0;
        st.session.clone()
    };

    let progress_cb = Arc::clone(&ctx.progress_cb);
    let mut forward_progress = |event: CPushProgressEvent| {
        emit_progress_event(&progress_cb, event);
        Ok(())
    };

    match ctx
        .coordinator
        .execute_planned_groups(
            &ctx.storage_dir,
            &ctx.store,
            &ctx.request_id,
            &mut session,
            &mut forward_progress,
        )
        .await
    {
        Ok(reply) => {
            let final_summary = summary_hint
                .or_else(|| reply.session.message.as_ref().map(|m| m.title.clone()))
                .unwrap_or_else(|| "C-Push settled.".into());
            if let Ok(mut st) = ctx.run_state.lock() {
                st.session = session;
                st.settled_reply = Some(reply);
            }
            PiToolExecutionResult {
                ok: true,
                content: final_summary.clone(),
                details: Some(json!({
                    "stopAgent": true,
                    "summary": final_summary,
                    "links": []
                })),
                is_error: false,
            }
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}
