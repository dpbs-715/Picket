//! Local persistence and cross-restart reconciliation for custom action & `submit` sessions.
//!
//! Persisted atomically in `~/.picket/action-sessions.json` (`0600`) inside the `0700` data dir.
//! Never stores API keys, `commitToken`s, `branchToken`s, or platform credentials; stores only
//! non-secret checkpoints (Issues, PR/MR URLs, branches, verified commit SHAs, owned isolated
//! worktree paths, and preflight path hashes for Step 5 verified duplicate cleanup).

use super::{
    message_user::format_card_for_checkpoint,
    types::{
        CustomActionLink, CustomActionMessageCommitSummary, CustomActionTimelineItem,
        CustomActionUiMessage, MessageUserCard,
    },
};
use crate::{
    cpush::types::CPushLandedCommit,
    i18n::{self, LanguageKind},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

pub const ACTION_SESSIONS_FILE: &str = "action-sessions.json";
const ACTION_SESSIONS_VERSION: u8 = 1;
const MAX_STORED_SESSIONS: usize = 64;

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Formats a UTC timestamp `YYYYMMDDTHHMMSSZ` without external dependencies.
pub fn utc_timestamp_compact() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;

    // Howard Hinnant's civil-from-days algorithm (Gregorian calendar).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}{month:02}{day:02}T{hour:02}{min:02}{sec:02}Z")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SubmitWorktreeEntryState {
    File { mode: u32, sha256: String },
    Symlink { target: String },
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitPathPreflightSnapshot {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    /// `"partial"`, `"staged"`, `"unstaged"`, `"untracked"`, or `"deleted"`.
    pub range_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_oid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_oid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_sha256: Option<String>,
    pub worktree_state: SubmitWorktreeEntryState,
    /// Exact content transferred into the isolated submit worktree for remote-tip verification.
    pub submitted_state: SubmitWorktreeEntryState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitGroupCheckpoint {
    pub group_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_number: Option<String>,
    #[serde(default)]
    pub issue_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub worktree_removed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<String>,
    /// `"open"`, `"merged"`, `"closed"`, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_state: Option<String>,
    #[serde(default)]
    pub transferred_paths: Vec<String>,
    #[serde(default)]
    pub preflight_snapshots: Vec<SubmitPathPreflightSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitPromotionCheckpoint {
    pub source_branch: String,
    pub target_branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_head_sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedDuplicateEntry {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitCleanupRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_dir: Option<String>,
    #[serde(default)]
    pub cleaned_paths: Vec<String>,
    #[serde(default)]
    pub retained_paths: Vec<RetainedDuplicateEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitSessionLedger {
    /// `"custom"`, `"submit"`, `"submitMerge"`, or `"submitPromote"`.
    #[serde(default)]
    pub workflow_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_head: Option<String>,
    #[serde(default)]
    pub groups: Vec<SubmitGroupCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion: Option<SubmitPromotionCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<SubmitCleanupRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedActionSession {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub action_id: String,
    pub action_label: String,
    pub action_prompt_hash: String,
    /// `"running"`, `"needsDecision"`, `"interrupted"`, `"done"`, `"cancelled"`, or `"failed"`.
    pub status: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<MessageUserCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_message_id: Option<String>,
    #[serde(default)]
    pub timeline: Vec<CustomActionTimelineItem>,
    #[serde(default)]
    pub commits: Vec<CPushLandedCommit>,
    #[serde(default)]
    pub links: Vec<CustomActionLink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub edited_file_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub submit_ledger: SubmitSessionLedger,
}

/// Lightweight UI-facing cold-restore snapshot sent inside `WorkbenchRestoreReply.actionSessions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSessionSnapshot {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub action_id: String,
    pub action_label: String,
    /// `"interrupted"`, `"done"`, `"cancelled"`, or `"failed"`.
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<CustomActionUiMessage>,
    pub timeline: Vec<CustomActionTimelineItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub links: Vec<CustomActionLink>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredActionSessionsFile {
    version: u8,
    #[serde(default)]
    sessions: Vec<PersistedActionSession>,
}

fn sessions_file_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(ACTION_SESSIONS_FILE)
}

fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|err| format!("Failed to create Picket storage directory: {err}"))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|err| {
        format!("Failed to set 0700 permissions on Picket storage directory: {err}")
    })?;
    Ok(())
}

pub fn load_all_sessions(storage_dir: &Path) -> Vec<PersistedActionSession> {
    let path = sessions_file_path(storage_dir);
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<StoredActionSessionsFile>(&raw) else {
        return Vec::new();
    };
    if parsed.version != ACTION_SESSIONS_VERSION {
        return Vec::new();
    }
    parsed.sessions
}

pub fn save_all_sessions(
    storage_dir: &Path,
    mut sessions: Vec<PersistedActionSession>,
) -> Result<(), String> {
    ensure_private_dir(storage_dir)?;
    sessions.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    sessions.truncate(MAX_STORED_SESSIONS);

    let payload = StoredActionSessionsFile {
        version: ACTION_SESSIONS_VERSION,
        sessions,
    };
    let encoded = serde_json::to_vec_pretty(&payload)
        .map_err(|err| format!("Failed to serialize action sessions: {err}"))?;

    let mut temp = NamedTempFile::new_in(storage_dir)
        .map_err(|err| format!("Failed to create temp action-sessions file: {err}"))?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("Failed to set 0600 permissions on action-sessions file: {err}"))?;
    temp.write_all(&encoded)
        .map_err(|err| format!("Failed to write action-sessions file: {err}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|err| format!("Failed to sync action-sessions file: {err}"))?;
    temp.persist(sessions_file_path(storage_dir))
        .map_err(|err| format!("Failed to persist action-sessions file: {err}"))?;
    Ok(())
}

pub fn upsert_session(storage_dir: &Path, session: PersistedActionSession) -> Result<(), String> {
    let mut all = load_all_sessions(storage_dir);
    if let Some(existing) = all.iter_mut().find(|s| s.run_id == session.run_id) {
        *existing = session;
    } else {
        all.push(session);
    }
    save_all_sessions(storage_dir, all)
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

/// On cold startup, transitions any stale `"running"` / `"needsDecision"` sessions to
/// `"interrupted"` without rolling back or deleting any created Issues, branches, PRs, or
/// isolated worktrees, reconciles whether recorded isolated worktree directories still exist,
/// and returns undismissed snapshots for the UI.
pub fn reconcile_sessions_on_startup(
    storage_dir: &Path,
    locale: LanguageKind,
) -> Vec<ActionSessionSnapshot> {
    let mut all = load_all_sessions(storage_dir);
    if all.is_empty() {
        return Vec::new();
    }
    let mut changed = false;
    for session in &mut all {
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
            changed = true;
        }
        for group in &mut session.submit_ledger.groups {
            if let Some(ref wt_path) = group.worktree_path
                && !group.worktree_removed
                && !Path::new(wt_path).exists()
            {
                group.worktree_removed = true;
                changed = true;
            }
        }
    }
    if changed {
        let _ = save_all_sessions(storage_dir, all.clone());
    }

    // Keep at most the latest undismissed session per (repository_id, worktree_id).
    all.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for session in all {
        let key = (session.repository_id.clone(), session.worktree_id.clone());
        if !seen.insert(key) {
            continue;
        }
        let message = build_restored_ui_message(&session, locale);
        let Some(msg) = message else {
            continue;
        };
        if session.dismissed_message_id.as_deref() == Some(msg.id.as_str()) {
            continue;
        }
        out.push(ActionSessionSnapshot {
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
    out
}

fn build_restored_ui_message(
    session: &PersistedActionSession,
    locale: LanguageKind,
) -> Option<CustomActionUiMessage> {
    let commit_summaries: Vec<CustomActionMessageCommitSummary> = session
        .commits
        .iter()
        .map(|c| CustomActionMessageCommitSummary {
            hash: c.short_oid.clone(),
            title: c.title.clone(),
        })
        .collect();
    match session.status.as_str() {
        "interrupted" => Some(CustomActionUiMessage {
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
            Some(CustomActionUiMessage {
                id: format!("done-action-{}", session.run_id),
                kind: "done".into(),
                title,
                description,
                scope: None,
                files: None,
                commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                links: (!session.links.is_empty()).then_some(session.links.clone()),
                card: session.card.clone(),
                questionnaire: None,
                progress: None,
            })
        }
        _ => None,
    }
}

pub fn dismiss_session_message(
    storage_dir: &Path,
    repository_id: &str,
    worktree_id: &str,
    run_id: Option<&str>,
    message_id: &str,
) -> Result<(), String> {
    let mut all = load_all_sessions(storage_dir);
    let mut updated = false;
    for session in &mut all {
        if session.repository_id != repository_id || session.worktree_id != worktree_id {
            continue;
        }
        if let Some(target_run) = run_id
            && session.run_id != target_run
        {
            continue;
        }
        session.dismissed_message_id = Some(message_id.to_owned());
        updated = true;
    }
    if updated {
        save_all_sessions(storage_dir, all)?;
    }
    Ok(())
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
