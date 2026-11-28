#[cfg(feature = "smoke-harness")]
use crate::pi_runtime::{PiEvent, ProbeReport};
use crate::{i18n, pi_runtime::PiSupervisor, runner::RunProcessManager, tray::TrayCoordinator};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

#[cfg(feature = "smoke-harness")]
pub struct DesktopLifecycle {
    pub ui_smoke: bool,
    pub restart_smoke: bool,
    pub keep_smoke_repo: bool,
    triggered: AtomicBool,
    exiting: AtomicBool,
    quit_prompt_open: AtomicBool,
    pi_verified: AtomicBool,
    git_verified: AtomicBool,
    close_probe_active: AtomicBool,
    restart_verified: AtomicBool,
    failed: AtomicBool,
}

#[cfg(not(feature = "smoke-harness"))]
#[derive(Default)]
pub struct DesktopLifecycle {
    exiting: AtomicBool,
    quit_prompt_open: AtomicBool,
}

#[cfg(feature = "smoke-harness")]
impl DesktopLifecycle {
    pub fn with_flags(ui_smoke: bool, restart_smoke: bool, keep_smoke_repo: bool) -> Self {
        Self {
            ui_smoke: ui_smoke || restart_smoke,
            restart_smoke,
            keep_smoke_repo,
            triggered: AtomicBool::new(false),
            exiting: AtomicBool::new(false),
            quit_prompt_open: AtomicBool::new(false),
            pi_verified: AtomicBool::new(false),
            git_verified: AtomicBool::new(false),
            close_probe_active: AtomicBool::new(false),
            restart_verified: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        }
    }
    pub fn is_ui_smoke(&self) -> bool {
        self.ui_smoke
    }
    pub fn start_ui_smoke(&self) -> bool {
        self.ui_smoke && !self.triggered.swap(true, Ordering::SeqCst)
    }
    pub fn observe(&self, request_id: &str, event: &PiEvent) {
        if !self.ui_smoke {
            return;
        }
        if let PiEvent::Ready { pid, .. } = event {
            println!(
                "{}",
                serde_json::json!({ "type": "uiSmokeChild", "requestId": request_id, "pid": pid })
            );
        }
        if request_id == "ui-close" && matches!(event, PiEvent::TextDelta { .. }) {
            self.close_probe_active.store(true, Ordering::SeqCst);
        }
    }
    pub fn pi_verified(&self, report: &ProbeReport) {
        if self.ui_smoke
            && report.request_id == "ui-complete"
            && report.status == "completed"
            && report.child_exit_code == 0
        {
            self.pi_verified.store(true, Ordering::SeqCst);
        }
    }
    pub fn git_verified(&self) {
        if self.ui_smoke {
            self.git_verified.store(true, Ordering::SeqCst);
        }
    }
    pub fn mark_restart_verified(&self) {
        if self.restart_smoke {
            self.restart_verified.store(true, Ordering::SeqCst);
        }
    }
    pub fn may_close_probe(&self) -> bool {
        if self.restart_smoke {
            return self.restart_verified.load(Ordering::SeqCst);
        }
        self.ui_smoke
            && self.pi_verified.load(Ordering::SeqCst)
            && self.git_verified.load(Ordering::SeqCst)
            && self.close_probe_active.load(Ordering::SeqCst)
    }
    pub fn fail(&self) {
        self.failed.store(true, Ordering::SeqCst);
    }
}

#[cfg(not(feature = "smoke-harness"))]
impl DesktopLifecycle {
    pub fn is_ui_smoke(&self) -> bool {
        false
    }
}

pub async fn cleanup_for_restart(app: &tauri::AppHandle) {
    app.state::<DesktopLifecycle>()
        .exiting
        .store(true, Ordering::SeqCst);
    app.state::<TrayCoordinator>().teardown(app);
    app.state::<crate::models::store::ModelStore>()
        .cancel_all_list_requests();
    let runner = app.state::<RunProcessManager>().inner().clone();
    let supervisor = app.state::<PiSupervisor>().inner().clone();
    let repositories = app
        .state::<crate::repository::RepositoryStore>()
        .inner()
        .clone();
    runner.shutdown_all().await;
    repositories.shutdown().await;
    if let Err(error) = app
        .state::<crate::repository::editor::EditorLauncher>()
        .cleanup()
    {
        eprintln!("Temporary editor snapshot cleanup failed before restart: {error}");
    }
    #[cfg(feature = "smoke-harness")]
    {
        let _ = app
            .state::<crate::repository::smoke::SmokeState>()
            .cleanup();
        app.state::<crate::models::smoke::ModelSmokeState>()
            .cleanup();
    }
    if let Err(error) = supervisor.shutdown().await {
        eprintln!("Pi cleanup failed before restart: {error}");
    }
}

/// Checks for active local Run processes before quitting. If none are active (or in smoke mode),
/// proceeds directly to `exit_after_cleanup`. Otherwise shows a native confirmation dialog and
/// cleanly stops every Run process group upon confirmation.
pub fn request_app_quit(app: &tauri::AppHandle) {
    let lifecycle = app.state::<DesktopLifecycle>();
    if lifecycle.exiting.load(Ordering::SeqCst) {
        return;
    }
    if lifecycle.is_ui_smoke() {
        exit_after_cleanup(app);
        return;
    }
    let active = app.state::<RunProcessManager>().active_snapshots();
    if active.is_empty() {
        exit_after_cleanup(app);
        return;
    }
    if lifecycle.quit_prompt_open.swap(true, Ordering::SeqCst) {
        return;
    }
    let locale = app.state::<TrayCoordinator>().current_locale();
    let names: Vec<String> = active
        .iter()
        .map(|svc| format!("{} · {}", svc.repository_name, svc.label))
        .collect();
    let title = i18n::runner::quit_confirm_title(locale);
    let message = i18n::runner::quit_confirm_message(locale, &names);
    let ok_label = i18n::runner::quit_confirm_ok_button(locale);
    let cancel_label = i18n::runner::quit_confirm_cancel_button(locale);
    let app_clone = app.clone();
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            ok_label.to_owned(),
            cancel_label.to_owned(),
        ))
        .show(move |confirmed| {
            app_clone
                .state::<DesktopLifecycle>()
                .quit_prompt_open
                .store(false, Ordering::SeqCst);
            if confirmed {
                exit_after_cleanup(&app_clone);
            }
        });
}

pub fn exit_after_cleanup(app: &tauri::AppHandle) {
    if app
        .state::<DesktopLifecycle>()
        .exiting
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let runner = app.state::<RunProcessManager>().inner().clone();
    let supervisor = app.state::<PiSupervisor>().inner().clone();
    let repositories = app
        .state::<crate::repository::RepositoryStore>()
        .inner()
        .clone();
    app.state::<TrayCoordinator>().teardown(app);
    app.state::<crate::models::store::ModelStore>()
        .cancel_all_list_requests();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        runner.shutdown_all().await;
        repositories.shutdown().await;
        let mut editor_cleanup_failed = false;
        if let Err(error) = app
            .state::<crate::repository::editor::EditorLauncher>()
            .cleanup()
        {
            eprintln!("Temporary editor snapshot cleanup failed: {error}");
            editor_cleanup_failed = true;
            #[cfg(feature = "smoke-harness")]
            app.state::<DesktopLifecycle>().fail();
        }
        #[cfg(feature = "smoke-harness")]
        {
            let preserve_repo = app.state::<DesktopLifecycle>().keep_smoke_repo
                && !app
                    .state::<DesktopLifecycle>()
                    .failed
                    .load(Ordering::SeqCst);
            if preserve_repo {
                let _ = app
                    .state::<crate::repository::smoke::SmokeState>()
                    .preserve_for_restart();
            } else if let Err(error) = app
                .state::<crate::repository::smoke::SmokeState>()
                .cleanup()
            {
                eprintln!("Temporary repository read proof cleanup failed: {error}");
                app.state::<DesktopLifecycle>().fail();
            }
            app.state::<crate::models::smoke::ModelSmokeState>()
                .cleanup();
        }
        let result = supervisor.shutdown().await;
        #[cfg(feature = "smoke-harness")]
        let passed = {
            let lifecycle = app.state::<DesktopLifecycle>();
            let passed = result.is_ok()
                && !editor_cleanup_failed
                && (!lifecycle.ui_smoke
                    || (lifecycle.may_close_probe() && !lifecycle.failed.load(Ordering::SeqCst)));
            if lifecycle.ui_smoke {
                println!(
                    "{}",
                    serde_json::json!({ "type": "uiSmokeExit", "status": if passed { "passed" } else { "failed" } })
                );
            }
            passed
        };
        #[cfg(not(feature = "smoke-harness"))]
        let passed = result.is_ok() && !editor_cleanup_failed;
        if let Err(error) = result {
            eprintln!("Pi cleanup failed: {error}");
        }
        app.exit(if passed { 0 } else { 1 });
    });
}
