//! Custom Actions execution, generation, approval, and cancellation IPC commands.

use super::{picket_data_dir, runtime_paths};
use crate::{
    actions::{
        CustomActionCoordinator,
        types::{
            CustomActionCommandReply, CustomActionProgressEvent, DismissCustomActionDoneRequest,
            GenerateCustomActionProgressEvent, GenerateCustomActionReply,
            GenerateCustomActionRequest, RespondCustomActionRequest, StartCustomActionRequest,
        },
    },
    cpush::CPushCoordinator,
    foundation::pi::PiSupervisor,
    models::store::ModelStore,
    repository::RepositoryStore,
};
use tauri::{State, ipc::Channel};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_custom_action(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    cpush: State<'_, CPushCoordinator>,
    actions: State<'_, CustomActionCoordinator>,
    request: StartCustomActionRequest,
    on_event: Channel<CustomActionProgressEvent>,
) -> Result<CustomActionCommandReply, String> {
    let data_dir = picket_data_dir()?;
    let paths = runtime_paths(&app)?;
    actions
        .start_custom_action(
            &data_dir,
            &paths,
            &supervisor,
            &models,
            &store,
            &cpush,
            &request,
            move |event| {
                let _ = on_event.send(event);
            },
        )
        .await
}

#[tauri::command]
pub fn respond_custom_action(
    actions: State<'_, CustomActionCoordinator>,
    request: RespondCustomActionRequest,
) -> Result<bool, String> {
    actions.respond_question(&request)
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
) -> Result<GenerateCustomActionReply, String> {
    let data_dir = picket_data_dir()?;
    let paths = runtime_paths(&app)?;
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
) -> Result<(), String> {
    supervisor.cancel(&request_id)
}

#[tauri::command]
pub fn dismiss_custom_action_done(
    actions: State<'_, CustomActionCoordinator>,
    request: DismissCustomActionDoneRequest,
) -> Result<bool, String> {
    let data_dir = picket_data_dir()?;
    actions.dismiss_done_notice(&data_dir, &request)
}
