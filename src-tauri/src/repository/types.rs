use crate::{
    actions::types::{ActionApprovalMode, deserialize_approval_mode_or_default},
    i18n::{LanguageKind, deserialize_language_or_default},
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ThemeKind {
    #[default]
    System,
    Dark,
    Light,
}

pub fn deserialize_theme_or_default<'de, D>(deserializer: D) -> Result<ThemeKind, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<ThemeKind>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadTarget {
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub scope_version: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRequest {
    pub request_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalKeepRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub paths: Vec<String>,
    pub keep: bool,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocalKeepReply {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'localKeep'"))]
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub snapshot: RepositorySnapshot,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum BranchAction {
    SwitchBranch,
    CheckoutRemoteBranch,
}
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BranchRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub action: BranchAction,
    pub branch: String,
    pub branch_token: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BranchReply {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'branch'"))]
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub action: BranchAction,
    #[cfg_attr(test, ts(type = "'completed'"))]
    pub outcome: &'static str,
    #[cfg_attr(test, ts(as = "RepositorySnapshot"))]
    pub snapshot: Option<RepositorySnapshot>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenFileRequest {
    pub request_id: String,
    pub target: ReadTarget,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    pub editor: EditorKind,
    pub line: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum OpenFileAction {
    Open,
    Goto,
    Diff,
    StagedDiff,
    Merge,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct OpenFileReply {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'openFile'"))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum WorkbenchTab {
    Commit,
    Review,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
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
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PersistedRepositorySettings {
    pub repository_id: String,
    pub editor: EditorKind,
    pub commit_language: LanguageKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub local_keep_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<PersistedCustomAction>>", optional))]
    pub custom_actions: Vec<PersistedCustomAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub active_action_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(
        test,
        ts(as = "Option<Vec<crate::runner::PersistedRunCommand>>", optional)
    )]
    pub run_commands: Vec<crate::runner::PersistedRunCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<crate::runner::RunEnvEntry>>", optional))]
    pub run_shared_env: Vec<crate::runner::RunEnvEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorktreeState {
    pub repository_id: String,
    pub worktree_id: String,
    pub prompt_text: String,
    pub selected_ids: Vec<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub unselected_ids: Vec<String>,
    pub scope_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tab: Option<WorkbenchTab>,
}
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkbenchSaveRequest {
    #[cfg_attr(test, ts(type = "number"))]
    pub revision: u64,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_theme_or_default")]
    #[cfg_attr(test, ts(as = "Option<ThemeKind>", optional))]
    pub theme: ThemeKind,
    #[serde(default, deserialize_with = "deserialize_approval_mode_or_default")]
    #[cfg_attr(test, ts(as = "Option<ActionApprovalMode>", optional))]
    pub action_approval_mode: ActionApprovalMode,
    pub editor: EditorKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub default_commit_language: LanguageKind,
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<PersistedRepositorySettings>>", optional))]
    pub repository_settings: Vec<PersistedRepositorySettings>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub window_preferences: Option<PersistedWindowPreferences>,
    pub active_worktree_id: Option<String>,
    pub empty_prompt: String,
    pub worktrees: Vec<PersistedWorktreeState>,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchRestoreReply {
    #[cfg_attr(test, ts(type = "2"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
    #[cfg_attr(test, ts(as = "Option<ThemeKind>", optional))]
    pub theme: ThemeKind,
    #[cfg_attr(test, ts(as = "Option<ActionApprovalMode>", optional))]
    pub action_approval_mode: ActionApprovalMode,
    pub editor: EditorKind,
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub default_commit_language: LanguageKind,
    #[cfg_attr(test, ts(as = "Option<Vec<PersistedRepositorySettings>>", optional))]
    pub repository_settings: Vec<PersistedRepositorySettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub window_preferences: Option<PersistedWindowPreferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub installed_editors: Option<Vec<EditorKind>>,
    pub active_worktree_id: Option<String>,
    pub empty_prompt: String,
    pub worktrees: Vec<PersistedWorktreeState>,
    pub snapshots: Vec<RepositorySnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub connections: Option<Vec<crate::models::types::SavedProviderConnection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub models: Option<Vec<crate::models::types::SavedModelConfiguration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional = nullable, type = "string | null"))]
    pub active_model_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub action_sessions: Option<Vec<crate::actions::types::ActionSessionSnapshot>>,
    pub warning: Option<String>,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReadReply {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<ReadTarget>,
    pub snapshot: Option<RepositorySnapshot>,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub repository: RepositoryInfo,
    pub worktrees: Vec<WorktreeSnapshot>,
    pub active_worktree_id: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub read_sequence: u64,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub id: String,
    pub name: String,
    pub branches: Vec<Branch>,
    #[cfg_attr(test, ts(as = "Option<Vec<RemoteBranch>>", optional))]
    pub remote_branches: Vec<RemoteBranch>,
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub local_keep_paths: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranch {
    pub name: String,
    pub remote: String,
    pub local_name: String,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Remote {
    #[serde(rename_all = "camelCase")]
    Tracked {
        branch: String,
        #[cfg_attr(test, ts(type = "number"))]
        ahead: u64,
        #[cfg_attr(test, ts(type = "number"))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub previous_path: Option<String>,
    #[cfg_attr(test, ts(type = "'A' | 'M' | 'D' | 'R' | 'U' | 'T' | 'C'"))]
    pub status: String,
    pub git_status: String,
    pub range: Range,
    pub binary: bool,
    pub ignored_for_review: bool,
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub local_keep: bool,
    pub content_version: String,
    pub commit_token: String,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Range {
    #[cfg_attr(test, ts(type = "'partial' | 'staged' | 'unstaged'"))]
    pub kind: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub added: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub removed: u64,
}
