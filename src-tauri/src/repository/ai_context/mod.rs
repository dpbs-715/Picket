//! Read-only Git scope verification and unified diff extraction for AI tasks
//! (`review_selected_scope`, `plan_cpush`, and `start_cpush`).
//! Holds the repository gate only while reading a consistent Git snapshot; releases it
//! before any external model request starts.

mod cpush_scope;
mod patch;
mod review_scope;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use patch::truncate_patch_head_tail;
pub use patch::{
    is_sensitive_local_file, ranges_cover_all_lines, read_authorized_scope_diff,
    review_patch_budget_for_model,
};

use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct DeferredPatchNotice {
    pub file_id: String,
    pub path: String,
    pub total_lines: usize,
    pub initial_ranges: Vec<(usize, usize)>,
    pub deferred_only: bool,
    pub partial_notice: String,
    pub omitted_notice: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedDiffFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub patch: String,
}

#[derive(Clone, Debug)]
pub struct ExtractedAiScope {
    pub branch: String,
    pub files: Vec<ExtractedDiffFile>,
    pub skipped_paths: Vec<String>,
    pub reviewed_file_ids: Vec<String>,
    pub authorized_patches: HashMap<String, String>,
    pub deferred_notices: Vec<DeferredPatchNotice>,
}

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
