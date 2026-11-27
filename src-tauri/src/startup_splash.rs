//! Cold-launch whale mascot splash orchestration and native parabolic leap handoff.
//!
//! Keeps the main workbench window hidden while pre-positioning it to persisted geometry and
//! running repository hydration in parallel, floats a pure-transparent frameless splash window
//! centered on the target monitor, and leaps it along a smooth parabolic arc right into the
//! restored window position before smoothly revealing the hydrated main window.
use crate::{
    repository::{
        persistence::{read_persisted_theme, read_persisted_window_preferences},
        types::{PersistedWindowPreferences, WindowPosition, WindowSize},
    },
    window_motion::{self, MonitorWorkArea},
};
use serde::Serialize;
use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};

pub const SPLASH_WINDOW_LABEL: &str = "splash";
pub const SPLASH_WINDOW_SIZE: f64 = 220.0;
/// Optical resting anchor of the whale silhouette inside the 220×220 splash canvas
/// (leaves generous headroom above for expressive center bounces without clipping).
pub const WHALE_ANCHOR_X: f64 = 110.0;
pub const WHALE_ANCHOR_Y: f64 = 132.0;

const DEFAULT_FULL_WIDTH: f64 = 400.0;
const DEFAULT_FULL_HEIGHT: f64 = 700.0;
const MIN_FULL_WIDTH: f64 = 400.0;
const MIN_FULL_HEIGHT: f64 = 480.0;
const COMPACT_WIDTH: f64 = 400.0;
const COMPACT_DEFAULT_HEIGHT: f64 = 86.0;
const COMPACT_MIN_HEIGHT: f64 = 82.0;
const MAX_STORED_DIMENSION: f64 = 16_384.0;
const TITLEBAR_VISIBLE_HEIGHT: f64 = 38.0;
const MIN_VISIBLE_HORIZONTAL_OVERLAP: f64 = 80.0;
const MAIN_REVEAL_DURATION_MS: u32 = 240;
const MAIN_REVEAL_TRIGGER_PROGRESS: f64 = 0.90;
const SPLASH_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(12);
const POST_LANDING_CLEANUP_GRACE: Duration = Duration::from_millis(900);

#[derive(Clone, Debug, PartialEq)]
pub struct StartupWindowPlan {
    pub pinned: bool,
    pub compact_mode: bool,
    pub target_x: f64,
    pub target_y: f64,
    pub width: f64,
    pub height: f64,
    pub min_width: f64,
    pub min_height: f64,
    pub primary_height: f64,
    pub splash_start_x: f64,
    pub splash_start_y: f64,
    pub splash_end_x: f64,
    pub splash_end_y: f64,
    pub dx: f64,
    pub dy: f64,
    pub distance: f64,
    pub arc_height: f64,
    pub leap_duration_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParabolicSample {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplashLaunchPlan {
    pub dx: f64,
    pub dy: f64,
    pub distance: f64,
    pub arc_height: f64,
    pub leap_duration_ms: u32,
    pub compact_mode: bool,
    pub workbench_already_ready: bool,
    pub theme: String,
}

#[derive(Default)]
enum StartupPhase {
    #[default]
    Bypassed,
    Splashing {
        plan: StartupWindowPlan,
        theme: &'static str,
        splash_ready: bool,
        workbench_ready: bool,
        jump_started: bool,
    },
    HandingOff,
    Completed,
}

#[derive(Default)]
pub struct StartupSplashCoordinator {
    phase: Mutex<StartupPhase>,
}

fn normalize_full_size(size: Option<WindowSize>) -> (f64, f64) {
    match size {
        Some(s) => (
            f64::from(s.width).clamp(MIN_FULL_WIDTH, MAX_STORED_DIMENSION),
            f64::from(s.height).clamp(MIN_FULL_HEIGHT, MAX_STORED_DIMENSION),
        ),
        None => (DEFAULT_FULL_WIDTH, DEFAULT_FULL_HEIGHT),
    }
}

/// Mirrors `resolveVisibleWindowPosition` in `src/desktop/window.ts` so native startup placement
/// matches frontend monitor-clamping rules identically.
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

pub fn compute_startup_window_plan(
    prefs: Option<&PersistedWindowPreferences>,
    monitors: &[MonitorWorkArea],
    primary: Option<MonitorWorkArea>,
    primary_height: f64,
) -> StartupWindowPlan {
    let pinned = prefs.is_some_and(|p| p.pinned);
    let compact_mode = prefs.is_some_and(|p| p.compact_mode);
    let (width, height, min_width, min_height) = if compact_mode {
        (
            COMPACT_WIDTH,
            COMPACT_DEFAULT_HEIGHT,
            COMPACT_WIDTH,
            COMPACT_MIN_HEIGHT,
        )
    } else {
        let (w, h) = normalize_full_size(prefs.and_then(|p| p.full_size));
        (w, h, MIN_FULL_WIDTH, MIN_FULL_HEIGHT)
    };
    let desired_pos = prefs.and_then(|p| {
        if compact_mode {
            p.compact_position
        } else {
            p.full_position
        }
    });
    let (target_x, target_y, target_monitor) =
        resolve_visible_window_position(desired_pos, width, height, monitors, primary);

    // Center the floating whale splash on the monitor where the target window lives so the leap
    // always stays on the user's active display without jumping awkwardly across display bezels.
    let splash_start_x = (target_monitor.x + target_monitor.width * 0.5 - WHALE_ANCHOR_X).round();
    let splash_start_y = (target_monitor.y + target_monitor.height * 0.5 - WHALE_ANCHOR_Y).round();

    let anchor_x = target_x + width * 0.5;
    let anchor_y = if compact_mode {
        target_y + height * 0.5
    } else {
        target_y + (height * 0.44).min(280.0)
    };
    let splash_end_x = (anchor_x - WHALE_ANCHOR_X).round();
    let splash_end_y = (anchor_y - WHALE_ANCHOR_Y).round();

    let dx = splash_end_x - splash_start_x;
    let dy = splash_end_y - splash_start_y;
    let distance = dx.hypot(dy);

    let raw_arc = if distance < 36.0 {
        76.0
    } else {
        (distance * 0.28 + 68.0).clamp(76.0, 175.0)
    };
    let mid_y = (splash_start_y + splash_end_y) * 0.5;
    // Keep the whale silhouette below the menu bar at the jump apex.
    let max_arc = (mid_y - (target_monitor.y - 36.0)).max(36.0);
    let arc_height = raw_arc.min(max_arc).round();

    let leap_duration_ms = if distance < 36.0 {
        440
    } else {
        (400.0 + distance * 0.16).clamp(440.0, 580.0).round() as u32
    };

    StartupWindowPlan {
        pinned,
        compact_mode,
        target_x,
        target_y,
        width,
        height,
        min_width,
        min_height,
        primary_height,
        splash_start_x,
        splash_start_y,
        splash_end_x,
        splash_end_y,
        dx,
        dy,
        distance,
        arc_height,
        leap_duration_ms,
    }
}

/// Blends linear ballistic progression with soft sine easing so takeoff feels springy and landing
/// settles gently; matches `computeParabolicProgress` in `src/features/splash/whaleMath.ts`.
pub fn compute_parabolic_jump_sample(plan: &StartupWindowPlan, progress: f64) -> ParabolicSample {
    let t = progress.clamp(0.0, 1.0);
    let ease_sine = 0.5 * (1.0 - (std::f64::consts::PI * t).cos());
    let u = 0.65 * t + 0.35 * ease_sine;
    let arc_lift = 4.0 * plan.arc_height * t * (1.0 - t);
    ParabolicSample {
        x: plan.splash_start_x + plan.dx * u,
        y: plan.splash_start_y + plan.dy * u - arc_lift,
    }
}

impl StartupSplashCoordinator {
    /// Initializes startup window geometry and optionally spawns the floating whale splash window.
    /// Must run on the main thread during Tauri `.setup(...)`.
    pub fn setup_startup(&self, app: &AppHandle, ui_smoke: bool, data_dir: Option<&Path>) {
        let Some(main_window) = app.get_webview_window("main") else {
            return;
        };

        let startup_theme = data_dir.map(read_persisted_theme).unwrap_or_default();
        let resolved_theme = window_motion::apply_window_theme(&main_window, startup_theme, true);

        let persisted = data_dir.and_then(read_persisted_window_preferences);
        let (monitors, primary, primary_height) =
            window_motion::read_monitors_on_main(&main_window);
        let plan =
            compute_startup_window_plan(persisted.as_ref(), &monitors, primary, primary_height);

        // Pre-position the hidden main window before it ever appears so there is never a
        // centered default-size flash or jump to persisted coordinates.
        if persisted.is_some() || !ui_smoke {
            let _ = window_motion::configure_main_window_startup_on_main(
                &main_window,
                plan.target_x,
                plan.target_y,
                plan.width,
                plan.height,
                plan.min_width,
                plan.min_height,
                plan.pinned,
            );
        }

        if ui_smoke || window_motion::prefers_reduced_motion() {
            if let Ok(mut phase) = self.phase.lock() {
                *phase = StartupPhase::Bypassed;
            }
            let _ = main_window.show();
            let _ = main_window.set_focus();
            return;
        }

        let splash_result = tauri::WebviewWindowBuilder::new(
            app,
            SPLASH_WINDOW_LABEL,
            tauri::WebviewUrl::App("splash.html".into()),
        )
        .title("Picket")
        .inner_size(SPLASH_WINDOW_SIZE, SPLASH_WINDOW_SIZE)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build();

        let Ok(splash_window) = splash_result else {
            if let Ok(mut phase) = self.phase.lock() {
                *phase = StartupPhase::Bypassed;
            }
            let _ = main_window.show();
            let _ = main_window.set_focus();
            return;
        };

        let _ = window_motion::apply_window_theme(&splash_window, startup_theme, false);

        if window_motion::prepare_splash_window_on_main(
            &splash_window,
            plan.splash_start_x,
            plan.splash_start_y,
            SPLASH_WINDOW_SIZE,
            plan.primary_height,
        )
        .is_err()
        {
            let _ = splash_window.destroy();
            if let Ok(mut phase) = self.phase.lock() {
                *phase = StartupPhase::Bypassed;
            }
            let _ = main_window.show();
            let _ = main_window.set_focus();
            return;
        }

        if let Ok(mut phase) = self.phase.lock() {
            *phase = StartupPhase::Splashing {
                plan,
                theme: resolved_theme,
                splash_ready: false,
                workbench_ready: false,
                jump_started: false,
            };
        }

        // Failsafe watchdog: guarantee the main window always becomes visible even if the splash
        // webview fails to initialize or emit completion.
        let watchdog_app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SPLASH_WATCHDOG_TIMEOUT).await;
            let should_force = {
                let coordinator = watchdog_app.state::<StartupSplashCoordinator>();
                coordinator
                    .phase
                    .lock()
                    .map(|p| {
                        matches!(
                            *p,
                            StartupPhase::Splashing { .. } | StartupPhase::HandingOff
                        )
                    })
                    .unwrap_or(false)
            };
            if should_force {
                finish_splash_and_reveal(&watchdog_app, true).await;
            }
        });
    }

    pub fn may_reopen_main(&self) -> bool {
        self.phase
            .lock()
            .map(|phase| matches!(*phase, StartupPhase::Bypassed | StartupPhase::Completed))
            .unwrap_or(true)
    }
}

async fn finish_splash_and_reveal(app: &AppHandle, force_show_main: bool) {
    {
        let coordinator = app.state::<StartupSplashCoordinator>();
        if let Ok(mut phase) = coordinator.phase.lock() {
            if matches!(*phase, StartupPhase::Completed) {
                return;
            }
            *phase = StartupPhase::Completed;
        }
    }
    if let Some(main) = app.get_webview_window("main") {
        if force_show_main {
            let _ = window_motion::reveal_main_window_smoothly(main, 0).await;
        } else {
            window_motion::ensure_main_window_opaque(main).await;
        }
    }
    if let Some(splash) = app.get_webview_window(SPLASH_WINDOW_LABEL) {
        let _ = splash.destroy();
    }
}

#[tauri::command]
pub async fn splash_ready(
    app: AppHandle,
    coordinator: State<'_, StartupSplashCoordinator>,
) -> Result<SplashLaunchPlan, String> {
    let launch_plan = {
        let mut phase = coordinator
            .phase
            .lock()
            .map_err(|_| "Startup splash lock poisoned".to_string())?;
        match &mut *phase {
            StartupPhase::Splashing {
                plan,
                theme,
                splash_ready,
                workbench_ready,
                ..
            } => {
                *splash_ready = true;
                SplashLaunchPlan {
                    dx: plan.dx,
                    dy: plan.dy,
                    distance: plan.distance,
                    arc_height: plan.arc_height,
                    leap_duration_ms: plan.leap_duration_ms,
                    compact_mode: plan.compact_mode,
                    workbench_already_ready: *workbench_ready,
                    theme: (*theme).to_string(),
                }
            }
            _ => {
                return Err("Splash session is no longer active".into());
            }
        }
    };

    if let Some(splash) = app.get_webview_window(SPLASH_WINDOW_LABEL) {
        window_motion::show_splash_window(splash).await?;
    }

    Ok(launch_plan)
}

#[tauri::command]
pub async fn notify_workbench_ready(
    app: AppHandle,
    coordinator: State<'_, StartupSplashCoordinator>,
) -> Result<(), String> {
    let should_emit_to_splash = {
        let mut phase = coordinator
            .phase
            .lock()
            .map_err(|_| "Startup splash lock poisoned".to_string())?;
        match &mut *phase {
            StartupPhase::Splashing {
                splash_ready,
                workbench_ready,
                jump_started,
                ..
            } => {
                *workbench_ready = true;
                *splash_ready && !*jump_started
            }
            _ => false,
        }
    };

    if should_emit_to_splash && let Some(splash) = app.get_webview_window(SPLASH_WINDOW_LABEL) {
        let _ = splash.emit("picket:splash-ready-to-land", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn run_splash_jump(
    app: AppHandle,
    coordinator: State<'_, StartupSplashCoordinator>,
) -> Result<(), String> {
    let plan = {
        let mut phase = coordinator
            .phase
            .lock()
            .map_err(|_| "Startup splash lock poisoned".to_string())?;
        match &mut *phase {
            StartupPhase::Splashing {
                plan, jump_started, ..
            } if !*jump_started => {
                *jump_started = true;
                let cloned = plan.clone();
                *phase = StartupPhase::HandingOff;
                cloned
            }
            _ => return Ok(()),
        }
    };

    let Some(splash) = app.get_webview_window(SPLASH_WINDOW_LABEL) else {
        finish_splash_and_reveal(&app, true).await;
        return Ok(());
    };
    let main = app.get_webview_window("main");

    let duration = Duration::from_millis(u64::from(plan.leap_duration_ms));
    let start = Instant::now();
    let mut ticker = tokio::time::interval(Duration::from_millis(8));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut revealed_main = false;

    loop {
        ticker.tick().await;
        let t = if duration.is_zero() {
            1.0
        } else {
            (start.elapsed().as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0)
        };
        let sample = compute_parabolic_jump_sample(&plan, t);
        let _ = window_motion::move_splash_window_origin(
            splash.clone(),
            sample.x,
            sample.y,
            SPLASH_WINDOW_SIZE,
            plan.primary_height,
        )
        .await;

        if !revealed_main && t >= MAIN_REVEAL_TRIGGER_PROGRESS {
            revealed_main = true;
            if let Some(ref main_window) = main {
                let _ = window_motion::reveal_main_window_smoothly(
                    main_window.clone(),
                    MAIN_REVEAL_DURATION_MS,
                )
                .await;
            }
        }

        if t >= 1.0 {
            break;
        }
    }

    if !revealed_main && let Some(main_window) = main {
        let _ =
            window_motion::reveal_main_window_smoothly(main_window, MAIN_REVEAL_DURATION_MS).await;
    }

    // Safety grace cleanup in case `complete_splash` is delayed or dropped.
    let cleanup_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(POST_LANDING_CLEANUP_GRACE).await;
        finish_splash_and_reveal(&cleanup_app, false).await;
    });

    Ok(())
}

#[tauri::command]
pub async fn complete_splash(app: AppHandle) -> Result<(), String> {
    finish_splash_and_reveal(&app, false).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary_monitor() -> MonitorWorkArea {
        MonitorWorkArea {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 875.0,
        }
    }

    fn secondary_monitor_right() -> MonitorWorkArea {
        MonitorWorkArea {
            x: 1440.0,
            y: -120.0,
            width: 2560.0,
            height: 1415.0,
        }
    }

    #[test]
    fn first_launch_centers_both_main_and_splash_on_primary_monitor() {
        let mon = primary_monitor();
        let plan = compute_startup_window_plan(None, &[mon], Some(mon), 900.0);

        assert!(!plan.pinned);
        assert!(!plan.compact_mode);
        assert_eq!(plan.width, 400.0);
        assert_eq!(plan.height, 700.0);
        assert_eq!(plan.target_x, 520.0);
        assert_eq!(plan.target_y, 113.0);
        // Whale starts centered horizontally and jumps lightly in place:
        assert_eq!(plan.splash_start_x, 610.0);
        assert_eq!(plan.splash_end_x, 610.0);
        assert!(plan.arc_height >= 76.0);
        assert!(plan.leap_duration_ms >= 440);
    }

    #[test]
    fn compact_mode_on_secondary_monitor_starts_splash_on_that_same_monitor() {
        let primary = primary_monitor();
        let secondary = secondary_monitor_right();
        let prefs = PersistedWindowPreferences {
            pinned: true,
            compact_mode: true,
            pinned_expanded: false,
            full_size: Some(WindowSize {
                width: 440,
                height: 760,
            }),
            full_position: Some(WindowPosition { x: 100, y: 100 }),
            compact_position: Some(WindowPosition { x: 3500, y: -80 }),
        };

        let plan =
            compute_startup_window_plan(Some(&prefs), &[primary, secondary], Some(primary), 900.0);

        assert!(plan.pinned);
        assert!(plan.compact_mode);
        assert_eq!(plan.width, 400.0);
        assert_eq!(plan.height, 86.0);
        assert_eq!(plan.target_x, 3500.0);
        assert_eq!(plan.target_y, -80.0);
        // Splash starts at center of secondary monitor (1440 + 1280 - 110 = 2610):
        assert_eq!(plan.splash_start_x, 2610.0);
        // Compact bar center is (3500 + 200, -80 + 43) = (3700, -37):
        assert_eq!(plan.splash_end_x, 3700.0 - WHALE_ANCHOR_X);
        assert_eq!(plan.splash_end_y, -37.0 - WHALE_ANCHOR_Y);
        assert!(plan.dx > 0.0);
        assert!(plan.leap_duration_ms >= 440 && plan.leap_duration_ms <= 580);
    }

    #[test]
    fn disconnected_monitor_position_clamps_back_onto_primary_work_area() {
        let primary = primary_monitor();
        let prefs = PersistedWindowPreferences {
            pinned: false,
            compact_mode: false,
            pinned_expanded: false,
            full_size: Some(WindowSize {
                width: 420,
                height: 680,
            }),
            full_position: Some(WindowPosition { x: 4000, y: 200 }),
            compact_position: None,
        };

        let plan = compute_startup_window_plan(Some(&prefs), &[primary], Some(primary), 900.0);

        assert_eq!(plan.target_x, 1440.0 - 420.0);
        assert_eq!(plan.target_y, 200.0);
        assert_eq!(plan.splash_start_x, 610.0);
    }

    #[test]
    fn parabolic_jump_sample_hits_exact_endpoints_and_peaks_upward_at_midpoint() {
        let mon = primary_monitor();
        let prefs = PersistedWindowPreferences {
            pinned: true,
            compact_mode: true,
            pinned_expanded: false,
            full_size: None,
            full_position: None,
            compact_position: Some(WindowPosition { x: 980, y: 60 }),
        };
        let plan = compute_startup_window_plan(Some(&prefs), &[mon], Some(mon), 900.0);

        let start = compute_parabolic_jump_sample(&plan, 0.0);
        let mid = compute_parabolic_jump_sample(&plan, 0.5);
        let end = compute_parabolic_jump_sample(&plan, 1.0);

        assert!((start.x - plan.splash_start_x).abs() < 1e-6);
        assert!((start.y - plan.splash_start_y).abs() < 1e-6);
        assert!((end.x - plan.splash_end_x).abs() < 1e-6);
        assert!((end.y - plan.splash_end_y).abs() < 1e-6);
        // At t = 0.5, x is exact midpoint and y is lifted upward (smaller y) by arc_height:
        let linear_mid_y = (plan.splash_start_y + plan.splash_end_y) * 0.5;
        assert!((mid.x - (plan.splash_start_x + plan.splash_end_x) * 0.5).abs() < 1e-6);
        assert!((mid.y - (linear_mid_y - plan.arc_height)).abs() < 1e-6);
    }

    #[test]
    fn arc_height_clamps_near_top_menu_bar_so_whale_stays_visible() {
        let mon = primary_monitor();
        let prefs = PersistedWindowPreferences {
            pinned: true,
            compact_mode: true,
            pinned_expanded: false,
            full_size: None,
            full_position: None,
            compact_position: Some(WindowPosition { x: 40, y: 25 }),
        };
        let plan = compute_startup_window_plan(Some(&prefs), &[mon], Some(mon), 900.0);
        let mid = compute_parabolic_jump_sample(&plan, 0.5);
        // Peak window top + 36px whale top padding stays at or below menu bar top (mon.y = 25):
        assert!(mid.y + 36.0 >= mon.y - 1.0);
        assert!(plan.arc_height >= 36.0);
    }

    #[test]
    fn splash_launch_plan_serializes_resolved_theme_in_camel_case() {
        let plan = SplashLaunchPlan {
            dx: 120.0,
            dy: -40.0,
            distance: 126.49,
            arc_height: 103.0,
            leap_duration_ms: 460,
            compact_mode: true,
            workbench_already_ready: false,
            theme: "light".to_string(),
        };
        let value = serde_json::to_value(&plan).expect("serialize splash launch plan");
        assert_eq!(value["theme"], "light");
        assert_eq!(value["compactMode"], true);
        assert_eq!(value["workbenchAlreadyReady"], false);
    }
}
