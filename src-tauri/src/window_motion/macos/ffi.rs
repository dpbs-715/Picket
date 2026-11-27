//! Safe, typed AppKit / QuartzCore / Foundation helpers built on `objc2` and `block2`.

#[cfg(feature = "smoke-harness")]
use crate::window_motion::geometry::NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES;
use crate::window_motion::geometry::{NSPoint, NSRect, NSSize, next_collection_behavior};
use objc2::{msg_send, runtime::Bool, sel};
use objc2_app_kit::{
    NSAnimationContext, NSApplication, NSColor, NSScreen, NSWindow, NSWindowCollectionBehavior,
    NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSPoint as FNSPoint, NSRect as FNSRect, NSSize as FNSSize,
};
use objc2_quartz_core::CAMediaTimingFunction;
use std::ffi::c_void;

pub(super) fn to_foundation_point(p: NSPoint) -> FNSPoint {
    FNSPoint::new(p.x, p.y)
}

pub(super) fn to_foundation_size(s: NSSize) -> FNSSize {
    FNSSize::new(s.width, s.height)
}

pub(super) fn to_foundation_rect(r: NSRect) -> FNSRect {
    FNSRect::new(to_foundation_point(r.origin), to_foundation_size(r.size))
}

pub(super) fn from_foundation_rect(r: FNSRect) -> NSRect {
    NSRect {
        origin: NSPoint {
            x: r.origin.x,
            y: r.origin.y,
        },
        size: NSSize {
            width: r.size.width,
            height: r.size.height,
        },
    }
}

/// Converts a Tauri `ns_window()` raw pointer into a typed `&NSWindow` on the main thread.
///
/// # Safety
/// Caller must run on the AppKit main thread and ensure `window` remains alive for the lifetime
/// of the returned reference.
pub(super) unsafe fn ns_window_ref<'a>(
    window: &'a tauri::WebviewWindow,
    unavailable_msg: &str,
) -> Result<&'a NSWindow, String> {
    let raw: *mut c_void = window.ns_window().map_err(|e| e.to_string())?;
    unsafe { raw.cast::<NSWindow>().as_ref() }.ok_or_else(|| unavailable_msg.to_owned())
}

/// Obtains a `MainThreadMarker` inside callbacks dispatched via `WebviewWindow::run_on_main_thread`
/// or functions documented as `_on_main`.
pub(super) fn main_thread_marker() -> MainThreadMarker {
    // SAFETY: All callers execute on the macOS AppKit main thread (verified at runtime in debug).
    MainThreadMarker::new().unwrap_or_else(|| unsafe { MainThreadMarker::new_unchecked() })
}

pub(super) fn window_frame(win: &NSWindow) -> NSRect {
    from_foundation_rect(win.frame())
}

pub(super) fn window_content_rect(win: &NSWindow, frame: NSRect) -> NSRect {
    from_foundation_rect(win.contentRectForFrameRect(to_foundation_rect(frame)))
}

pub(super) fn set_window_content_min_size(win: &NSWindow, size: NSSize) {
    win.setContentMinSize(to_foundation_size(size));
}

pub(super) fn set_window_frame_instant(win: &NSWindow, frame: NSRect) {
    win.setFrame_display_animate(to_foundation_rect(frame), true, false);
}

pub(super) fn set_window_frame_origin(win: &NSWindow, origin: NSPoint) {
    win.setFrameOrigin(to_foundation_point(origin));
}

pub(super) fn set_window_alpha(win: &NSWindow, alpha: f64) {
    win.setAlphaValue(alpha);
}

pub(super) fn set_window_opaque(win: &NSWindow, opaque: bool) {
    win.setOpaque(opaque);
}

pub(super) fn set_window_clear_background(win: &NSWindow) {
    let clear = NSColor::clearColor();
    win.setBackgroundColor(Some(&clear));
}

pub(super) fn set_window_has_shadow(win: &NSWindow, has_shadow: bool) {
    win.setHasShadow(has_shadow);
}

pub(super) fn set_window_ignores_mouse_events(win: &NSWindow, ignores: bool) {
    win.setIgnoresMouseEvents(ignores);
}

pub(super) fn set_window_level(win: &NSWindow, level: isize) {
    win.setLevel(level);
}

pub(super) fn set_window_collection_behavior_raw(win: &NSWindow, behavior: usize) {
    win.setCollectionBehavior(NSWindowCollectionBehavior::from_bits_retain(behavior));
}

pub(super) fn order_window_front_regardless(win: &NSWindow) {
    win.orderFrontRegardless();
}

pub(super) fn make_window_key_and_order_front(win: &NSWindow) {
    win.makeKeyAndOrderFront(None);
}

pub(super) fn activate_app_macos() {
    let mtm = main_thread_marker();
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

pub(super) fn set_window_visible_on_all_spaces(win: &NSWindow, visible: bool) {
    let current = win.collectionBehavior().bits();
    let next = next_collection_behavior(current, visible);
    if next != current {
        set_window_collection_behavior_raw(win, next);
    }
}

#[cfg(feature = "smoke-harness")]
pub(super) fn is_window_visible_on_all_spaces(win: &NSWindow) -> bool {
    (win.collectionBehavior().bits() & NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES) != 0
}

pub(super) fn apply_pinned_on_main(
    window: &tauri::WebviewWindow,
    win: &NSWindow,
    pinned: bool,
) -> Result<bool, String> {
    if window.is_always_on_top().map_err(|e| e.to_string())? != pinned {
        window
            .set_always_on_top(pinned)
            .map_err(|e| e.to_string())?;
    }
    set_window_visible_on_all_spaces(win, pinned);
    window.is_always_on_top().map_err(|e| e.to_string())
}

pub(super) fn primary_screen_height(win: &NSWindow) -> f64 {
    let mtm = main_thread_marker();
    let screens = NSScreen::screens(mtm);
    if let Some(primary) = screens.firstObject() {
        let height = primary.frame().size.height;
        if height > 0.0 {
            return height;
        }
    }
    // Fallback to window's screen if NSScreen.screens is empty (headless edge case).
    if let Some(screen) = win.screen() {
        let height = screen.frame().size.height;
        if height > 0.0 {
            return height;
        }
    }
    900.0
}

pub(super) fn prefers_reduced_motion_macos() -> bool {
    let workspace = NSWorkspace::sharedWorkspace();
    let sel = sel!(accessibilityDisplayShouldReduceMotion);
    let responds: Bool = unsafe { msg_send![&*workspace, respondsToSelector: sel] };
    if !responds.as_bool() {
        return false;
    }
    let reduce: Bool = unsafe { msg_send![&*workspace, accessibilityDisplayShouldReduceMotion] };
    reduce.as_bool()
}

/// Applies Picket's shared easing curve (`cubic-bezier(0.22, 1, 0.36, 1)`) and duration
/// to the active `NSAnimationContext`.
pub(super) fn configure_current_animation_context(duration_ms: u32) {
    let ctx = NSAnimationContext::currentContext();
    ctx.setDuration(f64::from(duration_ms) / 1000.0);
    let timing_fn =
        CAMediaTimingFunction::functionWithControlPoints(0.22_f32, 1.0_f32, 0.36_f32, 1.0_f32);
    ctx.setTimingFunction(Some(&timing_fn));
}
