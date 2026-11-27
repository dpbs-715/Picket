//! Unified local persistence and cross-restart reconciliation for Custom Actions,
//! `submit`, C-Push, and Pull-Rebase sessions.
//!
//! Persisted atomically in `~/.picket/action-sessions.json` (`0600`, schema v2) inside the
//! `0700` data dir. Seamlessly migrates legacy `~/.picket/cpush-sessions.json` (v1) and
//! `action-sessions.json` (v1) on startup without losing completed commits or checkpoints.
//! Never stores API keys, `commitToken`s, `branchToken`s, or platform credentials.

use super::{
    coordinator::cpush_helpers::{enter_conflict_state, summarize_commits},
    message_user::format_card_for_checkpoint,
    types::*,
};
use crate::{
    i18n::{self, LanguageKind},
    repository::{self, remote::ConflictedFileSummary, types::RepositorySnapshot},
    storage, time,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

pub const ACTION_SESSIONS_FILE: &str = "action-sessions.json";
const LEGACY_CPUSH_SESSIONS_FILE: &str = "cpush-sessions.json";
const ACTION_SESSIONS_VERSION: u8 = 2;
const MAX_STORED_SESSIONS: usize = 64;
const MAX_STORED_CPUSH_SESSIONS: usize = 128;

pub fn now_millis() -> u64 {
    time::now_millis()
}

/// Formats a UTC timestamp `YYYYMMDDTHHMMSSZ` without external dependencies.
pub fn utc_timestamp_compact() -> String {
    time::now_utc_compact()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SubmitWorktreeEntryState {
    File { mode: u32, sha256: String },
    Symlink { target: String },
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct RetainedDuplicateEntry {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitCleanupRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_dir: Option<String>,
    #[serde(default)]
    pub cleaned_paths: Vec<String>,
    #[serde(default)]
    pub retained_paths: Vec<RetainedDuplicateEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
struct StoredActionSessionsFile {
    version: u8,
    #[serde(default)]
    sessions: Vec<PersistedActionSession>,
    #[serde(default)]
    c_push_sessions: Vec<StoredCPushSession>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyCPushSessionsFile {
    version: u8,
    #[serde(default)]
    sessions: Vec<StoredCPushSession>,
}

fn sessions_file_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(ACTION_SESSIONS_FILE)
}

fn legacy_cpush_file_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(LEGACY_CPUSH_SESSIONS_FILE)
}

fn load_store_unlocked(
    storage_dir: &Path,
    locale: LanguageKind,
) -> (StoredActionSessionsFile, Option<String>, bool) {
    let path = sessions_file_path(storage_dir);
    let mut warning: Option<String> = None;
    let mut needs_write = false;

    let mut store = match fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<StoredActionSessionsFile>(&bytes) {
            Ok(parsed) if parsed.version == 1 || parsed.version == ACTION_SESSIONS_VERSION => {
                if parsed.version != ACTION_SESSIONS_VERSION {
                    needs_write = true;
                }
                parsed
            }
            _ => {
                storage::backup_corrupt_copy(&path, &bytes);
                warning = Some(i18n::cpush::cpush_sessions_corrupted_warning(locale).into());
                StoredActionSessionsFile {
                    version: ACTION_SESSIONS_VERSION,
                    ..StoredActionSessionsFile::default()
                }
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => StoredActionSessionsFile {
            version: ACTION_SESSIONS_VERSION,
            ..StoredActionSessionsFile::default()
        },
        Err(_) => {
            warning = Some(i18n::cpush::cpush_sessions_unreadable_warning(locale).into());
            StoredActionSessionsFile {
                version: ACTION_SESSIONS_VERSION,
                ..StoredActionSessionsFile::default()
            }
        }
    };

    // One-time graceful migration from legacy `~/.picket/cpush-sessions.json` (v1):
    let legacy_path = legacy_cpush_file_path(storage_dir);
    if legacy_path.exists() {
        match fs::read(&legacy_path) {
            Ok(bytes) => match serde_json::from_slice::<LegacyCPushSessionsFile>(&bytes) {
                Ok(legacy) if legacy.version == 1 => {
                    let existing_wt: HashSet<String> = store
                        .c_push_sessions
                        .iter()
                        .map(|s| s.worktree_id.clone())
                        .collect();
                    for item in legacy.sessions {
                        if !existing_wt.contains(&item.worktree_id) {
                            store.c_push_sessions.push(item);
                        }
                    }
                    needs_write = true;
                    if save_store_unlocked(storage_dir, store.clone()).is_ok() {
                        let _ = fs::remove_file(&legacy_path);
                        needs_write = false;
                    }
                }
                _ => {
                    storage::backup_corrupt_copy(&legacy_path, &bytes);
                    let _ = fs::remove_file(&legacy_path);
                    if warning.is_none() {
                        warning =
                            Some(i18n::cpush::cpush_sessions_corrupted_warning(locale).into());
                    }
                }
            },
            Err(_) => {
                if warning.is_none() {
                    warning = Some(i18n::cpush::cpush_sessions_unreadable_warning(locale).into());
                }
            }
        }
    }

    store.version = ACTION_SESSIONS_VERSION;
    (store, warning, needs_write)
}

fn save_store_unlocked(
    storage_dir: &Path,
    mut store: StoredActionSessionsFile,
) -> Result<(), String> {
    store.version = ACTION_SESSIONS_VERSION;
    store
        .sessions
        .sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    store.sessions.truncate(MAX_STORED_SESSIONS);
    store.c_push_sessions.truncate(MAX_STORED_CPUSH_SESSIONS);

    storage::write_json_atomic(storage_dir, &sessions_file_path(storage_dir), &store, false)
}

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
                    session.message = Some(CPushUiMessage {
                        id: format!("interrupted-push-{}", session.run_id),
                        kind: "pushFailed".into(),
                        title: i18n::cpush::cpush_interrupted_push_title(locale).into(),
                        description: i18n::cpush::cpush_interrupted_push_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            CPushMessageOption {
                                id: "retry-push".into(),
                                label: i18n::cpush::cpush_option_retry_push(locale).into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
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
                        options.push(CPushMessageOption {
                            id: "retry-remaining".into(),
                            label: i18n::cpush::cpush_option_confirm_continue_remaining(locale)
                                .into(),
                            recommended: Some(true),
                        });
                    }
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush::cpush_option_dismiss_interrupted(
                            locale,
                            !session.commits.is_empty(),
                        )
                        .into(),
                        recommended: (!has_remaining_commits).then_some(true),
                    });
                    session.message = Some(CPushUiMessage {
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

fn apply_dismiss_unlocked(
    store: &mut StoredActionSessionsFile,
    repository_id: &str,
    worktree_id: &str,
    run_id: Option<&str>,
    message_id: &str,
) -> bool {
    let clean_run = run_id.map(str::trim).filter(|id| !id.is_empty());
    let clean_msg = message_id.trim();
    let mut updated = false;

    for session in &mut store.sessions {
        if session.repository_id != repository_id || session.worktree_id != worktree_id {
            continue;
        }
        if let Some(target_run) = clean_run
            && session.run_id != target_run
        {
            continue;
        }
        session.dismissed_message_id = Some(message_id.to_owned());
        updated = true;
    }

    for session in &mut store.c_push_sessions {
        if session.repository_id != repository_id
            || session.worktree_id != worktree_id
            || session.session_status != CPushSessionStatus::Done
        {
            continue;
        }
        if let Some(target_run) = clean_run
            && session.run_id != target_run
        {
            continue;
        }
        if let Some(msg) = &session.message
            && !clean_msg.is_empty()
            && msg.id != clean_msg
        {
            continue;
        }
        if session.notice_dismissed && session.message.is_none() {
            continue;
        }
        session.notice_dismissed = true;
        session.message = None;
        updated = true;
    }

    updated
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
