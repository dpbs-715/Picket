//! Shared IPC types for Phase 2 custom actions, controlled CLI environment inspection,
//! structured `message_user` communication cards & Q&A round-trips, and Pi-guided `Create` button generation.

#[cfg(test)]
pub use super::message_user::MessageUserAnswer;
pub use super::message_user::{
    MessageQuestionContext, MessageQuestionItem, MessageQuestionOption, MessageTone,
    MessageUserCard, MessageUserRequest, MessageUserResponse,
};
use crate::{
    i18n::{LanguageKind, deserialize_language_or_default},
    repository::{
        ai_context::FrozenCPushFile,
        types::{CommitSelection, PersistedCustomAction, ReadTarget, RepositorySnapshot},
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CUSTOM_ACTION_PROTOCOL_VERSION: u8 = 1;
pub const CPUSH_PROTOCOL_VERSION: u8 = 1;

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum CPushSessionStatus {
    Idle,
    Running,
    NeedsDecision,
    Conflict,
    WaitingManual,
    PushFailed,
    Done,
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

/// Unified UI message model shared by both C-Push and Custom Action workflows.
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
pub type CustomActionTimelineItem = WorkflowTimelineItem;
pub type CustomActionMessageCommitSummary = WorkflowMessageCommitSummary;
pub type CustomActionMessageProgress = WorkflowMessageProgress;
pub type CustomActionUiMessage = WorkflowUiMessage;
pub type CPushTimelineItem = WorkflowTimelineItem;
pub type CPushMessageFileChip = WorkflowMessageFileChip;
pub type CPushMessageCommitSummary = WorkflowMessageCommitSummary;
pub type CPushMessageProgress = WorkflowMessageProgress;
pub type CPushUiMessage = WorkflowUiMessage;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedActionUpdate {
    pub repository_id: String,
    pub action_id: String,
    pub trusted_prompt_hash: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartCustomActionRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub action: PersistedCustomAction,
    #[serde(default)]
    pub selections: Vec<CommitSelection>,
    #[serde(default)]
    pub extra_prompt: String,
    #[serde(default)]
    pub follow_up_prompt: Option<String>,
    #[serde(default)]
    pub resume_from_run_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub commit_language: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RespondCustomActionRequest {
    pub request_id: String,
    pub run_id: String,
    pub call_id: String,
    pub response: MessageUserResponse,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomActionProgressEvent {
    pub protocol_version: u8,
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub action_id: String,
    pub action_label: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_call_id: Option<String>,
    pub message: Option<CustomActionUiMessage>,
    pub timeline: Vec<CustomActionTimelineItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub links: Vec<CustomActionLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_action_update: Option<TrustedActionUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_snapshot: Option<RepositorySnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomActionCommandReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub action_id: String,
    pub action_label: String,
    pub status: &'static str,
    pub summary: String,
    pub message: Option<CustomActionUiMessage>,
    pub timeline: Vec<CustomActionTimelineItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub links: Vec<CustomActionLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_action_update: Option<TrustedActionUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_snapshot: Option<RepositorySnapshot>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateCustomActionRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub user_request: String,
    #[serde(default)]
    pub existing_labels: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
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
#[serde(rename_all = "camelCase")]
pub struct GenerateCustomActionProgressEvent {
    pub protocol_version: u8,
    pub run_id: String,
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
#[serde(rename_all = "camelCase")]
pub struct GenerateCustomActionReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<CustomActionDraft>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CPushSessionSnapshot {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub frozen_scope_version: String,
    pub model_id: String,
    pub frozen_file_ids: Vec<String>,
    pub committed_file_ids: Vec<String>,
    pub skipped_file_ids: Vec<String>,
    pub session_status: CPushSessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pending_call_id: Option<String>,
    pub message: Option<CPushUiMessage>,
    pub timeline: Vec<CPushTimelineItem>,
    pub qa_log: Vec<CPushQaItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub can_retry_remaining: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub auto_collapse: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredCPushSession {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub frozen_scope_version: String,
    pub model_id: String,
    pub branch: String,
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
    pub timeline: Vec<CPushTimelineItem>,
    pub session_status: CPushSessionStatus,
    #[serde(default, skip_serializing)]
    pub pending_call_id: Option<String>,
    pub message: Option<CPushUiMessage>,
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

impl StoredCPushSession {
    pub fn to_public(&self) -> CPushSessionSnapshot {
        CPushSessionSnapshot {
            run_id: self.run_id.clone(),
            repository_id: self.repository_id.clone(),
            worktree_id: self.worktree_id.clone(),
            session_id: self.session_id.clone(),
            frozen_scope_version: self.frozen_scope_version.clone(),
            model_id: self.model_id.clone(),
            frozen_file_ids: self
                .frozen_files
                .iter()
                .map(|f| f.file_id.clone())
                .collect(),
            committed_file_ids: self.committed_file_ids.clone(),
            skipped_file_ids: self.skipped_file_ids.clone(),
            session_status: self.session_status,
            pending_call_id: self.pending_call_id.clone(),
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
            can_retry_remaining: self.can_retry_remaining,
            auto_collapse: self.auto_collapse.then_some(true),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartCPushRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub selections: Vec<CommitSelection>,
    pub prompt: String,
    #[serde(default)]
    pub push: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub commit_language: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRebaseRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub locale: Option<LanguageKind>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconcileCPushRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub locale: Option<LanguageKind>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RespondCPushRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub run_id: String,
    pub message_id: String,
    #[serde(default)]
    pub option_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub locale: Option<LanguageKind>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CPushProgressEvent {
    pub protocol_version: u8,
    pub session: CPushSessionSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_snapshot: Option<RepositorySnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CPushCommandReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub session: CPushSessionSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_snapshot: Option<RepositorySnapshot>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DismissSessionDoneRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub run_id: Option<String>,
    pub message_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
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
