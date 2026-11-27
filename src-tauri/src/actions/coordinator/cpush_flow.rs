//! New-engine C-Push orchestration (`start_cpush`):
//! runs on the unified `PiCommand::RunCustomAction { mode: Cpush }` tool loop with
//! live `message_user` Q&A, optional `read_scope_diff`/`read_repo_file`/`search_repo`,
//! hot-synced/disabled skills, Rust hard sensitive-file safety interception, batch
//! exact-scope commits, deterministic remote sync/push, and `stopAgent: true` 1-turn fast path.

use super::{
    CPUSH_AI_IDLE_TIMEOUT, CustomActionCoordinator, SessionAuthGrants,
    cpush_helpers::{file_basename, scope_badge},
};
use crate::{
    actions::types::*,
    i18n::{self, LanguageKind},
    models::store::ModelStore,
    pi_runtime::{
        CustomActionRunMode, PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome,
        PiToolExecutionResult, RequestContext,
    },
    repository::{self, RepositoryStore, ai_context::FrozenCPushFile},
    skills::RuntimeSkillSnapshot,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

type SharedProgressCb = Arc<Mutex<Box<dyn FnMut(CPushProgressEvent) -> Result<(), String> + Send>>>;

struct CPushLiveRunState {
    session: StoredCPushSession,
    qa_confirmed: bool,
    user_cancelled: bool,
    settled_reply: Option<CPushCommandReply>,
}

struct CPushToolContext {
    coordinator: CustomActionCoordinator,
    store: RepositoryStore,
    storage_dir: PathBuf,
    skills: Arc<RuntimeSkillSnapshot>,
    grants: Arc<Mutex<SessionAuthGrants>>,
    run_state: Arc<Mutex<CPushLiveRunState>>,
    progress_cb: SharedProgressCb,
    request_id: String,
    locale: LanguageKind,
}

fn emit_progress_event(cb: &SharedProgressCb, event: CPushProgressEvent) {
    if let Ok(mut f) = cb.lock() {
        let _ = f(event);
    }
}

fn resolve_cpush_file_token<'a>(
    token: &str,
    files: &'a [FrozenCPushFile],
) -> Option<&'a FrozenCPushFile> {
    let clean = token.trim();
    if clean.is_empty() {
        return None;
    }
    if let Some(found) = files.iter().find(|f| f.file_id == clean) {
        return Some(found);
    }
    if let Some(rest) = clean.strip_prefix('@')
        && let Ok(idx) = rest.parse::<usize>()
        && idx >= 1
        && idx <= files.len()
    {
        return files.get(idx - 1);
    }
    let normalized = clean.trim_start_matches("./");
    if let Some(found) = files.iter().find(|f| f.path == normalized) {
        return Some(found);
    }
    files
        .iter()
        .find(|f| f.previous_path.as_deref() == Some(normalized))
}

fn summarize_user_response(
    req: &MessageUserRequest,
    resp: &MessageUserResponse,
) -> (String, Option<String>) {
    let mut parts = Vec::new();
    let mut first_opt_id: Option<String> = None;
    for ans in &resp.answers {
        if first_opt_id.is_none() {
            first_opt_id = ans.selected_option_ids.first().cloned();
        }
        let mut ans_parts = Vec::new();
        if !ans.selected_labels.is_empty() {
            ans_parts.push(ans.selected_labels.join(", "));
        } else if !ans.selected_option_ids.is_empty() {
            ans_parts.push(ans.selected_option_ids.join(", "));
        }
        if let Some(ref custom) = ans.custom_text {
            let trimmed = custom.trim();
            if !trimmed.is_empty() {
                ans_parts.push(trimmed.to_owned());
            }
        }
        if !ans_parts.is_empty() {
            parts.push(ans_parts.join(" — "));
        }
    }
    if let Some(ref note) = resp.global_note {
        let trimmed = note.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_owned());
        }
    }
    let label = if parts.is_empty() {
        req.questions_slice()
            .first()
            .map(|q| q.header.clone())
            .unwrap_or_else(|| "Confirmed".into())
    } else {
        parts.join("; ")
    };
    (label, first_opt_id)
}

fn build_sensitive_questionnaire(
    locale: LanguageKind,
    sensitive_files: &[&FrozenCPushFile],
    has_other_files: bool,
) -> (
    MessageUserRequest,
    Vec<CPushMessageFileChip>,
    String,
    String,
    String,
) {
    let names: Vec<String> = sensitive_files
        .iter()
        .map(|f| file_basename(&f.path))
        .collect();
    let affected_ids: Vec<String> = sensitive_files.iter().map(|f| f.file_id.clone()).collect();
    let (title, scope, description, legacy_opts) =
        i18n::cpush::cpush_sensitive_question(locale, &names, has_other_files);
    let chips: Vec<CPushMessageFileChip> = sensitive_files
        .iter()
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
    let options: Vec<MessageQuestionOption> = legacy_opts
        .into_iter()
        .map(|o| {
            let desc = if o.id == "skip" {
                match locale {
                    LanguageKind::Zh => "将敏感文件保留在本地工作区，不纳入本次提交".into(),
                    LanguageKind::En => {
                        "Keep sensitive files in the local working tree and exclude from commits"
                            .into()
                    }
                }
            } else {
                match locale {
                    LanguageKind::Zh => "确认不含私密凭据后一并提交".into(),
                    LanguageKind::En => {
                        "Include in this commit after confirming no secrets are exposed".into()
                    }
                }
            };
            MessageQuestionOption {
                id: o.id,
                label: o.label,
                description: desc,
                recommended: o.recommended,
                preview: None,
            }
        })
        .collect();
    let header = match locale {
        LanguageKind::Zh => "敏感文件确认".into(),
        LanguageKind::En => "Sensitive files".into(),
    };
    let request = MessageUserRequest {
        title: Some(title.clone()),
        subtitle: Some(scope.clone()),
        tone: Some(MessageTone::Warning),
        blocks: None,
        links: None,
        questions: Some(vec![MessageQuestionItem {
            id: "sensitive-files".into(),
            header,
            question: description.clone(),
            multi_select: None,
            options,
        }]),
        context: Some(MessageQuestionContext {
            scope: Some(scope.clone()),
            affected_file_ids: Some(affected_ids),
            links: None,
            allow_dont_ask_again: None,
            dont_ask_again_label: None,
        }),
    };
    (request, chips, title, scope, description)
}

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

async fn handle_cpush_commit_exact_scope(
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

async fn dispatch_cpush_tool(
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

impl CustomActionCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub async fn start_cpush<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &StartCPushRequest,
        on_progress: F,
    ) -> Result<CPushCommandReply, String>
    where
        F: FnMut(CPushProgressEvent) -> Result<(), String> + Send + 'static,
    {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::CPush,
            locale,
        )?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self.lookup_processed_cpush_request(&request.request_id)? {
            return Ok(existing_reply);
        }
        self.ensure_no_busy_cpush_session(&request.target.worktree_id, locale, true)?;

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

        let live_snapshot = repo_store
            .refresh_with_locale(&request.target, locale)
            .await
            .ok();
        let local_keep_paths: Vec<String> = live_snapshot
            .as_ref()
            .map(|s| s.repository.local_keep_paths.clone())
            .unwrap_or_default();
        let remote_branches: Vec<String> = live_snapshot
            .as_ref()
            .map(|s| {
                s.repository
                    .remote_branches
                    .iter()
                    .map(|b| b.name.clone())
                    .collect()
            })
            .unwrap_or_default();

        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));
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
            links: None,
            card: None,
            questionnaire: None,
            progress: Some(CPushMessageProgress {
                completed: 1,
                total: 4,
                label: i18n::cpush::cpush_progress_planning_steps(locale).into(),
            }),
        };

        let push_authorized = request.push.unwrap_or(true);
        let session = StoredCPushSession {
            run_id: run_id.clone(),
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            frozen_scope_version: request.target.scope_version.clone(),
            model_id: request.model_id.clone(),
            branch: extracted.branch.clone(),
            initial_head: extracted.initial_head.clone(),
            prompt: request.prompt.clone(),
            recent_commits: extracted.recent_commits.clone(),
            unselected_paths: extracted.unselected_paths.clone(),
            frozen_files: extracted.files.clone(),
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: Vec::new(),
            commits: Vec::new(),
            qa_log: Vec::new(),
            timeline: initial_timeline,
            session_status: CPushSessionStatus::Running,
            pending_call_id: None,
            message: Some(progress_msg),
            can_retry_remaining: false,
            push_authorized,
            auto_collapse: false,
            notice_dismissed: false,
            locale,
            commit_language: request.commit_language,
        };

        let progress_cb: SharedProgressCb = Arc::new(Mutex::new(Box::new(on_progress)));
        self.save_cpush_in_memory(&session)?;
        let _ = self.persist_cpush_sessions(storage_dir).await;
        emit_progress_event(
            &progress_cb,
            CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: session.to_public(),
                repository_snapshot: None,
            },
        );

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
        let command = PiCommand::RunCustomAction {
            mode: Some(CustomActionRunMode::Cpush),
            request_id: run_id.clone(),
            context: Box::new(context.clone()),
            model: Box::new(spec.to_pi_spec()),
            branch: session.branch.clone(),
            detached: false,
            remote_branches,
            action_label: "C-Push".into(),
            action_prompt: String::new(),
            extra_prompt: session.prompt.clone(),
            selected_files: files_wire,
            unselected_paths: session.unselected_paths.clone(),
            local_keep_paths,
            recent_commits: Some(session.recent_commits.clone()),
            push_authorized: Some(push_authorized),
            upstream: None,
            commits: None,
            conflict_files: None,
            locale,
            commit_language: request.commit_language,
            previous_checkpoint: None,
            follow_up_prompt: None,
            skill_overrides: skill_snapshot.catalog_option(),
        };

        let run_state = Arc::new(Mutex::new(CPushLiveRunState {
            session: session.clone(),
            qa_confirmed: false,
            user_cancelled: false,
            settled_reply: None,
        }));
        let tool_ctx = Arc::new(CPushToolContext {
            coordinator: self.clone(),
            store: repo_store.clone(),
            storage_dir: storage_dir.to_path_buf(),
            skills: skill_snapshot,
            grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
            run_state: Arc::clone(&run_state),
            progress_cb: Arc::clone(&progress_cb),
            request_id: request.request_id.clone(),
            locale,
        });

        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());

        let outcome = supervisor
            .run_task_with_tools(
                paths,
                run_id.clone(),
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                api_key_for_redaction.as_deref(),
                |_event| Ok(()),
                move |call_id, tool_name, args| {
                    dispatch_cpush_tool(Arc::clone(&tool_ctx), call_id, tool_name, args)
                },
            )
            .await;

        let (settled_reply, user_cancelled, latest_session) = {
            let mut st = run_state
                .lock()
                .map_err(|_| "C-Push run state lock is unavailable".to_string())?;
            (
                st.settled_reply.take(),
                st.user_cancelled,
                st.session.clone(),
            )
        };

        if let Some(reply) = settled_reply {
            self.record_processed_cpush_request(&request.request_id, &reply)?;
            return Ok(reply);
        }

        let reset_idle_and_emit = || async {
            let _ = self.remove_cpush_in_memory(&latest_session.worktree_id);
            let _ = self.persist_cpush_sessions(storage_dir).await;
            let idle_snapshot = CPushSessionSnapshot {
                session_status: CPushSessionStatus::Idle,
                pending_call_id: None,
                message: None,
                timeline: Vec::new(),
                ..latest_session.to_public()
            };
            emit_progress_event(
                &progress_cb,
                CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: idle_snapshot.clone(),
                    repository_snapshot: None,
                },
            );
            idle_snapshot
        };

        if user_cancelled || matches!(outcome, Ok(PiTaskOutcome::Cancelled)) {
            let idle_snapshot = reset_idle_and_emit().await;
            let reply = CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request.request_id.clone(),
                session: idle_snapshot,
                repository_snapshot: None,
            };
            self.record_processed_cpush_request(&request.request_id, &reply)?;
            return Ok(reply);
        }

        match outcome {
            Ok(PiTaskOutcome::CustomActionCompleted { summary, .. }) => {
                let _ = reset_idle_and_emit().await;
                let detail = if summary.trim().is_empty() {
                    "Model finished without calling commit_exact_scope.".to_owned()
                } else {
                    summary
                };
                Err(detail)
            }
            Ok(other) => {
                let _ = reset_idle_and_emit().await;
                Err(format!(
                    "C-Push returned an unexpected event type: {other:?}"
                ))
            }
            Err(err) => {
                let _ = reset_idle_and_emit().await;
                Err(err)
            }
        }
    }
}
