//! Exact-scope local commit and remote sync/push tool handlers for Custom Actions.

use super::{
    context::{ActionToolContext, push_timeline_step},
    tools_workspace::sha256_hex,
};
use crate::{
    actions::types::CPushLandedCommit,
    i18n,
    pi_runtime::PiToolExecutionResult,
    repository::{
        read_tools::validate_safe_repo_path,
        remote::{PushOutcome, RebaseSyncOutcome},
        types::{CommitRequest, CommitSelection},
    },
};
use serde_json::{Value, json};
use std::{collections::HashSet, fs, sync::atomic::Ordering};

pub(super) async fn handle_commit_exact_scope(
    ctx: &ActionToolContext,
    args: &Value,
) -> PiToolExecutionResult {
    let requested_ids: Vec<String> = args
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
    let requested_edited_paths: Vec<String> = args
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
    let should_push = args.get("push").and_then(Value::as_bool).unwrap_or(false);
    if (requested_ids.is_empty() && requested_edited_paths.is_empty()) || message.is_empty() {
        return PiToolExecutionResult::error(
            "commit_exact_scope requires non-empty `message` and at least one entry in `fileIds` or `editedPaths`.",
        );
    }

    let (already_committed, action_edited_hashes, committed_edited_paths) = ctx
        .run_state
        .lock()
        .map(|st| {
            (
                st.committed_file_ids.clone(),
                st.action_edited_hashes.clone(),
                st.committed_edited_paths.clone(),
            )
        })
        .unwrap_or_default();

    let mut validated_edited_paths: Vec<String> = Vec::with_capacity(requested_edited_paths.len());
    let mut seen_edited: HashSet<String> = HashSet::new();
    for raw_path in &requested_edited_paths {
        let clean = match validate_safe_repo_path(raw_path) {
            Ok(p) => p,
            Err(err) => return PiToolExecutionResult::error(err),
        };
        if !seen_edited.insert(clean.clone()) {
            continue;
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
        validated_edited_paths.push(clean);
    }

    let needs_live_refresh = !validated_edited_paths.is_empty()
        || requested_ids.iter().any(|fid| {
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

    for fid in &requested_ids {
        if already_committed.contains(fid) {
            return PiToolExecutionResult::error(format!(
                "File `{fid}` was already committed in this run."
            ));
        }
        let Some(sel) = ctx.frozen_selections.iter().find(|s| s.file_id == *fid) else {
            return PiToolExecutionResult::error(format!(
                "File `{fid}` is outside the user's selected scope (or is in Local Keep). Never widen past selected files."
            ));
        };
        // If this pre-selected file was also edited via `edit_repo_file`, resolve its fresh token
        // after confirming its disk bytes still match what `edit_repo_file` wrote.
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
            committed_paths_to_record.push(path.to_owned());
            CommitSelection {
                file_id: live_change.id.clone(),
                commit_token: live_change.commit_token.clone(),
            }
        } else {
            sel.clone()
        };
        if seen_selection_ids.insert(resolved_sel.file_id.clone()) {
            committed_ids_to_record.push(resolved_sel.file_id.clone());
            matched_selections.push(resolved_sel);
        }
    }

    for path in &validated_edited_paths {
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
            if let Ok(mut st) = ctx.run_state.lock() {
                st.wrote_repo = true;
                for fid in &committed_ids_to_record {
                    st.committed_file_ids.insert(fid.clone());
                }
                for path in &committed_paths_to_record {
                    st.committed_edited_paths.insert(path.clone());
                }
                st.commits.push(CPushLandedCommit {
                    oid: landed.oid.clone(),
                    short_oid: landed.short_oid.clone(),
                    title: landed.title.clone(),
                    branch: landed.branch.clone(),
                    file_ids: committed_ids_to_record.clone(),
                });
                push_timeline_step(
                    &mut st.timeline,
                    "done",
                    format!("{} {}", landed.short_oid, landed.title),
                );
            }
            let mut response_json = json!({
                "committed": true,
                "oid": landed.oid,
                "shortOid": landed.short_oid,
                "title": landed.title,
                "branch": landed.branch,
            });
            let mut latest_snap = snap;
            if should_push {
                match ctx.store.push_branch(&ctx.target, None, ctx.locale).await {
                    Ok(PushOutcome::Pushed {
                        remote_branch,
                        snapshot,
                    }) => {
                        latest_snap = snapshot;
                        response_json["pushed"] = json!(true);
                        response_json["remoteBranch"] = json!(remote_branch);
                    }
                    Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                        latest_snap = snapshot;
                        response_json["pushed"] = json!(false);
                        response_json["pushNote"] =
                            json!(format!("No upstream configured. Remotes: {remotes:?}"));
                    }
                    Ok(PushOutcome::Failed {
                        reason, snapshot, ..
                    }) => {
                        latest_snap = snapshot;
                        response_json["pushed"] = json!(false);
                        response_json["pushError"] = json!(reason);
                    }
                    Err(err) => {
                        response_json["pushed"] = json!(false);
                        response_json["pushError"] = json!(err);
                    }
                }
            }
            (ctx.emit)("running", None, None, Some(latest_snap));
            ctx.persist("running", String::new()).await;
            PiToolExecutionResult::success(response_json.to_string())
        }
        Err(err) => PiToolExecutionResult::error(err.message),
    }
}

pub(super) async fn handle_sync_and_push(
    ctx: &ActionToolContext,
    args: &Value,
) -> PiToolExecutionResult {
    let mode = args.get("mode").and_then(Value::as_str).unwrap_or("push");
    let upstream_remote = args
        .get("setUpstreamRemote")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    match mode {
        "pullRebase" => match ctx
            .store
            .sync_rebase_onto_upstream_with_locale(&ctx.target, ctx.locale)
            .await
        {
            Ok(RebaseSyncOutcome::AlreadyUpToDate { snapshot }) => {
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::success(json!({ "status": "alreadyUpToDate" }).to_string())
            }
            Ok(RebaseSyncOutcome::Completed {
                rebased_commits,
                snapshot,
            }) => {
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.wrote_repo = true;
                }
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::success(
                    json!({
                        "status": "rebased",
                        "rebasedCount": rebased_commits.len()
                    })
                    .to_string(),
                )
            }
            Ok(RebaseSyncOutcome::Conflict {
                upstream,
                conflicted_files,
                snapshot,
            }) => {
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(format!(
                    "Rebase onto {upstream} hit conflicts in {} file(s); resolve conflicts before continuing.",
                    conflicted_files.len()
                ))
            }
            Ok(RebaseSyncOutcome::MissingUpstream { remotes, .. }) => {
                PiToolExecutionResult::error(format!(
                    "Current branch has no upstream tracking branch (available remotes: {remotes:?})."
                ))
            }
            Ok(RebaseSyncOutcome::UnsafeDirtyOverlap {
                overlapping_paths, ..
            }) => PiToolExecutionResult::error(format!(
                "Uncommitted local changes overlap upstream commits: {overlapping_paths:?}."
            )),
            Ok(RebaseSyncOutcome::UnsafePublishedHistory { reason, .. }) => {
                PiToolExecutionResult::error(reason)
            }
            Err(err) => PiToolExecutionResult::error(err),
        },
        _ => match ctx
            .store
            .push_branch(&ctx.target, upstream_remote, ctx.locale)
            .await
        {
            Ok(PushOutcome::Pushed {
                remote_branch,
                snapshot,
            }) => {
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.wrote_repo = true;
                    push_timeline_step(
                        &mut st.timeline,
                        "done",
                        i18n::actions::action_pushed_remote_label(ctx.locale, &remote_branch),
                    );
                }
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::success(
                    json!({
                        "pushed": true,
                        "remoteBranch": remote_branch
                    })
                    .to_string(),
                )
            }
            Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(format!(
                    "Branch has no upstream. Pass setUpstreamRemote (available: {remotes:?})."
                ))
            }
            Ok(PushOutcome::Failed {
                reason,
                non_fast_forward,
                snapshot,
                ..
            }) => {
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(format!(
                    "Push failed (nonFastForward={non_fast_forward}): {reason}"
                ))
            }
            Err(err) => PiToolExecutionResult::error(err),
        },
    }
}
