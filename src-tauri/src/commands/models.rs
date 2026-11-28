//! Provider connection, model catalog, and credential reveal IPC commands.

use super::{picket_data_dir, runtime_paths};
use crate::{
    error::{AppError, AppErrorCode},
    models::{
        store::ModelStore,
        types::{
            ConnectionIdRequest, CreateProviderConnectionInput, ListModelsInput, ModelIdRequest,
            ModelListReply, ModelMutateReply, ModelSaveReply, ModelTestReply,
            ProviderRevealKeyReply, ProviderSaveReply, ProviderUpdateReply, SaveModelInput,
            TestConnectionInput, UpdateProviderConnectionInput,
        },
    },
    pi_runtime::PiSupervisor,
};
use tauri::State;

#[tauri::command]
pub async fn test_model_connection(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    input: TestConnectionInput,
) -> Result<ModelTestReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    let paths =
        runtime_paths(&app).map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
    models
        .test_connection_with_storage(&data_dir, &paths, &supervisor, &input)
        .await
}

#[tauri::command]
pub async fn list_provider_models(
    models: State<'_, ModelStore>,
    input: ListModelsInput,
) -> Result<ModelListReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models.list_provider_models(&data_dir, &input).await
}

#[tauri::command]
pub async fn create_provider_connection(
    models: State<'_, ModelStore>,
    input: CreateProviderConnectionInput,
) -> Result<ProviderSaveReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models.create_provider_connection(&data_dir, &input).await
}

#[tauri::command]
pub async fn update_provider_connection(
    models: State<'_, ModelStore>,
    input: UpdateProviderConnectionInput,
) -> Result<ProviderUpdateReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models.update_provider_connection(&data_dir, &input).await
}

#[tauri::command]
pub async fn delete_provider_connection(
    models: State<'_, ModelStore>,
    request: ConnectionIdRequest,
) -> Result<ModelMutateReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models
        .delete_provider_connection_with_locale(&data_dir, &request.connection_id, request.locale)
        .await
}

#[tauri::command]
pub async fn save_model(
    models: State<'_, ModelStore>,
    input: SaveModelInput,
) -> Result<ModelSaveReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models.save_model(&data_dir, &input).await
}

#[tauri::command]
pub async fn select_model(
    models: State<'_, ModelStore>,
    request: ModelIdRequest,
) -> Result<ModelMutateReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models
        .select_model_with_locale(&data_dir, &request.model_id, request.locale)
        .await
}

#[tauri::command]
pub async fn delete_model(
    models: State<'_, ModelStore>,
    request: ModelIdRequest,
) -> Result<ModelMutateReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models
        .delete_model_with_locale(&data_dir, &request.model_id, request.locale)
        .await
}

#[tauri::command]
pub async fn reveal_provider_api_key(
    models: State<'_, ModelStore>,
    request: ConnectionIdRequest,
) -> Result<ProviderRevealKeyReply, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    models
        .reveal_provider_api_key(&data_dir, &request.connection_id, request.locale)
        .await
}

#[tauri::command]
pub fn cancel_model_request(
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    request_id: String,
) -> Result<(), AppError> {
    if models.cancel_list_request(&request_id) {
        return Ok(());
    }
    supervisor
        .cancel(&request_id)
        .map_err(|msg| AppError::model(AppErrorCode::ModelRequestFailed, msg))
}
