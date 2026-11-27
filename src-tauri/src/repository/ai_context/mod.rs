//! Read-only Git scope verification and unified diff extraction for C-Push & Custom Actions.
//! Holds the repository gate only while reading a consistent Git snapshot; releases it
//! before any external model request starts.

mod cpush_scope;
mod patch;
#[cfg(test)]
mod tests;

pub use patch::is_sensitive_local_file;
#[cfg(test)]
pub(crate) use patch::truncate_patch_head_tail;

use serde::Serialize;

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenCPushFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub ignored_for_review: bool,
    pub binary: bool,
    pub content_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExtractedCPushScope {
    pub branch: String,
    pub initial_head: Option<String>,
    pub recent_commits: Vec<String>,
    pub unselected_paths: Vec<String>,
    pub files: Vec<FrozenCPushFile>,
}
