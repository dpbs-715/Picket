//! Native macOS main window animation, pinning, startup frame positioning, reveal, and activation.

#[cfg(feature = "smoke-harness")]
use super::ffi::is_window_visible_on_all_spaces;
use super::ffi::{
    activate_app_macos, apply_pinned_on_main, configure_current_animation_context,
    make_window_key_and_order_front, ns_window_ref, prefers_reduced_motion_macos,
    primary_screen_height, set_window_alpha, set_window_content_min_size, set_window_frame_instant,
    to_foundation_rect, window_content_rect, window_frame,
};
use crate::{
    repository::types::WindowPosition,
    window_motion::geometry::{
        AnimateWindowFrameReply, LogicalExtent, LogicalPoint, NSRect, NSSize, ValidatedRequest,
        WindowFramePlacement, appkit_frame_to_top_left, compute_target_frame,
        content_size_to_extent, frames_within_epsilon, resolve_visible_window_position,
    },
};
use block2::RcBlock;
use objc2::{
    Message, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, Bool},
};
use objc2_app_kit::NSAnimationContext;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

static ANIMATION_GENERATION: AtomicU64 = AtomicU64::new(0);

struct FallbackSnapTarget {
    generation: u64,
    target_frame: NSRect,
    final_min_size: NSSize,
    primary_height: f64,
    pinned: bool,
    previous_position: LogicalPoint,
    previous_size: LogicalExtent,
}

pub(crate) async fn animate_main_window(
    window: tauri::WebviewWindow,
    req: ValidatedRequest,
) -> Result<AnimateWindowFrameReply, String> {
    let (tx, rx) = oneshot::channel::<AnimateWindowFrameReply>();
    let (err_tx, err_rx) = oneshot::channel::<Result<Option<FallbackSnapTarget>, String>>();
    let win_for_main = window.clone();

    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let result = (|| -> Result<Option<FallbackSnapTarget>, String> {
                    let ns_window = unsafe {
                        ns_window_ref(&win_for_main, "Native NSWindow pointer is unavailable")
                    }?;
                    let effective_pinned = match req.pinned {
                        Some(pinned) => apply_pinned_on_main(&win_for_main, ns_window, pinned)?,
                        None => win_for_main.is_always_on_top().map_err(|e| e.to_string())?,
                    };

                    let before_frame = window_frame(ns_window);
                    let before_content = window_content_rect(ns_window, before_frame);
                    let primary_height = primary_screen_height(ns_window);
                    let previous_position = appkit_frame_to_top_left(before_frame, primary_height);
                    let previous_size = content_size_to_extent(before_content.size);
                    let target_content_w = req.width.unwrap_or(before_content.size.width);
                    let clamped_req = if req.placement == WindowFramePlacement::RestorePosition
                        && let (Some(tx_pos), Some(ty_pos)) = (req.target_x, req.target_y)
                    {
                        let (monitors, primary, _) =
                            super::displays::read_monitors_on_main(&win_for_main);
                        let (cx, cy, _) = resolve_visible_window_position(
                            Some(WindowPosition {
                                x: tx_pos.round() as i32,
                                y: ty_pos.round() as i32,
                            }),
                            target_content_w,
                            req.height,
                            &monitors,
                            primary,
                        );
                        ValidatedRequest {
                            target_x: Some(cx),
                            target_y: Some(cy),
                            ..req
                        }
                    } else {
                        req
                    };

                    let target_frame = compute_target_frame(
                        before_frame,
                        before_content.size,
                        primary_height,
                        clamped_req,
                    );
                    let final_min_size = NSSize {
                        width: req.min_width.min(target_content_w),
                        height: req.min_height.min(req.height),
                    };
                    let pre_min_size = NSSize {
                        width: final_min_size.width.min(before_content.size.width),
                        height: final_min_size.height.min(before_content.size.height),
                    };

                    let generation = ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
                    let should_animate = req.duration_ms > 0
                        && !frames_within_epsilon(before_frame, target_frame)
                        && !prefers_reduced_motion_macos();

                    if !should_animate {
                        set_window_content_min_size(ns_window, pre_min_size);
                        if !frames_within_epsilon(before_frame, target_frame) {
                            set_window_frame_instant(ns_window, target_frame);
                        }
                        set_window_content_min_size(ns_window, final_min_size);
                        let after_frame = window_frame(ns_window);
                        let after_content = window_content_rect(ns_window, after_frame);
                        let _ = tx.send(AnimateWindowFrameReply {
                            pinned: effective_pinned,
                            previous_position,
                            previous_size,
                            position: appkit_frame_to_top_left(after_frame, primary_height),
                            size: content_size_to_extent(after_content.size),
                            animated: false,
                        });
                        return Ok(None);
                    }

                    set_window_content_min_size(ns_window, pre_min_size);
                    let retained_win = ns_window.retain();
                    let tx_slot = Mutex::new(Some(tx));
                    let completion_block = RcBlock::new(move || {
                        autoreleasepool(|_| {
                            if ANIMATION_GENERATION.load(Ordering::SeqCst) == generation {
                                let current = window_frame(&retained_win);
                                if !frames_within_epsilon(current, target_frame) {
                                    set_window_frame_instant(&retained_win, target_frame);
                                }
                                set_window_content_min_size(&retained_win, final_min_size);
                            }
                            let after_frame = window_frame(&retained_win);
                            let after_content = window_content_rect(&retained_win, after_frame);
                            let reply = AnimateWindowFrameReply {
                                pinned: effective_pinned,
                                previous_position,
                                previous_size,
                                position: appkit_frame_to_top_left(after_frame, primary_height),
                                size: content_size_to_extent(after_content.size),
                                animated: true,
                            };
                            if let Ok(mut slot) = tx_slot.lock()
                                && let Some(sender) = slot.take()
                            {
                                let _ = sender.send(reply);
                            }
                        });
                    });

                    NSAnimationContext::beginGrouping();
                    configure_current_animation_context(req.duration_ms);
                    NSAnimationContext::currentContext()
                        .setCompletionHandler(Some(&completion_block));
                    let animator: Option<Retained<AnyObject>> =
                        unsafe { msg_send![ns_window, animator] };
                    if let Some(animator) = animator {
                        let rect = to_foundation_rect(target_frame);
                        unsafe {
                            let _: () = msg_send![&*animator, setFrame: rect, display: Bool::YES];
                        }
                    } else {
                        set_window_frame_instant(ns_window, target_frame);
                    }
                    NSAnimationContext::endGrouping();

                    Ok(Some(FallbackSnapTarget {
                        generation,
                        target_frame,
                        final_min_size,
                        primary_height,
                        pinned: effective_pinned,
                        previous_position,
                        previous_size,
                    }))
                })();
                let _ = err_tx.send(result);
            });
        })
        .map_err(|e| e.to_string())?;

    let fallback_target = err_rx
        .await
        .map_err(|_| "Window frame animation task did not run on main thread".to_string())??;

    let timeout = Duration::from_millis(u64::from(req.duration_ms) + 800);
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(reply)) => Ok(reply),
        Ok(Err(_)) | Err(_) => {
            // Fallback snap if the OS animation context was cancelled/dropped (e.g. window occluded).
            if let Some(FallbackSnapTarget {
                generation,
                target_frame,
                final_min_size,
                primary_height,
                pinned,
                previous_position,
                previous_size,
            }) = fallback_target
            {
                let (snap_tx, snap_rx) = oneshot::channel::<AnimateWindowFrameReply>();
                let win_snap = window.clone();
                if window
                    .run_on_main_thread(move || {
                        autoreleasepool(|_| {
                            if let Ok(ns_window) =
                                unsafe { ns_window_ref(&win_snap, "NSWindow unavailable") }
                            {
                                if ANIMATION_GENERATION.load(Ordering::SeqCst) == generation {
                                    let current = window_frame(ns_window);
                                    if !frames_within_epsilon(current, target_frame) {
                                        set_window_frame_instant(ns_window, target_frame);
                                    }
                                    set_window_content_min_size(ns_window, final_min_size);
                                }
                                let after_frame = window_frame(ns_window);
                                let after_content = window_content_rect(ns_window, after_frame);
                                let _ = snap_tx.send(AnimateWindowFrameReply {
                                    pinned,
                                    previous_position,
                                    previous_size,
                                    position: appkit_frame_to_top_left(after_frame, primary_height),
                                    size: content_size_to_extent(after_content.size),
                                    animated: false,
                                });
                            }
                        });
                    })
                    .is_ok()
                    && let Ok(reply) = snap_rx.await
                {
                    return Ok(reply);
                }
            }
            read_current_reply_fallback(&window, req)
        }
    }
}

fn read_current_reply_fallback(
    window: &tauri::WebviewWindow,
    req: ValidatedRequest,
) -> Result<AnimateWindowFrameReply, String> {
    let scale = window.scale_factor().unwrap_or(1.0).max(0.5);
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let pinned = window.is_always_on_top().unwrap_or(false);
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(req.min_width, req.min_height)));
    let point = LogicalPoint {
        x: (f64::from(pos.x) / scale).round() as i32,
        y: (f64::from(pos.y) / scale).round() as i32,
    };
    let extent = LogicalExtent {
        width: (f64::from(size.width) / scale).round() as u32,
        height: (f64::from(size.height) / scale).round() as u32,
    };
    Ok(AnimateWindowFrameReply {
        pinned,
        previous_position: point,
        previous_size: extent,
        position: point,
        size: extent,
        animated: false,
    })
}

pub(crate) async fn set_main_window_pinned(
    window: tauri::WebviewWindow,
    pinned: bool,
) -> Result<bool, String> {
    let (tx, rx) = oneshot::channel::<Result<bool, String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let result = (|| -> Result<bool, String> {
                    let ns_window = unsafe {
                        ns_window_ref(&win_for_main, "Native NSWindow pointer is unavailable")
                    }?;
                    apply_pinned_on_main(&win_for_main, ns_window, pinned)
                })();
                let _ = tx.send(result);
            });
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Window pin task did not run on main thread".to_string())?
}

#[cfg(feature = "smoke-harness")]
pub(crate) async fn read_main_window_visible_on_all_spaces(
    window: tauri::WebviewWindow,
) -> Result<bool, String> {
    let (tx, rx) = oneshot::channel::<Result<bool, String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let result = (|| -> Result<bool, String> {
                    let ns_window = unsafe {
                        ns_window_ref(&win_for_main, "Native NSWindow pointer is unavailable")
                    }?;
                    Ok(is_window_visible_on_all_spaces(ns_window))
                })();
                let _ = tx.send(result);
            });
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Window spaces inspection task did not run on main thread".to_string())?
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
    autoreleasepool(|_| {
        let ns_window = unsafe { ns_window_ref(window, "Native NSWindow pointer is unavailable") }?;
        let _ = apply_pinned_on_main(window, ns_window, pinned)?;
        let before_frame = window_frame(ns_window);
        let before_content = window_content_rect(ns_window, before_frame);
        let primary_height = primary_screen_height(ns_window);
        let req = ValidatedRequest {
            width: Some(width),
            height,
            min_width,
            min_height,
            placement: WindowFramePlacement::RestorePosition,
            target_x: Some(target_x),
            target_y: Some(target_y),
            duration_ms: 0,
            pinned: Some(pinned),
        };
        let target_frame =
            compute_target_frame(before_frame, before_content.size, primary_height, req);
        let final_min = NSSize {
            width: min_width.min(width),
            height: min_height.min(height),
        };
        let pre_min = NSSize {
            width: final_min.width.min(before_content.size.width),
            height: final_min.height.min(before_content.size.height),
        };
        set_window_content_min_size(ns_window, pre_min);
        set_window_frame_instant(ns_window, target_frame);
        set_window_content_min_size(ns_window, final_min);
        Ok(())
    })
}

pub async fn reveal_main_window_smoothly(
    window: tauri::WebviewWindow,
    duration_ms: u32,
) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<Result<(), String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            autoreleasepool(|_| {
                let res = (|| -> Result<(), String> {
                    let ns_window = unsafe {
                        ns_window_ref(&win_for_main, "Main NSWindow pointer is unavailable")
                    }?;
                    let animate = duration_ms > 0 && !prefers_reduced_motion_macos();
                    if !animate {
                        set_window_alpha(ns_window, 1.0);
                        win_for_main.show().map_err(|e| e.to_string())?;
                        let _ = win_for_main.set_focus();
                        return Ok(());
                    }
                    set_window_alpha(ns_window, 0.0);
                    win_for_main.show().map_err(|e| e.to_string())?;
                    let _ = win_for_main.set_focus();
                    NSAnimationContext::beginGrouping();
                    configure_current_animation_context(duration_ms);
                    let animator: Option<Retained<AnyObject>> =
                        unsafe { msg_send![ns_window, animator] };
                    if let Some(animator) = animator {
                        unsafe {
                            let _: () = msg_send![&*animator, setAlphaValue: 1.0_f64];
                        }
                    } else {
                        set_window_alpha(ns_window, 1.0);
                    }
                    NSAnimationContext::endGrouping();
                    Ok(())
                })();
                let _ = tx.send(res);
            });
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Reveal main window task did not run on main thread".to_string())?
}

pub async fn ensure_main_window_opaque(window: tauri::WebviewWindow) {
    let win_for_main = window.clone();
    let _ = window.run_on_main_thread(move || {
        autoreleasepool(|_| {
            if let Ok(ns_window) = unsafe { ns_window_ref(&win_for_main, "NSWindow unavailable") } {
                set_window_alpha(ns_window, 1.0);
            }
        });
    });
}

pub fn activate_and_focus_main_on_main(window: &tauri::WebviewWindow) {
    autoreleasepool(|_| {
        if let Ok(ns_window) = unsafe { ns_window_ref(window, "NSWindow unavailable") } {
            set_window_alpha(ns_window, 1.0);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        activate_app_macos();
        if let Ok(ns_window) = unsafe { ns_window_ref(window, "NSWindow unavailable") } {
            make_window_key_and_order_front(ns_window);
        }
    });
}
