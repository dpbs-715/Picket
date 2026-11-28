//! Tauri IPC command handlers grouped by domain, plus shared desktop runtime helpers.

pub mod actions;
pub mod models;
pub mod repository;
pub mod runner;
pub mod skills;
#[cfg(feature = "smoke-harness")]
pub mod smoke;
pub mod updater;

use crate::pi_runtime::PiPaths;
use tauri::Manager;

#[derive(Default)]
pub struct RepositoryPicker(pub(crate) tokio::sync::Mutex<()>);

pub fn picket_data_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Unable to resolve HOME directory for ~/.picket storage".to_string())?;
    let home_path = std::path::PathBuf::from(home);
    if !home_path.is_absolute() {
        return Err("HOME environment variable must be an absolute path".to_string());
    }
    Ok(home_path.join(".picket"))
}

pub fn runtime_paths(app: &tauri::AppHandle) -> Result<PiPaths, String> {
    if cfg!(debug_assertions) {
        return Ok(PiPaths::development());
    }
    PiPaths::bundled(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &app.path().resource_dir().map_err(|e| e.to_string())?,
    )
}
