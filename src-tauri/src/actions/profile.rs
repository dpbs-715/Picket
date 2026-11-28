//! Declarative execution profiles shared by built-in workflows (C-Push, Pull & Rebase,
//! Conflict Resolution) and user-defined Custom Action buttons.
//!
//! System-level hard redlines (path sandbox, no `.git` tampering, no unauthorized writes to
//! unselected files or `Local Keep`, no destructive Git flags, zero credential leakage) are
//! enforced globally by Rust regardless of profile. Only business-level guardrails and tool
//! allowlists vary by profile.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const BUILTIN_CPUSH_ACTION_ID: &str = "builtin:cpush";
pub const BUILTIN_PULL_REBASE_ACTION_ID: &str = "builtin:pull-rebase";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ActionProfileKind {
    Cpush,
    #[default]
    CustomAction,
    ResolveConflict,
    PullRebase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitCoveragePolicy {
    /// Every authorized selected file (`@1..@N`) must appear once across commit groups or
    /// user-confirmed `skippedFileIds`.
    ExactAllSelected,
    /// Commit any subset of pre-selected `fileIds` and/or SHA-256 verified `editedPaths`.
    SubsetOrEdited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveFileGuardPolicy {
    /// Intercept sensitive local files (`.env`, private keys, etc.) with a mandatory confirmation
    /// questionnaire before committing unless the user already confirmed in this run.
    HardIntercept,
    /// Do not inject unrequested business-level confirmation popups into user-defined actions.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryHandoffPolicy {
    /// Transition immediately to deterministic zero-token Host Recovery (`stopAgent: true`).
    AutoHandoff,
    /// Record a verified `RecoveryHandle` and return the structured failure to the Agent first so
    /// it can either resolve in-workflow or call `handoff_to_recovery({ handleId })`.
    AgentFirst,
}

#[derive(Clone, Copy, Debug)]
pub struct ExecutionProfile {
    pub kind: ActionProfileKind,
    pub allowed_tools: &'static [&'static str],
    pub require_selected_scope: bool,
    pub commit_coverage: CommitCoveragePolicy,
    pub sensitive_file_guard: SensitiveFileGuardPolicy,
    pub commit_auto_push_default: bool,
    pub recovery_mode: RecoveryHandoffPolicy,
    pub idle_timeout: Duration,
}

const CPUSH_TOOLS: &[&str] = &[
    "read_skill_override_doc",
    "read_scope_diff",
    "read_repo_file",
    "search_repo",
    "message_user",
    "commit_exact_scope",
];

const CUSTOM_ACTION_TOOLS: &[&str] = &[
    "read_skill_override_doc",
    "read_scope_diff",
    "read_repo_file",
    "search_repo",
    "check_cli_env",
    "install_platform_cli",
    "message_user",
    "run_git",
    "run_platform_cli",
    "edit_repo_file",
    "write_repo_file",
    "run_repo_command",
    "commit_exact_scope",
    "sync_and_push",
    "handoff_to_recovery",
    "manage_submit_worktree",
    "cleanup_submit_duplicates",
    "record_action_checkpoint",
];

const RESOLVE_CONFLICT_TOOLS: &[&str] = &[
    "read_conflict_file",
    "write_conflict_resolution",
    "read_repo_file",
    "search_repo",
];

impl ActionProfileKind {
    pub const fn profile(self) -> ExecutionProfile {
        match self {
            Self::Cpush => ExecutionProfile {
                kind: Self::Cpush,
                allowed_tools: CPUSH_TOOLS,
                require_selected_scope: true,
                commit_coverage: CommitCoveragePolicy::ExactAllSelected,
                sensitive_file_guard: SensitiveFileGuardPolicy::HardIntercept,
                commit_auto_push_default: true,
                recovery_mode: RecoveryHandoffPolicy::AutoHandoff,
                idle_timeout: Duration::from_secs(90),
            },
            Self::CustomAction => ExecutionProfile {
                kind: Self::CustomAction,
                allowed_tools: CUSTOM_ACTION_TOOLS,
                require_selected_scope: false,
                commit_coverage: CommitCoveragePolicy::SubsetOrEdited,
                sensitive_file_guard: SensitiveFileGuardPolicy::None,
                commit_auto_push_default: false,
                recovery_mode: RecoveryHandoffPolicy::AgentFirst,
                idle_timeout: Duration::from_secs(120),
            },
            Self::ResolveConflict => ExecutionProfile {
                kind: Self::ResolveConflict,
                allowed_tools: RESOLVE_CONFLICT_TOOLS,
                require_selected_scope: false,
                commit_coverage: CommitCoveragePolicy::SubsetOrEdited,
                sensitive_file_guard: SensitiveFileGuardPolicy::None,
                commit_auto_push_default: false,
                recovery_mode: RecoveryHandoffPolicy::AutoHandoff,
                idle_timeout: Duration::from_secs(90),
            },
            Self::PullRebase => ExecutionProfile {
                kind: Self::PullRebase,
                allowed_tools: &[],
                require_selected_scope: false,
                commit_coverage: CommitCoveragePolicy::SubsetOrEdited,
                sensitive_file_guard: SensitiveFileGuardPolicy::None,
                commit_auto_push_default: false,
                recovery_mode: RecoveryHandoffPolicy::AutoHandoff,
                idle_timeout: Duration::from_secs(90),
            },
        }
    }

    pub const fn is_cpush_like(self) -> bool {
        matches!(self, Self::Cpush | Self::PullRebase)
    }
}

impl ExecutionProfile {
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        self.allowed_tools.contains(&tool_name)
    }
}
