//! Skills repository sync, toggling, and reset IPC commands.

use super::picket_data_dir;
use crate::{
    error::AppError,
    skills::{
        SkillManager,
        sync::{
            GetSkillsStateRequest, ResetSkillsRequest, SkillsStateSnapshot, SyncSkillsRequest,
            UpdateEnabledSkillsRequest,
        },
    },
};
use tauri::State;

#[tauri::command]
pub async fn get_skills_state(
    skills: State<'_, SkillManager>,
    request: Option<GetSkillsStateRequest>,
) -> Result<SkillsStateSnapshot, AppError> {
    let _ = request;
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    Ok(skills.get_state(&data_dir).await)
}

#[tauri::command]
pub async fn sync_skills(
    skills: State<'_, SkillManager>,
    request: SyncSkillsRequest,
) -> Result<SkillsStateSnapshot, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    skills
        .sync_skills(&data_dir, &request)
        .await
        .map_err(AppError::skills)
}

#[tauri::command]
pub async fn set_enabled_skills(
    skills: State<'_, SkillManager>,
    request: UpdateEnabledSkillsRequest,
) -> Result<SkillsStateSnapshot, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    skills
        .set_enabled_skills(&data_dir, &request)
        .await
        .map_err(AppError::skills)
}

#[tauri::command]
pub async fn reset_skills_to_bundled(
    skills: State<'_, SkillManager>,
    request: ResetSkillsRequest,
) -> Result<SkillsStateSnapshot, AppError> {
    let data_dir = picket_data_dir().map_err(AppError::storage)?;
    skills
        .reset_to_bundled(&data_dir, &request)
        .await
        .map_err(AppError::skills)
}
