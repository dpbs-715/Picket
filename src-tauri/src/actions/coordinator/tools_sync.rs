//! Remote sync/push (`sync_and_push`), deterministic host sync+push recovery, and verified
//! `handoff_to_recovery` tool handler for `AgentFirst` custom actions.

use super::{
    CustomActionCoordinator,
    context::{ActionToolContext, RecoveryHandlePayload, push_timeline_step},
    session_helpers::{
        apply_rebased_commits, enter_conflict_state, enter_missing_upstream_state,
        enter_push_failed_state, mark_pushed_done, summarize_commits,
    },
};
use crate::{
    actions::types::*,
    i18n,
    pi_runtime::PiToolExecutionResult,
    repository::{
        RepositoryStore,
        remote::{PushOutcome, RebaseSyncOutcome},
        types::RepositorySnapshot,
    },
};
use serde_json::{Value, json};
use std::path::Path;

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
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.recovery_handles.clear();
                }
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::success(json!({ "status": "alreadyUpToDate" }).to_string())
            }
            Ok(RebaseSyncOutcome::Completed {
                rebased_commits,
                snapshot,
            }) => {
                if let Ok(mut st) = ctx.run_state.lock() {
                    st.wrote_repo = true;
                    st.recovery_handles.clear();
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
                let conflict_paths: Vec<String> =
                    conflicted_files.iter().map(|f| f.path.clone()).collect();
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::Conflict {
                    upstream: upstream.clone(),
                    conflicted_files,
                });
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(
                    json!({
                        "error": format!(
                            "Rebase onto {upstream} hit conflicts in {} file(s): {:?}.",
                            conflict_paths.len(),
                            conflict_paths
                        ),
                        "upstream": upstream,
                        "conflictedFiles": conflict_paths,
                        "recoveryHandle": handle,
                    })
                    .to_string(),
                )
            }
            Ok(RebaseSyncOutcome::MissingUpstream { remotes, snapshot }) => {
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::MissingUpstream {
                    remotes: remotes.clone(),
                });
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(
                    json!({
                        "error": format!(
                            "Current branch has no upstream tracking branch (available remotes: {remotes:?})."
                        ),
                        "remotes": remotes,
                        "recoveryHandle": handle,
                    })
                    .to_string(),
                )
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
                    st.recovery_handles.clear();
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
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::MissingUpstream {
                    remotes: remotes.clone(),
                });
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(
                    json!({
                        "error": format!(
                            "Branch has no upstream. Pass setUpstreamRemote (available: {remotes:?}) or hand off via handoff_to_recovery."
                        ),
                        "remotes": remotes,
                        "recoveryHandle": handle,
                    })
                    .to_string(),
                )
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                reason,
                non_fast_forward,
                snapshot,
            }) => {
                let handle = ctx.register_recovery_handle(RecoveryHandlePayload::PushFailed {
                    remote_target: remote_branch.clone(),
                    reason: reason.clone(),
                    title_override: None,
                });
                (ctx.emit)("running", None, None, Some(snapshot));
                PiToolExecutionResult::error(
                    json!({
                        "error": format!("Push failed (nonFastForward={non_fast_forward}): {reason}"),
                        "remoteBranch": remote_branch,
                        "nonFastForward": non_fast_forward,
                        "recoveryHandle": handle,
                    })
                    .to_string(),
                )
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
                PiToolExecutionResult::error(
                    json!({
                        "error": err,
                        "recoveryHandle": handle,
                    })
                    .to_string(),
                )
            }
        },
    }
}

/// Validates a host-issued `recoveryHandle.handleId` against real runtime state, transitions the
/// unified session into Host Recovery (`PushFailed`, `Conflict`, or `NeedsDecision` for missing
/// upstream), persists it, emits a progress event, and terminates the Agent loop cleanly.
pub(super) async fn handle_handoff_to_recovery(
    ctx: &ActionToolContext,
    args: &Value,
) -> PiToolExecutionResult {
    let handle_id = args
        .get("handleId")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if handle_id.is_empty() {
        return PiToolExecutionResult::error(
            "handoff_to_recovery requires a non-empty `handleId` from a previous tool's `recoveryHandle`.",
        );
    }
    let summary_hint = args
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let payload = {
        let Ok(st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("Action run state lock is unavailable.");
        };
        if st.settled_reply.is_some() {
            return PiToolExecutionResult::error("Action is already settled.");
        }
        st.recovery_handles.get(handle_id).cloned()
    };

    let Some(payload) = payload else {
        return PiToolExecutionResult::error(format!(
            "Invalid or expired recovery handle `{handle_id}`. Only pass a genuine `recoveryHandle.handleId` returned by `commit_exact_scope` or `sync_and_push` in this run."
        ));
    };

    // Always verify live Git state before handing off to Host Recovery (fail closed on error)
    // so stale or already-resolved recovery handles can never put the UI into a false state.
    let (live_status, live_snap) = match ctx
        .store
        .inspect_live_remote_status(&ctx.target, false, ctx.locale)
        .await
    {
        Ok(res) => res,
        Err(err) => {
            return PiToolExecutionResult::error(format!(
                "Cannot hand off to host recovery because live Git verification failed: {err}"
            ));
        }
    };

    match &payload {
        RecoveryHandlePayload::Conflict { .. } => {
            if !live_status.rebase_in_progress && live_status.conflicted_files.is_empty() {
                return PiToolExecutionResult::error(
                    "Cannot hand off conflict recovery: live Git no longer has an active conflict or rebase.",
                );
            }
        }
        RecoveryHandlePayload::MissingUpstream { .. } => {
            if live_status.upstream.is_some() {
                return PiToolExecutionResult::error(
                    "Cannot hand off missing-upstream recovery: the current branch now has an upstream configured.",
                );
            }
        }
        RecoveryHandlePayload::PushFailed { .. } => {
            if live_status.upstream.is_some() && live_status.ahead == 0 {
                return PiToolExecutionResult::error(
                    "Cannot hand off push-failed recovery: the current branch has no unpushed commits ahead of upstream.",
                );
            }
        }
    }

    let (mut session, trusted_update, links) = {
        let meta = ctx.persist_meta();
        let Ok(st) = ctx.run_state.lock() else {
            return PiToolExecutionResult::error("Action run state lock is unavailable.");
        };
        (
            st.to_stored_session(&meta),
            st.trusted_action_update.clone(),
            st.links.clone(),
        )
    };
    if let Some(live_wt) = live_snap
        .worktrees
        .iter()
        .find(|w| w.worktree_id == ctx.target.worktree_id)
    {
        session.branch = live_wt.branch.clone();
    }

    let snapshot = Some(live_snap);
    match payload {
        RecoveryHandlePayload::PushFailed {
            remote_target,
            reason,
            title_override,
            ..
        } => {
            session.push_authorized = true;
            enter_push_failed_state(
                &mut session,
                &remote_target,
                &reason,
                title_override.as_deref(),
            );
        }
        RecoveryHandlePayload::MissingUpstream { remotes, .. } => {
            session.push_authorized = true;
            enter_missing_upstream_state(&mut session, &remotes);
        }
        RecoveryHandlePayload::Conflict {
            upstream,
            conflicted_files,
            ..
        } => {
            let live_files = if !live_status.conflicted_files.is_empty() {
                live_status.conflicted_files
            } else {
                conflicted_files
            };
            let has_model = !session.model_id.is_empty();
            enter_conflict_state(
                &mut session,
                &upstream,
                &live_files,
                has_model,
                summary_hint.as_deref(),
            );
        }
    };

    if let Err(err) = ctx.coordinator.save_session_in_memory(&session) {
        return PiToolExecutionResult::error(err);
    }
    let _ = ctx
        .coordinator
        .persist_session(&ctx.storage_dir, &session)
        .await;
    let public = session.to_public();
    (ctx.raw_emit)(ActionProgressEvent {
        protocol_version: ACTION_PROTOCOL_VERSION,
        session: public.clone(),
        trusted_action_update: trusted_update.clone(),
        repository_snapshot: snapshot.clone(),
    });

    let final_summary = summary_hint
        .or_else(|| public.message.as_ref().map(|m| m.title.clone()))
        .unwrap_or_else(|| "Handed off to host recovery.".into());
    let reply = ActionCommandReply {
        protocol_version: ACTION_PROTOCOL_VERSION,
        kind: "action",
        request_id: ctx.request_id.clone(),
        session: public,
        trusted_action_update: trusted_update,
        repository_snapshot: snapshot,
    };

    if let Ok(mut st) = ctx.run_state.lock() {
        st.sync_from_stored(&session);
        st.settled_reply = Some(reply);
    }

    let wire_links: Vec<Value> = links
        .into_iter()
        .map(|l| json!({ "label": l.label, "url": l.url }))
        .collect();
    PiToolExecutionResult {
        ok: true,
        content: final_summary.clone(),
        details: Some(json!({
            "stopAgent": true,
            "summary": final_summary,
            "links": wire_links,
        })),
        is_error: false,
    }
}

impl CustomActionCoordinator {
    /// Deterministic remote sync, non-fast-forward rebase recovery, and push retry shared by
    /// C-Push fast path and zero-token Host Recovery (`RetryPush` / `SetUpstream`).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn sync_and_push_session(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredActionSession,
        set_upstream_remote: Option<&str>,
        initial_snapshot: Option<RepositorySnapshot>,
        on_progress: &mut (impl FnMut(ActionProgressEvent) -> Result<(), String> + Send),
    ) -> Result<ActionCommandReply, String> {
        let locale = session.locale;
        let read_target = session.read_target();
        let commit_summaries = summarize_commits(&session.commits);

        session.status = ActionSessionStatus::Running;
        session.pending_call_id = None;
        session.can_retry_remaining = false;
        session.auto_collapse = false;
        let mut push_timeline: Vec<WorkflowTimelineItem> = session
            .commits
            .iter()
            .map(|c| WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_local_commit_titled(
                    locale,
                    &c.short_oid,
                    &c.title,
                ),
            })
            .collect();
        push_timeline.push(WorkflowTimelineItem {
            state: "active".into(),
            label: i18n::cpush::cpush_timeline_pushing(locale).into(),
        });
        session.timeline = push_timeline;
        session.message = Some(WorkflowUiMessage {
            id: format!("progress-push-{request_id}"),
            kind: "progress".into(),
            title: i18n::cpush::cpush_progress_pushing_title(locale).into(),
            description: i18n::cpush::cpush_progress_pushing_desc(locale).into(),
            scope: Some(session.branch.clone()),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
            options: None,
            links: (!session.links.is_empty()).then_some(session.links.clone()),
            card: None,
            questionnaire: None,
            progress: Some(WorkflowMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush::cpush_progress_pushing_steps(locale).into(),
            }),
        });
        self.save_session_in_memory(session)?;
        let _ = self.persist_session(storage_dir, session).await;
        let _ = on_progress(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: session.to_public(),
            trusted_action_update: None,
            repository_snapshot: initial_snapshot.clone(),
        });

        let push_result = repo_store
            .push_branch(&read_target, set_upstream_remote, locale)
            .await;

        let final_snapshot = match push_result {
            Ok(PushOutcome::Pushed {
                remote_branch,
                snapshot,
            }) => {
                mark_pushed_done(session, &remote_branch, false);
                Some(snapshot)
            }
            Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                enter_missing_upstream_state(session, &remotes);
                Some(snapshot)
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                non_fast_forward: true,
                snapshot: pre_sync_snapshot,
                ..
            }) => {
                match repo_store
                    .sync_rebase_onto_upstream_with_locale(&read_target, locale)
                    .await
                {
                    outcome @ (Ok(RebaseSyncOutcome::Completed { .. })
                    | Ok(RebaseSyncOutcome::AlreadyUpToDate { .. })) => {
                        let reb_snap = match outcome {
                            Ok(RebaseSyncOutcome::Completed {
                                rebased_commits,
                                snapshot,
                                ..
                            }) => {
                                apply_rebased_commits(session, &rebased_commits);
                                snapshot
                            }
                            Ok(RebaseSyncOutcome::AlreadyUpToDate { snapshot, .. }) => snapshot,
                            _ => unreachable!(),
                        };
                        match repo_store.push_branch(&read_target, None, locale).await {
                            Ok(PushOutcome::Pushed {
                                remote_branch: pushed_target,
                                snapshot: pushed_snap,
                            }) => {
                                mark_pushed_done(session, &pushed_target, true);
                                Some(pushed_snap)
                            }
                            Ok(PushOutcome::Failed {
                                remote_branch: failed_target,
                                reason,
                                snapshot: failed_snap,
                                ..
                            }) => {
                                enter_push_failed_state(
                                    session,
                                    &failed_target,
                                    &reason,
                                    Some(i18n::cpush::cpush_rebased_push_failed_title(locale)),
                                );
                                Some(failed_snap)
                            }
                            Ok(PushOutcome::MissingUpstream { snapshot: s, .. }) => {
                                enter_push_failed_state(
                                    session,
                                    &remote_branch,
                                    i18n::cpush::cpush_missing_upstream_short_reason(locale),
                                    None,
                                );
                                Some(s)
                            }
                            Err(err) => {
                                enter_push_failed_state(session, &remote_branch, &err, None);
                                Some(reb_snap)
                            }
                        }
                    }
                    Ok(RebaseSyncOutcome::Conflict {
                        upstream,
                        conflicted_files,
                        snapshot: conf_snap,
                    }) => {
                        let has_model = !session.model_id.is_empty();
                        enter_conflict_state(
                            session,
                            &upstream,
                            &conflicted_files,
                            has_model,
                            None,
                        );
                        Some(conf_snap)
                    }
                    Ok(RebaseSyncOutcome::UnsafeDirtyOverlap {
                        upstream,
                        overlapping_paths,
                        snapshot: ov_snap,
                    }) => {
                        enter_push_failed_state(
                            session,
                            &upstream,
                            &i18n::cpush::cpush_dirty_overlap_push_failed_reason(
                                locale,
                                &overlapping_paths,
                            ),
                            Some(i18n::cpush::cpush_dirty_overlap_push_failed_title(locale)),
                        );
                        Some(ov_snap)
                    }
                    Ok(RebaseSyncOutcome::UnsafePublishedHistory {
                        upstream,
                        reason: pub_reason,
                        snapshot: pub_snap,
                    }) => {
                        enter_push_failed_state(
                            session,
                            &upstream,
                            &pub_reason,
                            Some(i18n::cpush::cpush_published_history_risk_title(locale)),
                        );
                        Some(pub_snap)
                    }
                    Ok(RebaseSyncOutcome::MissingUpstream { snapshot: ms, .. }) => {
                        enter_push_failed_state(
                            session,
                            &remote_branch,
                            i18n::cpush::cpush_missing_upstream_short_reason(locale),
                            None,
                        );
                        Some(ms)
                    }
                    Err(sync_err) => {
                        enter_push_failed_state(session, &remote_branch, &sync_err, None);
                        Some(pre_sync_snapshot)
                    }
                }
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                non_fast_forward: false,
                reason,
                snapshot,
            }) => {
                enter_push_failed_state(session, &remote_branch, &reason, None);
                Some(snapshot)
            }
            Err(err) => {
                let fallback_snap = match repo_store.refresh_with_locale(&read_target, locale).await
                {
                    Ok(s) => Some(s),
                    Err(_) => initial_snapshot,
                };
                let branch_name = session.branch.clone();
                enter_push_failed_state(session, &branch_name, &err, None);
                fallback_snap
            }
        };

        self.save_session_in_memory(session)?;
        self.persist_session(storage_dir, session).await?;
        let public = session.to_public();
        let _ = on_progress(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: public.clone(),
            trusted_action_update: None,
            repository_snapshot: final_snapshot.clone(),
        });
        Ok(ActionCommandReply {
            protocol_version: ACTION_PROTOCOL_VERSION,
            kind: "action",
            request_id: request_id.to_owned(),
            session: public,
            trusted_action_update: None,
            repository_snapshot: final_snapshot,
        })
    }
}
