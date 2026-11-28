//! Reusable pop-out Run Logs window lifecycle and strictly-validated localhost URL launcher.

use super::{
    manager::RunProcessManager,
    types::{RUN_LOGS_SELECT_EVENT, RUN_LOGS_WINDOW_LABEL, validate_local_http_url},
};
use crate::{
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
    tray::TrayCoordinator,
    window_motion,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LOGS_WINDOW_WIDTH: f64 = 760.0;
const LOGS_WINDOW_HEIGHT: f64 = 500.0;
const LOGS_WINDOW_MIN_WIDTH: f64 = 480.0;
const LOGS_WINDOW_MIN_HEIGHT: f64 = 320.0;

fn ensure_logs_window_on_main(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(existing) = app.get_webview_window(RUN_LOGS_WINDOW_LABEL) {
        return Ok(existing);
    }
    let builder = WebviewWindowBuilder::new(
        app,
        RUN_LOGS_WINDOW_LABEL,
        WebviewUrl::App("run-logs.html".into()),
    )
    .title("Picket · Run Logs")
    .inner_size(LOGS_WINDOW_WIDTH, LOGS_WINDOW_HEIGHT)
    .min_inner_size(LOGS_WINDOW_MIN_WIDTH, LOGS_WINDOW_MIN_HEIGHT)
    .resizable(true)
    .decorations(true)
    .visible(false);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(12.0, 18.0));

    let window = builder.build().map_err(|err| err.to_string())?;
    let theme = app.state::<TrayCoordinator>().current_theme();
    let _ = window_motion::apply_window_theme(&window, theme, true);
    Ok(window)
}

pub fn open_or_focus_logs_window(
    app: &AppHandle,
    process_id: Option<String>,
) -> Result<(), AppError> {
    if let Some(ref pid) = process_id {
        app.state::<RunProcessManager>()
            .set_selected_logs_process(Some(pid.clone()));
    }
    let app_for_main = app.clone();
    let pid_for_main = process_id;
    app.run_on_main_thread(move || {
        app_for_main
            .state::<TrayCoordinator>()
            .hide_popover_on_main(&app_for_main);
        if let Ok(window) = ensure_logs_window_on_main(&app_for_main) {
            app_for_main
                .state::<TrayCoordinator>()
                .sync_dock_visibility_on_main(&app_for_main, Some(true));
            window_motion::activate_and_focus_main_on_main(&window);
            if let Some(pid) = pid_for_main {
                let _ = window.emit(RUN_LOGS_SELECT_EVENT, pid);
            }
        }
    })
    .map_err(|err| AppError::window(err.to_string()))
}

pub fn hide_logs_window_on_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(RUN_LOGS_WINDOW_LABEL) {
        let _ = window.hide();
    }
    app.state::<TrayCoordinator>()
        .sync_dock_visibility_on_main(app, None);
}

pub fn destroy_logs_window_on_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(RUN_LOGS_WINDOW_LABEL) {
        let _ = window.destroy();
    }
}

pub fn open_local_url_in_browser(raw_url: &str, locale: LanguageKind) -> Result<(), AppError> {
    let valid = validate_local_http_url(raw_url).map_err(|_| {
        AppError::runner(
            AppErrorCode::RunnerInvalidInput,
            i18n::runner::invalid_local_url(locale),
        )
    })?;
    let status = std::process::Command::new("/usr/bin/open")
        .arg(&valid)
        .status()
        .map_err(|err| {
            AppError::runner(
                AppErrorCode::RunnerFailed,
                i18n::runner::open_local_url_failed(locale, &err.to_string()),
            )
        })?;
    if !status.success() {
        return Err(AppError::runner(
            AppErrorCode::RunnerFailed,
            i18n::runner::open_local_url_failed(locale, &format!("open exited with {status}")),
        ));
    }
    Ok(())
}
