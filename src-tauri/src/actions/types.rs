//! Shared IPC types for unified built-in & custom actions, controlled CLI environment inspection,
//! structured `message_user` communication cards & Q&A round-trips, zero-token Host Recovery,
//! and Pi-guided `Create` button generation.

#[cfg(test)]
pub use super::message_user::MessageUserAnswer;
pub use super::message_user::{
    MessageQuestionContext, MessageQuestionItem, MessageQuestionOption, MessageTone,
    MessageUserCard, MessageUserRequest, MessageUserResponse,
};
pub use super::profile::{
    ActionProfileKind, BUILTIN_CPUSH_ACTION_ID, BUILTIN_PULL_REBASE_ACTION_ID,
};
use crate::{
    actions::session_store::SubmitSessionLedger,
    i18n::{LanguageKind, deserialize_language_or_default},
    repository::{
        ai_context::FrozenCPushFile,
        types::{CommitSelection, PersistedCustomAction, ReadTarget, RepositorySnapshot},
    },
};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const ACTION_PROTOCOL_VERSION: u8 = 2;
pub const CUSTOM_ACTION_PROTOCOL_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ActionApprovalMode {
    Ask,
    #[default]
    Auto,
    AllowAll,
}

pub fn deserialize_approval_mode_or_default<'de, D>(
    deserializer: D,
) -> Result<ActionApprovalMode, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(match raw.as_deref() {
        Some("ask") => ActionApprovalMode::Ask,
        Some("allowAll") => ActionApprovalMode::AllowAll,
        _ => ActionApprovalMode::Auto,
    })
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ActionSessionStatus {
    #[default]
    Idle,
    Running,
    NeedsDecision,
    Conflict,
    WaitingManual,
    PushFailed,
    Interrupted,
    Done,
    Cancelled,
    Failed,
}

impl ActionSessionStatus {
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Running
                | Self::NeedsDecision
                | Self::Conflict
                | Self::WaitingManual
                | Self::PushFailed
        )
    }

    pub const fn is_interactive_recovery(self) -> bool {
        matches!(
            self,
            Self::NeedsDecision | Self::Conflict | Self::WaitingManual | Self::PushFailed
        )
    }

    pub const fn is_reconcilable(self) -> bool {
        matches!(
            self,
            Self::WaitingManual | Self::Conflict | Self::PushFailed
        )
    }

    pub const fn to_tray_status(self) -> crate::tray::TraySessionStatus {
        use crate::tray::TraySessionStatus;
        match self {
            Self::Running => TraySessionStatus::Running,
            Self::NeedsDecision => TraySessionStatus::NeedsDecision,
            Self::Conflict => TraySessionStatus::Conflict,
            Self::WaitingManual => TraySessionStatus::WaitingManual,
            Self::PushFailed => TraySessionStatus::PushFailed,
            Self::Done => TraySessionStatus::Done,
            Self::Idle | Self::Interrupted | Self::Cancelled | Self::Failed => {
                TraySessionStatus::Idle
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CPushCommitGroup {
    pub id: String,
    pub message: String,
    pub file_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct CPushQaItem {
    pub question_id: String,
    pub question: String,
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CPushLandedCommit {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
    pub file_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct WorkflowMessageOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
}

pub type CPushMessageOption = WorkflowMessageOption;

pub fn compute_prompt_hash(prompt: &str) -> String {
    let normalized = prompt.replace("\r\n", "\n");
    format!("{:x}", Sha256::digest(normalized.trim().as_bytes()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTimelineItem {
    #[cfg_attr(test, ts(type = "'done' | 'active' | 'failed'"))]
    pub state: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowMessageFileChip {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowMessageCommitSummary {
    pub hash: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowMessageProgress {
    pub completed: u32,
    pub total: u32,
    pub label: String,
}

/// Unified UI message model shared across all built-in and custom action workflows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct WorkflowUiMessage {
    pub id: String,
    #[cfg_attr(
        test,
        ts(
            type = "'progress' | 'question' | 'conflict' | 'waitingManual' | 'resolved' | 'pushFailed' | 'done' | 'error'"
        )
    )]
    pub kind: String,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<WorkflowMessageFileChip>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commits: Option<Vec<WorkflowMessageCommitSummary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<WorkflowMessageOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<WorkflowLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<MessageUserCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questionnaire: Option<MessageUserRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<WorkflowMessageProgress>,
}

pub type CustomActionLink = WorkflowLink;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedActionUpdate {
    pub repository_id: String,
    pub action_id: String,
    pub trusted_prompt_hash: String,
}

/// Unified request to start either a built-in preset (`profile: Cpush`) or a user Custom Action
/// (`profile: CustomAction`) on the shared action coordinator pipeline.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartActionRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    #[serde(default)]
    pub profile: ActionProfileKind,
    #[serde(default)]
    pub action: Option<PersistedCustomAction>,
    #[serde(default)]
    pub selections: Vec<CommitSelection>,
    #[serde(default)]
    pub extra_prompt: String,
    #[serde(default)]
    pub push: Option<bool>,
    #[serde(default)]
    pub follow_up_prompt: Option<String>,
    #[serde(default)]
    pub resume_from_run_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub commit_language: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_approval_mode_or_default")]
    #[cfg_attr(test, ts(as = "Option<ActionApprovalMode>", optional))]
    pub approval_mode: ActionApprovalMode,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RespondActionQuestionRequest {
    pub request_id: String,
    pub run_id: String,
    pub call_id: String,
    pub response: MessageUserResponse,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

/// Strongly typed zero-token Host Recovery & Pull-Rebase actions shared across all workflows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionRecoveryAction {
    #[serde(rename_all = "camelCase")]
    RetryPush {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        set_upstream_remote: Option<String>,
    },
    RetryRemainingCommits,
    PullRebase,
    AbortRebase,
    ManualMerge,
    #[serde(rename_all = "camelCase")]
    AgentConflictMerge {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instruction: Option<String>,
    },
    SetUpstreamFreeText {
        text: String,
    },
    DismissSession,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRecoveryRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    pub action: SessionRecoveryAction,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub locale: Option<LanguageKind>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconcileSessionRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub locale: Option<LanguageKind>,
}

/// Single UI-facing session snapshot used across live progress, settled replies, reconciliation,
/// and cold-startup restoration (`WorkbenchRestoreReply.actionSessions`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ActionSessionSnapshot {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub action_id: String,
    pub action_label: String,
    pub profile: ActionProfileKind,
    pub frozen_scope_version: String,
    pub model_id: String,
    pub frozen_file_ids: Vec<String>,
    pub committed_file_ids: Vec<String>,
    pub skipped_file_ids: Vec<String>,
    pub status: ActionSessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pending_call_id: Option<String>,
    pub summary: String,
    pub message: Option<WorkflowUiMessage>,
    pub timeline: Vec<WorkflowTimelineItem>,
    pub qa_log: Vec<CPushQaItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub links: Vec<WorkflowLink>,
    pub can_retry_remaining: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub auto_collapse: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ActionProgressEvent {
    #[cfg_attr(test, ts(type = "2"))]
    pub protocol_version: u8,
    pub session: ActionSessionSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub trusted_action_update: Option<TrustedActionUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub repository_snapshot: Option<RepositorySnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ActionCommandReply {
    #[cfg_attr(test, ts(type = "2"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'action'"))]
    pub kind: &'static str,
    pub request_id: String,
    pub session: ActionSessionSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub trusted_action_update: Option<TrustedActionUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub repository_snapshot: Option<RepositorySnapshot>,
}

/// Unified runtime & persisted session record stored in `~/.picket/action-sessions.json` (v4).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredActionSession {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    #[serde(default)]
    pub session_id: String,
    pub action_id: String,
    pub action_label: String,
    #[serde(default)]
    pub profile: ActionProfileKind,
    #[serde(default)]
    pub action_prompt_hash: String,
    #[serde(default)]
    pub frozen_scope_version: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_head: Option<String>,
    #[serde(default)]
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
    pub status: ActionSessionStatus,
    #[serde(default, skip_serializing)]
    pub pending_call_id: Option<String>,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<MessageUserCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<WorkflowUiMessage>,
    #[serde(default)]
    pub timeline: Vec<WorkflowTimelineItem>,
    #[serde(default)]
    pub qa_log: Vec<CPushQaItem>,
    #[serde(default)]
    pub commits: Vec<CPushLandedCommit>,
    #[serde(default)]
    pub links: Vec<WorkflowLink>,
    #[serde(default)]
    pub can_retry_remaining: bool,
    #[serde(default = "default_true")]
    pub push_authorized: bool,
    #[serde(default)]
    pub auto_collapse: bool,
    #[serde(default)]
    pub notice_dismissed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub edited_file_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub submit_ledger: SubmitSessionLedger,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub commit_language: LanguageKind,
}

impl StoredActionSession {
    pub fn to_public(&self) -> ActionSessionSnapshot {
        ActionSessionSnapshot {
            run_id: self.run_id.clone(),
            repository_id: self.repository_id.clone(),
            worktree_id: self.worktree_id.clone(),
            session_id: self.session_id.clone(),
            action_id: self.action_id.clone(),
            action_label: self.action_label.clone(),
            profile: self.profile,
            frozen_scope_version: self.frozen_scope_version.clone(),
            model_id: self.model_id.clone(),
            frozen_file_ids: self
                .frozen_files
                .iter()
                .map(|f| f.file_id.clone())
                .collect(),
            committed_file_ids: self.committed_file_ids.clone(),
            skipped_file_ids: self.skipped_file_ids.clone(),
            status: self.status,
            pending_call_id: self.pending_call_id.clone(),
            summary: self.summary.clone(),
            message: self.message.clone(),
            timeline: self.timeline.clone(),
            qa_log: self
                .qa_log
                .iter()
                .map(|qa| CPushQaItem {
                    question_id: qa.question_id.clone(),
                    question: qa.question.clone(),
                    answer: qa.answer.clone(),
                    option_id: None,
                })
                .collect(),
            commits: self.commits.clone(),
            links: self.links.clone(),
            can_retry_remaining: self.can_retry_remaining,
            auto_collapse: self.auto_collapse.then_some(true),
        }
    }

    pub fn read_target(&self) -> ReadTarget {
        ReadTarget {
            repository_id: self.repository_id.clone(),
            worktree_id: self.worktree_id.clone(),
            session_id: self.session_id.clone(),
            scope_version: self.frozen_scope_version.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateCustomActionRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub user_request: String,
    #[serde(default)]
    pub existing_labels: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionDraft {
    pub label: String,
    pub prompt: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct GenerateCustomActionProgressEvent {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    pub run_id: String,
    #[cfg_attr(
        test,
        ts(type = "'running' | 'needsDecision' | 'done' | 'cancelled' | 'failed'")
    )]
    pub status: &'static str,
    pub status_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<MessageUserCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub questionnaire: Option<MessageUserRequest>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct GenerateCustomActionReply {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'generateCustomAction'"))]
    pub kind: &'static str,
    pub request_id: String,
    #[cfg_attr(test, ts(type = "'done' | 'cancelled'"))]
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<CustomActionDraft>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DismissSessionDoneRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub run_id: Option<String>,
    pub message_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[cfg(test)]
pub type DismissCustomActionDoneRequest = DismissSessionDoneRequest;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepoIdentity {
    pub remote: String,
    pub platform: &'static str,
    pub host: String,
    pub repo_path: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliBinaryProbe {
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_head_sha_guard: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEnvSnapshot {
    pub git: CliBinaryProbe,
    pub gh: CliBinaryProbe,
    pub glab: CliBinaryProbe,
    pub brew: CliBinaryProbe,
    pub remotes: Vec<RemoteRepoIdentity>,
    pub detected_platform: &'static str,
}
