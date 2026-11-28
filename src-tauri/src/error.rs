//! End-to-end structured error types shared across Rust domains and the Tauri IPC boundary.
//!
//! Domain operations preserve machine-readable `code` and `category` values so Rust control flow
//! (such as unsupported commit guard checks) and frontend UX (such as 1-click Model Settings jump)
//! never match against localized display strings.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    Model,
    Repository,
    Git,
    Action,
    Auth,
    Storage,
    Updater,
    Skills,
    Runner,
    Window,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum AppErrorCode {
    // Model / Pi runtime errors
    ModelNotFound,
    ModelAuthFailed,
    ModelEndpointNotFound,
    ModelRateLimited,
    ModelServiceError,
    ModelConnectionFailed,
    ModelTimeout,
    ModelToolsUnsupported,
    ModelInvalidConfig,
    ModelMissingCredential,
    ModelCancelled,
    ModelRequestFailed,
    PiRuntimeError,

    // OS local authentication errors
    LocalAuthCancelled,
    LocalAuthFailed,
    LocalAuthUnavailable,

    // Repository & Git write guard errors
    RepoBusy,
    RepoNotOpened,
    RepoChangedDuringRead,
    RepoInvalidTarget,
    CommitSubmoduleUnsupported,
    CommitSparseOrHiddenFlag,
    CommitExternalFilterBlocked,
    CommitDetachedHead,
    CommitScopeChanged,
    CommitTokenInvalid,
    CommitConflictUnresolved,
    CommitLockFilePresent,
    CommitFailed,
    BranchSwitchFailed,
    GitFailed,

    // Action orchestration errors
    ActionBusy,
    ActionCancelled,
    ActionPolicyDenied,
    ActionInvalidInput,
    ActionFailed,

    // Storage / Updater / Skills / Runner / Window / Internal
    StorageFailed,
    UpdaterDraining,
    UpdaterFailed,
    SkillsFailed,
    RunnerInvalidInput,
    RunnerAlreadyRunning,
    RunnerNotFound,
    RunnerFailed,
    WindowFailed,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Error)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: AppErrorCode,
    pub category: ErrorCategory,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}

impl AppError {
    pub fn new(code: AppErrorCode, category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            code,
            category,
            message: message.into(),
            retryable: None,
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }

    pub fn map_message(mut self, f: impl FnOnce(String) -> String) -> Self {
        self.message = f(self.message);
        self
    }

    pub fn model(code: AppErrorCode, message: impl Into<String>) -> Self {
        let retryable = matches!(
            code,
            AppErrorCode::ModelRateLimited
                | AppErrorCode::ModelServiceError
                | AppErrorCode::ModelConnectionFailed
                | AppErrorCode::ModelTimeout
        );
        let mut err = Self::new(code, ErrorCategory::Model, message);
        if retryable {
            err.retryable = Some(true);
        }
        err
    }

    pub fn repository(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, ErrorCategory::Repository, message)
    }

    pub fn git(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, ErrorCategory::Git, message)
    }

    pub fn action(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, ErrorCategory::Action, message)
    }

    pub fn auth(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, ErrorCategory::Auth, message)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::StorageFailed, ErrorCategory::Storage, message)
    }

    pub fn updater(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, ErrorCategory::Updater, message)
    }

    pub fn skills(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::SkillsFailed, ErrorCategory::Skills, message)
    }

    pub fn runner(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, ErrorCategory::Runner, message)
    }

    pub fn window(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::WindowFailed, ErrorCategory::Window, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, ErrorCategory::Internal, message)
    }

    pub const fn is_unsupported_commit_guard(&self) -> bool {
        matches!(
            self.code,
            AppErrorCode::CommitSubmoduleUnsupported
                | AppErrorCode::CommitSparseOrHiddenFlag
                | AppErrorCode::CommitExternalFilterBlocked
        )
    }

    pub fn from_pi_error_code(error_code: Option<&str>, message: impl Into<String>) -> Self {
        let code = match error_code {
            Some("modelAuthFailed") => AppErrorCode::ModelAuthFailed,
            Some("modelEndpointNotFound") => AppErrorCode::ModelEndpointNotFound,
            Some("modelRateLimited") => AppErrorCode::ModelRateLimited,
            Some("modelServiceError") => AppErrorCode::ModelServiceError,
            Some("modelConnectionFailed") => AppErrorCode::ModelConnectionFailed,
            Some("modelTimeout") => AppErrorCode::ModelTimeout,
            Some("modelToolsUnsupported") => AppErrorCode::ModelToolsUnsupported,
            Some("modelInvalidConfig") => AppErrorCode::ModelInvalidConfig,
            Some("modelRequestFailed") => AppErrorCode::ModelRequestFailed,
            Some("piRuntimeError") => AppErrorCode::PiRuntimeError,
            _ => AppErrorCode::ModelRequestFailed,
        };
        Self::model(code, message)
    }
}

/// Strongly-typed commit guard failures used by Git commit verification and C-Push retry logic.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CommitGuardError {
    #[error("{0}")]
    SubmoduleUnsupported(String),
    #[error("{0}")]
    SparseOrHiddenFlag(String),
    #[error("{0}")]
    ExternalFilterBlocked(String),
}

impl From<CommitGuardError> for AppError {
    fn from(value: CommitGuardError) -> Self {
        match value {
            CommitGuardError::SubmoduleUnsupported(msg) => {
                AppError::git(AppErrorCode::CommitSubmoduleUnsupported, msg).with_retryable(false)
            }
            CommitGuardError::SparseOrHiddenFlag(msg) => {
                AppError::git(AppErrorCode::CommitSparseOrHiddenFlag, msg).with_retryable(false)
            }
            CommitGuardError::ExternalFilterBlocked(msg) => {
                AppError::git(AppErrorCode::CommitExternalFilterBlocked, msg).with_retryable(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_structured_app_error_in_camel_case() {
        let err = AppError::model(
            AppErrorCode::ModelAuthFailed,
            "Authentication failed (401).",
        );
        let val = serde_json::to_value(&err).unwrap();
        assert_eq!(
            val,
            serde_json::json!({
                "code": "modelAuthFailed",
                "category": "model",
                "message": "Authentication failed (401)."
            })
        );

        let retryable_err = AppError::model(AppErrorCode::ModelRateLimited, "Rate limited.");
        assert_eq!(retryable_err.retryable, Some(true));
        let val2 = serde_json::to_value(&retryable_err).unwrap();
        assert_eq!(val2["retryable"], serde_json::json!(true));
    }

    #[test]
    fn commit_guard_errors_preserve_non_retryable_classification() {
        let sub: AppError = CommitGuardError::SubmoduleUnsupported("submodule".into()).into();
        let sparse: AppError = CommitGuardError::SparseOrHiddenFlag("sparse".into()).into();
        let filter: AppError = CommitGuardError::ExternalFilterBlocked("filter".into()).into();
        let normal = AppError::git(AppErrorCode::CommitFailed, "hook failed");

        assert!(sub.is_unsupported_commit_guard());
        assert!(sparse.is_unsupported_commit_guard());
        assert!(filter.is_unsupported_commit_guard());
        assert!(!normal.is_unsupported_commit_guard());
    }

    #[test]
    fn maps_sidecar_pi_error_codes_to_model_category() {
        let err = AppError::from_pi_error_code(Some("modelRateLimited"), "Quota exceeded");
        assert_eq!(err.category, ErrorCategory::Model);
        assert_eq!(err.code, AppErrorCode::ModelRateLimited);
        assert_eq!(err.retryable, Some(true));

        let unknown = AppError::from_pi_error_code(None, "Unknown provider failure");
        assert_eq!(unknown.category, ErrorCategory::Model);
        assert_eq!(unknown.code, AppErrorCode::ModelRequestFailed);
    }
}
