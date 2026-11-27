//! macOS native window motion implementation grouped by responsibility.

mod displays;
mod ffi;
mod main_window;
mod overlays;

pub use displays::{
    mouse_location_logical_on_main, prefers_reduced_motion, read_monitors_on_main,
    read_monitors_with_scale_on_main,
};
#[cfg(feature = "smoke-harness")]
pub(super) use main_window::read_main_window_visible_on_all_spaces;
pub use main_window::{
    activate_and_focus_main_on_main, configure_main_window_startup_on_main,
    ensure_main_window_opaque, reveal_main_window_smoothly,
};
pub(super) use main_window::{animate_main_window, set_main_window_pinned};
pub use overlays::{
    move_splash_window_origin, prepare_splash_window_on_main, prepare_tray_popover_window_on_main,
    resize_tray_popover_height, show_splash_window, show_tray_popover_on_main,
};
