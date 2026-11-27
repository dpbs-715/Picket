//! Step-by-step execution of planned C-Push commit groups with partial-failure recovery.

use super::{
    CPushCoordinator,
    helpers::{file_basename, summarize_commits},
    types::*,
};
use crate::{
    i18n,
    repository::{
        RepositoryStore,
        ai_context::FrozenCPushFile,
        types::{CommitRequest, RepositorySnapshot},
    },
};
use std::{collections::HashMap, path::Path};

fn is_unsupported_commit_guard_error(err: &str, locale: i18n::LanguageKind) -> bool {
    err == i18n::repository::commit_submodule_error(locale)
        || err == i18n::repository::commit_sparse_or_hidden_flag_error(locale)
        || err == i18n::repository::runner_external_filter_write_error(locale)
}

impl CPushCoordinator {
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
                progress: None,
            });
            self.save_in_memory(session)?;
            self.persist_all(storage_dir).await?;
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
                progress: Some(CPushMessageProgress {
                    completed: (step_idx + 1) as u32,
                    total: (total + 2) as u32,
                    label: i18n::cpush::cpush_progress_step_label(locale, step_idx + 1, total),
                }),
            });
            self.save_in_memory(session)?;
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
                    self.save_in_memory(session)?;
                    self.persist_all(storage_dir).await?;
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
                        progress: None,
                    });
                    self.save_in_memory(session)?;
                    self.persist_all(storage_dir).await?;
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
                progress: None,
            });
            self.save_in_memory(session)?;
            self.persist_all(storage_dir).await?;
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

        // All planned local commits landed — proceed directly to remote sync & push (`M5`)!
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
}
