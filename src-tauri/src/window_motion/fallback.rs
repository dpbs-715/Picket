//! Non-macOS fallback window motion implementation using standard Tauri window APIs.

use super::geometry::{
    AnimateWindowFrameReply, LogicalExtent, LogicalPoint, MonitorWorkArea, ScaledMonitorWorkAreas,
    ValidatedRequest, WindowFramePlacement, resolve_visible_window_position,
};
use crate::repository::types::WindowPosition;

pub(super) async fn set_main_window_pinned(
    window: tauri::WebviewWindow,
    pinned: bool,
) -> Result<bool, String> {
    window
        .set_always_on_top(pinned)
        .map_err(|e| e.to_string())?;
    window.is_always_on_top().map_err(|e| e.to_string())
}

#[cfg(feature = "smoke-harness")]
pub(super) async fn read_main_window_visible_on_all_spaces(
    _window: tauri::WebviewWindow,
) -> Result<bool, String> {
    Ok(false)
}

pub fn prefers_reduced_motion() -> bool {
    false
}

pub fn read_monitors_on_main(
    window: &tauri::WebviewWindow,
) -> (Vec<MonitorWorkArea>, Option<MonitorWorkArea>, f64) {
    let monitors: Vec<MonitorWorkArea> = window
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let scale = m.scale_factor().max(0.5);
            let area = m.work_area();
            MonitorWorkArea {
                x: f64::from(area.position.x) / scale,
                y: f64::from(area.position.y) / scale,
                width: f64::from(area.size.width) / scale,
                height: f64::from(area.size.height) / scale,
            }
        })
        .collect();
    let primary = monitors.first().copied();
    let primary_height = primary.map_or(900.0, |p| p.height);
    (monitors, primary, primary_height)
}

#[allow(clippy::too_many_arguments)]
pub fn configure_main_window_startup_on_main(
    window: &tauri::WebviewWindow,
    target_x: f64,
    target_y: f64,
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
    pinned: bool,
) -> Result<(), String> {
    window
        .set_always_on_top(pinned)
        .map_err(|e| e.to_string())?;
    window
        .set_min_size(Some(tauri::LogicalSize::new(
            min_width.min(width),
            min_height.min(height),
        )))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::LogicalPosition::new(target_x, target_y))
        .map_err(|e| e.to_string())?;
    window
        .set_min_size(Some(tauri::LogicalSize::new(min_width, min_height)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn prepare_splash_window_on_main(
    window: &tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    _size: f64,
    _primary_height: f64,
) -> Result<(), String> {
    window
        .set_position(tauri::LogicalPosition::new(top_left_x, top_left_y))
        .map_err(|e| e.to_string())
}

pub async fn show_splash_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())
}

pub async fn move_splash_window_origin(
    window: tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    _size: f64,
    _primary_height: f64,
) -> Result<(), String> {
    window
        .set_position(tauri::LogicalPosition::new(top_left_x, top_left_y))
        .map_err(|e| e.to_string())
}

pub async fn reveal_main_window_smoothly(
    window: tauri::WebviewWindow,
    _duration_ms: u32,
) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    Ok(())
}

pub async fn ensure_main_window_opaque(_window: tauri::WebviewWindow) {}

pub fn read_monitors_with_scale_on_main(window: &tauri::WebviewWindow) -> ScaledMonitorWorkAreas {
    let (monitors, primary, primary_height) = read_monitors_on_main(window);
    let scaled: Vec<(MonitorWorkArea, f64)> = monitors.into_iter().map(|m| (m, 1.0)).collect();
    let scaled_primary = primary.map(|m| (m, 1.0));
    (scaled, scaled_primary, primary_height)
}

pub fn mouse_location_logical_on_main(_primary_height: f64) -> Option<(f64, f64)> {
    None
}

pub fn prepare_tray_popover_window_on_main(
    _window: &tauri::WebviewWindow,
    _width: f64,
    _height: f64,
) -> Result<(), String> {
    Ok(())
}

pub fn show_tray_popover_on_main(
    window: &tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    width: f64,
    height: f64,
    _primary_height: f64,
) -> Result<(), String> {
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::LogicalPosition::new(top_left_x, top_left_y))
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    Ok(())
}

pub async fn resize_tray_popover_height(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())
}

pub fn activate_and_focus_main_on_main(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

pub(super) async fn animate_main_window(
    window: tauri::WebviewWindow,
    req: ValidatedRequest,
) -> Result<AnimateWindowFrameReply, String> {
    let scale = window.scale_factor().unwrap_or(1.0).max(0.5);
    let before_pos = window.outer_position().map_err(|e| e.to_string())?;
    let before_size = window.inner_size().map_err(|e| e.to_string())?;
    let previous_position = LogicalPoint {
        x: (f64::from(before_pos.x) / scale).round() as i32,
        y: (f64::from(before_pos.y) / scale).round() as i32,
    };
    let previous_size = LogicalExtent {
        width: (f64::from(before_size.width) / scale).round() as u32,
        height: (f64::from(before_size.height) / scale).round() as u32,
    };
    if let Some(pinned) = req.pinned {
        window
            .set_always_on_top(pinned)
            .map_err(|e| e.to_string())?;
    }
    let target_w = req.width.unwrap_or(f64::from(previous_size.width));
    let pre_min_w = req
        .min_width
        .min(target_w)
        .min(f64::from(previous_size.width));
    let pre_min_h = req
        .min_height
        .min(req.height)
        .min(f64::from(previous_size.height));
    window
        .set_min_size(Some(tauri::LogicalSize::new(pre_min_w, pre_min_h)))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(target_w, req.height))
        .map_err(|e| e.to_string())?;
    match req.placement {
        WindowFramePlacement::AnchorTopLeft => {
            window.set_position(before_pos).map_err(|e| e.to_string())?;
        }
        WindowFramePlacement::RestorePosition => {
            if let (Some(x), Some(y)) = (req.target_x, req.target_y) {
                let (monitors, primary, _) = read_monitors_on_main(&window);
                let (cx, cy, _) = resolve_visible_window_position(
                    Some(WindowPosition {
                        x: x.round() as i32,
                        y: y.round() as i32,
                    }),
                    target_w,
                    req.height,
                    &monitors,
                    primary,
                );
                window
                    .set_position(tauri::LogicalPosition::new(cx, cy))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    window
        .set_min_size(Some(tauri::LogicalSize::new(req.min_width, req.min_height)))
        .map_err(|e| e.to_string())?;
    let after_scale = window.scale_factor().unwrap_or(scale).max(0.5);
    let after_pos = window.outer_position().map_err(|e| e.to_string())?;
    let after_size = window.inner_size().map_err(|e| e.to_string())?;
    Ok(AnimateWindowFrameReply {
        pinned: window.is_always_on_top().map_err(|e| e.to_string())?,
        previous_position,
        previous_size,
        position: LogicalPoint {
            x: (f64::from(after_pos.x) / after_scale).round() as i32,
            y: (f64::from(after_pos.y) / after_scale).round() as i32,
        },
        size: LogicalExtent {
            width: (f64::from(after_size.width) / after_scale).round() as u32,
            height: (f64::from(after_size.height) / after_scale).round() as u32,
        },
        animated: false,
    })
}
