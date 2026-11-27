//! In-app update check, installation, cancellation, and release page IPC commands.

use crate::{
    actions::CustomActionCoordinator,
    error::{AppError, AppErrorCode},
    repo_lock::RepoWriteCoordinator,
    repository::RepositoryStore,
    updater::{
        UpdateManager,
        types::{
            CancelUpdateRequest, CheckUpdateRequest, InstallUpdateRequest, OpenReleasePageRequest,
            UpdateStateSnapshot,
        },
    },
};
use tauri::State;

#[tauri::command]
pub fn get_update_status(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
) -> Result<UpdateStateSnapshot, AppError> {
    updater
        .get_status(&app)
        .map_err(|msg| AppError::updater(AppErrorCode::UpdaterFailed, msg))
}

#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
    request: CheckUpdateRequest,
) -> Result<UpdateStateSnapshot, AppError> {
    updater
        .check(&app, request.manual, request.locale)
        .await
        .map_err(|msg| AppError::updater(AppErrorCode::UpdaterFailed, msg))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn install_update(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
    repo_writes: State<'_, RepoWriteCoordinator>,
    repositories: State<'_, RepositoryStore>,
    actions: State<'_, CustomActionCoordinator>,
    request: InstallUpdateRequest,
) -> Result<UpdateStateSnapshot, AppError> {
    updater
        .install(
            &app,
            &request.version,
            request.locale,
            &repo_writes,
            &repositories,
            &actions,
        )
        .await
        .map_err(|msg| AppError::updater(AppErrorCode::UpdaterFailed, msg))
}

#[tauri::command]
pub fn cancel_update(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
    request: CancelUpdateRequest,
) -> Result<UpdateStateSnapshot, AppError> {
    updater
        .cancel(&app, request.locale)
        .map_err(|msg| AppError::updater(AppErrorCode::UpdaterFailed, msg))
}

#[tauri::command]
pub fn open_release_page(request: OpenReleasePageRequest) -> Result<(), AppError> {
    UpdateManager::open_release_page(request.locale)
        .map_err(|msg| AppError::updater(AppErrorCode::UpdaterFailed, msg))
}
