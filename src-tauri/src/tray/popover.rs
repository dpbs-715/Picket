//! Menu bar icon assets, anchor coordinate conversion, popover positioning/clamping,
//! Dock activation-policy toggling, and borderless popover WebView construction.

use super::TrayCoordinator;
use crate::window_motion::{self, MonitorWorkArea};
use tauri::{AppHandle, Manager, Position, Rect, Size, image::Image};

pub const TRAY_ID: &str = "picket-status-tray";
pub const TRAY_POPOVER_LABEL: &str = "tray-popover";
pub const TRAY_POPOVER_WIDTH: f64 = 320.0;
pub const TRAY_POPOVER_MIN_HEIGHT: f64 = 152.0;
pub const TRAY_POPOVER_DEFAULT_HEIGHT: f64 = 228.0;
pub const TRAY_POPOVER_MAX_HEIGHT: f64 = 460.0;

pub(super) const TRAY_ICON_WIDTH: u32 = 48;
pub(super) const TRAY_ICON_HEIGHT: u32 = 36;
const TRAY_SCREEN_MARGIN: f64 = 8.0;
const TRAY_BELOW_BAR_GAP: f64 = 6.0;

pub(super) const TRAY_ICON_NORMAL: &[u8] = include_bytes!("../../icons/tray-template.rgba");
pub(super) const TRAY_ICON_DOT: &[u8] = include_bytes!("../../icons/tray-template-dot.rgba");

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

pub(super) fn clamp_popover_height(height: f64) -> Result<f64, String> {
    if !height.is_finite() || height <= 0.0 {
        return Err("Invalid tray popover height".into());
    }
    Ok(height
        .clamp(TRAY_POPOVER_MIN_HEIGHT, TRAY_POPOVER_MAX_HEIGHT)
        .ceil())
}

pub(super) fn tray_icon_image(attention: bool) -> Image<'static> {
    let bytes = if attention {
        TRAY_ICON_DOT
    } else {
        TRAY_ICON_NORMAL
    };
    Image::new(bytes, TRAY_ICON_WIDTH, TRAY_ICON_HEIGHT)
}

pub(super) fn apply_dock_visibility_on_main(app: &AppHandle, show_dock: bool) {
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

pub(super) fn ensure_tray_popover_window_on_main(
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
    let initial_theme = app.state::<TrayCoordinator>().current_theme();
    let _ = window_motion::apply_window_theme(&built, initial_theme, false);
    Some(built)
}
