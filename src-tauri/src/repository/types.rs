use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadTarget {
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub scope_version: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRequest {
    pub request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshRequest {
    pub request_id: String,
    pub target: ReadTarget,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BranchAction {
    SwitchBranch,
    CheckoutRemoteBranch,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BranchRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub action: BranchAction,
    pub branch: String,
    pub branch_token: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub action: BranchAction,
    pub outcome: &'static str,
    pub snapshot: Option<RepositorySnapshot>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitSelection {
    pub file_id: String,
    pub commit_token: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub selections: Vec<CommitSelection>,
    pub message: String,
    pub push: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
    pub parent: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub snapshot: RepositorySnapshot,
    pub commit: CommitResult,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorKind {
    Idea,
    Rustrover,
    Webstorm,
    Vscode,
    Cursor,
}
impl EditorKind {
    pub const ALL: [Self; 5] = [
        Self::Idea,
        Self::Rustrover,
        Self::Webstorm,
        Self::Vscode,
        Self::Cursor,
    ];
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenFileRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub file_id: String,
    pub editor: EditorKind,
    pub line: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenFileAction {
    Open,
    Goto,
    Diff,
    StagedDiff,
    Merge,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFileReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub editor: EditorKind,
    pub file_path: String,
    pub action: OpenFileAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbenchTab {
    Commit,
    Review,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedWorktreeState {
    pub repository_id: String,
    pub worktree_id: String,
    pub draft_text: String,
    pub draft_editing: bool,
    pub draft_outdated: bool,
    pub selected_ids: Vec<String>,
    pub scope_version: String,
    pub tab: WorkbenchTab,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkbenchSaveRequest {
    pub revision: u64,
    pub editor: EditorKind,
    pub active_worktree_id: Option<String>,
    pub empty_draft: String,
    pub empty_editing: bool,
    pub worktrees: Vec<PersistedWorktreeState>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchRestoreReply {
    pub protocol_version: u8,
    pub editor: EditorKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_editors: Option<Vec<EditorKind>>,
    pub active_worktree_id: Option<String>,
    pub empty_draft: String,
    pub empty_editing: bool,
    pub worktrees: Vec<PersistedWorktreeState>,
    pub snapshots: Vec<RepositorySnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<crate::models::types::SavedModelConfiguration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_model_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_push_sessions: Option<Vec<crate::cpush::types::CPushSessionSnapshot>>,
    pub warning: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadReply {
    pub protocol_version: u8,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ReadTarget>,
    pub snapshot: Option<RepositorySnapshot>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub repository: RepositoryInfo,
    pub worktrees: Vec<WorktreeSnapshot>,
    pub active_worktree_id: String,
    pub read_sequence: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub id: String,
    pub name: String,
    pub branches: Vec<Branch>,
    pub remote_branches: Vec<RemoteBranch>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranch {
    pub name: String,
    pub remote: String,
    pub local_name: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshot {
    pub repository_id: String,
    pub worktree_id: String,
    pub label: String,
    pub path: String,
    pub branch: String,
    pub detached: bool,
    pub head: Option<String>,
    pub status_version: String,
    pub branch_token: Option<String>,
    pub read_error: Option<String>,
    pub remote: Remote,
    pub files: Vec<Change>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Remote {
    #[serde(rename_all = "camelCase")]
    Tracked {
        branch: String,
        ahead: u64,
        behind: u64,
        local_only: bool,
    },
    Missing {
        reason: String,
    },
    Error {
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub git_status: String,
    pub range: Range,
    pub binary: bool,
    pub ignored_for_review: bool,
    pub content_version: String,
    pub commit_token: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub kind: String,
    pub added: u64,
    pub removed: u64,
}
