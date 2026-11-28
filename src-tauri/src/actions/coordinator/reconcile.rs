//! Reconciliation of `WaitingManual`, `Conflict`, and `PushFailed` sessions against live Git & remote.

use super::{
    CustomActionCoordinator,
    session_helpers::{apply_rebased_commits, conflict_chips, summarize_commits},
};
use crate::{
    actions::types::*,
    error::{AppError, AppErrorCode},
    i18n,
    repository::{self, RepositoryStore},
};
use std::path::Path;

impl CustomActionCoordinator {
    /// Reconcile `WaitingManual` / `Conflict` / `PushFailed` sessions when returning from IDE or Terminal (`R04`).
    pub async fn reconcile_worktree(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &ReconcileSessionRequest,
    ) -> Result<Option<ActionProgressEvent>, AppError> {
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        let candidate_status = self
            .inner
            .active_sessions
            .lock()
            .map_err(|_| AppError::internal("Session cache lock is unavailable"))?
            .get(&request.target.worktree_id)
            .map(|s| s.status);
        if !candidate_status.is_some_and(ActionSessionStatus::is_reconcilable) {
            return Ok(None);
        }

        let Some(_repo_lease) = self.inner.write_coordinator.try_acquire_silent(
            &request.target.repository_id,
            crate::repo_lock::RepoWriteKind::Reconcile,
        ) else {
            return Ok(None);
        };
        let wt_gate = self
            .worktree_gate(&request.target.worktree_id)
            .map_err(AppError::internal)?;
        let Ok(_wt_guard) = wt_gate.try_lock_owned() else {
            return Ok(None);
        };

        let mut session = match self
            .inner
            .active_sessions
            .lock()
            .map_err(|_| AppError::internal("Session cache lock is unavailable"))?
            .get(&request.target.worktree_id)
            .cloned()
        {
            Some(s) if s.status.is_reconcilable() => s,
            _ => return Ok(None),
        };
        if let Some(loc) = request.locale {
            session.locale = loc;
        }
        let locale = session.locale;

        let (local_live, local_snapshot) = repo_store
            .inspect_live_remote_status(&request.target, false, locale)
            .await
            .map_err(|msg| AppError::git(AppErrorCode::GitFailed, msg))?;

        // 1. If conflicts or rebase are still unfinished -> keep waiting without auto-writing (`R04`):
        if local_live.rebase_in_progress
            || local_live.merge_or_other_in_progress
            || !local_live.conflicted_files.is_empty()
        {
            if session.status == ActionSessionStatus::WaitingManual {
                let chips = conflict_chips(locale, &local_live.conflicted_files);
                let desc = if !local_live.conflicted_files.is_empty() {
                    let names: Vec<&str> = local_live
                        .conflicted_files
                        .iter()
                        .map(|f| f.path.as_str())
                        .collect();
                    i18n::cpush::cpush_manual_still_conflicted_desc(locale, &names)
                } else {
                    i18n::cpush::cpush_manual_rebase_incomplete_desc(locale).into()
                };
                if let Some(msg) = session.message.as_mut() {
                    msg.description = desc;
                    msg.files = (!chips.is_empty()).then_some(chips);
                }
                session.auto_collapse = false;
                self.save_session_in_memory(&session)
                    .map_err(AppError::internal)?;
                self.persist_session(storage_dir, &session)
                    .await
                    .map_err(AppError::storage)?;
                return Ok(Some(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: session.to_public(),
                    trusted_action_update: None,
                    repository_snapshot: Some(local_snapshot),
                }));
            }
            return Ok(None);
        }

        // 2. Rebase / conflict is no longer active! Verify live remote reachability:
        let (verified_live, verified_snapshot) = repo_store
            .inspect_live_remote_status(&request.target, true, locale)
            .await
            .map_err(|msg| AppError::git(AppErrorCode::GitFailed, msg))?;

        let remote_verified = verified_snapshot
            .worktrees
            .iter()
            .find(|w| w.worktree_id == request.target.worktree_id)
            .is_some_and(|w| {
                matches!(
                    w.remote,
                    repository::types::Remote::Tracked {
                        local_only: false,
                        ..
                    }
                )
            });

        if session.status == ActionSessionStatus::PushFailed {
            if let Some(spec) = &verified_live.upstream
                && verified_live.ahead == 0
                && remote_verified
            {
                session.status = ActionSessionStatus::Done;
                session.can_retry_remaining = false;
                session.auto_collapse = true;
                session.timeline.push(WorkflowTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush::cpush_timeline_terminal_pushed(locale, &spec.display),
                });
                let commits = summarize_commits(&session.commits);
                let title = i18n::cpush::cpush_terminal_pushed_title(locale, &spec.display);
                session.summary = title.clone();
                session.message = Some(WorkflowUiMessage {
                    id: format!("done-ext-push-{}", session.run_id),
                    kind: "done".into(),
                    title,
                    description: i18n::cpush::cpush_terminal_pushed_from_failed_desc(locale).into(),
                    scope: Some(i18n::cpush::cpush_synced_scope(locale, &session.branch)),
                    files: None,
                    commits: (!commits.is_empty()).then_some(commits),
                    options: None,
                    links: (!session.links.is_empty()).then_some(session.links.clone()),
                    card: None,
                    questionnaire: None,
                    progress: None,
                });
                self.save_session_in_memory(&session)
                    .map_err(AppError::internal)?;
                self.persist_session(storage_dir, &session)
                    .await
                    .map_err(AppError::storage)?;
                return Ok(Some(ActionProgressEvent {
                    protocol_version: ACTION_PROTOCOL_VERSION,
                    session: session.to_public(),
                    trusted_action_update: None,
                    repository_snapshot: Some(verified_snapshot),
                }));
            }
            return Ok(None);
        }

        // `WaitingManual` or `Conflict` where conflicts & rebase are now resolved!
        apply_rebased_commits(&mut session, &verified_live.unpushed_commits);

        if let Some(spec) = &verified_live.upstream
            && verified_live.ahead == 0
            && remote_verified
        {
            session.status = ActionSessionStatus::Done;
            session.can_retry_remaining = false;
            session.auto_collapse = true;
            session.qa_log.push(CPushQaItem {
                question_id: format!("manual-ext-{}", session.run_id),
                question: i18n::cpush::cpush_qa_manual_ext_question(locale).into(),
                answer: i18n::cpush::cpush_qa_manual_ext_answer(locale).into(),
                option_id: None,
            });
            session.timeline.push(WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_manual_resolved_and_pushed(
                    locale,
                    &spec.display,
                ),
            });
            let commits = summarize_commits(&session.commits);
            let title = i18n::cpush::cpush_terminal_pushed_title(locale, &spec.display);
            session.summary = title.clone();
            session.message = Some(WorkflowUiMessage {
                id: format!("done-manual-pushed-{}", session.run_id),
                kind: "done".into(),
                title,
                description: i18n::cpush::cpush_terminal_pushed_from_manual_desc(locale).into(),
                scope: Some(i18n::cpush::cpush_synced_scope(locale, &session.branch)),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: None,
                links: (!session.links.is_empty()).then_some(session.links.clone()),
                card: None,
                questionnaire: None,
                progress: None,
            });
        } else if !session.push_authorized {
            session.status = ActionSessionStatus::Done;
            session.can_retry_remaining = false;
            session.auto_collapse = false;
            session.timeline.push(WorkflowTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_manual_rebase_done(locale).into(),
            });
            let commits = summarize_commits(&session.commits);
            let title: String = i18n::cpush::cpush_manual_rebase_done_title(locale).into();
            session.summary = title.clone();
            session.message = Some(WorkflowUiMessage {
                id: format!("done-manual-rebase-{}", session.run_id),
                kind: "done".into(),
                title,
                description: i18n::cpush::cpush_manual_rebase_done_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: None,
                links: (!session.links.is_empty()).then_some(session.links.clone()),
                card: None,
                questionnaire: None,
                progress: None,
            });
        } else {
            let target_label = verified_live
                .upstream
                .as_ref()
                .map(|u| u.display.as_str())
                .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale));
            session.status = ActionSessionStatus::NeedsDecision;
            session.can_retry_remaining = false;
            session.auto_collapse = false;
            session.timeline = vec![
                WorkflowTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush::cpush_timeline_manual_rebase_done(locale).into(),
                },
                WorkflowTimelineItem {
                    state: "active".into(),
                    label: i18n::cpush::cpush_timeline_awaiting_continue_push(locale, target_label),
                },
            ];
            let commits = summarize_commits(&session.commits);
            let title: String = i18n::cpush::cpush_resolved_continue_push_title(locale).into();
            session.summary = title.clone();
            session.message = Some(WorkflowUiMessage {
                id: format!("resolved-continue-push-{}", session.run_id),
                kind: "resolved".into(),
                title,
                description: i18n::cpush::cpush_resolved_continue_push_desc(locale, target_label),
                scope: Some(i18n::cpush::cpush_push_stage_scope(locale, target_label)),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: Some(vec![
                    WorkflowMessageOption {
                        id: "continue-push".into(),
                        label: i18n::cpush::cpush_option_continue_push(locale).into(),
                        recommended: Some(true),
                    },
                    WorkflowMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush::cpush_option_skip_push_keep_local(locale).into(),
                        recommended: None,
                    },
                ]),
                links: (!session.links.is_empty()).then_some(session.links.clone()),
                card: None,
                questionnaire: None,
                progress: None,
            });
        }

        self.save_session_in_memory(&session)
            .map_err(AppError::internal)?;
        self.persist_session(storage_dir, &session)
            .await
            .map_err(AppError::storage)?;
        Ok(Some(ActionProgressEvent {
            protocol_version: ACTION_PROTOCOL_VERSION,
            session: session.to_public(),
            trusted_action_update: None,
            repository_snapshot: Some(verified_snapshot),
        }))
    }
}
