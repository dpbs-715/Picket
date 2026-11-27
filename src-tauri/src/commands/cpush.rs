//! C-Push execution, pull-with-rebase, and session reconciliation IPC commands
//! routed through the unified `CustomActionCoordinator`.

use super::{picket_data_dir, runtime_paths};
use crate::{
    actions::{
        CustomActionCoordinator,
        types::{
            CPushCommandReply, CPushProgressEvent, PullRebaseRequest, ReconcileCPushRequest,
            RespondCPushRequest, StartCPushRequest,
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
pub async fn start_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request: StartCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    let app_cb = app.clone();
    let result = actions
        .start_cpush(
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
                Ok(())
            },
        )
        .await;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}

#[tauri::command]
pub async fn respond_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request: RespondCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    let app_cb = app.clone();
    let result = actions
        .respond_cpush(
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
                Ok(())
            },
        )
        .await;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}

#[tauri::command]
pub async fn pull_rebase_repository(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    actions: State<'_, CustomActionCoordinator>,
    request: PullRebaseRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let app_cb = app.clone();
    let result = actions
        .pull_rebase(&data_dir, &store, &request, move |event| {
            let _ = on_event.send(event);
            app_cb
                .state::<crate::tray::TrayCoordinator>()
                .refresh_from_app(&app_cb);
            Ok(())
        })
        .await;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}

#[tauri::command]
pub async fn reconcile_cpush_session(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    actions: State<'_, CustomActionCoordinator>,
    request: ReconcileCPushRequest,
) -> Result<Option<CPushProgressEvent>, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let result = actions
        .reconcile_worktree(&data_dir, &store, &request)
        .await
        .map_err(|msg| AppError::action(AppErrorCode::ActionFailed, msg));
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}
