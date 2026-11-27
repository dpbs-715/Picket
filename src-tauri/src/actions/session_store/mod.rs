//! Unified local persistence and cross-restart reconciliation for Custom Actions,
//! `submit`, C-Push, and Pull-Rebase sessions.
//!
//! Persisted atomically in `~/.picket/action-sessions.json` (`0600`, schema v3 unified tagged
//! array) inside the `0700` data dir. Seamlessly migrates v2/v1 `action-sessions.json` and
//! legacy `~/.picket/cpush-sessions.json` (v1) on startup without losing commits or checkpoints.
//! Never stores API keys, `commitToken`s, `branchToken`s, or platform credentials.

mod reconcile;
mod schema;
mod storage_io;

pub(crate) use reconcile::reconcile_all_sessions_on_startup_async;
#[cfg(test)]
pub use reconcile::reconcile_sessions_on_startup;
#[cfg(test)]
pub use schema::ACTION_SESSIONS_FILE;
pub use schema::{
    ActionSessionSnapshot, PersistedActionSession, RetainedDuplicateEntry, SubmitCleanupRecord,
    SubmitGroupCheckpoint, SubmitPathPreflightSnapshot, SubmitPromotionCheckpoint,
    SubmitSessionLedger, SubmitWorktreeEntryState, now_millis, utc_timestamp_compact,
};

use self::storage_io::{
    apply_dismiss_unlocked, load_store_unlocked, save_store_unlocked, sessions_file_path,
};
use crate::{
    actions::{message_user::format_card_for_checkpoint, types::StoredCPushSession},
    i18n::LanguageKind,
    storage,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn load_all_sessions(storage_dir: &Path) -> Vec<PersistedActionSession> {
    let path = sessions_file_path(storage_dir);
    storage::with_file_lock(&path, || {
        Ok(load_store_unlocked(storage_dir, LanguageKind::En)
            .0
            .sessions)
    })
    .unwrap_or_default()
}

#[cfg(test)]
pub fn upsert_session(storage_dir: &Path, session: PersistedActionSession) -> Result<(), String> {
    let path = sessions_file_path(storage_dir);
    storage::with_file_lock(&path, || {
        let (mut store, _, _) = load_store_unlocked(storage_dir, LanguageKind::En);
        if let Some(existing) = store
            .sessions
            .iter_mut()
            .find(|s| s.run_id == session.run_id)
        {
            *existing = session;
        } else {
            store.sessions.push(session);
        }
        save_store_unlocked(storage_dir, store)
    })
}

pub async fn upsert_session_async(
    storage_dir: &Path,
    session: PersistedActionSession,
) -> Result<(), String> {
    let dir = storage_dir.to_path_buf();
    let path = sessions_file_path(&dir);
    storage::run_blocking_locked(path, move || {
        let (mut store, _, _) = load_store_unlocked(&dir, LanguageKind::En);
        if let Some(existing) = store
            .sessions
            .iter_mut()
            .find(|s| s.run_id == session.run_id)
        {
            *existing = session;
        } else {
            store.sessions.push(session);
        }
        save_store_unlocked(&dir, store)
    })
    .await
}

pub(crate) async fn save_cpush_sessions_async(
    storage_dir: &Path,
    cpush_sessions: Vec<StoredCPushSession>,
) -> Result<(), String> {
    let dir = storage_dir.to_path_buf();
    let path = sessions_file_path(&dir);
    storage::run_blocking_locked(path, move || {
        let (mut store, _, _) = load_store_unlocked(&dir, LanguageKind::En);
        store.c_push_sessions = cpush_sessions;
        save_store_unlocked(&dir, store)
    })
    .await
}

pub fn find_session_by_run_id(
    storage_dir: &Path,
    repository_id: &str,
    worktree_id: &str,
    run_id: &str,
) -> Option<PersistedActionSession> {
    load_all_sessions(storage_dir).into_iter().find(|s| {
        s.repository_id == repository_id && s.worktree_id == worktree_id && s.run_id == run_id
    })
}

pub fn find_latest_resumable_session(
    storage_dir: &Path,
    repository_id: &str,
    worktree_id: &str,
    action_id: Option<&str>,
) -> Option<PersistedActionSession> {
    let mut all = load_all_sessions(storage_dir);
    all.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    all.into_iter().find(|s| {
        s.repository_id == repository_id
            && s.worktree_id == worktree_id
            && action_id.map(|id| s.action_id == id).unwrap_or(true)
            && has_meaningful_checkpoint(s)
    })
}

pub fn has_meaningful_checkpoint(session: &PersistedActionSession) -> bool {
    !session.links.is_empty()
        || !session.commits.is_empty()
        || !session.edited_file_hashes.is_empty()
        || !session.submit_ledger.groups.is_empty()
        || session.submit_ledger.promotion.is_some()
        || session.submit_ledger.cleanup.is_some()
}

pub fn find_owned_worktree_group(
    storage_dir: &Path,
    repository_id: &str,
    candidate_path: &Path,
) -> Option<(PersistedActionSession, SubmitGroupCheckpoint)> {
    let canon_target =
        fs::canonicalize(candidate_path).unwrap_or_else(|_| candidate_path.to_path_buf());
    for session in load_all_sessions(storage_dir) {
        if session.repository_id != repository_id {
            continue;
        }
        for group in &session.submit_ledger.groups {
            if let Some(ref wt_path) = group.worktree_path {
                let recorded = PathBuf::from(wt_path);
                let canon_recorded = fs::canonicalize(&recorded).unwrap_or(recorded);
                if canon_recorded == canon_target {
                    return Some((session.clone(), group.clone()));
                }
            }
        }
    }
    None
}

#[cfg(test)]
pub fn dismiss_session_message(
    storage_dir: &Path,
    repository_id: &str,
    worktree_id: &str,
    run_id: Option<&str>,
    message_id: &str,
) -> Result<bool, String> {
    let path = sessions_file_path(storage_dir);
    storage::with_file_lock(&path, || {
        let (mut store, _, _) = load_store_unlocked(storage_dir, LanguageKind::En);
        let updated =
            apply_dismiss_unlocked(&mut store, repository_id, worktree_id, run_id, message_id);
        if updated {
            save_store_unlocked(storage_dir, store)?;
        }
        Ok(updated)
    })
}

pub async fn dismiss_session_message_async(
    storage_dir: &Path,
    repository_id: String,
    worktree_id: String,
    run_id: Option<String>,
    message_id: String,
) -> Result<bool, String> {
    let dir = storage_dir.to_path_buf();
    let path = sessions_file_path(&dir);
    storage::run_blocking_locked(path, move || {
        let (mut store, _, _) = load_store_unlocked(&dir, LanguageKind::En);
        let updated = apply_dismiss_unlocked(
            &mut store,
            &repository_id,
            &worktree_id,
            run_id.as_deref(),
            &message_id,
        );
        if updated {
            save_store_unlocked(&dir, store)?;
        }
        Ok(updated)
    })
    .await
}

/// Formats a compact, verified checkpoint summary for the Agent's `<previous_session_checkpoint>`
/// prompt block when resuming or re-running an action on the same repository/worktree.
pub fn format_resume_checkpoint_for_prompt(session: &PersistedActionSession) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Previous runId={} action=\"{}\" status={}",
        session.run_id, session.action_label, session.status
    ));
    if !session.summary.trim().is_empty() {
        lines.push(format!("Summary: {}", session.summary.trim()));
    }
    if let Some(ref card) = session.card {
        lines.extend(format_card_for_checkpoint(card));
    }
    if !session.submit_ledger.workflow_kind.is_empty() {
        lines.push(format!(
            "Workflow: {} (firstTarget={:?}, finalTarget={:?}, originalBranch={:?}, originalHead={:?})",
            session.submit_ledger.workflow_kind,
            session.submit_ledger.first_target,
            session.submit_ledger.final_target,
            session.submit_ledger.original_branch,
            session.submit_ledger.original_head,
        ));
    }
    if !session.submit_ledger.groups.is_empty() {
        lines.push("Recorded submit groups:".into());
        for group in &session.submit_ledger.groups {
            let wt_exists = group
                .worktree_path
                .as_deref()
                .map(|p| Path::new(p).exists())
                .unwrap_or(false);
            lines.push(format!(
                "  - group={} title=\"{}\" issue={:?} (closed={}) branch={:?} base={:?} remote={:?} headSha={:?} pr={:?} (state={:?}) worktree={:?} (exists={}, removed={}) paths={:?}",
                group.group_id,
                group.title,
                group.issue_url,
                group.issue_closed,
                group.branch,
                group.base_ref,
                group.remote,
                group.verified_head_sha,
                group.pr_url,
                group.pr_state,
                group.worktree_path,
                wt_exists,
                group.worktree_removed,
                group.transferred_paths,
            ));
        }
    }
    if let Some(ref promo) = session.submit_ledger.promotion {
        lines.push(format!(
            "Promotion PR/MR: {} -> {} url={:?} state={:?} headSha={:?}",
            promo.source_branch,
            promo.target_branch,
            promo.pr_url,
            promo.pr_state,
            promo.verified_head_sha,
        ));
    }
    if let Some(ref cleanup) = session.submit_ledger.cleanup {
        lines.push(format!(
            "Step 5 cleanup: backupDir={:?} cleanedPaths={:?} retained={}",
            cleanup.backup_dir,
            cleanup.cleaned_paths,
            cleanup.retained_paths.len(),
        ));
    }
    if !session.links.is_empty() {
        lines.push("Preserved links:".into());
        for link in &session.links {
            lines.push(format!("  - {}: {}", link.label, link.url));
        }
    }
    if !session.commits.is_empty() {
        lines.push("Recorded commits:".into());
        for c in &session.commits {
            lines.push(format!(
                "  - {} {} (branch={})",
                c.short_oid, c.title, c.branch
            ));
        }
    }
    if !session.edited_file_hashes.is_empty() {
        lines.push("Action-modified files recorded in previous run (available for commit_exact_scope if unchanged on disk):".into());
        for path in session.edited_file_hashes.keys() {
            lines.push(format!("  - {path}"));
        }
    }
    if !session.timeline.is_empty() {
        lines.push("Recent execution steps:".into());
        for step in session.timeline.iter().rev().take(8).rev() {
            lines.push(format!("  - [{}] {}", step.state, step.label));
        }
    }
    lines.join("\n")
}
