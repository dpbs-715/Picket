//! Low-level Apple AppKit / QuartzCore / Objective-C runtime FFI helpers (zero third-party dylibs).

use crate::window_motion::geometry::{
    NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES, NSPoint, NSRect, NSSize,
    next_collection_behavior,
};
use std::{
    ffi::{c_char, c_void},
    mem, ptr,
};

pub(super) type ObjcBool = u8;
pub(super) const BLOCK_HAS_COPY_DISPOSE: i32 = 1 << 25;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    pub(super) fn objc_getClass(name: *const c_char) -> *mut c_void;
    pub(super) fn sel_registerName(name: *const c_char) -> *const c_void;
    pub(super) fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    pub(super) fn objc_msgSend_stret();
    pub(super) fn objc_retain(obj: *mut c_void) -> *mut c_void;
    pub(super) fn objc_release(obj: *mut c_void);
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

#[link(name = "System", kind = "dylib")]
unsafe extern "C" {
    pub(super) static _NSConcreteStackBlock: *const c_void;
}

pub(super) struct AutoreleasePoolGuard(*mut c_void);

impl AutoreleasePoolGuard {
    pub(super) fn new() -> Self {
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

pub(super) unsafe fn msg_send_rect(receiver: *mut c_void, sel: *const c_void) -> NSRect {
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

pub(super) unsafe fn msg_send_rect_with_rect(
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

pub(super) unsafe fn msg_send_point(receiver: *mut c_void, sel: *const c_void) -> NSPoint {
    let f: unsafe extern "C" fn(*mut c_void, *const c_void) -> NSPoint =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, sel) }
}

pub(super) unsafe fn window_frame(ns_window: *mut c_void) -> NSRect {
    let sel = unsafe { sel_registerName(c"frame".as_ptr()) };
    unsafe { msg_send_rect(ns_window, sel) }
}

pub(super) unsafe fn window_content_rect(ns_window: *mut c_void, frame: NSRect) -> NSRect {
    let sel = unsafe { sel_registerName(c"contentRectForFrameRect:".as_ptr()) };
    unsafe { msg_send_rect_with_rect(ns_window, sel, frame) }
}

pub(super) unsafe fn set_window_content_min_size(ns_window: *mut c_void, size: NSSize) {
    let sel = unsafe { sel_registerName(c"setContentMinSize:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSSize) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, size) };
}

pub(super) unsafe fn set_window_frame_instant(ns_window: *mut c_void, frame: NSRect) {
    let sel = unsafe { sel_registerName(c"setFrame:display:animate:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSRect, ObjcBool, ObjcBool) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, frame, 1, 0) };
}

pub(super) unsafe fn set_window_frame_origin(ns_window: *mut c_void, origin: NSPoint) {
    let sel = unsafe { sel_registerName(c"setFrameOrigin:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, NSPoint) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, origin) };
}

pub(super) unsafe fn set_window_alpha(ns_window: *mut c_void, alpha: f64) {
    let sel = unsafe { sel_registerName(c"setAlphaValue:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, alpha) };
}

pub(super) unsafe fn set_window_opaque(ns_window: *mut c_void, opaque: bool) {
    let sel = unsafe { sel_registerName(c"setOpaque:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, u8::from(opaque)) };
}

pub(super) unsafe fn set_window_clear_background(ns_window: *mut c_void) {
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

pub(super) unsafe fn set_window_has_shadow(ns_window: *mut c_void, has_shadow: bool) {
    let sel = unsafe { sel_registerName(c"setHasShadow:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, u8::from(has_shadow)) };
}

pub(super) unsafe fn set_window_ignores_mouse_events(ns_window: *mut c_void, ignores: bool) {
    let sel = unsafe { sel_registerName(c"setIgnoresMouseEvents:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, ObjcBool) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, u8::from(ignores)) };
}

pub(super) unsafe fn set_window_level(ns_window: *mut c_void, level: isize) {
    let sel = unsafe { sel_registerName(c"setLevel:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, isize) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, level) };
}

pub(super) unsafe fn set_window_collection_behavior_raw(ns_window: *mut c_void, behavior: usize) {
    let sel = unsafe { sel_registerName(c"setCollectionBehavior:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, usize) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, behavior) };
}

pub(super) unsafe fn order_window_front_regardless(ns_window: *mut c_void) {
    let sel = unsafe { sel_registerName(c"orderFrontRegardless".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel) };
}

pub(super) unsafe fn make_window_key_and_order_front(ns_window: *mut c_void) {
    let sel = unsafe { sel_registerName(c"makeKeyAndOrderFront:".as_ptr()) };
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_window, sel, ptr::null_mut()) };
}

pub(super) unsafe fn activate_app_macos() {
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

pub(super) unsafe fn set_window_visible_on_all_spaces(ns_window: *mut c_void, visible: bool) {
    let current = unsafe { window_collection_behavior(ns_window) };
    let next = next_collection_behavior(current, visible);
    if next != current {
        unsafe { set_window_collection_behavior_raw(ns_window, next) };
    }
}

pub(super) unsafe fn is_window_visible_on_all_spaces(ns_window: *mut c_void) -> bool {
    (unsafe { window_collection_behavior(ns_window) }
        & NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES)
        != 0
}

pub(super) fn apply_pinned_on_main(
    window: &tauri::WebviewWindow,
    ns_window: *mut c_void,
    pinned: bool,
) -> Result<bool, String> {
    if window.is_always_on_top().map_err(|e| e.to_string())? != pinned {
        window
            .set_always_on_top(pinned)
            .map_err(|e| e.to_string())?;
    }
    unsafe { set_window_visible_on_all_spaces(ns_window, pinned) };
    window.is_always_on_top().map_err(|e| e.to_string())
}

pub(super) unsafe fn primary_screen_height(ns_window: *mut c_void) -> f64 {
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

pub(super) unsafe fn prefers_reduced_motion_macos() -> bool {
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
    let send_responds: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void) -> ObjcBool =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    if unsafe { send_responds(workspace, responds_sel, reduce_sel) } == 0 {
        return false;
    }
    let send_bool: unsafe extern "C" fn(*mut c_void, *const c_void) -> ObjcBool =
        unsafe { mem::transmute(objc_msgSend as *const ()) };
    unsafe { send_bool(workspace, reduce_sel) != 0 }
}
