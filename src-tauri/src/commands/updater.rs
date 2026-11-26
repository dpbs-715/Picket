//! In-app update check, installation, cancellation, and release page IPC commands.

use crate::{
    actions::CustomActionCoordinator,
    cpush::CPushCoordinator,
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
) -> Result<UpdateStateSnapshot, String> {
    updater.get_status(&app)
}

#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
    request: CheckUpdateRequest,
) -> Result<UpdateStateSnapshot, String> {
    updater.check(&app, request.manual, request.locale).await
}

#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
    repositories: State<'_, RepositoryStore>,
    cpush: State<'_, CPushCoordinator>,
    actions: State<'_, CustomActionCoordinator>,
    request: InstallUpdateRequest,
) -> Result<UpdateStateSnapshot, String> {
    updater
        .install(
            &app,
            &request.version,
            request.locale,
            &repositories,
            &cpush,
            &actions,
        )
        .await
}

#[tauri::command]
pub fn cancel_update(
    app: tauri::AppHandle,
    updater: State<'_, UpdateManager>,
    request: CancelUpdateRequest,
) -> Result<UpdateStateSnapshot, String> {
    updater.cancel(&app, request.locale)
}

#[tauri::command]
pub fn open_release_page(request: OpenReleasePageRequest) -> Result<(), String> {
    UpdateManager::open_release_page(request.locale)
}
