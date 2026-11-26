use crate::i18n::{LanguageKind, deserialize_language_or_default};
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
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalKeepRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub paths: Vec<String>,
    pub keep: bool,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalKeepReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub snapshot: RepositorySnapshot,
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
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
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
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
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
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedWindowPreferences {
    pub pinned: bool,
    pub compact_mode: bool,
    pub pinned_expanded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_size: Option<WindowSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_position: Option<WindowPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_position: Option<WindowPosition>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedCustomAction {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_key: Option<String>,
    pub label: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_write_confirm: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_prompt_hash: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedRepositorySettings {
    pub repository_id: String,
    pub editor: EditorKind,
    pub commit_language: LanguageKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_keep_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_actions: Vec<PersistedCustomAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_action_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedWorktreeState {
    pub repository_id: String,
    pub worktree_id: String,
    pub prompt_text: String,
    pub selected_ids: Vec<String>,
    #[serde(default)]
    pub unselected_ids: Vec<String>,
    pub scope_version: String,
    pub tab: WorkbenchTab,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkbenchSaveRequest {
    pub revision: u64,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
    pub editor: EditorKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub default_commit_language: LanguageKind,
    #[serde(default)]
    pub repository_settings: Vec<PersistedRepositorySettings>,
    #[serde(default)]
    pub window_preferences: Option<PersistedWindowPreferences>,
    pub active_worktree_id: Option<String>,
    pub empty_prompt: String,
    pub worktrees: Vec<PersistedWorktreeState>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchRestoreReply {
    pub protocol_version: u8,
    pub locale: LanguageKind,
    pub editor: EditorKind,
    pub default_commit_language: LanguageKind,
    pub repository_settings: Vec<PersistedRepositorySettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_preferences: Option<PersistedWindowPreferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_editors: Option<Vec<EditorKind>>,
    pub active_worktree_id: Option<String>,
    pub empty_prompt: String,
    pub worktrees: Vec<PersistedWorktreeState>,
    pub snapshots: Vec<RepositorySnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<Vec<crate::models::types::SavedProviderConnection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<crate::models::types::SavedModelConfiguration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_model_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_push_sessions: Option<Vec<crate::cpush::types::CPushSessionSnapshot>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_sessions: Option<Vec<crate::actions::session_store::ActionSessionSnapshot>>,
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
    pub local_keep_paths: Vec<String>,
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
    pub local_keep: bool,
    pub content_version: String,
    pub commit_token: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub kind: String,
    pub added: u64,
    pub removed: u64,
}
