//! Platform-independent window geometry types, validation, and coordinate transforms.

use crate::repository::types::WindowPosition;
use serde::{Deserialize, Serialize};

pub(super) const MIN_WINDOW_WIDTH: f64 = 400.0;
pub(super) const MIN_WINDOW_HEIGHT: f64 = 82.0;
pub(super) const MAX_WINDOW_DIMENSION: f64 = 16_384.0;
pub(super) const MAX_COORDINATE_ABS: f64 = 65_536.0;
pub(super) const MAX_DURATION_MS: u32 = 600;
pub(super) const FRAME_EPSILON: f64 = 0.5;
const TITLEBAR_VISIBLE_HEIGHT: f64 = 38.0;
const MIN_VISIBLE_HORIZONTAL_OVERLAP: f64 = 80.0;
pub(super) const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
pub(super) const NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
pub(super) const NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT: usize = 1 << 3;
pub(super) const NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE: usize = 1 << 6;
/// Floats one level above `NSFloatingWindowLevel` (3) so the transient launch splash stays above
/// even a pinned main window while the main window fades in underneath its landing ripple.
pub(super) const NS_SPLASH_WINDOW_LEVEL: isize = 4;
/// Floats at `NSStatusWindowLevel` (25) right beneath the macOS menu bar and above floating windows.
pub(super) const NS_TRAY_POPOVER_WINDOW_LEVEL: isize = 25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorWorkArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub type ScaledMonitorWorkAreas = (
    Vec<(MonitorWorkArea, f64)>,
    Option<(MonitorWorkArea, f64)>,
    f64,
);

/// Compute the next macOS `NSWindow.collectionBehavior` bitmask for cross-Space visibility.
/// Clears mutually exclusive `MoveToActiveSpace` when enabling `CanJoinAllSpaces`, while
/// preserving unrelated window behaviors (such as `FullScreenPrimary` and `Managed`).
pub(super) fn next_collection_behavior(current: usize, visible_on_all_spaces: bool) -> usize {
    if visible_on_all_spaces {
        (current & !NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE)
            | NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
    } else {
        current & !NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowFramePlacement {
    AnchorTopLeft,
    RestorePosition,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimateWindowFrameRequest {
    /// Target logical width. When omitted, preserves the window's live logical width.
    pub width: Option<f64>,
    pub height: f64,
    pub min_width: f64,
    pub min_height: f64,
    pub placement: WindowFramePlacement,
    pub target_x: Option<f64>,
    pub target_y: Option<f64>,
    pub duration_ms: u32,
    pub pinned: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalExtent {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimateWindowFrameReply {
    pub pinned: bool,
    pub previous_position: LogicalPoint,
    pub previous_size: LogicalExtent,
    pub position: LogicalPoint,
    pub size: LogicalExtent,
    pub animated: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NSPoint {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NSSize {
    pub(super) width: f64,
    pub(super) height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NSRect {
    pub(super) origin: NSPoint,
    pub(super) size: NSSize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ValidatedRequest {
    pub(super) width: Option<f64>,
    pub(super) height: f64,
    pub(super) min_width: f64,
    pub(super) min_height: f64,
    pub(super) placement: WindowFramePlacement,
    pub(super) target_x: Option<f64>,
    pub(super) target_y: Option<f64>,
    pub(super) duration_ms: u32,
    pub(super) pinned: Option<bool>,
}

fn validate_dimension(value: f64, min: f64, label: &str) -> Result<f64, String> {
    if !value.is_finite() || value < min || value > MAX_WINDOW_DIMENSION {
        return Err(format!(
            "Invalid window {label}: expected finite value in [{min}, {MAX_WINDOW_DIMENSION}]"
        ));
    }
    Ok(value)
}

fn validate_coordinate(value: f64, label: &str) -> Result<f64, String> {
    if !value.is_finite() || value.abs() > MAX_COORDINATE_ABS {
        return Err(format!(
            "Invalid window {label}: expected finite value within ±{MAX_COORDINATE_ABS}"
        ));
    }
    Ok(value)
}

pub(super) fn validate_request(
    req: &AnimateWindowFrameRequest,
) -> Result<ValidatedRequest, String> {
    let width = match req.width {
        Some(w) => Some(validate_dimension(w, MIN_WINDOW_WIDTH, "width")?),
        None => None,
    };
    let height = validate_dimension(req.height, MIN_WINDOW_HEIGHT, "height")?;
    let min_width = validate_dimension(req.min_width, MIN_WINDOW_WIDTH, "minWidth")?;
    let min_height = validate_dimension(req.min_height, MIN_WINDOW_HEIGHT, "minHeight")?;
    let target_x = match req.target_x {
        Some(x) => Some(validate_coordinate(x, "targetX")?),
        None => None,
    };
    let target_y = match req.target_y {
        Some(y) => Some(validate_coordinate(y, "targetY")?),
        None => None,
    };
    Ok(ValidatedRequest {
        width,
        height,
        min_width,
        min_height,
        placement: req.placement,
        target_x,
        target_y,
        duration_ms: req.duration_ms.min(MAX_DURATION_MS),
        pinned: req.pinned,
    })
}

/// Convert an AppKit `NSWindow.frame` (bottom-left origin of primary screen) into
/// top-left logical coordinates matching Tauri's `outerPosition() / scaleFactor`.
pub(super) fn appkit_frame_to_top_left(frame: NSRect, primary_screen_height: f64) -> LogicalPoint {
    let top_y = primary_screen_height - (frame.origin.y + frame.size.height);
    LogicalPoint {
        x: frame.origin.x.round() as i32,
        y: top_y.round() as i32,
    }
}

pub(super) fn content_size_to_extent(size: NSSize) -> LogicalExtent {
    LogicalExtent {
        width: size.width.max(0.0).round() as u32,
        height: size.height.max(0.0).round() as u32,
    }
}

/// Compute the target AppKit `NSRect` in one atomic coordinate transform.
/// - `AnchorTopLeft` keeps `origin.x` and the top edge (`origin.y + size.height`) invariant
///   directly in floating-point AppKit screen points (0.0px Retina drift).
/// - `RestorePosition` converts top-left logical `(target_x, target_y)` back to AppKit
///   bottom-left origin using `primary_screen_height`.
pub(super) fn compute_target_frame(
    before_frame: NSRect,
    before_content_size: NSSize,
    primary_screen_height: f64,
    req: ValidatedRequest,
) -> NSRect {
    let chrome_w = (before_frame.size.width - before_content_size.width).max(0.0);
    let chrome_h = (before_frame.size.height - before_content_size.height).max(0.0);
    let target_content_w = req.width.unwrap_or(before_content_size.width);
    let target_content_h = req.height;
    let target_frame_w = target_content_w + chrome_w;
    let target_frame_h = target_content_h + chrome_h;

    let origin = match req.placement {
        WindowFramePlacement::AnchorTopLeft => NSPoint {
            x: before_frame.origin.x,
            y: before_frame.origin.y + before_frame.size.height - target_frame_h,
        },
        WindowFramePlacement::RestorePosition => {
            let fallback_top_left_y =
                primary_screen_height - (before_frame.origin.y + before_frame.size.height);
            let top_left_x = req.target_x.unwrap_or(before_frame.origin.x);
            let top_left_y = req.target_y.unwrap_or(fallback_top_left_y);
            NSPoint {
                x: top_left_x,
                y: primary_screen_height - top_left_y - target_frame_h,
            }
        }
    };

    NSRect {
        origin,
        size: NSSize {
            width: target_frame_w,
            height: target_frame_h,
        },
    }
}

pub(super) fn frames_within_epsilon(a: NSRect, b: NSRect) -> bool {
    (a.origin.x - b.origin.x).abs() <= FRAME_EPSILON
        && (a.origin.y - b.origin.y).abs() <= FRAME_EPSILON
        && (a.size.width - b.size.width).abs() <= FRAME_EPSILON
        && (a.size.height - b.size.height).abs() <= FRAME_EPSILON
}

/// Single authoritative multi-monitor window clamping helper shared by cold startup
/// placement (`startup_splash.rs`) and runtime `animate_window_frame` (`RestorePosition`).
pub fn resolve_visible_window_position(
    desired: Option<WindowPosition>,
    width: f64,
    height: f64,
    monitors: &[MonitorWorkArea],
    primary: Option<MonitorWorkArea>,
) -> (f64, f64, MonitorWorkArea) {
    let fallback_monitor =
        primary
            .or_else(|| monitors.first().copied())
            .unwrap_or(MonitorWorkArea {
                x: 0.0,
                y: 25.0,
                width: 1440.0,
                height: 875.0,
            });
    let pool: Vec<MonitorWorkArea> = if !monitors.is_empty() {
        monitors.to_vec()
    } else {
        vec![fallback_monitor]
    };

    let Some(pos) = desired else {
        let centered_x = (fallback_monitor.x + (fallback_monitor.width - width) * 0.5).round();
        let centered_y = (fallback_monitor.y + (fallback_monitor.height - height) * 0.5).round();
        return (centered_x, centered_y, fallback_monitor);
    };

    let desired_x = f64::from(pos.x);
    let desired_y = f64::from(pos.y);
    let required_overlap = MIN_VISIBLE_HORIZONTAL_OVERLAP.min(width.max(40.0));

    if let Some(matched) = pool.iter().copied().find(|monitor| {
        let overlap_left = desired_x.max(monitor.x);
        let overlap_right = (desired_x + width).min(monitor.x + monitor.width);
        let horizontal_ok = overlap_right - overlap_left >= required_overlap;
        let vertical_ok = desired_y >= monitor.y
            && desired_y + TITLEBAR_VISIBLE_HEIGHT <= monitor.y + monitor.height;
        horizontal_ok && vertical_ok
    }) {
        return (desired_x.round(), desired_y.round(), matched);
    }

    let max_x = fallback_monitor
        .x
        .max(fallback_monitor.x + fallback_monitor.width - width);
    let max_y = fallback_monitor
        .y
        .max(fallback_monitor.y + fallback_monitor.height - height);
    let clamped_x = desired_x.round().clamp(fallback_monitor.x, max_x);
    let clamped_y = desired_y.round().clamp(fallback_monitor.y, max_y);
    (clamped_x, clamped_y, fallback_monitor)
}
