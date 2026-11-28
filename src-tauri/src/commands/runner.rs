//! Tauri IPC commands for local Run processes, pop-out Run Logs window, user shell env refresh,
//! and read-only AI project command discovery.

use super::{picket_data_dir, runtime_paths};
use crate::{
    error::{AppError, AppErrorCode},
    i18n::LanguageKind,
    models::store::ModelStore,
    pi_runtime::PiSupervisor,
    repository::RepositoryStore,
    runner::{
        ClearRunLogsRequest, DiscoverRunCommandsReply, DiscoverRunCommandsRequest,
        GetRunLogsRequest, OpenRunLocalUrlRequest, OpenRunLogsWindowRequest,
        RUN_DISCOVER_PROGRESS_EVENT, RestartRunCommandRequest, RunProcessLogsReply,
        RunProcessManager, RunProcessSnapshot, StartRunCommandRequest, StopRunCommandRequest,
        UserEnvSummary, discover, logs_window,
    },
};
use tauri::{Emitter, State};

#[tauri::command]
pub fn list_run_processes(manager: State<'_, RunProcessManager>) -> Vec<RunProcessSnapshot> {
    manager.list_snapshots()
}

#[tauri::command]
pub fn get_selected_run_logs_process(manager: State<'_, RunProcessManager>) -> Option<String> {
    manager.selected_logs_process()
}

#[tauri::command]
pub async fn start_run_command(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    manager: State<'_, RunProcessManager>,
    request: StartRunCommandRequest,
) -> Result<RunProcessSnapshot, AppError> {
    let ctx =
        RunProcessManager::resolve_target_context(&store, &request.target, request.locale).await?;
    manager
        .start_command(
            ctx,
            request.command,
            request.shared_env,
            request.locale,
            Some(app),
        )
        .await
}

#[tauri::command]
pub async fn stop_run_command(
    app: tauri::AppHandle,
    manager: State<'_, RunProcessManager>,
    request: StopRunCommandRequest,
) -> Result<RunProcessSnapshot, AppError> {
    manager
        .stop_process(&request.process_id, request.locale, Some(app))
        .await
}

#[tauri::command]
pub async fn restart_run_command(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    manager: State<'_, RunProcessManager>,
    request: RestartRunCommandRequest,
) -> Result<RunProcessSnapshot, AppError> {
    let ctx =
        RunProcessManager::resolve_target_context(&store, &request.target, request.locale).await?;
    manager
        .restart_command(
            ctx,
            request.command,
            request.shared_env,
            request.locale,
            Some(app),
        )
        .await
}

#[tauri::command]
pub fn get_run_process_logs(
    manager: State<'_, RunProcessManager>,
    request: GetRunLogsRequest,
) -> RunProcessLogsReply {
    manager.get_logs(&request.process_id)
}

#[tauri::command]
pub fn clear_run_process_logs(
    app: tauri::AppHandle,
    manager: State<'_, RunProcessManager>,
    request: ClearRunLogsRequest,
) -> bool {
    manager.clear_logs(&request.process_id, Some(&app))
}

#[tauri::command]
pub fn open_run_logs_window(
    app: tauri::AppHandle,
    request: OpenRunLogsWindowRequest,
) -> Result<(), AppError> {
    logs_window::open_or_focus_logs_window(&app, request.process_id)
}

#[tauri::command]
pub fn open_run_local_url(request: OpenRunLocalUrlRequest) -> Result<(), AppError> {
    logs_window::open_local_url_in_browser(&request.url, request.locale)
}

#[tauri::command]
pub async fn refresh_user_shell_env(
    manager: State<'_, RunProcessManager>,
    locale: Option<LanguageKind>,
) -> Result<UserEnvSummary, AppError> {
    Ok(manager.refresh_user_env(locale.unwrap_or_default()).await)
}

#[tauri::command]
pub async fn discover_run_commands(
    app: tauri::AppHandle,
    supervisor: State<'_, PiSupervisor>,
    manager: State<'_, RunProcessManager>,
    models: State<'_, ModelStore>,
    store: State<'_, RepositoryStore>,
    request: DiscoverRunCommandsRequest,
) -> Result<DiscoverRunCommandsReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    let emit_app = app.clone();
    discover::discover_run_commands(
        &data_dir,
        &paths,
        &supervisor,
        &manager,
        &models,
        &store,
        &request,
        move |event| {
            let _ = emit_app.emit(RUN_DISCOVER_PROGRESS_EVENT, event);
        },
    )
    .await
}

#[tauri::command]
pub fn cancel_discover_run_commands(
    supervisor: State<'_, PiSupervisor>,
    manager: State<'_, RunProcessManager>,
    request_id: String,
) -> Result<bool, AppError> {
    manager.mark_discover_cancelled(&request_id);
    supervisor.cancel_or_pre_cancel(&request_id);
    Ok(true)
}
