#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod actions;
mod bounded_cache;
mod commands;
mod error;
#[cfg(any(test, feature = "smoke-harness"))]
mod foundation;
mod git_env;
mod i18n;
mod lifecycle;
mod models;
mod pi_runtime;
#[cfg(test)]
mod protocol_contract_tests;
#[cfg(test)]
mod protocol_export;
mod repo_lock;
mod repository;
mod skills;
mod startup_splash;
mod storage;
mod time;
mod tray;
mod updater;
mod window_motion;

use actions::CustomActionCoordinator;
use commands::{RepositoryPicker, picket_data_dir};
use lifecycle::{DesktopLifecycle, exit_after_cleanup};
#[cfg(feature = "smoke-harness")]
use models::smoke::ModelSmokeState;
use models::store::ModelStore;
#[cfg(feature = "smoke-harness")]
use pi_runtime::PiPaths;
use pi_runtime::PiSupervisor;
use repository::{RepositoryStore, editor::EditorLauncher, persistence::WorkbenchPersistence};
use skills::SkillManager;
use tauri::Manager;
use updater::UpdateManager;

#[cfg(feature = "smoke-harness")]
fn run_foundation_smoke() {
    let paths = if cfg!(debug_assertions) {
        Ok(PiPaths::development())
    } else {
        std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| {
                let contents = exe
                    .parent()
                    .and_then(|p| p.parent())
                    .ok_or("Invalid macOS application bundle")?;
                PiPaths::bundled(&exe, &contents.join("Resources"))
            })
    };
    let runtime = tokio::runtime::Runtime::new().expect("Cannot create runtime");
    let result = paths.and_then(|paths| runtime.block_on(foundation::smoke(&paths)));
    match result {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("Foundation smoke failed: {error}");
            std::process::exit(1);
        }
    }
}

fn main() {
    #[cfg(feature = "smoke-harness")]
    if std::env::args().any(|arg| arg == "--foundation-smoke") {
        run_foundation_smoke();
        return;
    }

    let repo_writes = repo_lock::RepoWriteCoordinator::default();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(repo_writes.clone())
        .manage(RepositoryStore::with_write_coordinator(repo_writes.clone()))
        .manage(EditorLauncher::default())
        .manage(WorkbenchPersistence::default())
        .manage(ModelStore::default())
        .manage(RepositoryPicker::default())
        .manage(PiSupervisor::default())
        .manage(CustomActionCoordinator::with_write_coordinator(repo_writes))
        .manage(SkillManager::default())
        .manage(UpdateManager::default())
        .manage(startup_splash::StartupSplashCoordinator::default())
        .manage(tray::TrayCoordinator::default());

    #[cfg(feature = "smoke-harness")]
    let builder = builder
        .manage(ModelSmokeState::default())
        .manage(repository::smoke::SmokeState::default())
        .manage(DesktopLifecycle::with_flags(
            std::env::args().any(|arg| arg == "--foundation-ui-smoke"),
            std::env::args().any(|arg| arg == "--foundation-restart-smoke"),
            std::env::args().any(|arg| arg == "--keep-smoke-repo-once"),
        ));

    #[cfg(not(feature = "smoke-harness"))]
    let builder = builder.manage(DesktopLifecycle::default());

    let builder = builder.setup(|app| {
        let ui_smoke = app.state::<DesktopLifecycle>().is_ui_smoke();
        let data_dir = picket_data_dir().ok();
        app.state::<startup_splash::StartupSplashCoordinator>()
            .setup_startup(app.handle(), ui_smoke, data_dir.as_deref());
        app.state::<tray::TrayCoordinator>()
            .setup_tray(app.handle(), ui_smoke);
        Ok(())
    });

    macro_rules! picket_invoke_handler {
        ($($extra:path),* $(,)?) => {
            tauri::generate_handler![
                startup_splash::splash_ready,
                startup_splash::notify_workbench_ready,
                startup_splash::run_splash_jump,
                startup_splash::complete_splash,
                window_motion::set_window_pinned,
                window_motion::set_window_theme,
                window_motion::animate_window_frame,
                tray::read_tray_state,
                tray::sync_tray_context,
                tray::request_tray_refresh,
                tray::resize_tray_popover,
                tray::hide_tray_popover,
                tray::tray_open_main_window,
                tray::tray_quit_app,
                commands::updater::get_update_status,
                commands::updater::check_for_update,
                commands::updater::install_update,
                commands::updater::cancel_update,
                commands::updater::open_release_page,
                commands::repository::open_repository,
                commands::repository::refresh_repository,
                commands::repository::set_repository_local_keep,
                commands::repository::change_repository_branch,
                commands::repository::open_in_editor,
                commands::repository::load_workbench_state,
                commands::repository::save_workbench_state,
                commands::models::test_model_connection,
                commands::models::list_provider_models,
                commands::models::create_provider_connection,
                commands::models::update_provider_connection,
                commands::models::delete_provider_connection,
                commands::models::save_model,
                commands::models::select_model,
                commands::models::delete_model,
                commands::models::reveal_provider_api_key,
                commands::models::cancel_model_request,
                commands::cpush::start_cpush,
                commands::cpush::respond_cpush,
                commands::cpush::pull_rebase_repository,
                commands::cpush::reconcile_cpush_session,
                commands::actions::start_custom_action,
                commands::actions::respond_custom_action,
                commands::actions::generate_custom_action,
                commands::actions::cancel_custom_action,
                commands::actions::dismiss_session_done,
                commands::skills::get_skills_state,
                commands::skills::sync_skills,
                commands::skills::set_enabled_skills,
                commands::skills::reset_skills_to_bundled,
                $($extra),*
            ]
        };
    }

    #[cfg(feature = "smoke-harness")]
    let builder = builder
        .invoke_handler(picket_invoke_handler![
            commands::smoke::run_pi_probe,
            commands::smoke::cancel_pi_probe,
            commands::smoke::run_git_probe,
            commands::smoke::close_smoke_window,
            commands::smoke::read_smoke_window_spaces_state,
            commands::smoke::fail_ui_smoke,
            tray::verify_tray_smoke,
            commands::smoke::start_model_smoke_server,
            commands::smoke::set_smoke_local_auth,
            commands::smoke::verify_model_m3_smoke,
            commands::smoke::prepare_cpush_m4_smoke,
            commands::smoke::verify_cpush_m4_smoke,
            commands::smoke::prepare_m5_conflict_smoke,
            commands::smoke::verify_m5_conflict_smoke,
            commands::smoke::verify_restart_smoke,
            commands::smoke::verify_repository_read_smoke,
            commands::smoke::verify_repository_branch_smoke,
            commands::smoke::verify_repository_commit_smoke,
            commands::smoke::verify_repository_ide_smoke,
            commands::smoke::verify_repository_persistence_smoke,
        ])
        .on_page_load(|window, payload| {
            if window.label() == "main"
                && payload.event() == tauri::webview::PageLoadEvent::Finished
                && window.state::<DesktopLifecycle>().start_ui_smoke()
            {
                let event_name = if window.state::<DesktopLifecycle>().restart_smoke {
                    "picket:foundation-restart-smoke"
                } else {
                    "picket:foundation-smoke"
                };
                if let Err(error) = window.eval(format!(
                    "window.__PICKET_PENDING_SMOKE_EVENT__ = '{event_name}'; window.dispatchEvent(new Event('{event_name}'));"
                )) {
                    eprintln!("Cannot start UI smoke: {error}");
                    window.state::<DesktopLifecycle>().fail();
                    exit_after_cleanup(window.app_handle());
                }
            }
        });

    #[cfg(not(feature = "smoke-harness"))]
    let builder = builder.invoke_handler(picket_invoke_handler![]);

    let app = builder
        .on_window_event(|window, event| match (window.label(), event) {
            ("main", tauri::WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                if window.state::<DesktopLifecycle>().is_ui_smoke() {
                    exit_after_cleanup(window.app_handle());
                } else {
                    window
                        .state::<tray::TrayCoordinator>()
                        .hide_main_to_tray_on_main(window.app_handle());
                }
            }
            (tray::TRAY_POPOVER_LABEL, tauri::WindowEvent::Focused(false)) => {
                window
                    .state::<tray::TrayCoordinator>()
                    .handle_popover_blur_on_main(window.app_handle());
            }
            (tray::TRAY_POPOVER_LABEL, tauri::WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                window
                    .state::<tray::TrayCoordinator>()
                    .hide_popover_on_main(window.app_handle());
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize Picket");

    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } => {
            api.prevent_exit();
            exit_after_cleanup(app);
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. }
            if app
                .state::<startup_splash::StartupSplashCoordinator>()
                .may_reopen_main() =>
        {
            app.state::<tray::TrayCoordinator>()
                .restore_main_from_tray_on_main(app, None);
        }
        _ => {}
    });
}
