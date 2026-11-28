//! Unified built-in & custom action execution, Q&A responses, zero-token host recovery,
//! reconciliation, cancellation, session dismissal, and Pi-guided button generation IPC commands.

use super::{picket_data_dir, runtime_paths};
use crate::{
    actions::{
        CustomActionCoordinator,
        types::{
            ActionCommandReply, ActionProgressEvent, DismissSessionDoneRequest,
            GenerateCustomActionProgressEvent, GenerateCustomActionReply,
            GenerateCustomActionRequest, ReconcileSessionRequest, RespondActionQuestionRequest,
            SessionRecoveryRequest, StartActionRequest,
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
pub async fn start_action(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request: StartActionRequest,
    on_event: Channel<ActionProgressEvent>,
) -> Result<ActionCommandReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    let app_cb = app.clone();
    let result = actions
        .start_action(
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
pub fn respond_action_question(
    actions: State<'_, CustomActionCoordinator>,
    request: RespondActionQuestionRequest,
) -> Result<bool, AppError> {
    actions.respond_question(&request)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_session_recovery(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request: SessionRecoveryRequest,
    on_event: Channel<ActionProgressEvent>,
) -> Result<ActionCommandReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    let app_cb = app.clone();
    let result = actions
        .run_session_recovery(
            &data_dir,
            &paths,
            &supervisor,
            &models,
            &store,
            &request,
            move |event| {
                let send_res = on_event.send(event).map_err(|e| e.to_string());
                app_cb
                    .state::<crate::tray::TrayCoordinator>()
                    .refresh_from_app(&app_cb);
                send_res
            },
        )
        .await;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}

#[tauri::command]
pub async fn reconcile_action_session(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    actions: State<'_, CustomActionCoordinator>,
    request: ReconcileSessionRequest,
) -> Result<Option<ActionProgressEvent>, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let result = actions
        .reconcile_worktree(&data_dir, &store, &request)
        .await;
    if matches!(result, Ok(Some(_))) {
        app.state::<crate::tray::TrayCoordinator>()
            .refresh_from_app(&app);
    }
    result
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
pub fn cancel_action(
    supervisor: State<'_, PiSupervisor>,
    actions: State<'_, CustomActionCoordinator>,
    request_id: String,
) -> Result<(), AppError> {
    actions.cancel_pending_questions_for_run(&request_id);
    let active_pi_request_id = actions.resolve_active_pi_request_id(&request_id);
    supervisor
        .cancel(&active_pi_request_id)
        .map_err(|msg| AppError::action(AppErrorCode::ActionFailed, msg))
}

#[tauri::command]
pub async fn dismiss_session_done(
    app: tauri::AppHandle,
    actions: State<'_, CustomActionCoordinator>,
    request: DismissSessionDoneRequest,
) -> Result<bool, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let result = actions.dismiss_done_notice_async(&data_dir, &request).await;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    result
}
