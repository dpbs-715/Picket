//! Custom Actions execution, generation, approval, cancellation, and unified session dismissal IPC commands.

use super::{picket_data_dir, runtime_paths};
use crate::{
    actions::{
        CustomActionCoordinator,
        types::{
            CustomActionCommandReply, CustomActionProgressEvent, DismissSessionDoneRequest,
            GenerateCustomActionProgressEvent, GenerateCustomActionReply,
            GenerateCustomActionRequest, RespondCustomActionRequest, StartCustomActionRequest,
        },
    },
    error::{AppError, AppErrorCode},
    models::store::ModelStore,
    pi_runtime::PiSupervisor,
    repository::RepositoryStore,
};
use tauri::{Manager, State, ipc::Channel};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_custom_action(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request: StartCustomActionRequest,
    on_event: Channel<CustomActionProgressEvent>,
) -> Result<CustomActionCommandReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    let app_cb = app.clone();
    let result = actions
        .start_custom_action(
            &data_dir,
            &paths,
            &supervisor,
            &models,
            &store,
            &request,
            move |event| {
                let _ = on_event.send(event);
                app_cb
                    .state::<crate::tray::TrayCoordinator>()
                    .refresh_from_app(&app_cb);
            },
        )
        .await;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}

#[tauri::command]
pub fn respond_custom_action(
    actions: State<'_, CustomActionCoordinator>,
    request: RespondCustomActionRequest,
) -> Result<bool, AppError> {
    actions
        .respond_question(&request)
        .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_custom_action(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request: GenerateCustomActionRequest,
    on_event: Channel<GenerateCustomActionProgressEvent>,
) -> Result<GenerateCustomActionReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    actions
        .generate_custom_action(
            &data_dir,
            &paths,
            &supervisor,
            &models,
            &store,
            &request,
            move |event| {
                let _ = on_event.send(event);
            },
        )
        .await
}

#[tauri::command]
pub fn cancel_custom_action(
    supervisor: State<'_, PiSupervisor>,
    request_id: String,
) -> Result<(), AppError> {
    supervisor
        .cancel(&request_id)
        .map_err(|msg| AppError::action(AppErrorCode::ActionFailed, msg))
}

#[tauri::command]
pub async fn dismiss_session_done(
    app: tauri::AppHandle,
    actions: State<'_, CustomActionCoordinator>,
    request: DismissSessionDoneRequest,
) -> Result<bool, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let result = actions
        .dismiss_done_notice_async(&data_dir, &request)
        .await
        .map_err(|msg| AppError::action(AppErrorCode::ActionFailed, msg));
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}
