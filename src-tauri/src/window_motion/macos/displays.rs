//! macOS display geometry, backing scale factors, reduced-motion preference, and cursor position.

use super::ffi::{
    AutoreleasePoolGuard, msg_send_point, msg_send_rect, objc_getClass, objc_msgSend,
    prefers_reduced_motion_macos, primary_screen_height, sel_registerName,
};
use crate::window_motion::geometry::{MonitorWorkArea, ScaledMonitorWorkAreas};
use std::{ffi::c_void, mem};

pub fn prefers_reduced_motion() -> bool {
    let _pool = AutoreleasePoolGuard::new();
    unsafe { prefers_reduced_motion_macos() }
}

/// Reads all active macOS screens' logical work areas with their backing scale factor plus
/// `NSScreen.screens[0].frame.size.height`. Must run on the main thread.
pub fn read_monitors_with_scale_on_main(window: &tauri::WebviewWindow) -> ScaledMonitorWorkAreas {
    let _pool = AutoreleasePoolGuard::new();
    let mut monitors = Vec::new();
    let mut primary_height = 900.0;
    unsafe {
        let ns_screen_cls = objc_getClass(c"NSScreen".as_ptr());
        if !ns_screen_cls.is_null() {
            let screens_sel = sel_registerName(c"screens".as_ptr());
            let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
                mem::transmute(objc_msgSend as *const ());
            let screens = send_obj(ns_screen_cls, screens_sel);
            if !screens.is_null() {
                let count_sel = sel_registerName(c"count".as_ptr());
                let send_usize: unsafe extern "C" fn(*mut c_void, *const c_void) -> usize =
                    mem::transmute(objc_msgSend as *const ());
                let obj_at_sel = sel_registerName(c"objectAtIndex:".as_ptr());
                let send_at: unsafe extern "C" fn(
                    *mut c_void,
                    *const c_void,
                    usize,
                ) -> *mut c_void = mem::transmute(objc_msgSend as *const ());
                let frame_sel = sel_registerName(c"frame".as_ptr());
                let visible_sel = sel_registerName(c"visibleFrame".as_ptr());
                let scale_sel = sel_registerName(c"backingScaleFactor".as_ptr());
                let send_f64: unsafe extern "C" fn(*mut c_void, *const c_void) -> f64 =
                    mem::transmute(objc_msgSend as *const ());
                let count = send_usize(screens, count_sel);
                if count > 0 {
                    let first = send_at(screens, obj_at_sel, 0);
                    if !first.is_null() {
                        let first_frame = msg_send_rect(first, frame_sel);
                        if first_frame.size.height > 0.0 {
                            primary_height = first_frame.size.height;
                        }
                    }
                    for idx in 0..count {
                        let screen = send_at(screens, obj_at_sel, idx);
                        if screen.is_null() {
                            continue;
                        }
                        let vf = msg_send_rect(screen, visible_sel);
                        let scale = send_f64(screen, scale_sel).max(1.0);
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
                }
            }
        }
        if monitors.is_empty()
            && let Ok(ns_window) = window.ns_window()
            && !ns_window.is_null()
        {
            primary_height = primary_screen_height(ns_window);
        }
    }
    let primary = monitors.first().copied();
    (monitors, primary, primary_height)
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
    let _pool = AutoreleasePoolGuard::new();
    let event_cls = unsafe { objc_getClass(c"NSEvent".as_ptr()) };
    if event_cls.is_null() {
        return None;
    }
    let sel = unsafe { sel_registerName(c"mouseLocation".as_ptr()) };
    let pt = unsafe { msg_send_point(event_cls, sel) };
    if !pt.x.is_finite() || !pt.y.is_finite() {
        return None;
    }
    Some((pt.x, primary_height - pt.y))
}
