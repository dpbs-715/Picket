//! Native macOS floating overlay window operations for the launch splash and menu bar tray popover.

use super::ffi::{
    activate_app_macos, make_window_key_and_order_front, ns_window_ref,
    order_window_front_regardless, primary_screen_height, set_window_alpha,
    set_window_clear_background, set_window_collection_behavior_raw, set_window_frame_instant,
    set_window_frame_origin, set_window_has_shadow, set_window_ignores_mouse_events,
    set_window_level, set_window_opaque, window_frame,
};
use crate::window_motion::geometry::{
    NS_SPLASH_WINDOW_LEVEL, NS_TRAY_POPOVER_WINDOW_LEVEL,
    NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES, NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE,
    NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT, NSPoint, NSRect, NSSize, frames_within_epsilon,
};
use objc2::rc::autoreleasepool;
use tokio::sync::oneshot;

const OVERLAY_COLLECTION_BEHAVIOR: usize = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
    | NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT
    | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE;

pub fn prepare_splash_window_on_main(
    window: &tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    size: f64,
    primary_height: f64,
) -> Result<(), String> {
    autoreleasepool(|_| {
        let ns_window = unsafe { ns_window_ref(window, "Splash NSWindow pointer is unavailable") }?;
        let frame = NSRect {
            origin: NSPoint {
                x: top_left_x,
                y: primary_height - top_left_y - size,
            },
            size: NSSize {
                width: size,
                height: size,
            },
        };
        set_window_opaque(ns_window, false);
        set_window_clear_background(ns_window);
        set_window_has_shadow(ns_window, false);
        set_window_ignores_mouse_events(ns_window, true);
        set_window_level(ns_window, NS_SPLASH_WINDOW_LEVEL);
        set_window_collection_behavior_raw(ns_window, OVERLAY_COLLECTION_BEHAVIOR);
        set_window_frame_instant(ns_window, frame);
        set_window_alpha(ns_window, 1.0);
        Ok(())
    })
}

pub async fn show_splash_window(window: tauri::WebviewWindow) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<Result<(), String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let res = (|| -> Result<(), String> {
                    let ns_window = unsafe {
                        ns_window_ref(&win_for_main, "Splash NSWindow pointer is unavailable")
                    }?;
                    set_window_alpha(ns_window, 1.0);
                    win_for_main.show().map_err(|e| e.to_string())?;
                    set_window_level(ns_window, NS_SPLASH_WINDOW_LEVEL);
                    order_window_front_regardless(ns_window);
                    Ok(())
                })();
                let _ = tx.send(res);
            });
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Show splash task did not run on main thread".to_string())?
}

pub async fn move_splash_window_origin(
    window: tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    size: f64,
    primary_height: f64,
) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<Result<(), String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let res = (|| -> Result<(), String> {
                    let ns_window = unsafe {
                        ns_window_ref(&win_for_main, "Splash NSWindow pointer is unavailable")
                    }?;
                    let origin = NSPoint {
                        x: top_left_x,
                        y: primary_height - top_left_y - size,
                    };
                    set_window_frame_origin(ns_window, origin);
                    Ok(())
                })();
                let _ = tx.send(res);
            });
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Move splash origin task did not run on main thread".to_string())?
}

pub fn prepare_tray_popover_window_on_main(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    autoreleasepool(|_| {
        let ns_window =
            unsafe { ns_window_ref(window, "Tray popover NSWindow pointer is unavailable") }?;
        let primary_height = primary_screen_height(ns_window);
        let frame = NSRect {
            origin: NSPoint {
                x: 0.0,
                y: primary_height - height,
            },
            size: NSSize { width, height },
        };
        set_window_opaque(ns_window, false);
        set_window_clear_background(ns_window);
        set_window_has_shadow(ns_window, true);
        set_window_level(ns_window, NS_TRAY_POPOVER_WINDOW_LEVEL);
        set_window_collection_behavior_raw(ns_window, OVERLAY_COLLECTION_BEHAVIOR);
        set_window_frame_instant(ns_window, frame);
        set_window_alpha(ns_window, 1.0);
        Ok(())
    })
}

pub fn show_tray_popover_on_main(
    window: &tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    width: f64,
    height: f64,
    primary_height: f64,
) -> Result<(), String> {
    autoreleasepool(|_| {
        let ns_window =
            unsafe { ns_window_ref(window, "Tray popover NSWindow pointer is unavailable") }?;
        let frame = NSRect {
            origin: NSPoint {
                x: top_left_x,
                y: primary_height - top_left_y - height,
            },
            size: NSSize { width, height },
        };
        set_window_opaque(ns_window, false);
        set_window_clear_background(ns_window);
        set_window_has_shadow(ns_window, true);
        set_window_level(ns_window, NS_TRAY_POPOVER_WINDOW_LEVEL);
        set_window_collection_behavior_raw(ns_window, OVERLAY_COLLECTION_BEHAVIOR);
        set_window_frame_instant(ns_window, frame);
        set_window_alpha(ns_window, 1.0);
        window.show().map_err(|e| e.to_string())?;
        activate_app_macos();
        order_window_front_regardless(ns_window);
        make_window_key_and_order_front(ns_window);
        Ok(())
    })
}

pub async fn resize_tray_popover_height(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<Result<(), String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let res = (|| -> Result<(), String> {
                    let ns_window = unsafe {
                        ns_window_ref(
                            &win_for_main,
                            "Tray popover NSWindow pointer is unavailable",
                        )
                    }?;
                    let current = window_frame(ns_window);
                    let target = NSRect {
                        origin: NSPoint {
                            x: current.origin.x,
                            y: current.origin.y + current.size.height - height,
                        },
                        size: NSSize { width, height },
                    };
                    if !frames_within_epsilon(current, target) {
                        set_window_frame_instant(ns_window, target);
                    }
                    Ok(())
                })();
                let _ = tx.send(res);
            });
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Resize tray popover task did not run on main thread".to_string())?
}
