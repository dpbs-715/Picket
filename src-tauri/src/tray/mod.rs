//! macOS menu bar tray residency, dynamic Dock auto-hide, and Rust-authoritative status popover.
//!
//! Keeps Picket quietly resident in the macOS top menu bar when the main window is closed:
//! - Rust builds `TrayStateSnapshot` directly from `RepositoryStore`, `CustomActionCoordinator`,
//!   and `UpdateManager`, pushing live updates straight to `tray-popover` without relaying through
//!   a hidden main WebView.
//! - Opening the popover or clicking Refresh refreshes all opened repositories concurrently in Rust
//!   and updates both `tray-popover` and (if visible) the main window.

mod popover;
pub mod state;
#[cfg(test)]
mod tests;

pub use popover::{TRAY_ID, TRAY_POPOVER_LABEL};
pub use state::{
    TrayMainWindowAction, TraySessionStatus, TrayStateSnapshot, TrayUiContextUpdate,
    TrayWorktreeSessionInfo, build_tray_state_snapshot,
};

#[cfg(feature = "smoke-harness")]
use crate::lifecycle::DesktopLifecycle;
use crate::{
    actions::CustomActionCoordinator,
    i18n::LanguageKind,
    lifecycle::exit_after_cleanup,
    repository::{RepositoryStore, types::ThemeKind},
    updater::UpdateManager,
    window_motion::{self, MonitorWorkArea},
};
use popover::{
    TRAY_POPOVER_DEFAULT_HEIGHT, TRAY_POPOVER_WIDTH, apply_dock_visibility_on_main,
    clamp_popover_height, compute_popover_position, ensure_tray_popover_window_on_main,
    resolve_tray_anchor_logical, tray_icon_image,
};
use state::{build_tray_tooltip, tray_needs_attention};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, Rect, State,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const TRAY_BLUR_TOGGLE_DEBOUNCE: Duration = Duration::from_millis(220);

const TRAY_STATE_EVENT: &str = "picket:tray-state";
const TRAY_SNAPSHOTS_SYNCED_EVENT: &str = "picket:tray-snapshots-synced";
const TRAY_MAIN_ACTION_EVENT: &str = "picket:tray-main-action";
const TRAY_POPOVER_SHOWN_EVENT: &str = "picket:tray-popover-shown";

struct TrayRuntimeState {
    snapshot: TrayStateSnapshot,
    locale: LanguageKind,
    theme: ThemeKind,
    active_worktree_id: Option<String>,
    popover_height: f64,
    last_blur_hide: Option<Instant>,
    badge_active: bool,
    last_tooltip: String,
    main_hidden: bool,
}

impl Default for TrayRuntimeState {
    fn default() -> Self {
        Self {
            snapshot: TrayStateSnapshot::default(),
            locale: LanguageKind::En,
            theme: ThemeKind::System,
            active_worktree_id: None,
            popover_height: TRAY_POPOVER_DEFAULT_HEIGHT,
            last_blur_hide: None,
            badge_active: false,
            last_tooltip: "Picket".to_string(),
            main_hidden: false,
        }
    }
}

#[derive(Default)]
pub struct TrayCoordinator {
    state: Mutex<TrayRuntimeState>,
    refreshing: AtomicBool,
}

impl TrayCoordinator {
    pub(super) fn current_theme(&self) -> ThemeKind {
        self.state
            .lock()
            .map(|guard| guard.theme)
            .unwrap_or_default()
    }

    /// Updates lightweight UI context (`locale`, `theme`, `active_worktree_id`) and immediately
    /// rebuilds and pushes the authoritative tray snapshot from Rust stores.
    pub fn set_ui_context(
        &self,
        app: &AppHandle,
        locale: Option<LanguageKind>,
        theme: Option<ThemeKind>,
        active_worktree_id: Option<Option<String>>,
    ) {
        if let Ok(mut guard) = self.state.lock() {
            if let Some(next_locale) = locale {
                guard.locale = next_locale;
            }
            if let Some(next_theme) = theme {
                guard.theme = next_theme;
            }
            if let Some(next_active) = active_worktree_id {
                guard.active_worktree_id = next_active.filter(|s| !s.trim().is_empty());
            }
        }
        self.refresh_from_app(app);
    }

    /// Rebuilds the authoritative `TrayStateSnapshot` directly from Rust's `RepositoryStore`,
    /// `CustomActionCoordinator`, and `UpdateManager`, then updates the native menu bar icon,
    /// tooltip, and popover WebView without touching the main WebView.
    pub fn refresh_from_app(&self, app: &AppHandle) {
        let snapshots = app.state::<RepositoryStore>().cached_snapshots_in_order();
        let sessions = app
            .state::<CustomActionCoordinator>()
            .tray_session_statuses();
        let has_update = app.state::<UpdateManager>().has_update();
        let refreshing = self.refreshing.load(Ordering::SeqCst);

        let (locale, theme, active_worktree_id) = self
            .state
            .lock()
            .map(|g| (g.locale, g.theme, g.active_worktree_id.clone()))
            .unwrap_or((LanguageKind::En, ThemeKind::System, None));

        let snapshot = build_tray_state_snapshot(
            &snapshots,
            &sessions,
            active_worktree_id.as_deref(),
            locale,
            theme,
            has_update,
            refreshing,
        );
        self.publish_snapshot(app, snapshot);
    }

    /// Concurrently refreshes all opened repositories directly in Rust on a background task,
    /// pushing `refreshing: true -> false` straight to `tray-popover` without waking a hidden
    /// main WebView. If the main window is visible, forwards refreshed snapshots to keep it synced.
    pub fn trigger_background_refresh(&self, app: AppHandle) {
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        self.refresh_from_app(&app);
        let locale = self
            .state
            .lock()
            .map(|g| g.locale)
            .unwrap_or(LanguageKind::En);
        let store = app.state::<RepositoryStore>().inner().clone();
        tauri::async_runtime::spawn(async move {
            let refreshed = store.refresh_all_opened(locale).await;
            let coordinator = app.state::<TrayCoordinator>();
            coordinator.refreshing.store(false, Ordering::SeqCst);
            coordinator.refresh_from_app(&app);
            if !refreshed.is_empty()
                && let Some(main) = app.get_webview_window("main")
                && main.is_visible().unwrap_or(false)
            {
                let _ = main.emit(TRAY_SNAPSHOTS_SYNCED_EVENT, &refreshed);
            }
        });
    }

    /// Installs the native macOS menu bar tray icon and pre-warms the hidden status popover window.
    /// Must run on the main thread during Tauri `.setup(...)`.
    pub fn setup_tray(&self, app: &AppHandle, ui_smoke: bool) {
        let builder = TrayIconBuilder::with_id(TRAY_ID)
            .icon(tray_icon_image(false))
            .icon_as_template(true)
            .tooltip("Picket")
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left | MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    rect,
                    ..
                } = event
                {
                    let app = tray.app_handle();
                    app.state::<TrayCoordinator>()
                        .toggle_popover_on_main(app, rect);
                }
            });

        if let Err(error) = builder.build(app) {
            eprintln!("Failed to initialize menu bar tray icon: {error}");
            return;
        }

        if !ui_smoke {
            let initial_height = self
                .state
                .lock()
                .map(|s| s.popover_height)
                .unwrap_or(TRAY_POPOVER_DEFAULT_HEIGHT);
            let _ = ensure_tray_popover_window_on_main(app, initial_height);
        }
    }

    pub fn toggle_popover_on_main(&self, app: &AppHandle, rect: Rect) {
        let (height, recently_blurred) = self
            .state
            .lock()
            .map(|s| {
                let recent = s
                    .last_blur_hide
                    .is_some_and(|when| when.elapsed() < TRAY_BLUR_TOGGLE_DEBOUNCE);
                (s.popover_height, recent)
            })
            .unwrap_or((TRAY_POPOVER_DEFAULT_HEIGHT, false));

        if let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL)
            && popover.is_visible().unwrap_or(false)
        {
            let _ = popover.hide();
            return;
        }
        if recently_blurred {
            if let Ok(mut guard) = self.state.lock() {
                guard.last_blur_hide = None;
            }
            return;
        }

        let Some(popover) = ensure_tray_popover_window_on_main(app, height) else {
            return;
        };

        let (monitors_scaled, primary_scaled, primary_height) =
            window_motion::read_monitors_with_scale_on_main(&popover);
        let monitors: Vec<MonitorWorkArea> = monitors_scaled.iter().map(|(m, _)| *m).collect();
        let primary = primary_scaled.map(|(m, _)| m);
        let logical_cursor_x =
            window_motion::mouse_location_logical_on_main(primary_height).map(|(x, _)| x);
        let (icon_center_x, icon_bottom_y) =
            resolve_tray_anchor_logical(rect, logical_cursor_x, &monitors_scaled, primary_scaled);
        let (x, y) = compute_popover_position(
            icon_center_x,
            icon_bottom_y,
            TRAY_POPOVER_WIDTH,
            height,
            &monitors,
            primary,
        );

        self.refresh_from_app(app);
        let _ = popover.emit(TRAY_POPOVER_SHOWN_EVENT, ());

        // Keep Dock visibility untouched: showing the popover while `main` is hidden MUST keep
        // macOS activation policy at `Accessory` so the Dock icon never appears.
        let _ = window_motion::show_tray_popover_on_main(
            &popover,
            x,
            y,
            TRAY_POPOVER_WIDTH,
            height,
            primary_height,
        );

        // Refresh opened repositories directly in Rust without waking the main WebView.
        self.trigger_background_refresh(app.clone());
    }

    pub fn handle_popover_blur_on_main(&self, app: &AppHandle) {
        if let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL)
            && popover.is_visible().unwrap_or(false)
        {
            let _ = popover.hide();
            if let Ok(mut guard) = self.state.lock() {
                guard.last_blur_hide = Some(Instant::now());
            }
        }
    }

    pub fn hide_popover_on_main(&self, app: &AppHandle) {
        if let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL) {
            let _ = popover.hide();
        }
        if let Ok(mut guard) = self.state.lock() {
            guard.last_blur_hide = None;
        }
    }

    /// Hides the main window into the menu bar and automatically hides the Dock icon on macOS.
    pub fn hide_main_to_tray_on_main(&self, app: &AppHandle) {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.hide();
        }
        if let Ok(mut guard) = self.state.lock() {
            guard.main_hidden = true;
        }
        apply_dock_visibility_on_main(app, false);
    }

    /// Restores the Dock icon and brings the main window to the foreground, optionally forwarding
    /// a navigation action (focus worktree or open settings) to the workbench UI.
    pub fn restore_main_from_tray_on_main(
        &self,
        app: &AppHandle,
        action: Option<TrayMainWindowAction>,
    ) {
        self.hide_popover_on_main(app);
        if let Ok(mut guard) = self.state.lock() {
            guard.main_hidden = false;
        }
        apply_dock_visibility_on_main(app, true);
        if let Some(main) = app.get_webview_window("main") {
            window_motion::activate_and_focus_main_on_main(&main);
            if let Some(act) = action {
                let _ = main.emit(TRAY_MAIN_ACTION_EVENT, act);
            }
        }
    }

    fn publish_snapshot(&self, app: &AppHandle, snapshot: TrayStateSnapshot) {
        let attention = tray_needs_attention(&snapshot);
        let tooltip = build_tray_tooltip(&snapshot);
        let (icon_changed, tooltip_changed, snapshot_changed) = {
            let Ok(mut guard) = self.state.lock() else {
                return;
            };
            let icon_changed = guard.badge_active != attention;
            let tooltip_changed = guard.last_tooltip != tooltip;
            let snapshot_changed = guard.snapshot != snapshot;
            guard.snapshot = snapshot.clone();
            guard.badge_active = attention;
            guard.last_tooltip = tooltip.clone();
            (icon_changed, tooltip_changed, snapshot_changed)
        };

        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            if icon_changed {
                let _ = tray.set_icon(Some(tray_icon_image(attention)));
                let _ = tray.set_icon_as_template(true);
            }
            if tooltip_changed {
                let _ = tray.set_tooltip(Some(tooltip));
            }
        }

        if snapshot_changed && let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL) {
            let popover_for_theme = popover.clone();
            let snapshot_theme = snapshot.theme;
            let _ = popover.run_on_main_thread(move || {
                let _ =
                    window_motion::apply_window_theme(&popover_for_theme, snapshot_theme, false);
            });
            let _ = popover.emit(TRAY_STATE_EVENT, &snapshot);
        }
    }

    pub fn teardown(&self, app: &AppHandle) {
        let app_for_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(popover) = app_for_main.get_webview_window(TRAY_POPOVER_LABEL) {
                let _ = popover.destroy();
            }
            let _ = app_for_main.remove_tray_by_id(TRAY_ID);
        });
    }
}

#[tauri::command]
pub fn read_tray_state(
    app: AppHandle,
    coordinator: State<'_, TrayCoordinator>,
) -> Result<TrayStateSnapshot, crate::error::AppError> {
    coordinator.refresh_from_app(&app);
    coordinator
        .state
        .lock()
        .map(|s| s.snapshot.clone())
        .map_err(|_| crate::error::AppError::window("Tray state lock poisoned"))
}

#[tauri::command]
pub fn sync_tray_context(
    app: AppHandle,
    coordinator: State<'_, TrayCoordinator>,
    context: TrayUiContextUpdate,
) -> Result<(), crate::error::AppError> {
    coordinator.set_ui_context(
        &app,
        context.locale,
        context.theme,
        Some(context.active_worktree_id),
    );
    Ok(())
}

#[tauri::command]
pub fn request_tray_refresh(
    app: AppHandle,
    coordinator: State<'_, TrayCoordinator>,
) -> Result<(), crate::error::AppError> {
    coordinator.trigger_background_refresh(app);
    Ok(())
}

#[tauri::command]
pub async fn resize_tray_popover(
    app: AppHandle,
    coordinator: State<'_, TrayCoordinator>,
    height: f64,
) -> Result<(), crate::error::AppError> {
    let clamped = clamp_popover_height(height).map_err(crate::error::AppError::window)?;
    if let Ok(mut guard) = coordinator.state.lock() {
        guard.popover_height = clamped;
    }
    if let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL) {
        window_motion::resize_tray_popover_height(popover, TRAY_POPOVER_WIDTH, clamped)
            .await
            .map_err(crate::error::AppError::window)?;
    }
    Ok(())
}

#[tauri::command]
pub fn hide_tray_popover(app: AppHandle) -> Result<(), crate::error::AppError> {
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        app_for_main
            .state::<TrayCoordinator>()
            .hide_popover_on_main(&app_for_main);
    })
    .map_err(|e| crate::error::AppError::window(e.to_string()))
}

#[tauri::command]
pub fn tray_open_main_window(
    app: AppHandle,
    action: Option<TrayMainWindowAction>,
) -> Result<(), crate::error::AppError> {
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        app_for_main
            .state::<TrayCoordinator>()
            .restore_main_from_tray_on_main(&app_for_main, action);
    })
    .map_err(|e| crate::error::AppError::window(e.to_string()))
}

#[tauri::command]
pub fn tray_quit_app(app: AppHandle) -> Result<(), crate::error::AppError> {
    app.state::<TrayCoordinator>().teardown(&app);
    exit_after_cleanup(&app);
    Ok(())
}

#[cfg(feature = "smoke-harness")]
#[tauri::command]
pub async fn verify_tray_smoke(app: AppHandle) -> Result<TrayStateSnapshot, String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    if app.tray_by_id(TRAY_ID).is_none() {
        return Err("Menu bar tray icon was not initialized".into());
    }
    let coordinator = app.state::<TrayCoordinator>();
    coordinator.refresh_from_app(&app);
    let snapshot = coordinator
        .state
        .lock()
        .map(|s| s.snapshot.clone())
        .map_err(|_| "Tray state lock poisoned".to_string())?;
    // Verify main window can hide to background tray and cleanly restore back to visible:
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        let res = (|| -> Result<(), String> {
            let coordinator = app_for_main.state::<TrayCoordinator>();
            let main = app_for_main
                .get_webview_window("main")
                .ok_or("Missing main window")?;
            coordinator.hide_main_to_tray_on_main(&app_for_main);
            if main.is_visible().unwrap_or(true) {
                return Err("Main window remained visible after hide_main_to_tray".into());
            }
            coordinator.restore_main_from_tray_on_main(&app_for_main, None);
            if !main.is_visible().unwrap_or(false) {
                return Err("Main window did not restore after restore_main_from_tray".into());
            }
            Ok(())
        })();
        let _ = tx.send(res);
    })
    .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Tray smoke verification did not run on main thread".to_string())??;
    Ok(snapshot)
}
