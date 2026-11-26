//! Shared IPC types for Phase 2 custom actions, controlled CLI environment inspection,
//! real-time `ask_user_question` round-trips, and Pi-guided `Create` button generation.

use crate::{
    cpush::types::CPushLandedCommit,
    i18n::{LanguageKind, deserialize_language_or_default},
    repository::types::{CommitSelection, PersistedCustomAction, ReadTarget, RepositorySnapshot},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CUSTOM_ACTION_PROTOCOL_VERSION: u8 = 1;

pub fn compute_prompt_hash(prompt: &str) -> String {
    let normalized = prompt.replace("\r\n", "\n");
    format!("{:x}", Sha256::digest(normalized.trim().as_bytes()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionTimelineItem {
    pub state: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserQuestionOption {
    pub id: String,
    pub label: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserQuestionItem {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<AskUserQuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserQuestionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_file_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<CustomActionLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_dont_ask_again: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dont_ask_again_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserQuestionRequest {
    pub questions: Vec<AskUserQuestionItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<AskUserQuestionContext>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserQuestionAnswer {
    pub question_index: usize,
    pub question_id: String,
    pub header: String,
    #[serde(default)]
    pub selected_option_ids: Vec<String>,
    #[serde(default)]
    pub selected_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserQuestionResponse {
    pub cancelled: bool,
    #[serde(default)]
    pub answers: Vec<AskUserQuestionAnswer>,
    #[serde(default)]
    pub unanswered: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dont_ask_again: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionMessageFileChip {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionMessageCommitSummary {
    pub hash: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionMessageProgress {
    pub completed: u32,
    pub total: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomActionUiMessage {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<CustomActionMessageFileChip>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commits: Option<Vec<CustomActionMessageCommitSummary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<CustomActionLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questionnaire: Option<AskUserQuestionRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<CustomActionMessageProgress>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub response: AskUserQuestionResponse,
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
    pub questionnaire: Option<AskUserQuestionRequest>,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DismissCustomActionDoneRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub run_id: Option<String>,
    pub message_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

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
