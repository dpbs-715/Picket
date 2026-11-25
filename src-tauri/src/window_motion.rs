//! Smooth native macOS `NSWindow` frame animation via `NSAnimationContext` and
//! Objective-C runtime FFI, using only Apple system frameworks (`AppKit`, `QuartzCore`,
//! `libobjc`, `libSystem`) so the packaged app bundle keeps zero third-party dylibs.
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::Manager;

const MIN_WINDOW_WIDTH: f64 = 400.0;
const MIN_WINDOW_HEIGHT: f64 = 82.0;
const MAX_WINDOW_DIMENSION: f64 = 16_384.0;
const MAX_COORDINATE_ABS: f64 = 65_536.0;
const MAX_DURATION_MS: u32 = 600;
const FRAME_EPSILON: f64 = 0.5;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
const NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT: usize = 1 << 3;
const NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE: usize = 1 << 6;
/// Floats one level above `NSFloatingWindowLevel` (3) so the transient launch splash stays above
/// even a pinned main window while the main window fades in underneath its landing ripple.
const NS_SPLASH_WINDOW_LEVEL: isize = 4;
/// Floats at `NSStatusWindowLevel` (25) right beneath the macOS menu bar and above floating windows.
const NS_TRAY_POPOVER_WINDOW_LEVEL: isize = 25;

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
fn next_collection_behavior(current: usize, visible_on_all_spaces: bool) -> usize {
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
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ValidatedRequest {
    width: Option<f64>,
    height: f64,
    min_width: f64,
    min_height: f64,
    placement: WindowFramePlacement,
    target_x: Option<f64>,
    target_y: Option<f64>,
    duration_ms: u32,
    pinned: Option<bool>,
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

fn validate_request(req: &AnimateWindowFrameRequest) -> Result<ValidatedRequest, String> {
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
fn appkit_frame_to_top_left(frame: NSRect, primary_screen_height: f64) -> LogicalPoint {
    let top_y = primary_screen_height - (frame.origin.y + frame.size.height);
    LogicalPoint {
        x: frame.origin.x.round() as i32,
        y: top_y.round() as i32,
    }
}

fn content_size_to_extent(size: NSSize) -> LogicalExtent {
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
fn compute_target_frame(
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

fn frames_within_epsilon(a: NSRect, b: NSRect) -> bool {
    (a.origin.x - b.origin.x).abs() <= FRAME_EPSILON
        && (a.origin.y - b.origin.y).abs() <= FRAME_EPSILON
        && (a.size.width - b.size.width).abs() <= FRAME_EPSILON
        && (a.size.height - b.size.height).abs() <= FRAME_EPSILON
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        ffi::{c_char, c_void},
        mem, ptr,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };
    use tokio::sync::oneshot;

    type ObjcBool = u8;
    const BLOCK_HAS_COPY_DISPOSE: i32 = 1 << 25;

    static ANIMATION_GENERATION: AtomicU64 = AtomicU64::new(0);

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "QuartzCore", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend();
        #[cfg(target_arch = "x86_64")]
        fn objc_msgSend_stret();
        fn objc_retain(obj: *mut c_void) -> *mut c_void;
        fn objc_release(obj: *mut c_void);
        fn objc_autoreleasePoolPush() -> *mut c_void;
        fn objc_autoreleasePoolPop(pool: *mut c_void);
    }

    #[link(name = "System", kind = "dylib")]
    unsafe extern "C" {
        static _NSConcreteStackBlock: *const c_void;
    }

    struct AutoreleasePoolGuard(*mut c_void);

    impl AutoreleasePoolGuard {
        fn new() -> Self {
            Self(unsafe { objc_autoreleasePoolPush() })
        }
    }

    impl Drop for AutoreleasePoolGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { objc_autoreleasePoolPop(self.0) };
            }
        }
    }

    unsafe fn msg_send_rect(receiver: *mut c_void, sel: *const c_void) -> NSRect {
        #[cfg(target_arch = "aarch64")]
        {
            let f: unsafe extern "C" fn(*mut c_void, *const c_void) -> NSRect =
                unsafe { mem::transmute(objc_msgSend as *const ()) };
            unsafe { f(receiver, sel) }
        }
        #[cfg(target_arch = "x86_64")]
        {
            let mut out = NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 0.0,
                    height: 0.0,
                },
            };
            let f: unsafe extern "C" fn(*mut NSRect, *mut c_void, *const c_void) =
                unsafe { mem::transmute(objc_msgSend_stret as *const ()) };
            unsafe { f(&mut out, receiver, sel) };
            out
        }
    }

    unsafe fn msg_send_rect_with_rect(
        receiver: *mut c_void,
        sel: *const c_void,
        arg: NSRect,
    ) -> NSRect {
        #[cfg(target_arch = "aarch64")]
        {
            let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSRect) -> NSRect =
                unsafe { mem::transmute(objc_msgSend as *const ()) };
            unsafe { f(receiver, sel, arg) }
        }
        #[cfg(target_arch = "x86_64")]
        {
            let mut out = NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 0.0,
                    height: 0.0,
                },
            };
            let f: unsafe extern "C" fn(*mut NSRect, *mut c_void, *const c_void, NSRect) =
                unsafe { mem::transmute(objc_msgSend_stret as *const ()) };
            unsafe { f(&mut out, receiver, sel, arg) };
            out
        }
    }

    unsafe fn msg_send_point(receiver: *mut c_void, sel: *const c_void) -> NSPoint {
        let f: unsafe extern "C" fn(*mut c_void, *const c_void) -> NSPoint =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, sel) }
    }

    unsafe fn window_frame(ns_window: *mut c_void) -> NSRect {
        let sel = unsafe { sel_registerName(c"frame".as_ptr()) };
        unsafe { msg_send_rect(ns_window, sel) }
    }

    unsafe fn window_content_rect(ns_window: *mut c_void, frame: NSRect) -> NSRect {
        let sel = unsafe { sel_registerName(c"contentRectForFrameRect:".as_ptr()) };
        unsafe { msg_send_rect_with_rect(ns_window, sel, frame) }
    }

    unsafe fn set_window_content_min_size(ns_window: *mut c_void, size: NSSize) {
        let sel = unsafe { sel_registerName(c"setContentMinSize:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSSize) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, size) };
    }

    unsafe fn set_window_frame_instant(ns_window: *mut c_void, frame: NSRect) {
        let sel = unsafe { sel_registerName(c"setFrame:display:animate:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSRect, ObjcBool, ObjcBool) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, frame, 1, 0) };
    }

    unsafe fn set_window_frame_origin(ns_window: *mut c_void, origin: NSPoint) {
        let sel = unsafe { sel_registerName(c"setFrameOrigin:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSPoint) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, origin) };
    }

    unsafe fn set_window_alpha(ns_window: *mut c_void, alpha: f64) {
        let sel = unsafe { sel_registerName(c"setAlphaValue:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, alpha) };
    }

    unsafe fn set_window_opaque(ns_window: *mut c_void, opaque: bool) {
        let sel = unsafe { sel_registerName(c"setOpaque:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, u8::from(opaque)) };
    }

    unsafe fn set_window_clear_background(ns_window: *mut c_void) {
        let color_cls = unsafe { objc_getClass(c"NSColor".as_ptr()) };
        if color_cls.is_null() {
            return;
        }
        let clear_sel = unsafe { sel_registerName(c"clearColor".as_ptr()) };
        let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        let clear = unsafe { send_obj(color_cls, clear_sel) };
        if !clear.is_null() {
            let bg_sel = unsafe { sel_registerName(c"setBackgroundColor:".as_ptr()) };
            let send_bg: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
                unsafe { mem::transmute(objc_msgSend as *const ()) };
            unsafe { send_bg(ns_window, bg_sel, clear) };
        }
    }

    unsafe fn set_window_has_shadow(ns_window: *mut c_void, has_shadow: bool) {
        let sel = unsafe { sel_registerName(c"setHasShadow:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, u8::from(has_shadow)) };
    }

    unsafe fn set_window_ignores_mouse_events(ns_window: *mut c_void, ignores: bool) {
        let sel = unsafe { sel_registerName(c"setIgnoresMouseEvents:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, u8::from(ignores)) };
    }

    unsafe fn set_window_level(ns_window: *mut c_void, level: isize) {
        let sel = unsafe { sel_registerName(c"setLevel:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, isize) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, level) };
    }

    unsafe fn set_window_collection_behavior_raw(ns_window: *mut c_void, behavior: usize) {
        let sel = unsafe { sel_registerName(c"setCollectionBehavior:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, usize) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, behavior) };
    }

    unsafe fn order_window_front_regardless(ns_window: *mut c_void) {
        let sel = unsafe { sel_registerName(c"orderFrontRegardless".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel) };
    }

    unsafe fn make_window_key_and_order_front(ns_window: *mut c_void) {
        let sel = unsafe { sel_registerName(c"makeKeyAndOrderFront:".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel, ptr::null_mut()) };
    }

    unsafe fn activate_app_macos() {
        let app_cls = unsafe { objc_getClass(c"NSApplication".as_ptr()) };
        if app_cls.is_null() {
            return;
        }
        let shared_sel = unsafe { sel_registerName(c"sharedApplication".as_ptr()) };
        let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        let shared = unsafe { send_obj(app_cls, shared_sel) };
        if shared.is_null() {
            return;
        }
        let activate_sel = unsafe { sel_registerName(c"activateIgnoringOtherApps:".as_ptr()) };
        let send_bool: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { send_bool(shared, activate_sel, 1) };
    }

    unsafe fn window_collection_behavior(ns_window: *mut c_void) -> usize {
        let sel = unsafe { sel_registerName(c"collectionBehavior".as_ptr()) };
        let f: unsafe extern "C" fn(*mut c_void, *const c_void) -> usize =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(ns_window, sel) }
    }

    unsafe fn set_window_visible_on_all_spaces(ns_window: *mut c_void, visible: bool) {
        let current = unsafe { window_collection_behavior(ns_window) };
        let next = next_collection_behavior(current, visible);
        if next != current {
            let sel = unsafe { sel_registerName(c"setCollectionBehavior:".as_ptr()) };
            let f: unsafe extern "C" fn(*mut c_void, *const c_void, usize) =
                unsafe { mem::transmute(objc_msgSend as *const ()) };
            unsafe { f(ns_window, sel, next) };
        }
    }

    unsafe fn is_window_visible_on_all_spaces(ns_window: *mut c_void) -> bool {
        (unsafe { window_collection_behavior(ns_window) }
            & NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES)
            != 0
    }

    fn apply_pinned_on_main(
        window: &tauri::WebviewWindow,
        ns_window: *mut c_void,
        pinned: bool,
    ) -> Result<bool, String> {
        window
            .set_always_on_top(pinned)
            .map_err(|e| e.to_string())?;
        unsafe { set_window_visible_on_all_spaces(ns_window, pinned) };
        window.is_always_on_top().map_err(|e| e.to_string())
    }

    unsafe fn primary_screen_height(ns_window: *mut c_void) -> f64 {
        let ns_screen_cls = unsafe { objc_getClass(c"NSScreen".as_ptr()) };
        if !ns_screen_cls.is_null() {
            let screens_sel = unsafe { sel_registerName(c"screens".as_ptr()) };
            let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
                unsafe { mem::transmute(objc_msgSend as *const ()) };
            let screens = unsafe { send_obj(ns_screen_cls, screens_sel) };
            if !screens.is_null() {
                let count_sel = unsafe { sel_registerName(c"count".as_ptr()) };
                let send_usize: unsafe extern "C" fn(*mut c_void, *const c_void) -> usize =
                    unsafe { mem::transmute(objc_msgSend as *const ()) };
                if unsafe { send_usize(screens, count_sel) } > 0 {
                    let obj_at_sel = unsafe { sel_registerName(c"objectAtIndex:".as_ptr()) };
                    let send_at: unsafe extern "C" fn(
                        *mut c_void,
                        *const c_void,
                        usize,
                    ) -> *mut c_void = unsafe { mem::transmute(objc_msgSend as *const ()) };
                    let primary = unsafe { send_at(screens, obj_at_sel, 0) };
                    if !primary.is_null() {
                        let frame_sel = unsafe { sel_registerName(c"frame".as_ptr()) };
                        let rect = unsafe { msg_send_rect(primary, frame_sel) };
                        if rect.size.height > 0.0 {
                            return rect.size.height;
                        }
                    }
                }
            }
        }
        // Fallback to window's screen if NSScreen.screens is empty (headless edge case).
        let screen_sel = unsafe { sel_registerName(c"screen".as_ptr()) };
        let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        let screen = unsafe { send_obj(ns_window, screen_sel) };
        if !screen.is_null() {
            let frame_sel = unsafe { sel_registerName(c"frame".as_ptr()) };
            let rect = unsafe { msg_send_rect(screen, frame_sel) };
            if rect.size.height > 0.0 {
                return rect.size.height;
            }
        }
        900.0
    }

    unsafe fn prefers_reduced_motion_macos() -> bool {
        let workspace_cls = unsafe { objc_getClass(c"NSWorkspace".as_ptr()) };
        if workspace_cls.is_null() {
            return false;
        }
        let shared_sel = unsafe { sel_registerName(c"sharedWorkspace".as_ptr()) };
        let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        let workspace = unsafe { send_obj(workspace_cls, shared_sel) };
        if workspace.is_null() {
            return false;
        }
        let reduce_sel =
            unsafe { sel_registerName(c"accessibilityDisplayShouldReduceMotion".as_ptr()) };
        let responds_sel = unsafe { sel_registerName(c"respondsToSelector:".as_ptr()) };
        let send_responds: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_void,
        ) -> ObjcBool = unsafe { mem::transmute(objc_msgSend as *const ()) };
        if unsafe { send_responds(workspace, responds_sel, reduce_sel) } == 0 {
            return false;
        }
        let send_bool: unsafe extern "C" fn(*mut c_void, *const c_void) -> ObjcBool =
            unsafe { mem::transmute(objc_msgSend as *const ()) };
        unsafe { send_bool(workspace, reduce_sel) != 0 }
    }

    struct CompletionPayload {
        ns_window: usize,
        generation: u64,
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
        if !ns_window.is_null() && ANIMATION_GENERATION.load(Ordering::SeqCst) == payload.generation
        {
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

    pub async fn animate_main_window(
        window: tauri::WebviewWindow,
        req: ValidatedRequest,
    ) -> Result<AnimateWindowFrameReply, String> {
        let (tx, rx) = oneshot::channel::<AnimateWindowFrameReply>();
        let (err_tx, err_rx) = oneshot::channel::<Result<(), String>>();
        let win_for_main = window.clone();

        window
            .run_on_main_thread(move || {
                let _pool = AutoreleasePoolGuard::new();
                let result = (|| -> Result<(), String> {
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

                    let target_frame = compute_target_frame(
                        before_frame,
                        before_content.size,
                        primary_height,
                        req,
                    );
                    let target_content_w = req.width.unwrap_or(before_content.size.width);
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
                        return Ok(());
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
                        return Ok(());
                    }

                    unsafe {
                        set_window_content_min_size(ns_window, pre_min_size);
                        objc_retain(ns_window);
                    }
                    let payload = Arc::new(CompletionPayload {
                        ns_window: ns_window as usize,
                        generation,
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
                        let set_completion_sel =
                            sel_registerName(c"setCompletionHandler:".as_ptr());
                        let animator_sel = sel_registerName(c"animator".as_ptr());
                        let set_frame_sel = sel_registerName(c"setFrame:display:".as_ptr());

                        let send_void: unsafe extern "C" fn(*mut c_void, *const c_void) =
                            mem::transmute(objc_msgSend as *const ());
                        let send_obj: unsafe extern "C" fn(
                            *mut c_void,
                            *const c_void,
                        ) -> *mut c_void = mem::transmute(objc_msgSend as *const ());
                        let send_f64: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
                            mem::transmute(objc_msgSend as *const ());
                        let send_ptr: unsafe extern "C" fn(
                            *mut c_void,
                            *const c_void,
                            *const c_void,
                        ) = mem::transmute(objc_msgSend as *const ());
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

                    Ok(())
                })();
                let _ = err_tx.send(result);
            })
            .map_err(|e| e.to_string())?;

        err_rx
            .await
            .map_err(|_| "Window frame animation task did not run on main thread".to_string())??;

        let timeout = Duration::from_millis(u64::from(req.duration_ms) + 800);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) | Err(_) => {
                // Fallback read if the OS animation context was cancelled (e.g. window hidden).
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

    pub async fn set_main_window_pinned(
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

    pub async fn read_main_window_visible_on_all_spaces(
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

    pub fn prefers_reduced_motion() -> bool {
        let _pool = AutoreleasePoolGuard::new();
        unsafe { prefers_reduced_motion_macos() }
    }

    /// Reads all active macOS screens' logical work areas (excluding menu bar & Dock) in top-left
    /// global coordinates alongside `NSScreen.screens[0].frame.size.height`. Must run on main thread.
    pub fn read_monitors_on_main(
        window: &tauri::WebviewWindow,
    ) -> (Vec<MonitorWorkArea>, Option<MonitorWorkArea>, f64) {
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
                            if vf.size.width > 0.0 && vf.size.height > 0.0 {
                                monitors.push(MonitorWorkArea {
                                    x: vf.origin.x,
                                    y: primary_height - (vf.origin.y + vf.size.height),
                                    width: vf.size.width,
                                    height: vf.size.height,
                                });
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
        unsafe {
            set_window_content_min_size(ns_window, pre_min);
            set_window_frame_instant(ns_window, target_frame);
            set_window_content_min_size(ns_window, final_min);
        }
        Ok(())
    }

    pub fn prepare_splash_window_on_main(
        window: &tauri::WebviewWindow,
        top_left_x: f64,
        top_left_y: f64,
        size: f64,
        primary_height: f64,
    ) -> Result<(), String> {
        let _pool = AutoreleasePoolGuard::new();
        let ns_window = window.ns_window().map_err(|e| e.to_string())?;
        if ns_window.is_null() {
            return Err("Splash NSWindow pointer is unavailable".into());
        }
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
        unsafe {
            set_window_opaque(ns_window, false);
            set_window_clear_background(ns_window);
            set_window_has_shadow(ns_window, false);
            set_window_ignores_mouse_events(ns_window, true);
            set_window_level(ns_window, NS_SPLASH_WINDOW_LEVEL);
            set_window_collection_behavior_raw(
                ns_window,
                NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
                    | NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT
                    | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE,
            );
            set_window_frame_instant(ns_window, frame);
            set_window_alpha(ns_window, 1.0);
        }
        Ok(())
    }

    pub async fn show_splash_window(window: tauri::WebviewWindow) -> Result<(), String> {
        let (tx, rx) = oneshot::channel::<Result<(), String>>();
        let win_for_main = window.clone();
        window
            .run_on_main_thread(move || {
                let _pool = AutoreleasePoolGuard::new();
                let res = (|| -> Result<(), String> {
                    let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                    if ns_window.is_null() {
                        return Err("Splash NSWindow pointer is unavailable".into());
                    }
                    unsafe {
                        set_window_alpha(ns_window, 1.0);
                    }
                    win_for_main.show().map_err(|e| e.to_string())?;
                    unsafe {
                        set_window_level(ns_window, NS_SPLASH_WINDOW_LEVEL);
                        order_window_front_regardless(ns_window);
                    }
                    Ok(())
                })();
                let _ = tx.send(res);
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
                let _pool = AutoreleasePoolGuard::new();
                let res = (|| -> Result<(), String> {
                    let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                    if ns_window.is_null() {
                        return Err("Splash NSWindow pointer is unavailable".into());
                    }
                    let origin = NSPoint {
                        x: top_left_x,
                        y: primary_height - top_left_y - size,
                    };
                    unsafe { set_window_frame_origin(ns_window, origin) };
                    Ok(())
                })();
                let _ = tx.send(res);
            })
            .map_err(|e| e.to_string())?;
        rx.await
            .map_err(|_| "Move splash origin task did not run on main thread".to_string())?
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
                        let send_obj: unsafe extern "C" fn(
                            *mut c_void,
                            *const c_void,
                        ) -> *mut c_void = mem::transmute(objc_msgSend as *const ());
                        let send_f64: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
                            mem::transmute(objc_msgSend as *const ());
                        let send_ptr: unsafe extern "C" fn(
                            *mut c_void,
                            *const c_void,
                            *const c_void,
                        ) = mem::transmute(objc_msgSend as *const ());

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

    /// Reads all active macOS screens' logical work areas with their backing scale factor plus
    /// `NSScreen.screens[0].frame.size.height`. Must run on the main thread.
    pub fn read_monitors_with_scale_on_main(
        window: &tauri::WebviewWindow,
    ) -> ScaledMonitorWorkAreas {
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

    pub fn prepare_tray_popover_window_on_main(
        window: &tauri::WebviewWindow,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        let _pool = AutoreleasePoolGuard::new();
        let ns_window = window.ns_window().map_err(|e| e.to_string())?;
        if ns_window.is_null() {
            return Err("Tray popover NSWindow pointer is unavailable".into());
        }
        let primary_height = unsafe { primary_screen_height(ns_window) };
        let frame = NSRect {
            origin: NSPoint {
                x: 0.0,
                y: primary_height - height,
            },
            size: NSSize { width, height },
        };
        unsafe {
            set_window_opaque(ns_window, false);
            set_window_clear_background(ns_window);
            set_window_has_shadow(ns_window, true);
            set_window_level(ns_window, NS_TRAY_POPOVER_WINDOW_LEVEL);
            set_window_collection_behavior_raw(
                ns_window,
                NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
                    | NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT
                    | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE,
            );
            set_window_frame_instant(ns_window, frame);
            set_window_alpha(ns_window, 1.0);
        }
        Ok(())
    }

    pub fn show_tray_popover_on_main(
        window: &tauri::WebviewWindow,
        top_left_x: f64,
        top_left_y: f64,
        width: f64,
        height: f64,
        primary_height: f64,
    ) -> Result<(), String> {
        let _pool = AutoreleasePoolGuard::new();
        let ns_window = window.ns_window().map_err(|e| e.to_string())?;
        if ns_window.is_null() {
            return Err("Tray popover NSWindow pointer is unavailable".into());
        }
        let frame = NSRect {
            origin: NSPoint {
                x: top_left_x,
                y: primary_height - top_left_y - height,
            },
            size: NSSize { width, height },
        };
        unsafe {
            set_window_opaque(ns_window, false);
            set_window_clear_background(ns_window);
            set_window_has_shadow(ns_window, true);
            set_window_level(ns_window, NS_TRAY_POPOVER_WINDOW_LEVEL);
            set_window_collection_behavior_raw(
                ns_window,
                NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
                    | NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT
                    | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE,
            );
            set_window_frame_instant(ns_window, frame);
            set_window_alpha(ns_window, 1.0);
        }
        window.show().map_err(|e| e.to_string())?;
        unsafe {
            activate_app_macos();
            order_window_front_regardless(ns_window);
            make_window_key_and_order_front(ns_window);
        }
        Ok(())
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
                let _pool = AutoreleasePoolGuard::new();
                let res = (|| -> Result<(), String> {
                    let ns_window = win_for_main.ns_window().map_err(|e| e.to_string())?;
                    if ns_window.is_null() {
                        return Err("Tray popover NSWindow pointer is unavailable".into());
                    }
                    let current = unsafe { window_frame(ns_window) };
                    let target = NSRect {
                        origin: NSPoint {
                            x: current.origin.x,
                            y: current.origin.y + current.size.height - height,
                        },
                        size: NSSize { width, height },
                    };
                    if !frames_within_epsilon(current, target) {
                        unsafe { set_window_frame_instant(ns_window, target) };
                    }
                    Ok(())
                })();
                let _ = tx.send(res);
            })
            .map_err(|e| e.to_string())?;
        rx.await
            .map_err(|_| "Resize tray popover task did not run on main thread".to_string())?
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
}

#[cfg(not(target_os = "macos"))]
mod fallback {
    use super::*;

    pub async fn set_main_window_pinned(
        window: tauri::WebviewWindow,
        pinned: bool,
    ) -> Result<bool, String> {
        window
            .set_always_on_top(pinned)
            .map_err(|e| e.to_string())?;
        window.is_always_on_top().map_err(|e| e.to_string())
    }

    pub async fn read_main_window_visible_on_all_spaces(
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

    pub fn read_monitors_with_scale_on_main(
        window: &tauri::WebviewWindow,
    ) -> ScaledMonitorWorkAreas {
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

    pub async fn animate_main_window(
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
                    window
                        .set_position(tauri::LogicalPosition::new(x, y))
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
}

pub fn prefers_reduced_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::prefers_reduced_motion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::prefers_reduced_motion()
    }
}

pub fn read_monitors_on_main(
    window: &tauri::WebviewWindow,
) -> (Vec<MonitorWorkArea>, Option<MonitorWorkArea>, f64) {
    #[cfg(target_os = "macos")]
    {
        macos::read_monitors_on_main(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::read_monitors_on_main(window)
    }
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
    #[cfg(target_os = "macos")]
    {
        macos::configure_main_window_startup_on_main(
            window, target_x, target_y, width, height, min_width, min_height, pinned,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::configure_main_window_startup_on_main(
            window, target_x, target_y, width, height, min_width, min_height, pinned,
        )
    }
}

pub fn prepare_splash_window_on_main(
    window: &tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    size: f64,
    primary_height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::prepare_splash_window_on_main(window, top_left_x, top_left_y, size, primary_height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::prepare_splash_window_on_main(
            window,
            top_left_x,
            top_left_y,
            size,
            primary_height,
        )
    }
}

pub async fn show_splash_window(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::show_splash_window(window).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::show_splash_window(window).await
    }
}

pub async fn move_splash_window_origin(
    window: tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    size: f64,
    primary_height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::move_splash_window_origin(window, top_left_x, top_left_y, size, primary_height).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::move_splash_window_origin(window, top_left_x, top_left_y, size, primary_height)
            .await
    }
}

pub async fn reveal_main_window_smoothly(
    window: tauri::WebviewWindow,
    duration_ms: u32,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::reveal_main_window_smoothly(window, duration_ms).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::reveal_main_window_smoothly(window, duration_ms).await
    }
}

pub async fn ensure_main_window_opaque(window: tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        macos::ensure_main_window_opaque(window).await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::ensure_main_window_opaque(window).await;
    }
}

pub fn read_monitors_with_scale_on_main(window: &tauri::WebviewWindow) -> ScaledMonitorWorkAreas {
    #[cfg(target_os = "macos")]
    {
        macos::read_monitors_with_scale_on_main(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::read_monitors_with_scale_on_main(window)
    }
}

pub fn mouse_location_logical_on_main(primary_height: f64) -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    {
        macos::mouse_location_logical_on_main(primary_height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::mouse_location_logical_on_main(primary_height)
    }
}

pub fn prepare_tray_popover_window_on_main(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::prepare_tray_popover_window_on_main(window, width, height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::prepare_tray_popover_window_on_main(window, width, height)
    }
}

pub fn show_tray_popover_on_main(
    window: &tauri::WebviewWindow,
    top_left_x: f64,
    top_left_y: f64,
    width: f64,
    height: f64,
    primary_height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::show_tray_popover_on_main(
            window,
            top_left_x,
            top_left_y,
            width,
            height,
            primary_height,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::show_tray_popover_on_main(
            window,
            top_left_x,
            top_left_y,
            width,
            height,
            primary_height,
        )
    }
}

pub async fn resize_tray_popover_height(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::resize_tray_popover_height(window, width, height).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::resize_tray_popover_height(window, width, height).await
    }
}

pub fn activate_and_focus_main_on_main(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        macos::activate_and_focus_main_on_main(window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::activate_and_focus_main_on_main(window);
    }
}

#[tauri::command]
pub async fn set_window_pinned(app: tauri::AppHandle, pinned: bool) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;

    #[cfg(target_os = "macos")]
    {
        macos::set_main_window_pinned(window, pinned).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::set_main_window_pinned(window, pinned).await
    }
}

pub async fn read_main_window_visible_on_all_spaces(
    app: &tauri::AppHandle,
) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;

    #[cfg(target_os = "macos")]
    {
        macos::read_main_window_visible_on_all_spaces(window).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::read_main_window_visible_on_all_spaces(window).await
    }
}

#[tauri::command]
pub async fn animate_window_frame(
    app: tauri::AppHandle,
    request: AnimateWindowFrameRequest,
) -> Result<AnimateWindowFrameReply, String> {
    let validated = validate_request(&request)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is unavailable".to_string())?;

    #[cfg(target_os = "macos")]
    {
        macos::animate_main_window(window, validated).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback::animate_main_window(window, validated).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_top_left_keeps_top_edge_invariant_across_compact_and_full_transitions() {
        let primary_height = 982.0;
        // Top-left at logical (120.5, 91.5) with size 400x700 -> bottom = 791.5 -> appkit_y = 190.5
        let full_frame = NSRect {
            origin: NSPoint { x: 120.5, y: 190.5 },
            size: NSSize {
                width: 400.0,
                height: 700.0,
            },
        };
        assert_eq!(
            appkit_frame_to_top_left(full_frame, primary_height),
            LogicalPoint { x: 121, y: 92 }
        );

        let collapse_req = validate_request(&AnimateWindowFrameRequest {
            width: Some(400.0),
            height: 86.0,
            min_width: 400.0,
            min_height: 82.0,
            placement: WindowFramePlacement::AnchorTopLeft,
            target_x: None,
            target_y: None,
            duration_ms: 185,
            pinned: Some(true),
        })
        .unwrap();

        let compact_frame =
            compute_target_frame(full_frame, full_frame.size, primary_height, collapse_req);
        // Exact floating-point top-left anchor: origin.x unchanged, origin.y + height unchanged!
        assert_eq!(compact_frame.origin.x, 120.5);
        assert_eq!(
            compact_frame.origin.y + compact_frame.size.height,
            full_frame.origin.y + full_frame.size.height
        );
        assert_eq!(compact_frame.size.height, 86.0);

        // Drawer height expansion with width: None preserves exact width (even custom width):
        let wide_compact = NSRect {
            origin: compact_frame.origin,
            size: NSSize {
                width: 460.0,
                height: 86.0,
            },
        };
        let drawer_req = validate_request(&AnimateWindowFrameRequest {
            width: None,
            height: 240.0,
            min_width: 400.0,
            min_height: 82.0,
            placement: WindowFramePlacement::AnchorTopLeft,
            target_x: None,
            target_y: None,
            duration_ms: 145,
            pinned: None,
        })
        .unwrap();
        let drawer_frame =
            compute_target_frame(wide_compact, wide_compact.size, primary_height, drawer_req);
        assert_eq!(drawer_frame.size.width, 460.0);
        assert_eq!(drawer_frame.size.height, 240.0);
        assert_eq!(
            drawer_frame.origin.y + drawer_frame.size.height,
            wide_compact.origin.y + wide_compact.size.height
        );
    }

    #[test]
    fn restore_position_round_trips_top_left_coordinates_including_secondary_displays() {
        let primary_height = 982.0;
        // Secondary display positioned to the right (x = 1800) and slightly above primary (y = -120):
        let req = validate_request(&AnimateWindowFrameRequest {
            width: Some(460.0),
            height: 90.0,
            min_width: 400.0,
            min_height: 82.0,
            placement: WindowFramePlacement::RestorePosition,
            target_x: Some(1800.0),
            target_y: Some(-120.0),
            duration_ms: 185,
            pinned: Some(false),
        })
        .unwrap();
        let current = NSRect {
            origin: NSPoint { x: 100.0, y: 100.0 },
            size: NSSize {
                width: 400.0,
                height: 700.0,
            },
        };
        let target = compute_target_frame(current, current.size, primary_height, req);
        assert_eq!(target.origin.x, 1800.0);
        assert_eq!(target.origin.y, primary_height - (-120.0) - 90.0);
        assert_eq!(
            appkit_frame_to_top_left(target, primary_height),
            LogicalPoint { x: 1800, y: -120 }
        );
    }

    #[test]
    fn request_validation_clamps_duration_and_rejects_unknown_or_invalid_fields() {
        let valid = validate_request(&AnimateWindowFrameRequest {
            width: Some(400.0),
            height: 700.0,
            min_width: 400.0,
            min_height: 480.0,
            placement: WindowFramePlacement::AnchorTopLeft,
            target_x: None,
            target_y: None,
            duration_ms: 5000,
            pinned: None,
        })
        .unwrap();
        assert_eq!(valid.duration_ms, MAX_DURATION_MS);

        assert!(
            validate_request(&AnimateWindowFrameRequest {
                width: Some(100.0),
                height: 700.0,
                min_width: 400.0,
                min_height: 480.0,
                placement: WindowFramePlacement::AnchorTopLeft,
                target_x: None,
                target_y: None,
                duration_ms: 185,
                pinned: None,
            })
            .is_err()
        );
        assert!(
            validate_request(&AnimateWindowFrameRequest {
                width: Some(400.0),
                height: f64::NAN,
                min_width: 400.0,
                min_height: 480.0,
                placement: WindowFramePlacement::AnchorTopLeft,
                target_x: None,
                target_y: None,
                duration_ms: 185,
                pinned: None,
            })
            .is_err()
        );

        // `deny_unknown_fields` rejects unexpected IPC keys:
        let bad_wire = serde_json::from_str::<AnimateWindowFrameRequest>(
            r#"{"width":400,"height":700,"minWidth":400,"minHeight":480,"placement":"anchorTopLeft","durationMs":185,"extra":"nope"}"#,
        );
        assert!(bad_wire.is_err());
    }

    #[test]
    fn collection_behavior_toggles_all_spaces_without_losing_unrelated_flags() {
        const MANAGED: usize = 1 << 2;
        const FULL_SCREEN_PRIMARY: usize = 1 << 7;
        let baseline = MANAGED | FULL_SCREEN_PRIMARY;

        // Pinning adds CanJoinAllSpaces while keeping existing managed/full-screen flags:
        let pinned = next_collection_behavior(baseline, true);
        assert_eq!(
            pinned,
            baseline | NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
        );
        assert_eq!(next_collection_behavior(pinned, true), pinned);

        // Enabling CanJoinAllSpaces clears mutually exclusive MoveToActiveSpace:
        let with_move = baseline | NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE;
        let pinned_from_move = next_collection_behavior(with_move, true);
        assert_eq!(
            pinned_from_move,
            baseline | NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
        );
        assert_eq!(
            pinned_from_move & NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE,
            0
        );

        // Unpinning removes only CanJoinAllSpaces and preserves all other flags:
        let unpinned = next_collection_behavior(pinned, false);
        assert_eq!(unpinned, baseline);
        assert_eq!(next_collection_behavior(unpinned, false), baseline);
    }
}
