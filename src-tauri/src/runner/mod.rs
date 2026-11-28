//! Repository-level local Run command execution, user login-shell environment loading,
//! reusable pop-out Run Logs window, and read-only AI project command discovery.

pub mod discover;
pub mod logs_window;
pub mod manager;
mod process_io;
#[cfg(test)]
mod tests;
pub mod types;
pub mod user_env;
pub mod workspace_scan;

pub use manager::RunProcessManager;
pub use types::{
    ClearRunLogsRequest, DiscoverRunCommandsReply, DiscoverRunCommandsRequest, GetRunLogsRequest,
    OpenRunLocalUrlRequest, OpenRunLogsWindowRequest, PersistedRunCommand,
    RUN_DISCOVER_PROGRESS_EVENT, RUN_LOGS_WINDOW_LABEL, RestartRunCommandRequest, RunEnvEntry,
    RunProcessLogsReply, RunProcessSnapshot, RunProcessStatus, StartRunCommandRequest,
    StopRunCommandRequest, UserEnvSummary, sanitize_run_command, sanitize_run_env_entries,
    validate_run_commands, validate_run_env_entries,
};
#[cfg(test)]
pub use types::{
    DiscoverRunCommandsProgressEvent, DiscoveredRunCommand, RunLogBatchEvent, RunLogLine,
    RunLogStream,
};
