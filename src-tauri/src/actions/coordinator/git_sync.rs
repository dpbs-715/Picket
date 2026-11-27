//! Deterministic C-Push group execution, standalone Pull & Rebase, remote sync/push,
//! and post-commit / conflict / retry response handlers.

use super::{
    CustomActionCoordinator,
    cpush_helpers::{
        apply_rebased_commits, conflict_chips, enter_conflict_state, enter_push_failed_state,
        file_basename, mark_pushed_done, summarize_commits,
    },
};
use crate::{
    actions::types::*,
    i18n,
    models::store::ModelStore,
    pi_runtime::{PiPaths, PiSupervisor},
    repository::{
        self, RepositoryStore,
        ai_context::FrozenCPushFile,
        remote::{PushOutcome, RebaseSyncOutcome},
        types::{CommitRequest, RepositorySnapshot},
    },
};
use std::{collections::HashMap, path::Path};

fn is_unsupported_commit_guard_error(err: &str, locale: i18n::LanguageKind) -> bool {
    err == i18n::repository::commit_submodule_error(locale)
        || err == i18n::repository::commit_sparse_or_hidden_flag_error(locale)
        || err == i18n::repository::runner_external_filter_write_error(locale)
}

impl CustomActionCoordinator {
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
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::PullRebase,
            locale,
        )?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self.lookup_processed_cpush_request(&request.request_id)? {
            return Ok(existing_reply);
        }
        self.ensure_no_busy_cpush_session(&request.target.worktree_id, locale, false)?;

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
                    pending_call_id: None,
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
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_cpush_request(&request.request_id, &reply)?;
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
                    pending_call_id: None,
                    message: None,
                    can_retry_remaining: false,
                    push_authorized: false,
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
                self.save_cpush_in_memory(&session)?;
                self.persist_cpush_sessions(storage_dir).await?;
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
                    repository_snapshot: Some(snapshot),
                };
                self.record_processed_cpush_request(&request.request_id, &reply)?;
                Ok(reply)
            }
        }
    }

    pub(super) async fn execute_planned_groups(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let total = session.planned_groups.len();
        if total == 0 {
            let read_target = self.session_read_target(session);
            if session.push_authorized
                && let Ok((live_remote, snap)) = repo_store
                    .inspect_live_remote_status(&read_target, false, locale)
                    .await
                && live_remote.ahead > 0
            {
                return self
                    .sync_and_push_commits(
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
            session.session_status = CPushSessionStatus::Done;
            session.pending_call_id = None;
            session.can_retry_remaining = false;
            session.timeline = vec![CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_all_skipped(locale).into(),
            }];
            session.message = Some(CPushUiMessage {
                id: format!("done-skipped-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush::cpush_all_skipped_title(locale).into(),
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
            self.save_cpush_in_memory(session)?;
            self.persist_cpush_sessions(storage_dir).await?;
            let public = session.to_public();
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public.clone(),
                repository_snapshot: None,
            });
            return Ok(CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request_id.to_owned(),
                session: public,
                repository_snapshot: None,
            });
        }

        let by_id: HashMap<String, FrozenCPushFile> = session
            .frozen_files
            .iter()
            .map(|f| (f.file_id.clone(), f.clone()))
            .collect();
        let read_target = self.session_read_target(session);
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

            session.session_status = CPushSessionStatus::Running;
            session.pending_call_id = None;
            session.can_retry_remaining = false;
            let mut timeline: Vec<CPushTimelineItem> = session
                .commits
                .iter()
                .enumerate()
                .map(|(i, c)| CPushTimelineItem {
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
            timeline.push(CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_step_writing(
                    locale,
                    step_idx + 1,
                    total,
                    &step_title,
                ),
            });
            session.timeline = timeline;
            session.message = Some(CPushUiMessage {
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
                progress: Some(CPushMessageProgress {
                    completed: (step_idx + 1) as u32,
                    total: (total + 2) as u32,
                    label: i18n::cpush::cpush_progress_step_label(locale, step_idx + 1, total),
                }),
            });
            self.save_cpush_in_memory(session)?;
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: session.to_public(),
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
                    .await?;
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
                    latest_snapshot = Some(snapshot.clone());
                    session.commits.push(CPushLandedCommit {
                        oid: commit_info.oid,
                        short_oid: commit_info.short_oid,
                        title: commit_info.title,
                        branch: commit_info.branch,
                        file_ids: group.file_ids.clone(),
                    });
                    session.committed_file_ids.extend(group.file_ids.clone());
                    session.next_step_index = step_idx + 1;
                    self.save_cpush_in_memory(session)?;
                    self.persist_cpush_sessions(storage_dir).await?;
                }
                Err(err) => {
                    if latest_snapshot.is_none()
                        && let Ok(refreshed) =
                            repo_store.refresh_with_locale(&read_target, locale).await
                    {
                        latest_snapshot = Some(refreshed);
                    }
                    let failed_chips: Vec<CPushMessageFileChip> = step_frozen_files
                        .iter()
                        .map(|f| CPushMessageFileChip {
                            id: f.file_id.clone(),
                            label: file_basename(&f.path),
                            badge: (f.range_kind == "partial")
                                .then(|| i18n::cpush::cpush_partial_badge(locale).into()),
                        })
                        .collect();
                    let commit_summaries = summarize_commits(&session.commits);
                    let mut failed_timeline: Vec<CPushTimelineItem> = session
                        .commits
                        .iter()
                        .enumerate()
                        .map(|(i, c)| CPushTimelineItem {
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
                    failed_timeline.push(CPushTimelineItem {
                        state: "failed".into(),
                        label: i18n::cpush::cpush_timeline_step_paused(
                            locale,
                            step_idx + 1,
                            total,
                            &step_title,
                        ),
                    });

                    let can_retry = !is_unsupported_commit_guard_error(&err, locale);
                    let options = if can_retry {
                        vec![
                            CPushMessageOption {
                                id: "retry-remaining".into(),
                                label: i18n::cpush::cpush_option_retry_remaining(locale).into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
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
                        vec![CPushMessageOption {
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
                        i18n::cpush::cpush_step_paused_desc(locale, &err, step_idx > 0)
                    } else {
                        i18n::cpush::cpush_step_blocked_desc(locale, &err, step_idx > 0)
                    };

                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.pending_call_id = None;
                    session.can_retry_remaining = can_retry;
                    session.timeline = failed_timeline;
                    session.message = Some(CPushUiMessage {
                        id: format!("err-{}-step-{}", session.run_id, step_idx + 1),
                        kind: "error".into(),
                        title: i18n::cpush::cpush_step_paused_title(locale, step_idx, total),
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
                    self.save_cpush_in_memory(session)?;
                    self.persist_cpush_sessions(storage_dir).await?;
                    let public = session.to_public();
                    let _ = on_progress(CPushProgressEvent {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        session: public.clone(),
                        repository_snapshot: latest_snapshot.clone(),
                    });
                    return Ok(CPushCommandReply {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        kind: "cPush",
                        request_id: request_id.to_owned(),
                        session: public,
                        repository_snapshot: latest_snapshot,
                    });
                }
            }
        }

        if !session.push_authorized {
            session.session_status = CPushSessionStatus::Done;
            session.pending_call_id = None;
            session.can_retry_remaining = false;
            let commit_summaries = summarize_commits(&session.commits);
            let mut done_timeline: Vec<CPushTimelineItem> = session
                .commits
                .iter()
                .enumerate()
                .map(|(i, c)| CPushTimelineItem {
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
            done_timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_local_only_done(locale).into(),
            });
            session.timeline = done_timeline;
            session.message = Some(CPushUiMessage {
                id: format!("done-commit-only-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush::cpush_local_only_done_title(locale, session.commits.len()),
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
            self.save_cpush_in_memory(session)?;
            self.persist_cpush_sessions(storage_dir).await?;
            let public = session.to_public();
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public.clone(),
                repository_snapshot: latest_snapshot.clone(),
            });
            return Ok(CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request_id.to_owned(),
                session: public,
                repository_snapshot: latest_snapshot,
            });
        }

        self.sync_and_push_commits(
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

        session.session_status = CPushSessionStatus::Running;
        session.pending_call_id = None;
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
            links: None,
            card: None,
            questionnaire: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush::cpush_progress_pushing_steps(locale).into(),
            }),
        });
        self.save_cpush_in_memory(session)?;
        let _ = self.persist_cpush_sessions(storage_dir).await;
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
                mark_pushed_done(session, &remote_branch, false);
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
                    session.pending_call_id = None;
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
                        links: None,
                        card: None,
                        questionnaire: None,
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

        self.save_cpush_in_memory(session)?;
        self.persist_cpush_sessions(storage_dir).await?;
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
            repository_snapshot: final_snapshot,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn respond_cpush(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &RespondCPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)?;
        let _repo_lease = self.inner.write_coordinator.try_acquire(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::CPush,
            req_locale,
        )?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self.lookup_processed_cpush_request(&request.request_id)? {
            return Ok(existing_reply);
        }

        let mut session = self
            .inner
            .cpush_sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or_else(|| i18n::cpush::cpush_no_pending_session_error(req_locale))?;
        if let Some(loc) = request.locale {
            session.locale = loc;
        }
        let locale = session.locale;

        if session.run_id != request.run_id
            || session.repository_id != request.target.repository_id
            || session.worktree_id != request.target.worktree_id
        {
            return Err(i18n::cpush::cpush_mismatched_reply_target_error(locale).into());
        }
        if !matches!(
            session.session_status,
            CPushSessionStatus::NeedsDecision
                | CPushSessionStatus::Conflict
                | CPushSessionStatus::WaitingManual
                | CPushSessionStatus::PushFailed
        ) {
            return Err(i18n::cpush::cpush_not_interactive_error(locale).into());
        }
        let current_msg = session
            .message
            .clone()
            .ok_or_else(|| i18n::cpush::cpush_missing_active_message_error(locale))?;
        if current_msg.id != request.message_id {
            return Err(i18n::cpush::cpush_stale_message_id_error(locale).into());
        }
        session.auto_collapse = false;
        if let Some(req_model) = &request.model_id
            && !req_model.trim().is_empty()
            && session.model_id.is_empty()
        {
            session.model_id = req_model.trim().to_owned();
        }

        let reply = if request.option_id.as_deref() == Some("dismiss-session") {
            self.respond_dismiss_session(
                storage_dir,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await?
        } else if request.option_id.as_deref() == Some("retry-remaining") {
            if !session.can_retry_remaining
                || session.next_step_index >= session.planned_groups.len()
            {
                return Err(i18n::cpush::cpush_no_retry_remaining_error(locale).into());
            }
            self.execute_planned_groups(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await?
        } else if matches!(
            request.option_id.as_deref(),
            Some("retry-push" | "continue-push")
        ) {
            self.sync_and_push_commits(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                None,
                None,
                &mut on_progress,
            )
            .await?
        } else if let Some(chosen_remote) = request
            .option_id
            .as_deref()
            .and_then(|opt| opt.strip_prefix("push-set-upstream:"))
        {
            self.respond_set_upstream_option(
                storage_dir,
                repo_store,
                request,
                &mut session,
                &current_msg,
                chosen_remote,
                &mut on_progress,
            )
            .await?
        } else if current_msg.id.starts_with("missing-upstream-")
            && let Some(free_text) = &request.text
        {
            self.respond_set_upstream_free_text(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &current_msg,
                free_text,
                &mut on_progress,
            )
            .await?
        } else if request.option_id.as_deref() == Some("abort-rebase") {
            self.respond_abort_rebase(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &mut on_progress,
            )
            .await?
        } else if request.option_id.as_deref() == Some("manual-merge") {
            self.respond_manual_merge(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                &current_msg,
                &mut on_progress,
            )
            .await?
        } else if session.session_status == CPushSessionStatus::Conflict
            && (request.option_id.as_deref() == Some("agent-merge") || request.text.is_some())
        {
            let instruction = request
                .text
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_owned();
            if request.option_id.is_none() && instruction.is_empty() {
                return Err(i18n::cpush::cpush_enter_merge_instruction_error(locale).into());
            }
            self.execute_agent_conflict_merge(
                storage_dir,
                paths,
                supervisor,
                models,
                repo_store,
                &request.request_id,
                &mut session,
                &current_msg,
                &instruction,
                &mut on_progress,
            )
            .await?
        } else {
            return Err(i18n::cpush::cpush_unsupported_reply_state_error(locale).into());
        };

        self.record_processed_cpush_request(&request.request_id, &reply)?;
        Ok(reply)
    }

    async fn respond_dismiss_session(
        &self,
        storage_dir: &Path,
        request_id: &str,
        session: &mut StoredCPushSession,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        if session.commits.is_empty() {
            session.session_status = CPushSessionStatus::Idle;
            session.can_retry_remaining = false;
            session.message = None;
            self.remove_cpush_in_memory(&session.worktree_id)?;
        } else {
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            let commit_summaries = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("done-local-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush::cpush_dismiss_kept_local_title(locale, session.commits.len()),
                description: i18n::cpush::cpush_dismiss_kept_local_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: Some(commit_summaries),
                options: None,
                links: None,
                card: None,
                questionnaire: None,
                progress: None,
            });
            self.save_cpush_in_memory(session)?;
        }
        self.persist_cpush_sessions(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: None,
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            repository_snapshot: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn respond_set_upstream_option(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &RespondCPushRequest,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        chosen_remote: &str,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let chosen = chosen_remote.to_owned();
        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: i18n::cpush::cpush_push_track_label(session.locale, &chosen, &session.branch),
            option_id: request.option_id.clone(),
        });
        self.sync_and_push_commits(
            storage_dir,
            repo_store,
            &request.request_id,
            session,
            Some(&chosen),
            None,
            on_progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn respond_set_upstream_free_text(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        free_text: &str,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let trimmed = free_text.trim();
        if trimmed.is_empty() {
            return Err(i18n::cpush::cpush_enter_remote_or_select_error(locale).into());
        }
        let available: Vec<String> = current_msg
            .options
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|o| o.id.strip_prefix("push-set-upstream:").map(str::to_owned))
            .collect();
        let matched: Vec<&String> = available
            .iter()
            .filter(|r| trimmed.contains(r.as_str()))
            .collect();
        if matched.len() == 1 {
            let chosen = matched[0].clone();
            session.qa_log.push(CPushQaItem {
                question_id: current_msg.id.clone(),
                question: current_msg.title.clone(),
                answer: trimmed.to_owned(),
                option_id: None,
            });
            return self
                .sync_and_push_commits(
                    storage_dir,
                    repo_store,
                    request_id,
                    session,
                    Some(&chosen),
                    None,
                    on_progress,
                )
                .await;
        }
        Err(i18n::cpush::cpush_ambiguous_remote_input_error(
            locale, &available,
        ))
    }

    async fn respond_abort_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let read_target = self.session_read_target(session);
        let snapshot = repo_store.abort_active_rebase(&read_target, locale).await?;
        if session.commits.is_empty() {
            session.session_status = CPushSessionStatus::Idle;
            session.message = None;
            self.remove_cpush_in_memory(&session.worktree_id)?;
        } else {
            let branch_name = session.branch.clone();
            enter_push_failed_state(
                session,
                &branch_name,
                i18n::cpush::cpush_aborted_rebase_reason(locale),
                Some(i18n::cpush::cpush_aborted_rebase_title(locale)),
            );
            self.save_cpush_in_memory(session)?;
        }
        self.persist_cpush_sessions(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: Some(snapshot.clone()),
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            repository_snapshot: Some(snapshot),
        })
    }

    async fn respond_manual_merge(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        current_msg: &CPushUiMessage,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let read_target = self.session_read_target(session);
        let (live, snapshot) = repo_store
            .inspect_live_remote_status(&read_target, false, locale)
            .await?;
        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: i18n::cpush::cpush_option_manual_merge(locale).into(),
            option_id: Some("manual-merge".into()),
        });
        session.session_status = CPushSessionStatus::WaitingManual;
        session.pending_call_id = None;
        session.can_retry_remaining = false;
        session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_chose_manual_merge(locale).into(),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_waiting_return_reconcile(locale).into(),
            },
        ];
        let chips = if !live.conflicted_files.is_empty() {
            Some(conflict_chips(locale, &live.conflicted_files))
        } else {
            current_msg.files.clone()
        };
        session.message = Some(CPushUiMessage {
            id: format!("waiting-manual-{}", session.run_id),
            kind: "waitingManual".into(),
            title: i18n::cpush::cpush_waiting_manual_title(locale).into(),
            description: i18n::cpush::cpush_waiting_manual_desc(locale).into(),
            scope: Some(i18n::cpush::cpush_waiting_manual_scope(locale).into()),
            files: chips,
            commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
            options: Some(vec![CPushMessageOption {
                id: "abort-rebase".into(),
                label: i18n::cpush::cpush_option_abort_rebase(locale, !session.commits.is_empty())
                    .into(),
                recommended: None,
            }]),
            links: None,
            card: None,
            questionnaire: None,
            progress: None,
        });
        self.save_cpush_in_memory(session)?;
        self.persist_cpush_sessions(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: Some(snapshot.clone()),
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            repository_snapshot: Some(snapshot),
        })
    }
}
