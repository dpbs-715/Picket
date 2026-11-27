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

pub use geometry::{
    AnimateWindowFrameReply, AnimateWindowFrameRequest, MonitorWorkArea,
    resolve_visible_window_position,
};
pub use platform::{
    activate_and_focus_main_on_main, configure_main_window_startup_on_main,
    ensure_main_window_opaque, mouse_location_logical_on_main, move_splash_window_origin,
    prefers_reduced_motion, prepare_splash_window_on_main, prepare_tray_popover_window_on_main,
    read_monitors_on_main, read_monitors_with_scale_on_main, resize_tray_popover_height,
    reveal_main_window_smoothly, show_splash_window, show_tray_popover_on_main,
};

use crate::repository::types::ThemeKind;
use tauri::{Manager, window::Color};
use tokio::sync::oneshot;

/// Apply native window appearance (`NSAppearance` on macOS) and optional opaque canvas background
/// so light/dark mode transitions and frame resize animations never expose mismatched edges.
pub fn apply_window_theme(
    window: &tauri::WebviewWindow,
    theme: ThemeKind,
    update_background: bool,
) -> &'static str {
    let target_theme = match theme {
        ThemeKind::System => None,
        ThemeKind::Dark => Some(tauri::Theme::Dark),
        ThemeKind::Light => Some(tauri::Theme::Light),
    };
    let _ = window.set_theme(target_theme);
    let resolved_dark = match theme {
        ThemeKind::Dark => true,
        ThemeKind::Light => false,
        ThemeKind::System => !matches!(window.theme(), Ok(tauri::Theme::Light)),
    };
    if update_background {
        let bg = if resolved_dark {
            Color(11, 13, 16, 255)
        } else {
            Color(247, 248, 250, 255)
        };
        let _ = window.set_background_color(Some(bg));
    }
    if resolved_dark { "dark" } else { "light" }
}

#[tauri::command]
pub async fn set_window_theme(app: tauri::AppHandle, theme: ThemeKind) -> Result<String, String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    let tray = app.get_webview_window(crate::tray::TRAY_POPOVER_LABEL);
    let (tx, rx) = oneshot::channel::<&'static str>();
    let main_for_thread = main.clone();
    main.run_on_main_thread(move || {
        let resolved = apply_window_theme(&main_for_thread, theme, true);
        if let Some(popover) = tray {
            let _ = apply_window_theme(&popover, theme, false);
        }
        let _ = tx.send(resolved);
    })
    .map_err(|e| e.to_string())?;
    let resolved = rx
        .await
        .map_err(|_| "Window theme task did not run on main thread".to_string())?;
    app.state::<crate::tray::TrayCoordinator>()
        .set_ui_context(&app, None, Some(theme), None);
    Ok(resolved.to_string())
}

#[tauri::command]
pub async fn set_window_pinned(app: tauri::AppHandle, pinned: bool) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    platform::set_main_window_pinned(window, pinned).await
}

#[cfg(feature = "smoke-harness")]
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
