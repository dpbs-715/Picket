//! Exact-scope local commit and remote sync/push tool handlers for Custom Actions.

use super::context::{ActionToolContext, push_timeline_step};
use crate::{
    cpush::types::CPushLandedCommit,
    foundation::pi::PiToolExecutionResult,
    i18n,
    repository::{
        remote::{PushOutcome, RebaseSyncOutcome},
        types::{CommitRequest, CommitSelection},
    },
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

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
    if requested_ids.is_empty() || message.is_empty() {
        return PiToolExecutionResult::error(
            "commit_exact_scope requires non-empty `fileIds` and `message`.",
        );
    }
    let already_committed = ctx
        .run_state
        .lock()
        .map(|st| st.committed_file_ids.clone())
        .unwrap_or_default();
    let mut matched_selections: Vec<CommitSelection> = Vec::new();
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
        matched_selections.push(sel.clone());
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
                for fid in &requested_ids {
                    st.committed_file_ids.insert(fid.clone());
                }
                st.commits.push(CPushLandedCommit {
                    oid: landed.oid.clone(),
                    short_oid: landed.short_oid.clone(),
                    title: landed.title.clone(),
                    branch: landed.branch.clone(),
                    file_ids: requested_ids.clone(),
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
            (ctx.persist)("running", String::new());
            PiToolExecutionResult::success(response_json.to_string())
        }
        Err(err) => PiToolExecutionResult::error(err),
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
