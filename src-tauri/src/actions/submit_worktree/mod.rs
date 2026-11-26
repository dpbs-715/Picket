//! Controlled isolated worktree engine for Phase 3 `submit` multi-group workflows.
//!
//! Enforces:
//! - Only authorized selected files (never unselected files or `Local Keep`) can be transferred.
//! - Partially staged files transfer ONLY their staged index content, leaving unstaged drafts untouched.
//! - Exact preflight snapshots of original index + working-tree entries are recorded for Step 5 cleanup.
//! - Isolated worktrees live under `<storage_dir>/submit-worktrees/<repo_id>/` (`0700`) so they never
//!   pollute user directories and survive OS restarts for cross-restart reconciliation.
//! - Worktree removal verifies Picket ownership, unlocked state, verified HEAD SHA, absence of active
//!   Git operations, and empty `git status --porcelain=v1 --untracked-files=all --ignored --ignore-submodules=none`
//!   before running `git worktree remove` WITHOUT `--force`.

mod commit;
mod create;
mod inspect;
mod remove;

pub use commit::commit_in_isolated_worktree;
pub use create::create_isolated_submit_worktree;
pub(crate) use inspect::{
    inspect_index_stage_entry, inspect_worktree_entry, run_git_bytes, sha256_hex,
    validate_repo_relative_path,
};
pub use remove::{parse_worktree_list, remove_verified_submit_worktree};
