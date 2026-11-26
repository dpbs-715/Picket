//! Smooth native macOS `NSWindow` frame animation via `NSAnimationContext` and
//! Objective-C runtime FFI, using only Apple system frameworks (`AppKit`, `QuartzCore`,
//! `libobjc`, `libSystem`) so the packaged app bundle keeps zero third-party dylibs.

#[cfg(not(target_os = "macos"))]
mod fallback;
mod geometry;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;

#[cfg(not(target_os = "macos"))]
use fallback as platform;
#[cfg(target_os = "macos")]
use macos as platform;

pub use geometry::{AnimateWindowFrameReply, AnimateWindowFrameRequest, MonitorWorkArea};
pub use platform::{
    activate_and_focus_main_on_main, configure_main_window_startup_on_main,
    ensure_main_window_opaque, mouse_location_logical_on_main, move_splash_window_origin,
    prefers_reduced_motion, prepare_splash_window_on_main, prepare_tray_popover_window_on_main,
    read_monitors_on_main, read_monitors_with_scale_on_main, resize_tray_popover_height,
    reveal_main_window_smoothly, show_splash_window, show_tray_popover_on_main,
};

use tauri::Manager;

#[tauri::command]
pub async fn set_window_pinned(app: tauri::AppHandle, pinned: bool) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    platform::set_main_window_pinned(window, pinned).await
}

pub async fn read_main_window_visible_on_all_spaces(
    app: &tauri::AppHandle,
) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    platform::read_main_window_visible_on_all_spaces(window).await
}

#[tauri::command]
pub async fn animate_window_frame(
    app: tauri::AppHandle,
    request: AnimateWindowFrameRequest,
) -> Result<AnimateWindowFrameReply, String> {
    let validated = geometry::validate_request(&request)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    platform::animate_main_window(window, validated).await
}
