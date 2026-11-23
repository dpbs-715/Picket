use crate::foundation::pi::PiCPushCommitGroup;
use crate::repository::{
    ai_context::FrozenCPushFile,
    types::{CommitSelection, ReadTarget, RepositorySnapshot},
};
use serde::{Deserialize, Serialize};

pub const CPUSH_PROTOCOL_VERSION: u8 = 1;

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushTimelineItem {
    pub state: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushQaItem {
    pub question_id: String,
    pub question: String,
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushLandedCommit {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
    pub file_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushMessageFileChip {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushMessageCommitSummary {
    pub hash: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushMessageOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushMessageProgress {
    pub completed: u32,
    pub total: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CPushUiMessage {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<CPushMessageFileChip>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commits: Option<Vec<CPushMessageCommitSummary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<CPushMessageOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<CPushMessageProgress>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    pub message: Option<CPushUiMessage>,
    pub timeline: Vec<CPushTimelineItem>,
    pub qa_log: Vec<CPushQaItem>,
    pub commits: Vec<CPushLandedCommit>,
    pub can_retry_remaining: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_collapse: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredCPushSession {
    pub run_id: String,
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub frozen_scope_version: String,
    pub model_id: String,
    pub branch: String,
    pub initial_head: Option<String>,
    pub draft: String,
    pub recent_commits: Vec<String>,
    pub unselected_paths: Vec<String>,
    pub frozen_files: Vec<FrozenCPushFile>,
    pub planned_groups: Vec<PiCPushCommitGroup>,
    pub skipped_file_ids: Vec<String>,
    pub next_step_index: usize,
    pub committed_file_ids: Vec<String>,
    pub commits: Vec<CPushLandedCommit>,
    pub qa_log: Vec<CPushQaItem>,
    pub timeline: Vec<CPushTimelineItem>,
    pub session_status: CPushSessionStatus,
    pub message: Option<CPushUiMessage>,
    pub can_retry_remaining: bool,
    #[serde(default = "default_true")]
    pub push_authorized: bool,
    #[serde(default)]
    pub auto_collapse: bool,
}

impl StoredCPushSession {
    pub fn to_public(&self) -> CPushSessionSnapshot {
        // Project only public fields — never include patches or sensitive metadata.
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
    pub draft: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitAndPushRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub selections: Vec<CommitSelection>,
    pub message: String,
    #[serde(default)]
    pub model_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRebaseRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub model_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconcileCPushRequest {
    pub request_id: String,
    pub target: ReadTarget,
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
