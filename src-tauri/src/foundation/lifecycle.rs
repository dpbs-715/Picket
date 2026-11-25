use super::pi::{PiEvent, PiSupervisor, ProbeReport};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

pub struct DesktopLifecycle {
    pub ui_smoke: bool,
    pub restart_smoke: bool,
    pub keep_smoke_repo: bool,
    triggered: AtomicBool,
    exiting: AtomicBool,
    pi_verified: AtomicBool,
    git_verified: AtomicBool,
    close_probe_active: AtomicBool,
    restart_verified: AtomicBool,
    failed: AtomicBool,
}

impl DesktopLifecycle {
    pub fn with_flags(ui_smoke: bool, restart_smoke: bool, keep_smoke_repo: bool) -> Self {
        Self {
            ui_smoke: ui_smoke || restart_smoke,
            restart_smoke,
            keep_smoke_repo,
            triggered: AtomicBool::new(false),
            exiting: AtomicBool::new(false),
            pi_verified: AtomicBool::new(false),
            git_verified: AtomicBool::new(false),
            close_probe_active: AtomicBool::new(false),
            restart_verified: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        }
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

pub async fn cleanup_for_restart(app: &tauri::AppHandle) {
    app.state::<DesktopLifecycle>()
        .exiting
        .store(true, Ordering::SeqCst);
    let supervisor = app.state::<PiSupervisor>().inner().clone();
    let repositories = app
        .state::<crate::repository::RepositoryStore>()
        .inner()
        .clone();
    repositories.shutdown().await;
    if let Err(error) = app
        .state::<crate::repository::editor::EditorLauncher>()
        .cleanup()
    {
        eprintln!("Temporary editor snapshot cleanup failed before restart: {error}");
    }
    let _ = app
        .state::<crate::repository::smoke::SmokeState>()
        .cleanup();
    app.state::<crate::models::smoke::ModelSmokeState>()
        .cleanup();
    if let Err(error) = supervisor.shutdown().await {
        eprintln!("Pi cleanup failed before restart: {error}");
    }
}

pub fn exit_after_cleanup(app: &tauri::AppHandle) {
    if app
        .state::<DesktopLifecycle>()
        .exiting
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let supervisor = app.state::<PiSupervisor>().inner().clone();
    let repositories = app
        .state::<crate::repository::RepositoryStore>()
        .inner()
        .clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        repositories.shutdown().await;
        if let Err(error) = app
            .state::<crate::repository::editor::EditorLauncher>()
            .cleanup()
        {
            eprintln!("Temporary editor snapshot cleanup failed: {error}");
            app.state::<DesktopLifecycle>().fail();
        }
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
        let result = supervisor.shutdown().await;
        let lifecycle = app.state::<DesktopLifecycle>();
        let passed = result.is_ok()
            && (!lifecycle.ui_smoke
                || (lifecycle.may_close_probe() && !lifecycle.failed.load(Ordering::SeqCst)));
        if lifecycle.ui_smoke {
            println!(
                "{}",
                serde_json::json!({ "type": "uiSmokeExit", "status": if passed { "passed" } else { "failed" } })
            );
        }
        if let Err(error) = result {
            eprintln!("Pi cleanup failed: {error}");
        }
        app.exit(if passed { 0 } else { 1 });
    });
}
