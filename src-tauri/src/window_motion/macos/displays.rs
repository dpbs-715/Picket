//! macOS display geometry, backing scale factors, reduced-motion preference, and cursor position.

use super::ffi::{
    main_thread_marker, ns_window_ref, prefers_reduced_motion_macos, primary_screen_height,
};
use crate::window_motion::geometry::{MonitorWorkArea, ScaledMonitorWorkAreas};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSEvent, NSScreen};

pub fn prefers_reduced_motion() -> bool {
    autoreleasepool(|_| prefers_reduced_motion_macos())
}

/// Reads all active macOS screens' logical work areas with their backing scale factor plus
/// `NSScreen.screens[0].frame.size.height`. Must run on the main thread.
pub fn read_monitors_with_scale_on_main(window: &tauri::WebviewWindow) -> ScaledMonitorWorkAreas {
    autoreleasepool(|_| {
        let mtm = main_thread_marker();
        let screens = NSScreen::screens(mtm);
        let mut monitors = Vec::with_capacity(screens.len());
        let mut primary_height = 900.0;

        if let Some(first) = screens.firstObject() {
            let first_height = first.frame().size.height;
            if first_height > 0.0 {
                primary_height = first_height;
            }
        }
        for screen in screens.iter() {
            let vf = screen.visibleFrame();
            let scale = screen.backingScaleFactor().max(1.0);
            if vf.size.width > 0.0 && vf.size.height > 0.0 {
                monitors.push((
                    MonitorWorkArea {
                        x: vf.origin.x,
                        y: primary_height - (vf.origin.y + vf.size.height),
                        width: vf.size.width,
                        height: vf.size.height,
                    },
                    scale,
                ));
            }
        }
        if monitors.is_empty()
            && let Ok(win) = unsafe { ns_window_ref(window, "Main NSWindow is unavailable") }
        {
            primary_height = primary_screen_height(win);
        }

        let primary = monitors.first().copied();
        (monitors, primary, primary_height)
    })
}

/// Reads all active macOS screens' logical work areas (excluding menu bar & Dock) in top-left
/// global coordinates alongside `NSScreen.screens[0].frame.size.height`. Must run on main thread.
pub fn read_monitors_on_main(
    window: &tauri::WebviewWindow,
) -> (Vec<MonitorWorkArea>, Option<MonitorWorkArea>, f64) {
    let (scaled_monitors, scaled_primary, primary_height) =
        read_monitors_with_scale_on_main(window);
    let monitors = scaled_monitors.into_iter().map(|(area, _)| area).collect();
    let primary = scaled_primary.map(|(area, _)| area);
    (monitors, primary, primary_height)
}

pub fn mouse_location_logical_on_main(primary_height: f64) -> Option<(f64, f64)> {
    autoreleasepool(|_| {
        let pt = NSEvent::mouseLocation();
        if !pt.x.is_finite() || !pt.y.is_finite() {
            return None;
        }
        Some((pt.x, primary_height - pt.y))
    })
}
