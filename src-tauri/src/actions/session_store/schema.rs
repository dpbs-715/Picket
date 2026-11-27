//! Persisted schema types for Custom Action, `submit`, C-Push, and Pull-Rebase sessions.

use crate::{actions::types::*, time};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ACTION_SESSIONS_FILE: &str = "action-sessions.json";
pub(super) const LEGACY_CPUSH_SESSIONS_FILE: &str = "cpush-sessions.json";
pub(super) const ACTION_SESSIONS_VERSION: u8 = 3;
pub(super) const MAX_STORED_SESSIONS: usize = 64;
pub(super) const MAX_STORED_CPUSH_SESSIONS: usize = 128;

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
    pub timeline: Vec<WorkflowTimelineItem>,
    #[serde(default)]
    pub commits: Vec<CPushLandedCommit>,
    #[serde(default)]
    pub links: Vec<WorkflowLink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub edited_file_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub submit_ledger: SubmitSessionLedger,
}

/// Lightweight UI-facing cold-restore snapshot sent inside `WorkbenchRestoreReply.actionSessions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ActionSessionSnapshot {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub action_id: String,
    pub action_label: String,
    /// `"interrupted"`, `"done"`, `"cancelled"`, or `"failed"`.
    #[cfg_attr(test, ts(type = "'interrupted' | 'done' | 'cancelled' | 'failed'"))]
    pub status: String,
    pub summary: String,
    pub message: Option<WorkflowUiMessage>,
    pub timeline: Vec<WorkflowTimelineItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub links: Vec<WorkflowLink>,
}

/// Unified tagged workflow session record persisted in `~/.picket/action-sessions.json` (v3).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum PersistedWorkflowSession {
    CustomAction(Box<PersistedActionSession>),
    Cpush(Box<StoredCPushSession>),
}

/// On-disk v3 schema for `~/.picket/action-sessions.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredActionSessionsFileV3 {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<PersistedWorkflowSession>,
}

/// Legacy v1 / v2 schema for `~/.picket/action-sessions.json` (losslessly upgraded to v3).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyActionSessionsFileV2 {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<PersistedActionSession>,
    #[serde(default)]
    pub(super) c_push_sessions: Vec<StoredCPushSession>,
}

/// In-memory view partitioned by workflow kind for storage I/O and startup reconciliation.
#[derive(Clone, Debug, Default)]
pub(super) struct StoredActionSessionsFile {
    pub(super) sessions: Vec<PersistedActionSession>,
    pub(super) c_push_sessions: Vec<StoredCPushSession>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyCPushSessionsFile {
    pub(super) version: u8,
    #[serde(default)]
    pub(super) sessions: Vec<StoredCPushSession>,
}
