//! Standalone Pull & Rebase, remote push with non-fast-forward rebase sync, and completion state.

use super::{
    CPushCoordinator,
    helpers::{
        apply_rebased_commits, enter_conflict_state, enter_push_failed_state, summarize_commits,
    },
    types::*,
};
use crate::{
    i18n,
    repository::{
        self, RepositoryStore,
        remote::{PushOutcome, RebaseSyncOutcome},
        types::RepositorySnapshot,
    },
};
use std::path::Path;

impl CPushCoordinator {
    /// Standalone Pull & Rebase (`push_authorized = false`, `R06`).
    pub async fn pull_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &PullRebaseRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self.lookup_processed_request(&request.request_id)? {
            return Ok(existing_reply);
        }
        self.ensure_no_busy_session(storage_dir, &request.target.worktree_id, locale, false)
            .await?;

        let outcome = repo_store
            .sync_rebase_onto_upstream_with_locale(&request.target, locale)
            .await?;
        match outcome {
            RebaseSyncOutcome::MissingUpstream { remotes, .. } => Err(
                i18n::cpush::pull_rebase_missing_upstream_error(locale, &remotes),
            ),
            RebaseSyncOutcome::UnsafeDirtyOverlap {
                upstream,
                overlapping_paths,
                ..
            } => Err(i18n::cpush::pull_rebase_dirty_overlap_error(
                locale,
                &upstream,
                &overlapping_paths,
            )),
            RebaseSyncOutcome::UnsafePublishedHistory { reason, .. } => Err(reason),
            RebaseSyncOutcome::AlreadyUpToDate { snapshot, .. }
            | RebaseSyncOutcome::Completed { snapshot, .. } => {
                let idle_session = CPushSessionSnapshot {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: request.model_id.clone().unwrap_or_default(),
                    frozen_file_ids: Vec::new(),
                    committed_file_ids: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    session_status: CPushSessionStatus::Idle,
                    message: None,
                    timeline: Vec::new(),
                    qa_log: Vec::new(),
                    commits: Vec::new(),
                    can_retry_remaining: false,
                    auto_collapse: None,
                };
                let reply = CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request.request_id.clone(),
                    session: idle_session,
                    prepared_plan: None,
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_request(&request.request_id, &reply)?;
                Ok(reply)
            }
            RebaseSyncOutcome::Conflict {
                upstream,
                conflicted_files,
                snapshot,
            } => {
                let active_tree = snapshot
                    .worktrees
                    .iter()
                    .find(|w| w.worktree_id == request.target.worktree_id);
                let branch = active_tree
                    .map(|w| w.branch.clone())
                    .unwrap_or_else(|| "HEAD".into());
                let model_id = request.model_id.clone().unwrap_or_default();
                let mut session = StoredCPushSession {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: model_id.clone(),
                    branch,
                    initial_head: active_tree.and_then(|w| w.head.clone()),
                    prompt: String::new(),
                    recent_commits: Vec::new(),
                    unselected_paths: Vec::new(),
                    frozen_files: Vec::new(),
                    planned_groups: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    next_step_index: 0,
                    committed_file_ids: Vec::new(),
                    commits: Vec::new(),
                    qa_log: Vec::new(),
                    timeline: Vec::new(),
                    session_status: CPushSessionStatus::Conflict,
                    message: None,
                    can_retry_remaining: false,
                    push_authorized: false,
                    plan_only: false,
                    auto_collapse: false,
                    notice_dismissed: false,
                    locale,
                    commit_language: locale,
                };
                enter_conflict_state(
                    &mut session,
                    &upstream,
                    &conflicted_files,
                    !model_id.is_empty(),
                    None,
                );
                self.save_in_memory(&session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: Some(snapshot.clone()),
                });
                let reply = CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request.request_id.clone(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_request(&request.request_id, &reply)?;
                Ok(reply)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn sync_and_push_commits(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        set_upstream_remote: Option<&str>,
        initial_snapshot: Option<RepositorySnapshot>,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let read_target = self.session_read_target(session);
        let commit_summaries = summarize_commits(&session.commits);

        // Emit pushing progress:
        session.session_status = CPushSessionStatus::Running;
        session.can_retry_remaining = false;
        session.auto_collapse = false;
        let mut push_timeline: Vec<CPushTimelineItem> = session
            .commits
            .iter()
            .map(|c| CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_local_commit_titled(
                    locale,
                    &c.short_oid,
                    &c.title,
                ),
            })
            .collect();
        push_timeline.push(CPushTimelineItem {
            state: "active".into(),
            label: i18n::cpush::cpush_timeline_pushing(locale).into(),
        });
        session.timeline = push_timeline;
        session.message = Some(CPushUiMessage {
            id: format!("progress-push-{request_id}"),
            kind: "progress".into(),
            title: i18n::cpush::cpush_progress_pushing_title(locale).into(),
            description: i18n::cpush::cpush_progress_pushing_desc(locale).into(),
            scope: Some(session.branch.clone()),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries.clone()),
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush::cpush_progress_pushing_steps(locale).into(),
            }),
        });
        self.save_in_memory(session)?;
        let _ = self.persist_all(storage_dir).await;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
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
                self.mark_pushed_done(session, &remote_branch, false);
                Some(snapshot)
            }
            Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                if remotes.is_empty() {
                    let branch_name = session.branch.clone();
                    enter_push_failed_state(
                        session,
                        &branch_name,
                        i18n::cpush::cpush_no_remote_configured_reason(locale),
                        Some(i18n::cpush::cpush_no_remote_configured_title(locale)),
                    );
                } else {
                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = false;
                    let mut timeline: Vec<CPushTimelineItem> = session
                        .commits
                        .iter()
                        .map(|c| CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_local_commit_written(
                                locale,
                                &c.short_oid,
                            ),
                        })
                        .collect();
                    timeline.push(CPushTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush::cpush_timeline_waiting_upstream_choice(locale).into(),
                    });
                    session.timeline = timeline;

                    let mut options: Vec<CPushMessageOption> = remotes
                        .iter()
                        .enumerate()
                        .map(|(idx, r)| CPushMessageOption {
                            id: format!("push-set-upstream:{r}"),
                            label: i18n::cpush::cpush_push_track_label(locale, r, &session.branch),
                            recommended: (remotes.len() == 1 && idx == 0).then_some(true),
                        })
                        .collect();
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush::cpush_option_keep_local_only(locale).into(),
                        recommended: None,
                    });
                    session.message = Some(CPushUiMessage {
                        id: format!(
                            "missing-upstream-{}-{}",
                            session.run_id,
                            session.qa_log.len() + 1
                        ),
                        kind: "question".into(),
                        title: i18n::cpush::cpush_missing_upstream_question_title(locale).into(),
                        description: i18n::cpush::cpush_missing_upstream_question_desc(
                            locale,
                            &session.branch,
                        ),
                        scope: Some(i18n::cpush::cpush_branch_scope(locale, &session.branch)),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(options),
                        progress: None,
                    });
                }
                Some(snapshot)
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                non_fast_forward: true,
                snapshot: pre_sync_snapshot,
                ..
            }) => {
                // Remote advanced (`behind > 0`): attempt safe sync rebase via the shared engine!
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
                                self.mark_pushed_done(session, &pushed_target, true);
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

        self.save_in_memory(session)?;
        self.persist_all(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: final_snapshot.clone(),
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            prepared_plan: None,
            repository_snapshot: final_snapshot,
        })
    }

    fn mark_pushed_done(
        &self,
        session: &mut StoredCPushSession,
        remote_branch: &str,
        rebased: bool,
    ) {
        let locale = session.locale;
        let count = session.commits.len().max(1);
        let commit_summaries = summarize_commits(&session.commits);
        let mut timeline: Vec<CPushTimelineItem> = session
            .commits
            .iter()
            .map(|c| CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_commit_item(locale, &c.short_oid, &c.title),
            })
            .collect();
        if rebased {
            timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_rebased_synced(locale, remote_branch),
            });
        }
        timeline.push(CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_pushed_done_title(locale, count, remote_branch),
        });

        session.timeline = timeline;
        session.session_status = CPushSessionStatus::Done;
        session.can_retry_remaining = false;
        session.auto_collapse = false;

        session.message = Some(CPushUiMessage {
            id: format!("done-{}", session.run_id),
            kind: "done".into(),
            title: i18n::cpush::cpush_pushed_done_title(locale, count, remote_branch),
            description: i18n::cpush::cpush_pushed_done_description(
                locale,
                session.skipped_file_ids.len(),
            ),
            scope: Some(i18n::cpush::cpush_synced_scope(locale, &session.branch)),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
            options: None,
            progress: None,
        });
    }
}
