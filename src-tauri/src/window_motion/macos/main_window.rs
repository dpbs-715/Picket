//! Native macOS main window animation, pinning, startup frame positioning, reveal, and activation.

#[cfg(feature = "smoke-harness")]
use super::ffi::is_window_visible_on_all_spaces;
use super::ffi::{
    _NSConcreteStackBlock, AutoreleasePoolGuard, BLOCK_HAS_COPY_DISPOSE, ObjcBool,
    activate_app_macos, apply_pinned_on_main, make_window_key_and_order_front, objc_getClass,
    objc_msgSend, objc_release, objc_retain, prefers_reduced_motion_macos, primary_screen_height,
    sel_registerName, set_window_alpha, set_window_content_min_size, set_window_frame_instant,
    window_content_rect, window_frame,
};
use crate::{
    repository::types::WindowPosition,
    window_motion::geometry::{
        AnimateWindowFrameReply, LogicalExtent, LogicalPoint, NSRect, NSSize, ValidatedRequest,
        WindowFramePlacement, appkit_frame_to_top_left, compute_target_frame,
        content_size_to_extent, frames_within_epsilon, resolve_visible_window_position,
    },
};
use std::{
    ffi::c_void,
    mem, ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

static ANIMATION_GENERATION: AtomicU64 = AtomicU64::new(0);

struct CompletionPayload {
    ns_window: usize,
    generation: u64,
    target_frame: NSRect,
    final_min_size: NSSize,
    primary_height: f64,
    pinned: bool,
    previous_position: LogicalPoint,
    previous_size: LogicalExtent,
    tx: Mutex<Option<oneshot::Sender<AnimateWindowFrameReply>>>,
}

impl Drop for CompletionPayload {
    fn drop(&mut self) {
        let win = self.ns_window as *mut c_void;
        if !win.is_null() {
            unsafe { objc_release(win) };
        }
    }
}

#[repr(C)]
struct VoidBlockDescriptor {
    reserved: usize,
    size: usize,
    copy_helper: unsafe extern "C" fn(*mut VoidCompletionBlock, *const VoidCompletionBlock),
    dispose_helper: unsafe extern "C" fn(*mut VoidCompletionBlock),
}

#[repr(C)]
struct VoidCompletionBlock {
    isa: *const c_void,
    flags: i32,
    reserved: i32,
    invoke: unsafe extern "C" fn(*mut VoidCompletionBlock),
    descriptor: *const VoidBlockDescriptor,
    payload: *const CompletionPayload,
}

static VOID_BLOCK_DESCRIPTOR: VoidBlockDescriptor = VoidBlockDescriptor {
    reserved: 0,
    size: mem::size_of::<VoidCompletionBlock>(),
    copy_helper: void_block_copy,
    dispose_helper: void_block_dispose,
};

unsafe extern "C" fn void_block_copy(
    _dst: *mut VoidCompletionBlock,
    src: *const VoidCompletionBlock,
) {
    let ptr = unsafe { (*src).payload };
    if !ptr.is_null() {
        unsafe { Arc::increment_strong_count(ptr) };
    }
}

unsafe extern "C" fn void_block_dispose(block: *mut VoidCompletionBlock) {
    let ptr = unsafe { (*block).payload };
    if !ptr.is_null() {
        unsafe { drop(Arc::from_raw(ptr)) };
    }
}

unsafe extern "C" fn void_block_invoke(block: *mut VoidCompletionBlock) {
    let payload_ptr = unsafe { (*block).payload };
    if payload_ptr.is_null() {
        return;
    }
    let _pool = AutoreleasePoolGuard::new();
    let payload = unsafe { &*payload_ptr };
    let ns_window = payload.ns_window as *mut c_void;
    if !ns_window.is_null() && ANIMATION_GENERATION.load(Ordering::SeqCst) == payload.generation {
        let current = unsafe { window_frame(ns_window) };
        if !frames_within_epsilon(current, payload.target_frame) {
            unsafe { set_window_frame_instant(ns_window, payload.target_frame) };
        }
        unsafe { set_window_content_min_size(ns_window, payload.final_min_size) };
    }
    let after_frame = unsafe { window_frame(ns_window) };
    let after_content = unsafe { window_content_rect(ns_window, after_frame) };
    let reply = AnimateWindowFrameReply {
        pinned: payload.pinned,
        previous_position: payload.previous_position,
        previous_size: payload.previous_size,
        position: appkit_frame_to_top_left(after_frame, payload.primary_height),
        size: content_size_to_extent(after_content.size),
        animated: true,
    };
    if let Ok(mut slot) = payload.tx.lock()
        && let Some(tx) = slot.take()
    {
        let _ = tx.send(reply);
    }
}

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
            let _pool = AutoreleasePoolGuard::new();
            let result = (|| -> Result<Option<FallbackSnapTarget>, String> {
                let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                if ns_window.is_null() {
                    return Err("Native NSWindow pointer is unavailable".into());
                }
                let effective_pinned = match req.pinned {
                    Some(pinned) => apply_pinned_on_main(&win_for_main, ns_window, pinned)?,
                    None => win_for_main.is_always_on_top().map_err(|e| e.to_string())?,
                };

                let before_frame = unsafe { window_frame(ns_window) };
                let before_content = unsafe { window_content_rect(ns_window, before_frame) };
                let primary_height = unsafe { primary_screen_height(ns_window) };
                let previous_position = appkit_frame_to_top_left(before_frame, primary_height);
                let previous_size = content_size_to_extent(before_content.size);
                let target_content_w = req.width.unwrap_or(before_content.size.width);
                let clamped_req = if req.placement == WindowFramePlacement::RestorePosition
                    && let (Some(tx), Some(ty)) = (req.target_x, req.target_y)
                {
                    let (monitors, primary, _) =
                        super::displays::read_monitors_on_main(&win_for_main);
                    let (cx, cy, _) = resolve_visible_window_position(
                        Some(WindowPosition {
                            x: tx.round() as i32,
                            y: ty.round() as i32,
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
                    && !unsafe { prefers_reduced_motion_macos() };

                if !should_animate {
                    unsafe {
                        set_window_content_min_size(ns_window, pre_min_size);
                        if !frames_within_epsilon(before_frame, target_frame) {
                            set_window_frame_instant(ns_window, target_frame);
                        }
                        set_window_content_min_size(ns_window, final_min_size);
                    }
                    let after_frame = unsafe { window_frame(ns_window) };
                    let after_content = unsafe { window_content_rect(ns_window, after_frame) };
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

                let anim_ctx_cls = unsafe { objc_getClass(c"NSAnimationContext".as_ptr()) };
                if anim_ctx_cls.is_null() {
                    unsafe {
                        set_window_content_min_size(ns_window, pre_min_size);
                        set_window_frame_instant(ns_window, target_frame);
                        set_window_content_min_size(ns_window, final_min_size);
                    }
                    let after_frame = unsafe { window_frame(ns_window) };
                    let after_content = unsafe { window_content_rect(ns_window, after_frame) };
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

                unsafe {
                    set_window_content_min_size(ns_window, pre_min_size);
                    objc_retain(ns_window);
                }
                let payload = Arc::new(CompletionPayload {
                    ns_window: ns_window as usize,
                    generation,
                    target_frame,
                    final_min_size,
                    primary_height,
                    pinned: effective_pinned,
                    previous_position,
                    previous_size,
                    tx: Mutex::new(Some(tx)),
                });
                let raw_payload = Arc::into_raw(payload);
                let completion_block = VoidCompletionBlock {
                    isa: unsafe { _NSConcreteStackBlock },
                    flags: BLOCK_HAS_COPY_DISPOSE,
                    reserved: 0,
                    invoke: void_block_invoke,
                    descriptor: &VOID_BLOCK_DESCRIPTOR,
                    payload: raw_payload,
                };

                unsafe {
                    let begin_sel = sel_registerName(c"beginGrouping".as_ptr());
                    let end_sel = sel_registerName(c"endGrouping".as_ptr());
                    let current_sel = sel_registerName(c"currentContext".as_ptr());
                    let set_duration_sel = sel_registerName(c"setDuration:".as_ptr());
                    let set_timing_sel = sel_registerName(c"setTimingFunction:".as_ptr());
                    let set_completion_sel = sel_registerName(c"setCompletionHandler:".as_ptr());
                    let animator_sel = sel_registerName(c"animator".as_ptr());
                    let set_frame_sel = sel_registerName(c"setFrame:display:".as_ptr());

                    let send_void: unsafe extern "C" fn(*mut c_void, *const c_void) =
                        mem::transmute(objc_msgSend as *const ());
                    let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
                        mem::transmute(objc_msgSend as *const ());
                    let send_f64: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
                        mem::transmute(objc_msgSend as *const ());
                    let send_ptr: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void) =
                        mem::transmute(objc_msgSend as *const ());
                    let send_set_frame: unsafe extern "C" fn(
                        *mut c_void,
                        *const c_void,
                        NSRect,
                        ObjcBool,
                    ) = mem::transmute(objc_msgSend as *const ());

                    send_void(anim_ctx_cls, begin_sel);
                    let ctx = send_obj(anim_ctx_cls, current_sel);
                    if !ctx.is_null() {
                        send_f64(ctx, set_duration_sel, f64::from(req.duration_ms) / 1000.0);
                        let timing_cls = objc_getClass(c"CAMediaTimingFunction".as_ptr());
                        if !timing_cls.is_null() {
                            let timing_sel =
                                sel_registerName(c"functionWithControlPoints::::".as_ptr());
                            let send_timing: unsafe extern "C" fn(
                                *mut c_void,
                                *const c_void,
                                f32,
                                f32,
                                f32,
                                f32,
                            )
                                -> *mut c_void = mem::transmute(objc_msgSend as *const ());
                            // Matches CSS `--ease-out: cubic-bezier(0.22, 1, 0.36, 1)`.
                            let timing_fn =
                                send_timing(timing_cls, timing_sel, 0.22, 1.0, 0.36, 1.0);
                            if !timing_fn.is_null() {
                                send_ptr(ctx, set_timing_sel, timing_fn as *const c_void);
                            }
                        }
                        send_ptr(
                            ctx,
                            set_completion_sel,
                            ptr::from_ref(&completion_block).cast::<c_void>(),
                        );
                    }
                    let animator = send_obj(ns_window, animator_sel);
                    if !animator.is_null() {
                        send_set_frame(animator, set_frame_sel, target_frame, 1);
                    } else {
                        set_window_frame_instant(ns_window, target_frame);
                    }
                    send_void(anim_ctx_cls, end_sel);
                    // Balance our stack ownership; NSAnimationContext copied the block on heap.
                    drop(Arc::from_raw(raw_payload));
                }

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
                        let _pool = AutoreleasePoolGuard::new();
                        if let Ok(ns_window) = win_snap.ns_window()
                            && !ns_window.is_null()
                        {
                            if ANIMATION_GENERATION.load(Ordering::SeqCst) == generation {
                                let current = unsafe { window_frame(ns_window) };
                                if !frames_within_epsilon(current, target_frame) {
                                    unsafe { set_window_frame_instant(ns_window, target_frame) };
                                }
                                unsafe { set_window_content_min_size(ns_window, final_min_size) };
                            }
                            let after_frame = unsafe { window_frame(ns_window) };
                            let after_content =
                                unsafe { window_content_rect(ns_window, after_frame) };
                            let _ = snap_tx.send(AnimateWindowFrameReply {
                                pinned,
                                previous_position,
                                previous_size,
                                position: appkit_frame_to_top_left(after_frame, primary_height),
                                size: content_size_to_extent(after_content.size),
                                animated: false,
                            });
                        }
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
            let _pool = AutoreleasePoolGuard::new();
            let result = (|| -> Result<bool, String> {
                let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                if ns_window.is_null() {
                    return Err("Native NSWindow pointer is unavailable".into());
                }
                apply_pinned_on_main(&win_for_main, ns_window, pinned)
            })();
            let _ = tx.send(result);
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
            let _pool = AutoreleasePoolGuard::new();
            let result = (|| -> Result<bool, String> {
                let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                if ns_window.is_null() {
                    return Err("Native NSWindow pointer is unavailable".into());
                }
                Ok(unsafe { is_window_visible_on_all_spaces(ns_window) })
            })();
            let _ = tx.send(result);
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
    let _pool = AutoreleasePoolGuard::new();
    let ns_window = window.ns_window().map_err(|e| e.to_string())?;
    if ns_window.is_null() {
        return Err("Native NSWindow pointer is unavailable".into());
    }
    let _ = apply_pinned_on_main(window, ns_window, pinned)?;
    let before_frame = unsafe { window_frame(ns_window) };
    let before_content = unsafe { window_content_rect(ns_window, before_frame) };
    let primary_height = unsafe { primary_screen_height(ns_window) };
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
    let target_frame = compute_target_frame(before_frame, before_content.size, primary_height, req);
    let final_min = NSSize {
        width: min_width.min(width),
        height: min_height.min(height),
    };
    let pre_min = NSSize {
        width: final_min.width.min(before_content.size.width),
        height: final_min.height.min(before_content.size.height),
    };
    unsafe {
        set_window_content_min_size(ns_window, pre_min);
        set_window_frame_instant(ns_window, target_frame);
        set_window_content_min_size(ns_window, final_min);
    }
    Ok(())
}

pub async fn reveal_main_window_smoothly(
    window: tauri::WebviewWindow,
    duration_ms: u32,
) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<Result<(), String>>();
    let win_for_main = window.clone();
    window
        .run_on_main_thread(move || {
            let _pool = AutoreleasePoolGuard::new();
            let res = (|| -> Result<(), String> {
                let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                if ns_window.is_null() {
                    return Err("Main NSWindow pointer is unavailable".into());
                }
                let animate = duration_ms > 0 && !unsafe { prefers_reduced_motion_macos() };
                if !animate {
                    unsafe { set_window_alpha(ns_window, 1.0) };
                    win_for_main.show().map_err(|e| e.to_string())?;
                    let _ = win_for_main.set_focus();
                    return Ok(());
                }
                unsafe { set_window_alpha(ns_window, 0.0) };
                win_for_main.show().map_err(|e| e.to_string())?;
                let _ = win_for_main.set_focus();
                unsafe {
                    let anim_ctx_cls = objc_getClass(c"NSAnimationContext".as_ptr());
                    if anim_ctx_cls.is_null() {
                        set_window_alpha(ns_window, 1.0);
                        return Ok(());
                    }
                    let begin_sel = sel_registerName(c"beginGrouping".as_ptr());
                    let end_sel = sel_registerName(c"endGrouping".as_ptr());
                    let current_sel = sel_registerName(c"currentContext".as_ptr());
                    let set_duration_sel = sel_registerName(c"setDuration:".as_ptr());
                    let set_timing_sel = sel_registerName(c"setTimingFunction:".as_ptr());
                    let animator_sel = sel_registerName(c"animator".as_ptr());
                    let set_alpha_sel = sel_registerName(c"setAlphaValue:".as_ptr());

                    let send_void: unsafe extern "C" fn(*mut c_void, *const c_void) =
                        mem::transmute(objc_msgSend as *const ());
                    let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
                        mem::transmute(objc_msgSend as *const ());
                    let send_f64: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
                        mem::transmute(objc_msgSend as *const ());
                    let send_ptr: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void) =
                        mem::transmute(objc_msgSend as *const ());

                    send_void(anim_ctx_cls, begin_sel);
                    let ctx = send_obj(anim_ctx_cls, current_sel);
                    if !ctx.is_null() {
                        send_f64(ctx, set_duration_sel, f64::from(duration_ms) / 1000.0);
                        let timing_cls = objc_getClass(c"CAMediaTimingFunction".as_ptr());
                        if !timing_cls.is_null() {
                            let timing_sel =
                                sel_registerName(c"functionWithControlPoints::::".as_ptr());
                            let send_timing: unsafe extern "C" fn(
                                *mut c_void,
                                *const c_void,
                                f32,
                                f32,
                                f32,
                                f32,
                            )
                                -> *mut c_void = mem::transmute(objc_msgSend as *const ());
                            let timing_fn =
                                send_timing(timing_cls, timing_sel, 0.22, 1.0, 0.36, 1.0);
                            if !timing_fn.is_null() {
                                send_ptr(ctx, set_timing_sel, timing_fn as *const c_void);
                            }
                        }
                    }
                    let animator = send_obj(ns_window, animator_sel);
                    if !animator.is_null() {
                        send_f64(animator, set_alpha_sel, 1.0);
                    } else {
                        set_window_alpha(ns_window, 1.0);
                    }
                    send_void(anim_ctx_cls, end_sel);
                }
                Ok(())
            })();
            let _ = tx.send(res);
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "Reveal main window task did not run on main thread".to_string())?
}

pub async fn ensure_main_window_opaque(window: tauri::WebviewWindow) {
    let win_for_main = window.clone();
    let _ = window.run_on_main_thread(move || {
        let _pool = AutoreleasePoolGuard::new();
        if let Ok(ns_window) = win_for_main.ns_window()
            && !ns_window.is_null()
        {
            unsafe { set_window_alpha(ns_window, 1.0) };
        }
    });
}

pub fn activate_and_focus_main_on_main(window: &tauri::WebviewWindow) {
    let _pool = AutoreleasePoolGuard::new();
    if let Ok(ns_window) = window.ns_window()
        && !ns_window.is_null()
    {
        unsafe { set_window_alpha(ns_window, 1.0) };
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    unsafe {
        activate_app_macos();
        if let Ok(ns_window) = window.ns_window()
            && !ns_window.is_null()
        {
            make_window_key_and_order_front(ns_window);
        }
    }
}
