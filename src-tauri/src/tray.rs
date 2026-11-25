//! macOS menu bar tray residency, dynamic Dock auto-hide, and minimal read-only status popover.
//!
//! Keeps Picket quietly resident in the macOS top menu bar when the main window is closed:
//! - Closing the main window hides it and switches macOS activation policy to `Accessory` (hides Dock icon).
//! - Clicking the menu bar icon toggles a lightweight borderless status popover anchored directly
//!   beneath the status item without ever showing the Dock icon or switching Desktop Spaces.
//! - Clicking "Open Main Window", Settings, a repository card, or a background notification
//!   restores `Regular` activation policy (Dock icon returns) and focuses the main window.
use crate::{
    foundation::lifecycle::{DesktopLifecycle, exit_after_cleanup},
    i18n::{self, LanguageKind},
    window_motion::{self, MonitorWorkArea},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, Position, Rect, Size, State,
    image::Image,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub const TRAY_ID: &str = "picket-status-tray";
pub const TRAY_POPOVER_LABEL: &str = "tray-popover";
pub const TRAY_POPOVER_WIDTH: f64 = 320.0;
pub const TRAY_POPOVER_MIN_HEIGHT: f64 = 152.0;
pub const TRAY_POPOVER_DEFAULT_HEIGHT: f64 = 228.0;
pub const TRAY_POPOVER_MAX_HEIGHT: f64 = 460.0;

const TRAY_ICON_WIDTH: u32 = 48;
const TRAY_ICON_HEIGHT: u32 = 36;
const TRAY_BLUR_TOGGLE_DEBOUNCE: Duration = Duration::from_millis(220);
const TRAY_SCREEN_MARGIN: f64 = 8.0;
const TRAY_BELOW_BAR_GAP: f64 = 6.0;
const MAX_TRAY_REPOSITORIES: usize = 64;

const TRAY_STATE_EVENT: &str = "picket:tray-state";
const TRAY_REFRESH_REQUESTED_EVENT: &str = "picket:tray-refresh-requested";
const TRAY_MAIN_ACTION_EVENT: &str = "picket:tray-main-action";
const TRAY_POPOVER_SHOWN_EVENT: &str = "picket:tray-popover-shown";

const TRAY_ICON_NORMAL: &[u8] = include_bytes!("../icons/tray-template.rgba");
const TRAY_ICON_DOT: &[u8] = include_bytes!("../icons/tray-template-dot.rgba");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraySessionStatus {
    #[default]
    Idle,
    Running,
    NeedsDecision,
    Conflict,
    WaitingManual,
    PushFailed,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrayRepositorySummary {
    pub repository_id: String,
    pub worktree_id: String,
    pub name: String,
    pub branch: String,
    pub detached: bool,
    pub ahead: u32,
    pub behind: u32,
    pub has_upstream: bool,
    pub change_count: u32,
    pub local_keep_count: u32,
    pub worktree_count: u32,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_error: Option<String>,
    #[serde(default)]
    pub session_status: TraySessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrayStateSnapshot {
    pub protocol_version: u8,
    #[serde(default)]
    pub locale: LanguageKind,
    pub has_update: bool,
    pub refreshing: bool,
    pub repositories: Vec<TrayRepositorySummary>,
}

impl Default for TrayStateSnapshot {
    fn default() -> Self {
        Self {
            protocol_version: 1,
            locale: LanguageKind::En,
            has_update: false,
            refreshing: false,
            repositories: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TrayMainWindowAction {
    Show,
    OpenSettings,
    #[serde(rename_all = "camelCase")]
    FocusWorktree {
        repository_id: String,
        worktree_id: String,
        open_dock: bool,
    },
}

struct TrayRuntimeState {
    snapshot: TrayStateSnapshot,
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
}

fn validate_tray_snapshot(snapshot: &TrayStateSnapshot) -> Result<(), String> {
    if snapshot.protocol_version != 1 {
        return Err("Unsupported tray state protocol version".into());
    }
    if snapshot.repositories.len() > MAX_TRAY_REPOSITORIES {
        return Err("Tray repository summary list exceeds limit".into());
    }
    Ok(())
}

pub fn tray_needs_attention(snapshot: &TrayStateSnapshot) -> bool {
    snapshot.has_update
        || snapshot.repositories.iter().any(|repo| {
            matches!(
                repo.session_status,
                TraySessionStatus::NeedsDecision
                    | TraySessionStatus::Conflict
                    | TraySessionStatus::PushFailed
            )
        })
}

pub fn build_tray_tooltip(snapshot: &TrayStateSnapshot) -> String {
    let locale = snapshot.locale;
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::Conflict)
    {
        return i18n::tray_tooltip_conflict(locale).to_string();
    }
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::NeedsDecision)
    {
        return i18n::tray_tooltip_needs_decision(locale).to_string();
    }
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::PushFailed)
    {
        return i18n::tray_tooltip_push_failed(locale).to_string();
    }
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::Running)
    {
        return i18n::tray_tooltip_running(locale).to_string();
    }
    let changed_repos = snapshot
        .repositories
        .iter()
        .filter(|r| r.change_count > 0 || r.ahead > 0 || r.behind > 0 || r.read_error.is_some())
        .count();
    if changed_repos > 0 {
        return i18n::tray_tooltip_changed_repos(locale, changed_repos);
    }
    if snapshot.has_update {
        return i18n::tray_tooltip_update_available(locale).to_string();
    }
    "Picket".to_string()
}

/// Resolves a Tauri tray `Rect` (physical on macOS) into logical top-left coordinates using the
/// clicked display's backing scale factor. Uses the unambiguous AppKit logical cursor X when
/// available so mixed 1x + 2x Retina multi-monitor setups never misidentify the active screen.
pub fn resolve_tray_anchor_logical(
    rect: Rect,
    logical_cursor_x: Option<f64>,
    monitors: &[(MonitorWorkArea, f64)],
    primary: Option<(MonitorWorkArea, f64)>,
) -> (f64, f64) {
    let fallback_scale = primary
        .or_else(|| monitors.first().copied())
        .map_or(2.0, |(_, s)| s.max(1.0));

    let matched_scale = match rect.position {
        Position::Logical(_) => 1.0,
        Position::Physical(pos) => logical_cursor_x
            .and_then(|cx| {
                monitors
                    .iter()
                    .copied()
                    .find(|(mon, _)| cx >= mon.x && cx <= mon.x + mon.width)
            })
            .or_else(|| {
                monitors.iter().copied().find(|(mon, scale)| {
                    let s = scale.max(1.0);
                    let lx = f64::from(pos.x) / s;
                    lx >= mon.x - 16.0 && lx <= mon.x + mon.width + 16.0
                })
            })
            .map_or(fallback_scale, |(_, s)| s.max(1.0)),
    };

    let (x, y) = match rect.position {
        Position::Logical(p) => (p.x, p.y),
        Position::Physical(p) => (
            f64::from(p.x) / matched_scale,
            f64::from(p.y) / matched_scale,
        ),
    };
    let (w, h) = match rect.size {
        Size::Logical(s) => (s.width, s.height),
        Size::Physical(s) => (
            f64::from(s.width) / matched_scale,
            f64::from(s.height) / matched_scale,
        ),
    };
    (x + w * 0.5, y + h)
}

/// Computes top-left logical coordinates for the tray popover anchored directly beneath the status
/// bar item and clamped cleanly inside the target monitor's visible work area.
pub fn compute_popover_position(
    icon_center_x: f64,
    icon_bottom_y: f64,
    width: f64,
    height: f64,
    monitors: &[MonitorWorkArea],
    primary: Option<MonitorWorkArea>,
) -> (f64, f64) {
    let fallback = primary
        .or_else(|| monitors.first().copied())
        .unwrap_or(MonitorWorkArea {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 875.0,
        });
    let target = monitors
        .iter()
        .copied()
        .find(|m| icon_center_x >= m.x && icon_center_x <= m.x + m.width)
        .unwrap_or(fallback);

    let min_x = target.x + TRAY_SCREEN_MARGIN;
    let max_x = (target.x + target.width - width - TRAY_SCREEN_MARGIN).max(min_x);
    let x = (icon_center_x - width * 0.5).clamp(min_x, max_x).round();

    let min_y = target.y + 4.0;
    let max_y = (target.y + target.height - height - TRAY_SCREEN_MARGIN).max(min_y);
    let desired_y = (icon_bottom_y + TRAY_BELOW_BAR_GAP).max(min_y);
    let y = desired_y.clamp(min_y, max_y).round();
    (x, y)
}

fn clamp_popover_height(height: f64) -> Result<f64, String> {
    if !height.is_finite() || height <= 0.0 {
        return Err("Invalid tray popover height".into());
    }
    Ok(height
        .clamp(TRAY_POPOVER_MIN_HEIGHT, TRAY_POPOVER_MAX_HEIGHT)
        .ceil())
}

fn tray_icon_image(attention: bool) -> Image<'static> {
    let bytes = if attention {
        TRAY_ICON_DOT
    } else {
        TRAY_ICON_NORMAL
    };
    Image::new(bytes, TRAY_ICON_WIDTH, TRAY_ICON_HEIGHT)
}

fn apply_dock_visibility_on_main(app: &AppHandle, show_dock: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if show_dock {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, show_dock);
    }
}

fn ensure_tray_popover_window_on_main(
    app: &AppHandle,
    height: f64,
) -> Option<tauri::WebviewWindow> {
    if let Some(existing) = app.get_webview_window(TRAY_POPOVER_LABEL) {
        return Some(existing);
    }
    let built = tauri::WebviewWindowBuilder::new(
        app,
        TRAY_POPOVER_LABEL,
        tauri::WebviewUrl::App("tray.html".into()),
    )
    .title("Picket")
    .inner_size(TRAY_POPOVER_WIDTH, height)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()
    .ok()?;
    let _ = window_motion::prepare_tray_popover_window_on_main(&built, TRAY_POPOVER_WIDTH, height);
    Some(built)
}

impl TrayCoordinator {
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

        // Pre-warm the hidden popover webview in normal launches so the first click opens
        // immediately without a blank frame.
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

        let snapshot = self.state.lock().map(|s| s.snapshot.clone()).ok();
        if let Some(ref snap) = snapshot {
            let _ = popover.emit(TRAY_STATE_EVENT, snap);
        }
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

        // Trigger one lightweight repository refresh when the user opens the popover.
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.emit(TRAY_REFRESH_REQUESTED_EVENT, ());
        }
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

    pub fn update_snapshot(&self, app: &AppHandle, snapshot: TrayStateSnapshot) {
        let attention = tray_needs_attention(&snapshot);
        let tooltip = build_tray_tooltip(&snapshot);
        let (icon_changed, tooltip_changed) = {
            let Ok(mut guard) = self.state.lock() else {
                return;
            };
            let icon_changed = guard.badge_active != attention;
            let tooltip_changed = guard.last_tooltip != tooltip;
            guard.snapshot = snapshot.clone();
            guard.badge_active = attention;
            guard.last_tooltip = tooltip.clone();
            (icon_changed, tooltip_changed)
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

        if let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL) {
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
    coordinator: State<'_, TrayCoordinator>,
) -> Result<TrayStateSnapshot, String> {
    coordinator
        .state
        .lock()
        .map(|s| s.snapshot.clone())
        .map_err(|_| "Tray state lock poisoned".to_string())
}

#[tauri::command]
pub fn sync_tray_state(
    app: AppHandle,
    coordinator: State<'_, TrayCoordinator>,
    snapshot: TrayStateSnapshot,
) -> Result<(), String> {
    validate_tray_snapshot(&snapshot)?;
    coordinator.update_snapshot(&app, snapshot);
    Ok(())
}

#[tauri::command]
pub fn request_tray_refresh(app: AppHandle) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit(TRAY_REFRESH_REQUESTED_EVENT, ());
    }
    Ok(())
}

#[tauri::command]
pub async fn resize_tray_popover(
    app: AppHandle,
    coordinator: State<'_, TrayCoordinator>,
    height: f64,
) -> Result<(), String> {
    let clamped = clamp_popover_height(height)?;
    if let Ok(mut guard) = coordinator.state.lock() {
        guard.popover_height = clamped;
    }
    if let Some(popover) = app.get_webview_window(TRAY_POPOVER_LABEL) {
        window_motion::resize_tray_popover_height(popover, TRAY_POPOVER_WIDTH, clamped).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn hide_tray_popover(app: AppHandle) -> Result<(), String> {
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        app_for_main
            .state::<TrayCoordinator>()
            .hide_popover_on_main(&app_for_main);
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tray_open_main_window(
    app: AppHandle,
    action: Option<TrayMainWindowAction>,
) -> Result<(), String> {
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        app_for_main
            .state::<TrayCoordinator>()
            .restore_main_from_tray_on_main(&app_for_main, action);
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tray_quit_app(app: AppHandle) -> Result<(), String> {
    app.state::<TrayCoordinator>().teardown(&app);
    exit_after_cleanup(&app);
    Ok(())
}

#[tauri::command]
pub async fn verify_tray_smoke(app: AppHandle) -> Result<TrayStateSnapshot, String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    if app.tray_by_id(TRAY_ID).is_none() {
        return Err("Menu bar tray icon was not initialized".into());
    }
    let snapshot = app
        .state::<TrayCoordinator>()
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

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::{PhysicalPosition, PhysicalSize};

    fn sample_repo(name: &str, changes: u32, status: TraySessionStatus) -> TrayRepositorySummary {
        TrayRepositorySummary {
            repository_id: format!("repo:{name}"),
            worktree_id: format!("wt:{name}"),
            name: name.to_string(),
            branch: "main".to_string(),
            detached: false,
            ahead: 0,
            behind: 0,
            has_upstream: true,
            change_count: changes,
            local_keep_count: 0,
            worktree_count: 1,
            active: false,
            read_error: None,
            session_status: status,
            status_detail: None,
        }
    }

    #[test]
    fn tray_template_assets_have_exact_48x36_rgba_dimensions() {
        let expected = (TRAY_ICON_WIDTH * TRAY_ICON_HEIGHT * 4) as usize;
        assert_eq!(TRAY_ICON_NORMAL.len(), expected);
        assert_eq!(TRAY_ICON_DOT.len(), expected);
        // Dot icon must have more opaque pixels than idle icon:
        let normal_alpha: u64 = TRAY_ICON_NORMAL
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| u64::from(px[3]))
            .sum();
        let dot_alpha: u64 = TRAY_ICON_DOT
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| u64::from(px[3]))
            .sum();
        assert!(normal_alpha > 0);
        assert!(dot_alpha > normal_alpha);
    }

    #[test]
    fn popover_positioning_centers_under_icon_clamps_right_edge_and_supports_secondary_monitors() {
        let primary = MonitorWorkArea {
            x: 0.0,
            y: 25.0,
            width: 1512.0,
            height: 945.0,
        };
        let secondary = MonitorWorkArea {
            x: 1512.0,
            y: 0.0,
            width: 2560.0,
            height: 1415.0,
        };

        // Right edge of primary Retina screen (icon center near x = 1480 -> physical 2960):
        let rect = Rect {
            position: Position::Physical(PhysicalPosition::new(2936, 0)),
            size: Size::Physical(PhysicalSize::new(48, 50)),
        };
        let (cx, bottom) = resolve_tray_anchor_logical(
            rect,
            Some(1480.0),
            &[(primary, 2.0), (secondary, 1.0)],
            Some((primary, 2.0)),
        );
        assert_eq!((cx, bottom), (1480.0, 25.0));

        let (px, py) = compute_popover_position(
            cx,
            bottom,
            TRAY_POPOVER_WIDTH,
            240.0,
            &[primary, secondary],
            Some(primary),
        );
        // Clamped 8px from right edge: 1512 - 320 - 8 = 1184
        assert_eq!(px, 1184.0);
        assert_eq!(py, 31.0);

        // Secondary 1x monitor icon centered at x = 2200:
        let sec_rect = Rect {
            position: Position::Physical(PhysicalPosition::new(2188, 0)),
            size: Size::Physical(PhysicalSize::new(24, 24)),
        };
        let (scx, sbottom) = resolve_tray_anchor_logical(
            sec_rect,
            Some(2200.0),
            &[(primary, 2.0), (secondary, 1.0)],
            Some((primary, 2.0)),
        );
        let (spx, spy) = compute_popover_position(
            scx,
            sbottom,
            TRAY_POPOVER_WIDTH,
            240.0,
            &[primary, secondary],
            Some(primary),
        );
        assert_eq!(spx, 2200.0 - 160.0);
        assert_eq!(spy, 30.0);
    }

    #[test]
    fn tray_attention_and_tooltip_prioritize_urgent_sessions_changes_and_updates() {
        let clean = TrayStateSnapshot {
            protocol_version: 1,
            locale: LanguageKind::Zh,
            has_update: false,
            refreshing: false,
            repositories: vec![sample_repo("alpha", 0, TraySessionStatus::Idle)],
        };
        assert!(!tray_needs_attention(&clean));
        assert_eq!(build_tray_tooltip(&clean), "Picket");

        let with_changes = TrayStateSnapshot {
            protocol_version: 1,
            locale: LanguageKind::En,
            has_update: false,
            refreshing: false,
            repositories: vec![
                sample_repo("alpha", 3, TraySessionStatus::Idle),
                sample_repo("beta", 1, TraySessionStatus::Idle),
            ],
        };
        assert!(!tray_needs_attention(&with_changes));
        assert_eq!(
            build_tray_tooltip(&with_changes),
            "Picket · 2 repositories with changes"
        );

        let with_decision = TrayStateSnapshot {
            protocol_version: 1,
            locale: LanguageKind::Zh,
            has_update: false,
            refreshing: false,
            repositories: vec![sample_repo("alpha", 2, TraySessionStatus::NeedsDecision)],
        };
        assert!(tray_needs_attention(&with_decision));
        assert_eq!(build_tray_tooltip(&with_decision), "Picket · 待你决定");

        let with_update = TrayStateSnapshot {
            protocol_version: 1,
            locale: LanguageKind::En,
            has_update: true,
            refreshing: false,
            repositories: vec![],
        };
        assert!(tray_needs_attention(&with_update));
        assert_eq!(
            build_tray_tooltip(&with_update),
            "Picket · Update available"
        );
    }

    #[test]
    fn tray_snapshot_validation_rejects_unknown_fields_and_invalid_version() {
        let bad = serde_json::from_str::<TrayStateSnapshot>(
            r#"{"protocolVersion":1,"locale":"en","hasUpdate":false,"refreshing":false,"repositories":[],"extra":1}"#,
        );
        assert!(bad.is_err());

        let wrong_version = TrayStateSnapshot {
            protocol_version: 2,
            ..TrayStateSnapshot::default()
        };
        assert!(validate_tray_snapshot(&wrong_version).is_err());
    }
}
