//! Remote synchronization, dirty-worktree lossless rebase, conflict stage extraction,
//! authorized post-merge validation, and fast-forward-only push coordinator.
//! Never invokes `git push --force`, `--force-with-lease`, or `git rebase --autostash`.

mod backup;
mod conflict;
mod helpers;
mod push;
mod rebase;
mod status;

use super::types::RepositorySnapshot;
pub(crate) use helpers::line_has_conflict_markers;

#[derive(Clone, Debug)]
pub struct UpstreamSpec {
    pub remote: String,
    pub remote_branch: String,
    pub display: String,
    pub tracking_ref: String,
}

#[derive(Clone, Debug)]
pub struct ConflictedFileSummary {
    pub file_id: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct RebasedCommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
}

#[derive(Clone, Debug)]
pub struct LiveRemoteStatus {
    pub upstream: Option<UpstreamSpec>,
    pub ahead: u64,
    pub rebase_in_progress: bool,
    pub merge_or_other_in_progress: bool,
    pub conflicted_files: Vec<ConflictedFileSummary>,
    pub unpushed_commits: Vec<RebasedCommitSummary>,
}

#[derive(Clone, Debug)]
pub enum RebaseSyncOutcome {
    AlreadyUpToDate {
        snapshot: RepositorySnapshot,
    },
    Completed {
        rebased_commits: Vec<RebasedCommitSummary>,
        snapshot: RepositorySnapshot,
    },
    Conflict {
        upstream: String,
        conflicted_files: Vec<ConflictedFileSummary>,
        snapshot: RepositorySnapshot,
    },
    MissingUpstream {
        remotes: Vec<String>,
        snapshot: RepositorySnapshot,
    },
    UnsafeDirtyOverlap {
        upstream: String,
        overlapping_paths: Vec<String>,
        snapshot: RepositorySnapshot,
    },
    UnsafePublishedHistory {
        upstream: String,
        reason: String,
        snapshot: RepositorySnapshot,
    },
}

#[derive(Clone, Debug)]
pub struct ExtractedConflictFile {
    pub file_id: String,
    pub path: String,
    pub base_content: Option<String>,
    pub upstream_content: String,
    pub local_content: String,
    pub conflicted_content: String,
    pub working_hash: String,
}

#[derive(Clone, Debug)]
pub struct ExtractedConflictBatch {
    pub upstream: String,
    pub files: Vec<ExtractedConflictFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedConflictFile {
    pub file_id: String,
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub enum RebaseContinueOutcome {
    Completed {
        upstream: String,
        rebased_commits: Vec<RebasedCommitSummary>,
        snapshot: RepositorySnapshot,
    },
    NextConflict {
        upstream: String,
        conflicted_files: Vec<ConflictedFileSummary>,
        snapshot: RepositorySnapshot,
    },
}

#[derive(Clone, Debug)]
pub enum PushOutcome {
    Pushed {
        remote_branch: String,
        snapshot: RepositorySnapshot,
    },
    MissingUpstream {
        remotes: Vec<String>,
        snapshot: RepositorySnapshot,
    },
    Failed {
        remote_branch: String,
        non_fast_forward: bool,
        reason: String,
        snapshot: RepositorySnapshot,
    },
}
