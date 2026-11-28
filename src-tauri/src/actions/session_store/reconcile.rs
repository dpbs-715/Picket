//! Cold-startup reconciliation for unified persisted Action Sessions against live Git state.

use super::{
    schema::now_millis,
    storage_io::{load_store_unlocked, save_store_unlocked, sessions_file_path},
};
use crate::{
    actions::{
        coordinator::session_helpers::{enter_conflict_state, summarize_commits},
        profile::ActionProfileKind,
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
    pub stored_sessions: Vec<StoredActionSession>,
    pub public_sessions: Vec<ActionSessionSnapshot>,
    pub warning: Option<String>,
}

/// On cold startup, reconciles all persisted workflow sessions (C-Push, Pull & Rebase, and Custom
/// Actions) in a single atomic locked pass over `~/.picket/action-sessions.json` (migrating v3/v2/v1
/// and legacy `cpush-sessions.json` if present) without rolling back or deleting any landed commits,
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
        let (mut sessions, warning, mut dirty) = load_store_unlocked(storage_dir, locale);
        let mut reconciled_stored: Vec<StoredActionSession> = Vec::with_capacity(sessions.len());

        for mut session in sessions.drain(..) {
            session.auto_collapse = false;
            session.pending_call_id = None;

            if session.profile == ActionProfileKind::CustomAction {
                if reconcile_custom_action_session(&mut session, &live_trees, locale) {
                    dirty = true;
                }
                reconciled_stored.push(session);
                continue;
            }

            // Built-in C-Push / Pull & Rebase session reconciliation:
            match reconcile_builtin_session(&mut session, &live_trees, locale) {
                BuiltinReconcileAction::Discard => {
                    dirty = true;
                }
                BuiltinReconcileAction::Keep { modified } => {
                    if modified {
                        dirty = true;
                    }
                    reconciled_stored.push(session);
                }
            }
        }

        if dirty {
            let _ = save_store_unlocked(storage_dir, reconciled_stored.clone());
        }

        // Project at most one visible, undismissed session per (repository_id, worktree_id),
        // giving active/recoverable sessions precedence over older completed ones.
        let mut ordered: Vec<&StoredActionSession> = reconciled_stored.iter().collect();
        ordered.sort_by_key(|s| (!s.status.is_active(), std::cmp::Reverse(s.updated_at_ms)));

        let mut seen = HashSet::new();
        let mut public_sessions = Vec::new();
        for session in ordered {
            let key = (session.repository_id.clone(), session.worktree_id.clone());
            if !seen.insert(key) {
                continue;
            }
            if let Some(snapshot) = project_visible_restored_snapshot(session, locale) {
                public_sessions.push(snapshot);
            }
        }

        Ok(StartupReconcileResult {
            stored_sessions: reconciled_stored,
            public_sessions,
            warning,
        })
    });

    result.unwrap_or(StartupReconcileResult {
        stored_sessions: Vec::new(),
        public_sessions: Vec::new(),
        warning: None,
    })
}

fn live_conflicts_for(
    live_trees: &HashMap<&str, &repository::types::WorktreeSnapshot>,
    worktree_id: &str,
) -> (Vec<ConflictedFileSummary>, Option<String>) {
    let Some(live_tree) = live_trees.get(worktree_id) else {
        return (Vec::new(), None);
    };
    let conflicts: Vec<ConflictedFileSummary> = live_tree
        .files
        .iter()
        .filter(|f| f.status == "U")
        .map(|f| ConflictedFileSummary {
            file_id: f.id.clone(),
            path: f.path.clone(),
        })
        .collect();
    let upstream = match &live_tree.remote {
        repository::types::Remote::Tracked { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    (conflicts, upstream)
}

fn reconcile_custom_action_session(
    session: &mut StoredActionSession,
    live_trees: &HashMap<&str, &repository::types::WorktreeSnapshot>,
    locale: LanguageKind,
) -> bool {
    let mut dirty = false;
    let is_live_process_wait = session.status == ActionSessionStatus::Running
        || (session.status == ActionSessionStatus::NeedsDecision
            && session
                .message
                .as_ref()
                .is_none_or(|m| m.questionnaire.is_some()));
    if is_live_process_wait {
        let (live_conflicts, upstream_opt) = live_conflicts_for(live_trees, &session.worktree_id);
        if !live_conflicts.is_empty() {
            session.locale = locale;
            let upstream = upstream_opt
                .as_deref()
                .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale));
            enter_conflict_state(
                session,
                upstream,
                &live_conflicts,
                !session.model_id.is_empty(),
                Some(i18n::cpush::cpush_restart_conflict_reason(locale)),
            );
            session.updated_at_ms = now_millis();
            dirty = true;
        } else {
            session.status = ActionSessionStatus::Interrupted;
            session.message = None;
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
    dirty
}

enum BuiltinReconcileAction {
    Discard,
    Keep { modified: bool },
}

fn reconcile_builtin_session(
    session: &mut StoredActionSession,
    live_trees: &HashMap<&str, &repository::types::WorktreeSnapshot>,
    locale: LanguageKind,
) -> BuiltinReconcileAction {
    // Live `message_user` questions from a dead process cannot resume their oneshot;
    // if no commits were landed yet, discard cleanly so the user is never stuck.
    if session.status == ActionSessionStatus::NeedsDecision
        && session.commits.is_empty()
        && session.planned_groups.is_empty()
        && session
            .message
            .as_ref()
            .is_some_and(|m| m.questionnaire.is_some())
    {
        return BuiltinReconcileAction::Discard;
    }

    if session.status != ActionSessionStatus::Running {
        return BuiltinReconcileAction::Keep { modified: false };
    }

    session.locale = locale;
    session.updated_at_ms = now_millis();
    if session.commits.is_empty() && session.planned_groups.is_empty() {
        let (conflicts, _) = live_conflicts_for(live_trees, &session.worktree_id);
        if conflicts.is_empty() {
            return BuiltinReconcileAction::Discard;
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
                    .unwrap_or_else(|| i18n::cpush::cpush_commit_completed_fallback(locale))
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
        let (live_conflicts, upstream_opt) = live_conflicts_for(live_trees, &session.worktree_id);
        if !live_conflicts.is_empty() {
            let upstream = upstream_opt
                .as_deref()
                .unwrap_or_else(|| i18n::repository::remote_branch_fallback(locale));
            enter_conflict_state(
                session,
                upstream,
                &live_conflicts,
                !session.model_id.is_empty(),
                Some(i18n::cpush::cpush_restart_conflict_reason(locale)),
            );
            return BuiltinReconcileAction::Keep { modified: true };
        }
    }

    let has_remaining_commits = session.next_step_index < session.planned_groups.len();
    let commit_summaries = summarize_commits(&session.commits);

    if !has_remaining_commits && !session.commits.is_empty() && session.push_authorized {
        session.status = ActionSessionStatus::PushFailed;
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
        session.status = ActionSessionStatus::NeedsDecision;
        session.can_retry_remaining = has_remaining_commits;
        let mut options = Vec::new();
        if has_remaining_commits {
            options.push(WorkflowMessageOption {
                id: "retry-remaining".into(),
                label: i18n::cpush::cpush_option_confirm_continue_remaining(locale).into(),
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
            title: i18n::cpush::cpush_interrupted_commits_title(locale, session.commits.len()),
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

    BuiltinReconcileAction::Keep { modified: true }
}

fn project_visible_restored_snapshot(
    session: &StoredActionSession,
    locale: LanguageKind,
) -> Option<ActionSessionSnapshot> {
    if session.status == ActionSessionStatus::Idle
        || session.status == ActionSessionStatus::Cancelled
        || (session.status == ActionSessionStatus::Done && session.notice_dismissed)
    {
        return None;
    }

    if session.message.is_some() {
        let msg_id = session.message.as_ref().map(|m| m.id.as_str());
        if msg_id.is_some() && session.dismissed_message_id.as_deref() == msg_id {
            return None;
        }
        return Some(session.to_public());
    }

    // Custom Actions persisted without a pre-rendered `message` (e.g. upgraded from v2/v3 or
    // freshly marked `Interrupted`) synthesize their restored UI message from card + summary.
    let msg = build_restored_ui_message(session, locale)?;
    if session.dismissed_message_id.as_deref() == Some(msg.id.as_str()) {
        return None;
    }
    let mut snapshot = session.to_public();
    snapshot.message = Some(msg);
    Some(snapshot)
}

#[cfg(test)]
pub fn reconcile_sessions_on_startup(
    storage_dir: &Path,
    locale: LanguageKind,
) -> Vec<ActionSessionSnapshot> {
    reconcile_all_sessions_on_startup(storage_dir, &[], locale).public_sessions
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
            stored_sessions: Vec::new(),
            public_sessions: Vec::new(),
            warning: None,
        })
}

fn build_restored_ui_message(
    session: &StoredActionSession,
    locale: LanguageKind,
) -> Option<WorkflowUiMessage> {
    let commit_summaries = summarize_commits(&session.commits);
    match session.status {
        ActionSessionStatus::Interrupted => Some(WorkflowUiMessage {
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
        ActionSessionStatus::Done => {
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
