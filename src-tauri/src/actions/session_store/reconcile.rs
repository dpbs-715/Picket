//! Cold-startup reconciliation for persisted Custom Action and C-Push sessions against live Git state.

use super::{
    schema::{ActionSessionSnapshot, PersistedActionSession, now_millis},
    storage_io::{load_store_unlocked, save_store_unlocked, sessions_file_path},
};
use crate::{
    actions::{
        coordinator::cpush_helpers::{enter_conflict_state, summarize_commits},
        types::*,
    },
    i18n::{self, LanguageKind},
    repository::{self, remote::ConflictedFileSummary, types::RepositorySnapshot},
    storage,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub(crate) struct StartupReconcileResult {
    pub action_sessions: Vec<ActionSessionSnapshot>,
    pub cpush_stored: Vec<StoredCPushSession>,
    pub cpush_public: Vec<CPushSessionSnapshot>,
    pub warning: Option<String>,
}

/// On cold startup, reconciles BOTH Custom Action sessions AND C-Push / Pull-Rebase sessions
/// in a single atomic locked pass over `~/.picket/action-sessions.json` (migrating legacy
/// `cpush-sessions.json` if present) without rolling back or deleting any landed commits,
/// Issues, branches, PRs, or isolated worktrees.
pub(crate) fn reconcile_all_sessions_on_startup(
    storage_dir: &Path,
    snapshots: &[RepositorySnapshot],
    locale: LanguageKind,
) -> StartupReconcileResult {
    let path = sessions_file_path(storage_dir);
    let live_trees: HashMap<&str, &repository::types::WorktreeSnapshot> = snapshots
        .iter()
        .flat_map(|snap| {
            snap.worktrees
                .iter()
                .map(|wt| (wt.worktree_id.as_str(), wt))
        })
        .collect();

    let result = storage::with_file_lock(&path, || {
        let (mut store, warning, mut dirty) = load_store_unlocked(storage_dir, locale);

        // 1. Reconcile Custom Action sessions:
        for session in &mut store.sessions {
            if session.status == "running" || session.status == "needsDecision" {
                session.status = "interrupted".into();
                session.updated_at_ms = now_millis();
                for step in &mut session.timeline {
                    if step.state == "active" {
                        step.state = "failed".into();
                    }
                }
                if session.summary.trim().is_empty() {
                    session.summary =
                        i18n::actions::action_interrupted_summary(locale, &session.action_label);
                }
                dirty = true;
            }
            for group in &mut session.submit_ledger.groups {
                if let Some(ref wt_path) = group.worktree_path
                    && !group.worktree_removed
                    && !Path::new(wt_path).exists()
                {
                    group.worktree_removed = true;
                    dirty = true;
                }
            }
        }

        // 2. Reconcile C-Push / Pull-Rebase sessions against live Git snapshots:
        let mut reconciled_cpush_stored: Vec<StoredCPushSession> =
            Vec::with_capacity(store.c_push_sessions.len());
        let mut cpush_public: Vec<CPushSessionSnapshot> = Vec::new();

        for mut session in store.c_push_sessions.drain(..) {
            session.auto_collapse = false;
            session.pending_call_id = None;

            // Live `message_user` questions from a dead process cannot resume their oneshot;
            // if no commits were landed yet, discard cleanly so the user is never stuck.
            if session.session_status == CPushSessionStatus::NeedsDecision
                && session.commits.is_empty()
                && session.planned_groups.is_empty()
                && session
                    .message
                    .as_ref()
                    .is_some_and(|m| m.questionnaire.is_some())
            {
                dirty = true;
                continue;
            }

            // Interrupted `Running` sessions must NEVER auto-rerun commits, rebases, or pushes:
            if session.session_status == CPushSessionStatus::Running {
                dirty = true;
                session.locale = locale;
                if session.commits.is_empty() && session.planned_groups.is_empty() {
                    let live_has_conflict = live_trees
                        .get(session.worktree_id.as_str())
                        .is_some_and(|wt| wt.files.iter().any(|f| f.status == "U"));
                    if !live_has_conflict {
                        continue;
                    }
                }

                if let Some(live_tree) = live_trees.get(session.worktree_id.as_str()) {
                    // Check if an in-progress local commit step landed right before exit:
                    if session.next_step_index < session.planned_groups.len()
                        && let Some(live_head) = &live_tree.head
                    {
                        let expected_prev = if session.next_step_index == 0 {
                            session.initial_head.as_deref()
                        } else {
                            session
                                .commits
                                .get(session.next_step_index - 1)
                                .map(|c| c.oid.as_str())
                        };
                        if Some(live_head.as_str()) != expected_prev
                            && !session.commits.iter().any(|c| &c.oid == live_head)
                        {
                            let group = &session.planned_groups[session.next_step_index];
                            let first_line = group
                                .message
                                .lines()
                                .next()
                                .unwrap_or_else(|| {
                                    i18n::cpush::cpush_commit_completed_fallback(locale)
                                })
                                .to_owned();
                            session.commits.push(CPushLandedCommit {
                                oid: live_head.clone(),
                                short_oid: live_head.chars().take(7).collect(),
                                title: first_line,
                                branch: live_tree.branch.clone(),
                                file_ids: group.file_ids.clone(),
                            });
                            session.committed_file_ids.extend(group.file_ids.clone());
                            session.next_step_index += 1;
                        }
                    }

                    // If live Git currently has conflicted files, surface `Conflict` directly:
                    let live_conflicts: Vec<ConflictedFileSummary> = live_tree
                        .files
                        .iter()
                        .filter(|f| f.status == "U")
                        .map(|f| ConflictedFileSummary {
                            file_id: f.id.clone(),
                            path: f.path.clone(),
                        })
                        .collect();
                    if !live_conflicts.is_empty() {
                        let upstream = match &live_tree.remote {
                            repository::types::Remote::Tracked { branch, .. } => branch.as_str(),
                            _ => i18n::repository::remote_branch_fallback(locale),
                        };
                        let has_model = !session.model_id.is_empty();
                        enter_conflict_state(
                            &mut session,
                            upstream,
                            &live_conflicts,
                            has_model,
                            Some(i18n::cpush::cpush_restart_conflict_reason(locale)),
                        );
                        cpush_public.push(session.to_public());
                        reconciled_cpush_stored.push(session);
                        continue;
                    }
                }

                let has_remaining_commits = session.next_step_index < session.planned_groups.len();
                let commit_summaries = summarize_commits(&session.commits);

                if !has_remaining_commits && !session.commits.is_empty() && session.push_authorized
                {
                    session.session_status = CPushSessionStatus::PushFailed;
                    session.can_retry_remaining = false;
                    session.message = Some(WorkflowUiMessage {
                        id: format!("interrupted-push-{}", session.run_id),
                        kind: "pushFailed".into(),
                        title: i18n::cpush::cpush_interrupted_push_title(locale).into(),
                        description: i18n::cpush::cpush_interrupted_push_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            WorkflowMessageOption {
                                id: "retry-push".into(),
                                label: i18n::cpush::cpush_option_retry_push(locale).into(),
                                recommended: Some(true),
                            },
                            WorkflowMessageOption {
                                id: "dismiss-session".into(),
                                label: i18n::cpush::cpush_option_keep_local_end(locale).into(),
                                recommended: None,
                            },
                        ]),
                        links: None,
                        card: None,
                        questionnaire: None,
                        progress: None,
                    });
                } else {
                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = has_remaining_commits;
                    let mut options = Vec::new();
                    if has_remaining_commits {
                        options.push(WorkflowMessageOption {
                            id: "retry-remaining".into(),
                            label: i18n::cpush::cpush_option_confirm_continue_remaining(locale)
                                .into(),
                            recommended: Some(true),
                        });
                    }
                    options.push(WorkflowMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush::cpush_option_dismiss_interrupted(
                            locale,
                            !session.commits.is_empty(),
                        )
                        .into(),
                        recommended: (!has_remaining_commits).then_some(true),
                    });
                    session.message = Some(WorkflowUiMessage {
                        id: format!("interrupted-{}", session.run_id),
                        kind: "error".into(),
                        title: i18n::cpush::cpush_interrupted_commits_title(
                            locale,
                            session.commits.len(),
                        ),
                        description: i18n::cpush::cpush_interrupted_commits_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(options),
                        links: None,
                        card: None,
                        questionnaire: None,
                        progress: None,
                    });
                }
            }

            if !(session.session_status == CPushSessionStatus::Done && session.notice_dismissed) {
                cpush_public.push(session.to_public());
            }
            reconciled_cpush_stored.push(session);
        }

        store.c_push_sessions = reconciled_cpush_stored.clone();
        if dirty {
            let _ = save_store_unlocked(storage_dir, store.clone());
        }

        // Keep at most the latest undismissed Custom Action session per (repository_id, worktree_id):
        store
            .sessions
            .sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
        let mut seen = HashSet::new();
        let mut action_sessions = Vec::new();
        for session in store.sessions {
            let key = (session.repository_id.clone(), session.worktree_id.clone());
            if !seen.insert(key) {
                continue;
            }
            let Some(msg) = build_restored_ui_message(&session, locale) else {
                continue;
            };
            if session.dismissed_message_id.as_deref() == Some(msg.id.as_str()) {
                continue;
            }
            action_sessions.push(ActionSessionSnapshot {
                run_id: session.run_id,
                repository_id: session.repository_id,
                worktree_id: session.worktree_id,
                action_id: session.action_id,
                action_label: session.action_label,
                status: session.status,
                summary: session.summary,
                message: Some(msg),
                timeline: session.timeline,
                commits: session.commits,
                links: session.links,
            });
        }

        Ok(StartupReconcileResult {
            action_sessions,
            cpush_stored: reconciled_cpush_stored,
            cpush_public,
            warning,
        })
    });

    result.unwrap_or(StartupReconcileResult {
        action_sessions: Vec::new(),
        cpush_stored: Vec::new(),
        cpush_public: Vec::new(),
        warning: None,
    })
}

#[cfg(test)]
pub fn reconcile_sessions_on_startup(
    storage_dir: &Path,
    locale: LanguageKind,
) -> Vec<ActionSessionSnapshot> {
    reconcile_all_sessions_on_startup(storage_dir, &[], locale).action_sessions
}

pub(crate) async fn reconcile_all_sessions_on_startup_async(
    storage_dir: &Path,
    snapshots: Vec<RepositorySnapshot>,
    locale: LanguageKind,
) -> StartupReconcileResult {
    let dir = storage_dir.to_path_buf();
    tokio::task::spawn_blocking(move || reconcile_all_sessions_on_startup(&dir, &snapshots, locale))
        .await
        .unwrap_or(StartupReconcileResult {
            action_sessions: Vec::new(),
            cpush_stored: Vec::new(),
            cpush_public: Vec::new(),
            warning: None,
        })
}

fn build_restored_ui_message(
    session: &PersistedActionSession,
    locale: LanguageKind,
) -> Option<WorkflowUiMessage> {
    let commit_summaries: Vec<WorkflowMessageCommitSummary> = session
        .commits
        .iter()
        .map(|c| WorkflowMessageCommitSummary {
            hash: c.short_oid.clone(),
            title: c.title.clone(),
        })
        .collect();
    match session.status.as_str() {
        "interrupted" => Some(WorkflowUiMessage {
            id: format!("interrupted-action-{}", session.run_id),
            kind: "error".into(),
            title: i18n::actions::action_interrupted_title(locale, &session.action_label),
            description: if session.summary.trim().is_empty() {
                i18n::actions::action_interrupted_summary(locale, &session.action_label)
            } else {
                session.summary.clone()
            },
            scope: None,
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
            options: None,
            links: (!session.links.is_empty()).then_some(session.links.clone()),
            card: session.card.clone(),
            questionnaire: None,
            progress: None,
        }),
        "done" => {
            let (title, description) = if let Some(ref card) = session.card {
                (
                    card.title.clone(),
                    card.subtitle
                        .clone()
                        .unwrap_or_else(|| session.summary.clone()),
                )
            } else {
                (
                    i18n::actions::action_completed_title(locale, &session.action_label),
                    session.summary.clone(),
                )
            };
            Some(WorkflowUiMessage {
                id: format!("done-action-{}", session.run_id),
                kind: "done".into(),
                title,
                description,
                scope: None,
                files: None,
                commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                options: None,
                links: (!session.links.is_empty()).then_some(session.links.clone()),
                card: session.card.clone(),
                questionnaire: None,
                progress: None,
            })
        }
        _ => None,
    }
}
