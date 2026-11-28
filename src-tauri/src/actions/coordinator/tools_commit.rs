//! Unified `commit_exact_scope` tool handler and deterministic sequential commit execution.
//!
//! Serves both built-in C-Push (`CommitCoveragePolicy::ExactAllSelected`,
//! `SensitiveFileGuardPolicy::HardIntercept`, `RecoveryHandoffPolicy::AutoHandoff`,
//! `allow_early_settle = true`) and user Custom Actions (`CommitCoveragePolicy::SubsetOrEdited`,
//! `SensitiveFileGuardPolicy::None`, `RecoveryHandoffPolicy::AgentFirst`, supporting both single
//! commits and multi-group splits across pre-selected `fileIds` and SHA-256 verified `editedPaths`).

use super::{
    CustomActionCoordinator,
    context::{ActionToolContext, RecoveryHandlePayload, push_timeline_step},
    session_helpers::{
        build_sensitive_questionnaire, file_basename, resolve_frozen_file_token, summarize_commits,
        summarize_user_response,
    },
    tools_workspace::sha256_hex,
};
use crate::{
    actions::{
        profile::{CommitCoveragePolicy, SensitiveFileGuardPolicy},
        types::*,
    },
    error::{AppError, AppErrorCode},
    i18n,
    pi_runtime::PiToolExecutionResult,
    repository::{
        RepositoryStore,
        ai_context::FrozenCPushFile,
        read_tools::validate_safe_repo_path,
        remote::PushOutcome,
        types::{CommitRequest, CommitSelection, RepositorySnapshot},
    },
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{Arc, atomic::Ordering},
};

pub(super) async fn handle_commit_exact_scope(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
) -> PiToolExecutionResult {
    match ctx.profile.commit_coverage {
        CommitCoveragePolicy::ExactAllSelected => {
            handle_exact_all_selected_commit(ctx, call_id, args).await
        }
        CommitCoveragePolicy::SubsetOrEdited => handle_subset_or_edited_commit(ctx, args).await,
    }
}

async fn handle_exact_all_selected_commit(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
) -> PiToolExecutionResult {
    let locale = ctx.locale;
    let (frozen_files, mut qa_confirmed) = {
        let Ok(st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("Action run state lock is unavailable.");
        };
        if st.settled_reply.is_some() {
            return PiToolExecutionResult::error("Commits were already settled in this run.");
        }
        (st.frozen_files.clone(), st.qa_confirmed)
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
        let Some(matched) = resolve_frozen_file_token(token, &frozen_files) else {
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
            let Some(matched) = resolve_frozen_file_token(token, &frozen_files) else {
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

    // Hard Rust sensitive-file safety net (enabled only when profile requires HardIntercept):
    if ctx.profile.sensitive_file_guard == SensitiveFileGuardPolicy::HardIntercept && !qa_confirmed
    {
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
            let (question_id, stored) = {
                let meta = ctx.persist_meta();
                let Ok(mut st) = ctx.run_state.lock() else {
                    return PiToolExecutionResult::error("Action run state lock is unavailable.");
                };
                let q_id = format!("q-{}-{}", ctx.run_id, st.qa_log.len() + 1);
                let card = q_req.to_card(&q_title);
                st.status = ActionSessionStatus::NeedsDecision;
                st.pending_call_id = Some(sensitive_call_id.clone());
                st.can_retry_remaining = false;
                st.timeline = vec![
                    WorkflowTimelineItem {
                        state: "done".into(),
                        label: i18n::cpush::cpush_timeline_frozen_count(
                            locale,
                            st.frozen_files.len(),
                        ),
                    },
                    WorkflowTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush::cpush_timeline_waiting_decision(locale, &q_title),
                    },
                ];
                st.message = Some(WorkflowUiMessage {
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
                let stored = st.to_stored_session(&meta);
                let _ = ctx.coordinator.save_session_in_memory(&stored);
                (q_id, stored)
            };
            let waiter = match ctx.coordinator.register_pending_question(
                &ctx.run_id,
                &sensitive_call_id,
                &q_req,
                Arc::clone(&ctx.grants),
            ) {
                Ok(w) => w,
                Err(err) => return PiToolExecutionResult::error(err),
            };
            let _ = ctx
                .coordinator
                .persist_session(&ctx.storage_dir, &stored)
                .await;
            (ctx.raw_emit)(ActionProgressEvent {
                protocol_version: ACTION_PROTOCOL_VERSION,
                session: stored.to_public(),
                trusted_action_update: None,
                repository_snapshot: None,
            });

            match waiter.wait().await {
                Ok(resp) => {
                    if resp.cancelled {
                        if let Ok(mut st) = ctx.run_state.lock() {
                            st.user_cancelled = true;
                            st.pending_call_id = None;
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
                        st.pending_call_id = None;
                        st.qa_log.push(CPushQaItem {
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
        let meta = ctx.persist_meta();
        let Ok(mut st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("Action run state lock is unavailable.");
        };
        if requested_push == Some(false) {
            st.push_authorized = false;
        }
        st.pending_call_id = None;
        st.planned_groups = groups;
        st.skipped_file_ids = skipped_file_ids;
        st.next_step_index = 0;
        st.to_stored_session(&meta)
    };

    let raw_emit = Arc::clone(&ctx.raw_emit);
    let mut forward_progress = |event: ActionProgressEvent| {
        raw_emit(event);
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
                st.sync_from_stored(&session);
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

struct ParsedSubsetGroup {
    file_ids: Vec<String>,
    edited_paths: Vec<String>,
    message: String,
}

fn parse_subset_groups(args: &Value) -> Result<Vec<ParsedSubsetGroup>, String> {
    if let Some(groups_arr) = args.get("groups").and_then(Value::as_array) {
        if groups_arr.is_empty() {
            return Err(
                "commit_exact_scope `groups` must contain at least one commit group.".into(),
            );
        }
        let mut out = Vec::with_capacity(groups_arr.len());
        for (idx, item) in groups_arr.iter().enumerate() {
            let Some(obj) = item.as_object() else {
                return Err(format!(
                    "Invalid commit group at index {idx}: expected an object."
                ));
            };
            let file_ids: Vec<String> = obj
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
            let edited_paths: Vec<String> = obj
                .get("editedPaths")
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
            let message = obj
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if (file_ids.is_empty() && edited_paths.is_empty()) || message.is_empty() {
                return Err(format!(
                    "Commit group {} requires non-empty `message` and at least one entry in `fileIds` or `editedPaths`.",
                    idx + 1
                ));
            }
            out.push(ParsedSubsetGroup {
                file_ids,
                edited_paths,
                message,
            });
        }
        return Ok(out);
    }

    let file_ids: Vec<String> = args
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
    let edited_paths: Vec<String> = args
        .get("editedPaths")
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
    let message = args
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if (file_ids.is_empty() && edited_paths.is_empty()) || message.is_empty() {
        return Err(
            "commit_exact_scope requires non-empty `message` and at least one entry in `fileIds` or `editedPaths`."
                .into(),
        );
    }
    Ok(vec![ParsedSubsetGroup {
        file_ids,
        edited_paths,
        message,
    }])
}

async fn handle_subset_or_edited_commit(
    ctx: &ActionToolContext,
    args: &Value,
) -> PiToolExecutionResult {
    let groups = match parse_subset_groups(args) {
        Ok(g) => g,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    let should_push = args
        .get("push")
        .and_then(Value::as_bool)
        .unwrap_or(ctx.profile.commit_auto_push_default);

    let (frozen_files, mut already_committed, action_edited_hashes, mut committed_edited_paths) =
        ctx.run_state
            .lock()
            .map(|st| {
                (
                    st.frozen_files.clone(),
                    st.committed_file_ids.clone(),
                    st.action_edited_hashes.clone(),
                    st.committed_edited_paths.clone(),
                )
            })
            .unwrap_or_default();

    // Validate all groups upfront before writing any commit so partial invalid plans fail cleanly.
    let mut resolved_groups: Vec<(Vec<String>, Vec<String>, String)> =
        Vec::with_capacity(groups.len());
    let mut batch_seen_ids: HashSet<String> = HashSet::new();
    let mut batch_seen_edited: HashSet<String> = HashSet::new();

    for group in groups {
        let mut canonical_ids = Vec::with_capacity(group.file_ids.len());
        for raw_token in &group.file_ids {
            let canonical_id = resolve_frozen_file_token(raw_token, &frozen_files)
                .map(|f| f.file_id.clone())
                .unwrap_or_else(|| raw_token.clone());
            if already_committed.contains(&canonical_id) {
                return PiToolExecutionResult::error(format!(
                    "File `{canonical_id}` was already committed in this run."
                ));
            }
            if !batch_seen_ids.insert(canonical_id.clone()) {
                return PiToolExecutionResult::error(format!(
                    "File `{canonical_id}` appeared in multiple commit groups."
                ));
            }
            if !ctx
                .frozen_selections
                .iter()
                .any(|s| s.file_id == canonical_id)
            {
                return PiToolExecutionResult::error(format!(
                    "File `{raw_token}` is outside the user's selected scope (or is in Local Keep). Never widen past selected files."
                ));
            }
            canonical_ids.push(canonical_id);
        }

        let mut clean_edited = Vec::with_capacity(group.edited_paths.len());
        for raw_path in &group.edited_paths {
            let clean = match validate_safe_repo_path(raw_path) {
                Ok(p) => p,
                Err(err) => return PiToolExecutionResult::error(err),
            };
            if !batch_seen_edited.insert(clean.clone()) {
                return PiToolExecutionResult::error(format!(
                    "Edited path `{clean}` appeared in multiple commit groups."
                ));
            }
            if ctx.local_keep_paths.contains(&clean) {
                return PiToolExecutionResult::error(format!(
                    "Refused to commit `{clean}` because it is in Local Keep."
                ));
            }
            if committed_edited_paths.contains(&clean) {
                return PiToolExecutionResult::error(format!(
                    "Edited file `{clean}` was already committed in this run."
                ));
            }
            let Some(expected_hash) = action_edited_hashes.get(&clean) else {
                return PiToolExecutionResult::error(format!(
                    "File `{clean}` was not created or modified by this action (`edit_repo_file`, `write_repo_file`, or `run_repo_command`); cannot widen past authorized scope."
                ));
            };
            let live_bytes = match fs::read(ctx.root.join(&clean)) {
                Ok(bytes) => bytes,
                Err(err) => {
                    return PiToolExecutionResult::error(format!(
                        "Cannot read edited file `{clean}` before commit: {err}"
                    ));
                }
            };
            if &sha256_hex(&live_bytes) != expected_hash {
                return PiToolExecutionResult::error(format!(
                    "Action-modified file `{clean}` changed externally on disk; refusing to commit unverified changes."
                ));
            }
            clean_edited.push(clean);
        }

        resolved_groups.push((canonical_ids, clean_edited, group.message));
    }

    let mut landed_commits: Vec<CPushLandedCommit> = Vec::with_capacity(resolved_groups.len());
    let mut latest_snap: Option<RepositorySnapshot> = None;

    for (group_idx, (group_file_ids, group_edited_paths, message)) in
        resolved_groups.into_iter().enumerate()
    {
        let needs_live_refresh = group_idx > 0
            || !already_committed.is_empty()
            || !group_edited_paths.is_empty()
            || group_file_ids.iter().any(|fid| {
                ctx.live_changes
                    .iter()
                    .find(|c| c.id == *fid)
                    .is_some_and(|c| action_edited_hashes.contains_key(&c.path))
            });

        let live_tree_files = if needs_live_refresh {
            match ctx.store.refresh_with_locale(&ctx.target, ctx.locale).await {
                Ok(snap) => match snap
                    .worktrees
                    .into_iter()
                    .find(|wt| wt.worktree_id == ctx.target.worktree_id)
                {
                    Some(tree) => Some(tree.files),
                    None => {
                        return PiToolExecutionResult::error(
                            "Target worktree not found in refreshed repository snapshot.",
                        );
                    }
                },
                Err(err) => return PiToolExecutionResult::error(err),
            }
        } else {
            None
        };

        let mut matched_selections: Vec<CommitSelection> = Vec::new();
        let mut seen_selection_ids: HashSet<String> = HashSet::new();
        let mut committed_ids_to_record: Vec<String> = Vec::new();
        let mut committed_paths_to_record: Vec<String> = Vec::new();

        for fid in &group_file_ids {
            let Some(sel) = ctx.frozen_selections.iter().find(|s| s.file_id == *fid) else {
                return PiToolExecutionResult::error(format!(
                    "File `{fid}` is outside the user's selected scope."
                ));
            };
            let edited_path = ctx
                .live_changes
                .iter()
                .find(|c| c.id == *fid)
                .and_then(|c| {
                    action_edited_hashes
                        .get(&c.path)
                        .map(|h| (c.path.as_str(), h))
                });
            let resolved_sel = if let Some((path, expected_hash)) = edited_path {
                let live_bytes = match fs::read(ctx.root.join(path)) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return PiToolExecutionResult::error(format!(
                            "Cannot read edited file `{path}` before commit: {err}"
                        ));
                    }
                };
                if &sha256_hex(&live_bytes) != expected_hash {
                    return PiToolExecutionResult::error(format!(
                        "Action-modified file `{path}` changed externally on disk; refusing to commit unverified changes."
                    ));
                }
                let Some(live_change) = live_tree_files
                    .as_ref()
                    .and_then(|files| files.iter().find(|c| c.path == path))
                else {
                    return PiToolExecutionResult::error(format!(
                        "Edited file `{path}` has no uncommitted changes to commit."
                    ));
                };
                if live_change.range.kind == "partial" {
                    return PiToolExecutionResult::error(format!(
                        "Edited file `{path}` is partially staged; refusing to commit ambiguous index state."
                    ));
                }
                committed_paths_to_record.push(path.to_owned());
                CommitSelection {
                    file_id: live_change.id.clone(),
                    commit_token: live_change.commit_token.clone(),
                }
            } else if let Some(ref live_files) = live_tree_files {
                // Re-resolve fresh commitToken for subsequent groups after HEAD moved on group 1+,
                // verifying that content, staging range, and Local Keep state have not drifted.
                let Some(orig_change) = ctx.live_changes.iter().find(|c| c.id == *fid) else {
                    return PiToolExecutionResult::error(format!(
                        "File `{fid}` metadata missing from initial snapshot."
                    ));
                };
                let Some(live_change) = live_files.iter().find(|c| c.id == *fid) else {
                    return PiToolExecutionResult::error(format!(
                        "Selected file `{}` is no longer present in the working tree.",
                        orig_change.path
                    ));
                };
                if live_change.content_version != orig_change.content_version
                    || live_change.range.kind != orig_change.range.kind
                    || live_change.local_keep != orig_change.local_keep
                {
                    return PiToolExecutionResult::error(format!(
                        "Selected file `{}` changed externally since this action started; refusing to commit stale scope.",
                        orig_change.path
                    ));
                }
                CommitSelection {
                    file_id: live_change.id.clone(),
                    commit_token: live_change.commit_token.clone(),
                }
            } else {
                sel.clone()
            };
            if seen_selection_ids.insert(resolved_sel.file_id.clone()) {
                committed_ids_to_record.push(fid.clone());
                matched_selections.push(resolved_sel);
            }
        }

        for path in &group_edited_paths {
            let Some(live_change) = live_tree_files
                .as_ref()
                .and_then(|files| files.iter().find(|c| c.path == *path))
            else {
                return PiToolExecutionResult::error(format!(
                    "Edited file `{path}` has no uncommitted changes to commit."
                ));
            };
            if live_change.range.kind == "partial" {
                return PiToolExecutionResult::error(format!(
                    "Edited file `{path}` is partially staged; refusing to commit ambiguous index state."
                ));
            }
            committed_paths_to_record.push(path.clone());
            if seen_selection_ids.insert(live_change.id.clone()) {
                committed_ids_to_record.push(live_change.id.clone());
                matched_selections.push(CommitSelection {
                    file_id: live_change.id.clone(),
                    commit_token: live_change.commit_token.clone(),
                });
            }
        }

        let step_n = ctx.coordinator.inner.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let commit_req = CommitRequest {
            request_id: format!("act-commit-{step_n}"),
            target: ctx.target.clone(),
            selections: matched_selections,
            message,
            push: false,
            locale: ctx.locale,
        };
        match ctx.store.commit(&commit_req).await {
            Ok((snap, landed)) => {
                latest_snap = Some(snap);
                let landed_entry = CPushLandedCommit {
                    oid: landed.oid.clone(),
                    short_oid: landed.short_oid.clone(),
                    title: landed.title.clone(),
                    branch: landed.branch.clone(),
                    file_ids: committed_ids_to_record.clone(),
                };
                for fid in &committed_ids_to_record {
                    already_committed.insert(fid.clone());
                }
                for path in &committed_paths_to_record {
                    committed_edited_paths.insert(path.clone());
                }
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.wrote_repo = true;
                    for fid in &committed_ids_to_record {
                        st.record_committed_file_id(fid.clone());
                    }
                    for path in &committed_paths_to_record {
                        st.committed_edited_paths.insert(path.clone());
                    }
                    st.commits.push(landed_entry.clone());
                    push_timeline_step(
                        &mut st.timeline,
                        "done",
                        format!("{} {}", landed.short_oid, landed.title),
                    );
                }
                landed_commits.push(landed_entry);
            }
            Err(err) => {
                if let Some(snap) = latest_snap {
                    (ctx.emit)("running", None, None, Some(snap));
                    ctx.persist("running", String::new()).await;
                }
                return PiToolExecutionResult::error(err.message);
            }
        }
    }

    let last_commit = landed_commits.last().cloned();
    let commits_json: Vec<Value> = landed_commits
        .iter()
        .map(|c| {
            json!({
                "oid": c.oid,
                "shortOid": c.short_oid,
                "title": c.title,
                "branch": c.branch,
            })
        })
        .collect();
    let mut response_json = json!({
        "committed": true,
        "commitCount": landed_commits.len(),
        "commits": commits_json,
    });
    if let Some(last) = last_commit {
        response_json["oid"] = json!(last.oid);
        response_json["shortOid"] = json!(last.short_oid);
        response_json["title"] = json!(last.title);
        response_json["branch"] = json!(last.branch);
    }

    if should_push {
        match ctx.store.push_branch(&ctx.target, None, ctx.locale).await {
            Ok(PushOutcome::Pushed {
                remote_branch,
                snapshot,
            }) => {
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.recovery_handles.clear();
                }
                latest_snap = Some(snapshot);
                response_json["pushed"] = json!(true);
                response_json["remoteBranch"] = json!(remote_branch);
            }
            Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                latest_snap = Some(snapshot);
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::MissingUpstream {
                    remotes: remotes.clone(),
                });
                response_json["pushed"] = json!(false);
                response_json["pushNote"] =
                    json!(format!("No upstream configured. Remotes: {remotes:?}"));
                response_json["recoveryHandle"] = handle;
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                reason,
                non_fast_forward,
                snapshot,
            }) => {
                latest_snap = Some(snapshot);
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::PushFailed {
                    remote_target: remote_branch,
                    reason: reason.clone(),
                    title_override: None,
                });
                response_json["pushed"] = json!(false);
                response_json["nonFastForward"] = json!(non_fast_forward);
                response_json["pushError"] = json!(reason);
                response_json["recoveryHandle"] = handle;
            }
            Err(err) => {
                let branch = ctx
                    .run_state
                    .lock()
                    .map(|st| st.branch.clone())
                    .unwrap_or_else(|_| "HEAD".into());
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::PushFailed {
                    remote_target: branch,
                    reason: err.clone(),
                    title_override: None,
                });
                response_json["pushed"] = json!(false);
                response_json["pushError"] = json!(err);
                response_json["recoveryHandle"] = handle;
            }
        }
    }

    (ctx.emit)("running", None, None, latest_snap);
    ctx.persist("running", String::new()).await;
    PiToolExecutionResult::success(response_json.to_string())
}

impl CustomActionCoordinator {
    /// Deterministic sequential execution of planned commit groups (used by C-Push fast path and
    /// `RetryRemainingCommits` host recovery).
    pub(super) async fn execute_planned_groups(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredActionSession,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, String> {
        let locale = session.locale;
        let total = session.planned_groups.len();
        if total == 0 {
            let read_target = session.read_target();
            if session.push_authorized
                && let Ok((live_remote, snap)) = repo_store
                    .inspect_live_remote_status(&read_target, false, locale)
                    .await
                && live_remote.ahead > 0
            {
                return self
                    .sync_and_push_session(
                        storage_dir,
                        repo_store,
                        request_id,
                        session,
                        None,
                        Some(snap),
                        on_progress,
                    )
                    .await;
            }
            session.status = ActionSessionStatus::Done;
            session.pending_call_id = None;
            session.can_retry_remaining = false;
            session.timeline = vec![WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_all_skipped(locale).into(),
            }];
            let title: String = i18n::cpush::cpush_all_skipped_title(locale).into();
            session.summary = title.clone();
            session.message = Some(WorkflowUiMessage {
                id: format!("done-skipped-{}", session.run_id),
                kind: "done".into(),
                title,
                description: i18n::cpush::cpush_all_skipped_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: None,
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: None,
            });
            self.save_session_in_memory(session)?;
            self.persist_session(storage_dir, session).await?;
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

        let by_id: HashMap<String, FrozenCPushFile> = session
            .frozen_files
            .iter()
            .map(|f| (f.file_id.clone(), f.clone()))
            .collect();
        let read_target = session.read_target();
        let mut latest_snapshot: Option<RepositorySnapshot> = None;

        while session.next_step_index < total {
            let step_idx = session.next_step_index;
            let group = session.planned_groups[step_idx].clone();
            let step_title = group
                .message
                .lines()
                .next()
                .unwrap_or_else(|| i18n::cpush::cpush_commit_changes_fallback(locale))
                .to_owned();

            session.status = ActionSessionStatus::Running;
            session.pending_call_id = None;
            session.can_retry_remaining = false;
            let mut timeline: Vec<WorkflowTimelineItem> = session
                .commits
                .iter()
                .enumerate()
                .map(|(i, c)| WorkflowTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush::cpush_timeline_step_done(
                        locale,
                        i + 1,
                        total,
                        &c.short_oid,
                        &c.title,
                    ),
                })
                .collect();
            timeline.push(WorkflowTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_step_writing(
                    locale,
                    step_idx + 1,
                    total,
                    &step_title,
                ),
            });
            session.timeline = timeline;
            session.message = Some(WorkflowUiMessage {
                id: format!("progress-{}-step-{}", session.run_id, step_idx + 1),
                kind: "progress".into(),
                title: i18n::cpush::cpush_progress_step_title(locale, step_idx + 1, total),
                description: step_title.clone(),
                scope: Some(i18n::cpush::cpush_progress_step_scope(
                    locale,
                    &session.branch,
                    group.file_ids.len(),
                )),
                files: None,
                commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: Some(WorkflowMessageProgress {
                    completed: (step_idx + 1) as u32,
                    total: (total + 2) as u32,
                    label: i18n::cpush::cpush_progress_step_label(locale, step_idx + 1, total),
                }),
            });
            self.save_session_in_memory(session)?;
            let _ = on_progress(ActionProgressEvent {
                protocol_version: ACTION_PROTOCOL_VERSION,
                session: session.to_public(),
                trusted_action_update: None,
                repository_snapshot: latest_snapshot.clone(),
            });

            let expected_head = if step_idx == 0 {
                session.initial_head.clone()
            } else {
                Some(session.commits[step_idx - 1].oid.clone())
            };
            let step_frozen_files: Vec<FrozenCPushFile> = group
                .file_ids
                .iter()
                .filter_map(|id| by_id.get(id).cloned())
                .collect();

            let step_outcome = async {
                let (_, selections) = repo_store
                    .resolve_cpush_step_selections(
                        &read_target,
                        &session.branch,
                        expected_head.as_deref(),
                        &step_frozen_files,
                        locale,
                    )
                    .await
                    .map_err(|msg| AppError::git(AppErrorCode::CommitScopeChanged, msg))?;
                let step_req = CommitRequest {
                    request_id: format!("{request_id}-s{}", step_idx + 1),
                    target: read_target.clone(),
                    selections,
                    message: group.message.clone(),
                    push: false,
                    locale,
                };
                repo_store.commit(&step_req).await
            }
            .await;

            match step_outcome {
                Ok((snapshot, commit_info)) => {
                    latest_snapshot = Some(snapshot);
                    session.commits.push(CPushLandedCommit {
                        oid: commit_info.oid,
                        short_oid: commit_info.short_oid,
                        title: commit_info.title,
                        branch: commit_info.branch,
                        file_ids: group.file_ids.clone(),
                    });
                    session.committed_file_ids.extend(group.file_ids.clone());
                    session.next_step_index = step_idx + 1;
                    self.save_session_in_memory(session)?;
                    self.persist_session(storage_dir, session).await?;
                }
                Err(err) => {
                    if latest_snapshot.is_none()
                        && let Ok(refreshed) =
                            repo_store.refresh_with_locale(&read_target, locale).await
                    {
                        latest_snapshot = Some(refreshed);
                    }
                    let failed_chips: Vec<WorkflowMessageFileChip> = step_frozen_files
                        .iter()
                        .map(|f| WorkflowMessageFileChip {
                            id: f.file_id.clone(),
                            label: file_basename(&f.path),
                            badge: (f.range_kind == "partial")
                                .then(|| i18n::cpush::cpush_partial_badge(locale).into()),
                        })
                        .collect();
                    let commit_summaries = summarize_commits(&session.commits);
                    let mut failed_timeline: Vec<WorkflowTimelineItem> = session
                        .commits
                        .iter()
                        .enumerate()
                        .map(|(i, c)| WorkflowTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_step_done(
                                locale,
                                i + 1,
                                total,
                                &c.short_oid,
                                &c.title,
                            ),
                        })
                        .collect();
                    failed_timeline.push(WorkflowTimelineItem {
                        state: "failed".into(),
                        label: i18n::cpush::cpush_timeline_step_paused(
                            locale,
                            step_idx + 1,
                            total,
                            &step_title,
                        ),
                    });

                    let can_retry = !err.is_unsupported_commit_guard();
                    let err_message = err.message;
                    let options = if can_retry {
                        vec![
                            WorkflowMessageOption {
                                id: "retry-remaining".into(),
                                label: i18n::cpush::cpush_option_retry_remaining(locale).into(),
                                recommended: Some(true),
                            },
                            WorkflowMessageOption {
                                id: "dismiss-session".into(),
                                label: i18n::cpush::cpush_option_close_or_keep_completed(
                                    locale,
                                    step_idx > 0,
                                )
                                .into(),
                                recommended: None,
                            },
                        ]
                    } else {
                        vec![WorkflowMessageOption {
                            id: "dismiss-session".into(),
                            label: i18n::cpush::cpush_option_close_or_keep_completed(
                                locale,
                                step_idx > 0,
                            )
                            .into(),
                            recommended: Some(true),
                        }]
                    };
                    let description = if can_retry {
                        i18n::cpush::cpush_step_paused_desc(locale, &err_message, step_idx > 0)
                    } else {
                        i18n::cpush::cpush_step_blocked_desc(locale, &err_message, step_idx > 0)
                    };
                    let title = i18n::cpush::cpush_step_paused_title(locale, step_idx, total);

                    session.status = ActionSessionStatus::NeedsDecision;
                    session.pending_call_id = None;
                    session.can_retry_remaining = can_retry;
                    session.timeline = failed_timeline;
                    session.summary = title.clone();
                    session.message = Some(WorkflowUiMessage {
                        id: format!("err-{}-step-{}", session.run_id, step_idx + 1),
                        kind: "error".into(),
                        title,
                        description,
                        scope: Some(i18n::cpush::cpush_step_scope(locale, step_idx + 1, total)),
                        files: (!failed_chips.is_empty()).then_some(failed_chips),
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(options),
                        links: None,
                        card: None,
                        questionnaire: None,
                        progress: None,
                    });
                    self.save_session_in_memory(session)?;
                    self.persist_session(storage_dir, session).await?;
                    let public = session.to_public();
                    let _ = on_progress(ActionProgressEvent {
                        protocol_version: ACTION_PROTOCOL_VERSION,
                        session: public.clone(),
                        trusted_action_update: None,
                        repository_snapshot: latest_snapshot.clone(),
                    });
                    return Ok(ActionCommandReply {
                        protocol_version: ACTION_PROTOCOL_VERSION,
                        kind: "action",
                        request_id: request_id.to_owned(),
                        session: public,
                        trusted_action_update: None,
                        repository_snapshot: latest_snapshot,
                    });
                }
            }
        }

        if !session.push_authorized {
            session.status = ActionSessionStatus::Done;
            session.pending_call_id = None;
            session.can_retry_remaining = false;
            let commit_summaries = summarize_commits(&session.commits);
            let mut done_timeline: Vec<WorkflowTimelineItem> = session
                .commits
                .iter()
                .enumerate()
                .map(|(i, c)| WorkflowTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush::cpush_timeline_step_done(
                        locale,
                        i + 1,
                        total,
                        &c.short_oid,
                        &c.title,
                    ),
                })
                .collect();
            done_timeline.push(WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_local_only_done(locale).into(),
            });
            session.timeline = done_timeline;
            let title = i18n::cpush::cpush_local_only_done_title(locale, session.commits.len());
            session.summary = title.clone();
            session.message = Some(WorkflowUiMessage {
                id: format!("done-commit-only-{}", session.run_id),
                kind: "done".into(),
                title,
                description: i18n::cpush::cpush_local_only_done_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: Some(commit_summaries),
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: None,
            });
            self.save_session_in_memory(session)?;
            self.persist_session(storage_dir, session).await?;
            let public = session.to_public();
            let _ = on_progress(ActionProgressEvent {
                protocol_version: ACTION_PROTOCOL_VERSION,
                session: public.clone(),
                trusted_action_update: None,
                repository_snapshot: latest_snapshot.clone(),
            });
            return Ok(ActionCommandReply {
                protocol_version: ACTION_PROTOCOL_VERSION,
                kind: "action",
                request_id: request_id.to_owned(),
                session: public,
                trusted_action_update: None,
                repository_snapshot: latest_snapshot,
            });
        }

        self.sync_and_push_session(
            storage_dir,
            repo_store,
            request_id,
            session,
            None,
            latest_snapshot,
            on_progress,
        )
        .await
    }
}
