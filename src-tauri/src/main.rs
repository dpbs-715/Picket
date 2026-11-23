#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod foundation;
mod repository;

use foundation::lifecycle::{DesktopLifecycle, exit_after_cleanup};
use foundation::pi::{PiEvent, PiPaths, PiSupervisor, ProbeReport};
use repository::{
    RepositoryStore,
    types::{
        BranchAction, BranchReply, BranchRequest, CommitReply, CommitRequest, OpenRequest,
        ReadReply, RefreshRequest,
    },
};
use tauri::{Manager, State, ipc::Channel};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct RepositoryPicker(tokio::sync::Mutex<()>);

fn runtime_paths(app: &tauri::AppHandle) -> Result<PiPaths, String> {
    if cfg!(debug_assertions) {
        return Ok(PiPaths::development());
    }
    PiPaths::bundled(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &app.path().resource_dir().map_err(|e| e.to_string())?,
    )
}

#[tauri::command]
async fn run_pi_probe(
    app: tauri::AppHandle,
    supervisor: State<'_, PiSupervisor>,
    request_id: String,
    on_event: Channel<PiEvent>,
) -> Result<ProbeReport, String> {
    let paths = runtime_paths(&app)?;
    let event_request_id = request_id.clone();
    let report = supervisor
        .inner()
        .clone()
        .run_probe(&paths, request_id, |event| {
            app.state::<DesktopLifecycle>()
                .observe(&event_request_id, event);
            on_event.send(event.clone()).map_err(|e| e.to_string())
        })
        .await?;
    app.state::<DesktopLifecycle>().pi_verified(&report);
    Ok(report)
}

#[tauri::command]
fn cancel_pi_probe(supervisor: State<'_, PiSupervisor>, request_id: String) -> Result<(), String> {
    supervisor.cancel(&request_id)
}

#[tauri::command]
async fn run_git_probe(app: tauri::AppHandle) -> Result<foundation::git::GitProbeReport, String> {
    // The frontend never supplies a filesystem path or a Git command.
    let report = tauri::async_runtime::spawn_blocking(foundation::git::run_probe)
        .await
        .map_err(|e| e.to_string())??;
    app.state::<DesktopLifecycle>().git_verified();
    Ok(report)
}

#[tauri::command]
async fn open_repository(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    picker: State<'_, RepositoryPicker>,
    request: OpenRequest,
) -> Result<ReadReply, String> {
    repository::validate_request_id(&request.request_id)?;
    let _selection = picker
        .0
        .try_lock()
        .map_err(|_| "目录选择或仓库读取正在进行，请勿重复打开")?;
    let path = if app.state::<DesktopLifecycle>().ui_smoke {
        Some(app.state::<repository::smoke::SmokeState>().path()?)
    } else {
        let (send, receive) = tokio::sync::oneshot::channel();
        let window = app.get_webview_window("main").ok_or("工作台窗口不可用")?;
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title("选择 Git 工作目录")
            .pick_folder(move |path| {
                let _ = send.send(path);
            });
        receive
            .await
            .map_err(|_| "目录选择未完成，请重新打开")?
            .map(|path| path.into_path().map_err(|_| "请选择本机目录"))
            .transpose()?
    };
    let snapshot = match path {
        Some(path) => Some(store.open(&path).await?),
        None => None,
    };
    Ok(ReadReply {
        protocol_version: 1,
        request_id: request.request_id,
        target: None,
        snapshot,
    })
}

#[tauri::command]
async fn refresh_repository(
    store: State<'_, RepositoryStore>,
    request: RefreshRequest,
) -> Result<ReadReply, String> {
    repository::validate_request_id(&request.request_id)?;
    let snapshot = store.refresh(&request.target).await?;
    Ok(ReadReply {
        protocol_version: 1,
        request_id: request.request_id,
        target: Some(request.target),
        snapshot: Some(snapshot),
    })
}

#[tauri::command]
async fn change_repository_branch(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    picker: State<'_, RepositoryPicker>,
    request: BranchRequest,
) -> Result<BranchReply, String> {
    repository::validate_request_id(&request.request_id)?;
    let parent = if request.action == BranchAction::CreateWorktree {
        let _selection = picker
            .0
            .try_lock()
            .map_err(|_| "目录选择正在进行，请勿重复打开")?;
        let parent = if app.state::<DesktopLifecycle>().ui_smoke {
            Some(
                app.state::<repository::smoke::SmokeState>()
                    .path()?
                    .parent()
                    .ok_or("临时父目录不可用")?
                    .to_owned(),
            )
        } else {
            let (send, receive) = tokio::sync::oneshot::channel();
            let window = app.get_webview_window("main").ok_or("工作台窗口不可用")?;
            app.dialog()
                .file()
                .set_parent(&window)
                .set_title("选择新 Worktree 的父目录")
                .pick_folder(move |path| {
                    let _ = send.send(path);
                });
            receive
                .await
                .map_err(|_| "目录选择未完成，请重试")?
                .map(|path| path.into_path().map_err(|_| "请选择本机目录"))
                .transpose()?
        };
        if parent.is_none() {
            return Ok(BranchReply {
                protocol_version: 1,
                kind: "branch",
                request_id: request.request_id,
                target: request.target,
                action: request.action,
                outcome: "cancelled",
                snapshot: None,
            });
        }
        parent
    } else {
        None
    };
    let snapshot = store.change_branch(&request, parent.as_deref()).await?;
    Ok(BranchReply {
        protocol_version: 1,
        kind: "branch",
        request_id: request.request_id,
        target: request.target,
        action: request.action,
        outcome: "completed",
        snapshot: Some(snapshot),
    })
}

#[tauri::command]
async fn commit_repository(
    store: State<'_, RepositoryStore>,
    request: CommitRequest,
) -> Result<CommitReply, String> {
    repository::validate_request_id(&request.request_id)?;
    let (snapshot, commit) = store.commit(&request).await?;
    Ok(CommitReply {
        protocol_version: 1,
        kind: "commit",
        request_id: request.request_id,
        target: request.target,
        snapshot,
        commit,
    })
}

#[tauri::command]
fn verify_repository_read_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository read smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>().verify()
}

#[tauri::command]
fn verify_repository_branch_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository branch smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .verify_branches()?;
    println!(
        "{}",
        serde_json::json!({ "type": "uiSmokeBranchProof", "branchCreationVerified": true,
        "worktreeCreationVerified": true, "stagingAndDraftsPreserved": true })
    );
    Ok(())
}

#[tauri::command]
fn verify_repository_commit_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository commit smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .verify_commit()?;
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeCommitProof",
            "localCommitVerified": true,
            "partialStagingCommittedOnlyStaged": true,
            "unstagedAndUnselectedPreserved": true
        })
    );
    Ok(())
}

#[tauri::command]
fn close_smoke_window(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().may_close_probe() {
        return Err("UI smoke prerequisites are not satisfied".into());
    }
    app.get_webview_window("main")
        .ok_or("Missing main window")?
        .close()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn fail_ui_smoke(app: tauri::AppHandle, message: String) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    eprintln!("UI smoke failed: {message}");
    app.state::<DesktopLifecycle>().fail();
    exit_after_cleanup(&app);
    Ok(())
}

fn main() {
    if std::env::args().any(|arg| arg == "--foundation-smoke") {
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
        return;
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(RepositoryStore::default())
        .manage(RepositoryPicker::default())
        .manage(repository::smoke::SmokeState::default())
        .manage(PiSupervisor::default())
        .manage(DesktopLifecycle::new(
            std::env::args().any(|arg| arg == "--foundation-ui-smoke"),
        ))
        .invoke_handler(tauri::generate_handler![
            run_pi_probe,
            cancel_pi_probe,
            run_git_probe,
            close_smoke_window,
            fail_ui_smoke,
            open_repository,
            refresh_repository,
            change_repository_branch,
            commit_repository,
            verify_repository_read_smoke,
            verify_repository_branch_smoke,
            verify_repository_commit_smoke
        ])
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && window.state::<DesktopLifecycle>().start_ui_smoke()
                && let Err(error) =
                    window.eval("window.dispatchEvent(new Event('picket:foundation-smoke'))")
            {
                eprintln!("Cannot start UI smoke: {error}");
                window.state::<DesktopLifecycle>().fail();
                exit_after_cleanup(window.app_handle());
            }
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                exit_after_cleanup(window.app_handle());
            }
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize Picket");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
            exit_after_cleanup(app);
        }
    });
}
