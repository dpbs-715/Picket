//! Persisted schema types for unified Action Sessions (`action-sessions.json` v4)
//! plus lossless migration decoders for v3/v2/v1 and legacy `cpush-sessions.json`.

use crate::{
    actions::types::*,
    i18n::{LanguageKind, deserialize_language_or_default},
    repository::ai_context::FrozenCPushFile,
    time,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ACTION_SESSIONS_FILE: &str = "action-sessions.json";
pub(super) const LEGACY_CPUSH_SESSIONS_FILE: &str = "cpush-sessions.json";
pub(super) const ACTION_SESSIONS_VERSION: u8 = 4;
pub(super) const MAX_STORED_SESSIONS: usize = 128;

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

/// On-disk v4 schema for `~/.picket/action-sessions.json`: single flat array of unified sessions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredActionSessionsFileV4 {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<StoredActionSession>,
}

// ---------- Lossless migration decoders for v1 / v2 / v3 / legacy cpush-sessions.json ----------

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyPersistedActionSession {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub action_id: String,
    pub action_label: String,
    #[serde(default)]
    pub action_prompt_hash: String,
    pub status: String,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub card: Option<MessageUserCard>,
    #[serde(default)]
    pub dismissed_message_id: Option<String>,
    #[serde(default)]
    pub timeline: Vec<WorkflowTimelineItem>,
    #[serde(default)]
    pub commits: Vec<CPushLandedCommit>,
    #[serde(default)]
    pub links: Vec<WorkflowLink>,
    #[serde(default)]
    pub edited_file_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub submit_ledger: SubmitSessionLedger,
}

impl From<LegacyPersistedActionSession> for StoredActionSession {
    fn from(old: LegacyPersistedActionSession) -> Self {
        let status = match old.status.as_str() {
            "running" => ActionSessionStatus::Running,
            "needsDecision" => ActionSessionStatus::NeedsDecision,
            "conflict" => ActionSessionStatus::Conflict,
            "waitingManual" => ActionSessionStatus::WaitingManual,
            "pushFailed" => ActionSessionStatus::PushFailed,
            "interrupted" => ActionSessionStatus::Interrupted,
            "done" => ActionSessionStatus::Done,
            "cancelled" => ActionSessionStatus::Cancelled,
            "failed" => ActionSessionStatus::Failed,
            _ => ActionSessionStatus::Idle,
        };
        Self {
            run_id: old.run_id,
            repository_id: old.repository_id,
            worktree_id: old.worktree_id,
            session_id: String::new(),
            action_id: old.action_id,
            action_label: old.action_label,
            profile: ActionProfileKind::CustomAction,
            action_prompt_hash: old.action_prompt_hash,
            frozen_scope_version: String::new(),
            model_id: String::new(),
            branch: String::new(),
            initial_head: None,
            prompt: String::new(),
            recent_commits: Vec::new(),
            unselected_paths: Vec::new(),
            frozen_files: Vec::new(),
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: Vec::new(),
            status,
            pending_call_id: None,
            updated_at_ms: old.updated_at_ms,
            summary: old.summary,
            card: old.card,
            message: None,
            timeline: old.timeline,
            qa_log: Vec::new(),
            commits: old.commits,
            links: old.links,
            can_retry_remaining: false,
            push_authorized: false,
            auto_collapse: false,
            notice_dismissed: status == ActionSessionStatus::Done
                && old.dismissed_message_id.is_some(),
            dismissed_message_id: old.dismissed_message_id,
            edited_file_hashes: old.edited_file_hashes,
            submit_ledger: old.submit_ledger,
            locale: LanguageKind::En,
            commit_language: LanguageKind::En,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyStoredCPushSession {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    #[serde(default)]
    pub frozen_scope_version: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub initial_head: Option<String>,
    #[serde(default, alias = "draft")]
    pub prompt: String,
    #[serde(default)]
    pub recent_commits: Vec<String>,
    #[serde(default)]
    pub unselected_paths: Vec<String>,
    #[serde(default)]
    pub frozen_files: Vec<FrozenCPushFile>,
    #[serde(default)]
    pub planned_groups: Vec<CPushCommitGroup>,
    #[serde(default)]
    pub skipped_file_ids: Vec<String>,
    #[serde(default)]
    pub next_step_index: usize,
    #[serde(default)]
    pub committed_file_ids: Vec<String>,
    #[serde(default)]
    pub commits: Vec<CPushLandedCommit>,
    #[serde(default)]
    pub qa_log: Vec<CPushQaItem>,
    #[serde(default)]
    pub timeline: Vec<WorkflowTimelineItem>,
    pub session_status: ActionSessionStatus,
    #[serde(default)]
    pub message: Option<WorkflowUiMessage>,
    #[serde(default)]
    pub can_retry_remaining: bool,
    #[serde(default = "default_true")]
    pub push_authorized: bool,
    #[serde(default)]
    pub auto_collapse: bool,
    #[serde(default)]
    pub notice_dismissed: bool,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub commit_language: LanguageKind,
}

impl From<LegacyStoredCPushSession> for StoredActionSession {
    fn from(old: LegacyStoredCPushSession) -> Self {
        let is_pull_only =
            !old.push_authorized && old.frozen_files.is_empty() && old.commits.is_empty();
        let (action_id, action_label, profile) = if is_pull_only {
            (
                BUILTIN_PULL_REBASE_ACTION_ID.to_owned(),
                "Pull & Rebase".to_owned(),
                ActionProfileKind::PullRebase,
            )
        } else {
            (
                BUILTIN_CPUSH_ACTION_ID.to_owned(),
                "C-Push".to_owned(),
                ActionProfileKind::Cpush,
            )
        };
        let summary = old
            .message
            .as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_default();
        Self {
            run_id: old.run_id,
            repository_id: old.repository_id,
            worktree_id: old.worktree_id,
            session_id: old.session_id,
            action_id,
            action_label,
            profile,
            action_prompt_hash: String::new(),
            frozen_scope_version: old.frozen_scope_version,
            model_id: old.model_id,
            branch: old.branch,
            initial_head: old.initial_head,
            prompt: old.prompt,
            recent_commits: old.recent_commits,
            unselected_paths: old.unselected_paths,
            frozen_files: old.frozen_files,
            planned_groups: old.planned_groups,
            skipped_file_ids: old.skipped_file_ids,
            next_step_index: old.next_step_index,
            committed_file_ids: old.committed_file_ids,
            status: old.session_status,
            pending_call_id: None,
            updated_at_ms: now_millis(),
            summary,
            card: old.message.as_ref().and_then(|m| m.card.clone()),
            message: old.message,
            timeline: old.timeline,
            qa_log: old.qa_log,
            commits: old.commits,
            links: Vec::new(),
            can_retry_remaining: old.can_retry_remaining,
            push_authorized: old.push_authorized,
            auto_collapse: old.auto_collapse,
            notice_dismissed: old.notice_dismissed,
            dismissed_message_id: None,
            edited_file_hashes: BTreeMap::new(),
            submit_ledger: SubmitSessionLedger::default(),
            locale: old.locale,
            commit_language: old.commit_language,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum LegacyWorkflowSessionV3 {
    CustomAction(Box<LegacyPersistedActionSession>),
    Cpush(Box<LegacyStoredCPushSession>),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyActionSessionsFileV3 {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<LegacyWorkflowSessionV3>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyActionSessionsFileV2 {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<LegacyPersistedActionSession>,
    #[serde(default)]
    pub(super) c_push_sessions: Vec<LegacyStoredCPushSession>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyCPushSessionsFile {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<LegacyStoredCPushSession>,
}
