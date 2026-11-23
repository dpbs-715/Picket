#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod cpush;
mod foundation;
mod models;
mod repository;

use cpush::{
    CPushCoordinator,
    types::{
        CPushCommandReply, CPushProgressEvent, CommitAndPushRequest, PullRebaseRequest,
        ReconcileCPushRequest, RespondCPushRequest, StartCPushRequest,
    },
};
use foundation::lifecycle::{DesktopLifecycle, exit_after_cleanup};
use foundation::pi::{PiEvent, PiPaths, PiSupervisor, ProbeReport};
use models::{
    smoke::ModelSmokeState,
    store::ModelStore,
    types::{
        GenerateCommitReply, GenerateCommitRequest, ModelIdRequest, ModelInput, ModelMutateReply,
        ModelSaveReply, ModelStreamEvent, ModelTestReply, ReviewScopeReply, ReviewScopeRequest,
    },
};
use repository::{
    RepositoryStore,
    editor::EditorLauncher,
    persistence::WorkbenchPersistence,
    types::{
        BranchReply, BranchRequest, CommitReply, CommitRequest, EditorKind, OpenFileReply,
        OpenFileRequest, OpenRequest, ReadReply, RefreshRequest, WorkbenchRestoreReply,
        WorkbenchSaveRequest,
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
    if !cfg!(debug_assertions) && !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Diagnostic probes are disabled in production launches".into());
    }
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
fn cancel_pi_probe(
    app: tauri::AppHandle,
    supervisor: State<'_, PiSupervisor>,
    request_id: String,
) -> Result<(), String> {
    if !cfg!(debug_assertions) && !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Diagnostic probes are disabled in production launches".into());
    }
    supervisor.cancel(&request_id)
}

#[tauri::command]
async fn run_git_probe(app: tauri::AppHandle) -> Result<foundation::git::GitProbeReport, String> {
    if !cfg!(debug_assertions) && !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Diagnostic probes are disabled in production launches".into());
    }
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
        let (root, editor_bin) = app
            .state::<repository::smoke::SmokeState>()
            .path_and_editor_bin()?;
        app.state::<EditorLauncher>()
            .set_search_dirs(Some(vec![editor_bin]))?;
        Some(root)
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
    store: State<'_, RepositoryStore>,
    request: BranchRequest,
) -> Result<BranchReply, String> {
    repository::validate_request_id(&request.request_id)?;
    let snapshot = store.change_branch(&request).await?;
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
async fn open_in_editor(
    store: State<'_, RepositoryStore>,
    launcher: State<'_, EditorLauncher>,
    request: OpenFileRequest,
) -> Result<OpenFileReply, String> {
    repository::validate_request_id(&request.request_id)?;
    let outcome = store.open_in_editor(&launcher, &request).await?;
    Ok(OpenFileReply {
        protocol_version: 1,
        kind: "openFile",
        request_id: request.request_id,
        target: request.target,
        editor: request.editor,
        file_path: outcome.file_path,
        action: outcome.action,
        line: outcome.line,
    })
}

#[tauri::command]
async fn load_workbench_state(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    launcher: State<'_, EditorLauncher>,
    persistence: State<'_, WorkbenchPersistence>,
    models: State<'_, ModelStore>,
    cpush: State<'_, CPushCoordinator>,
) -> Result<WorkbenchRestoreReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let lifecycle = app.state::<DesktopLifecycle>();
    if lifecycle.ui_smoke {
        models.use_isolated_keychain(
            data_dir.join("picket-ui-smoke.keychain-db"),
            "picket-ui-smoke-pw",
        )?;
        if !lifecycle.restart_smoke {
            let (_, editor_bin) = app
                .state::<repository::smoke::SmokeState>()
                .path_and_editor_bin()?;
            launcher.set_search_dirs(Some(vec![editor_bin]))?;
        }
    }
    let mut restored = persistence.restore(&data_dir, &store).await?;
    if lifecycle.restart_smoke
        && let Some(first_snap) = restored.snapshots.first()
        && let Some(first_wt) = first_snap.worktrees.first()
        && let Some(parent) = std::path::Path::new(&first_wt.path).parent()
    {
        launcher.set_search_dirs(Some(vec![parent.join("editor-bin")]))?;
    }
    let installed = launcher.detect_installed_editors();
    if !installed.is_empty() && !installed.contains(&restored.editor) {
        restored.editor = installed[0];
    }
    restored.installed_editors = Some(installed);

    let (saved_models, active_model_id, model_warning) = models.restore(&data_dir).await?;
    restored.models = Some(saved_models);
    restored.active_model_id = Some(active_model_id);
    if let Some(m_warn) = model_warning {
        restored.warning = Some(match restored.warning {
            Some(existing) => format!("{existing} {m_warn}"),
            None => m_warn,
        });
    }
    let (cpush_sessions, cpush_warning) = cpush.restore(&data_dir, &restored.snapshots).await?;
    restored.c_push_sessions = Some(cpush_sessions);
    if let Some(c_warn) = cpush_warning {
        restored.warning = Some(match restored.warning {
            Some(existing) => format!("{existing} {c_warn}"),
            None => c_warn,
        });
    }
    Ok(restored)
}

#[tauri::command]
async fn test_model_connection(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    input: ModelInput,
) -> Result<ModelTestReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if app.state::<DesktopLifecycle>().ui_smoke {
        models.use_isolated_keychain(
            data_dir.join("picket-ui-smoke.keychain-db"),
            "picket-ui-smoke-pw",
        )?;
    }
    let paths = runtime_paths(&app)?;
    models
        .test_connection_with_storage(Some(&data_dir), &paths, &supervisor, &input)
        .await
}

#[tauri::command]
async fn save_model(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
    input: ModelInput,
) -> Result<ModelSaveReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if app.state::<DesktopLifecycle>().ui_smoke {
        models.use_isolated_keychain(
            data_dir.join("picket-ui-smoke.keychain-db"),
            "picket-ui-smoke-pw",
        )?;
    }
    models.save_model(&data_dir, &input).await
}

#[tauri::command]
async fn select_model(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
    request: ModelIdRequest,
) -> Result<ModelMutateReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    models.select_model(&data_dir, &request.model_id).await
}

#[tauri::command]
async fn delete_model(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
    request: ModelIdRequest,
) -> Result<ModelMutateReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if app.state::<DesktopLifecycle>().ui_smoke {
        models.use_isolated_keychain(
            data_dir.join("picket-ui-smoke.keychain-db"),
            "picket-ui-smoke-pw",
        )?;
    }
    models.delete_model(&data_dir, &request.model_id).await
}

#[tauri::command]
async fn generate_commit_message(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    request: GenerateCommitRequest,
    on_event: Channel<ModelStreamEvent>,
) -> Result<GenerateCommitReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let paths = runtime_paths(&app)?;
    models
        .generate_commit(&data_dir, &paths, &supervisor, &store, &request, |event| {
            let _ = on_event.send(event);
            Ok(())
        })
        .await
}

#[tauri::command]
async fn review_selected_scope(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    request: ReviewScopeRequest,
    on_event: Channel<ModelStreamEvent>,
) -> Result<ReviewScopeReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let paths = runtime_paths(&app)?;
    models
        .review_scope(&data_dir, &paths, &supervisor, &store, &request, |event| {
            let _ = on_event.send(event);
            Ok(())
        })
        .await
}

#[tauri::command]
fn cancel_model_request(
    supervisor: State<'_, PiSupervisor>,
    request_id: String,
) -> Result<(), String> {
    supervisor.cancel(&request_id)
}

#[tauri::command]
async fn start_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    cpush: State<'_, CPushCoordinator>,
    request: StartCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let paths = runtime_paths(&app)?;
    cpush
        .start_cpush(
            &data_dir,
            &paths,
            &supervisor,
            &models,
            &store,
            &request,
            |event| {
                let _ = on_event.send(event);
                Ok(())
            },
        )
        .await
}

#[tauri::command]
async fn respond_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    cpush: State<'_, CPushCoordinator>,
    request: RespondCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let paths = runtime_paths(&app)?;
    cpush
        .respond_cpush(
            &data_dir,
            &paths,
            &supervisor,
            &models,
            &store,
            &request,
            |event| {
                let _ = on_event.send(event);
                Ok(())
            },
        )
        .await
}

#[tauri::command]
async fn commit_and_push_repository(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    cpush: State<'_, CPushCoordinator>,
    request: CommitAndPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    cpush
        .commit_and_push(&data_dir, &store, &request, |event| {
            let _ = on_event.send(event);
            Ok(())
        })
        .await
}

#[tauri::command]
async fn pull_rebase_repository(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    cpush: State<'_, CPushCoordinator>,
    request: PullRebaseRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    cpush
        .pull_rebase(&data_dir, &store, &request, |event| {
            let _ = on_event.send(event);
            Ok(())
        })
        .await
}

#[tauri::command]
async fn reconcile_cpush_session(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    cpush: State<'_, CPushCoordinator>,
    request: ReconcileCPushRequest,
) -> Result<Option<CPushProgressEvent>, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    cpush.reconcile_worktree(&data_dir, &store, &request).await
}

#[tauri::command]
fn start_model_smoke_server(app: tauri::AppHandle) -> Result<String, String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<ModelSmokeState>().ensure_server()
}

#[tauri::command]
async fn verify_model_m3_smoke(app: tauri::AppHandle, secret: String) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let models_raw = std::fs::read_to_string(data_dir.join("models.json"))
        .map_err(|e| format!("Missing models.json in UI smoke: {e}"))?;
    if models_raw.contains(&secret) || models_raw.contains("apiKey") {
        return Err("models.json leaked secret or apiKey field".into());
    }
    let kc_bytes = std::fs::read(data_dir.join("picket-ui-smoke.keychain-db"))
        .map_err(|e| format!("Missing isolated macOS keychain file: {e}"))?;
    if kc_bytes
        .windows(secret.len())
        .any(|window| window == secret.as_bytes())
    {
        return Err("Keychain database contained unencrypted secret bytes".into());
    }
    let cold_vault = models::keychain::CredentialVault::with_keychain_file(
        data_dir.join("picket-ui-smoke.keychain-db"),
        "picket-ui-smoke-pw",
    );
    let cold_models = ModelStore::with_vault(cold_vault);
    let (restored_models, active_id, warn) = cold_models.restore(&data_dir).await?;
    let active_id = active_id.ok_or("Cold model restore lost activeModelId")?;
    if restored_models.len() != 1 || warn.is_some() {
        return Err("Cold model restore did not recover expected model catalog".into());
    }
    let resolved = cold_models
        .resolve_runtime_spec(&data_dir, &active_id)
        .await?;
    if resolved.api_key != secret {
        return Err("Cold macOS Keychain lookup did not recover saved API key".into());
    }
    let recorded = app.state::<ModelSmokeState>().requests()?;
    let expected_bearer = format!("Bearer {secret}");
    let saw_conn = recorded.iter().any(|req| {
        req.path.ends_with("/chat/completions")
            && req.authorization.as_deref() == Some(expected_bearer.as_str())
            && req.x_api_key.is_none()
            && req.body.contains("Connection check.")
    });
    let saw_generate = recorded.iter().any(|req| {
        req.path.ends_with("/chat/completions")
            && req.authorization.as_deref() == Some(expected_bearer.as_str())
            && req.x_api_key.is_none()
            && req.body.contains("commit message generator")
            && req.body.contains("+second=3")
            && !req.body.contains("lock: updated")
    });
    let saw_review = recorded.iter().any(|req| {
        req.path.ends_with("/chat/completions")
            && req.authorization.as_deref() == Some(expected_bearer.as_str())
            && req.body.contains("pre-commit code reviewer")
            && req.body.contains("+second=3")
            && !req.body.contains("pnpm-lock.yaml")
    });
    if !saw_conn || !saw_generate || !saw_review {
        return Err("Model smoke server did not observe connection test, scoped generate, and lockfile-filtered review requests".into());
    }
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeModelProof",
            "keychainEncryptedAndRestored": true,
            "partialScopeGenerateVerified": true,
            "lockfileFilteredReviewVerified": true
        })
    );
    Ok(())
}

#[tauri::command]
fn prepare_cpush_m4_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>().prepare_cpush()
}

#[tauri::command]
async fn verify_cpush_m4_smoke(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    secret: String,
) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .verify_cpush()?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let cpush_raw = std::fs::read_to_string(data_dir.join("cpush-sessions.json"))
        .map_err(|e| format!("Missing cpush-sessions.json in UI smoke: {e}"))?;
    if cpush_raw.contains(&secret)
        || cpush_raw.contains("apiKey")
        || cpush_raw.contains("commitToken")
    {
        return Err("cpush-sessions.json leaked secret or commitToken".into());
    }
    let root = app.state::<repository::smoke::SmokeState>().path()?;
    let live_snap = store.open(&root).await?;
    let cold_cpush = CPushCoordinator::default();
    let (sessions, warn) = cold_cpush.restore(&data_dir, &[live_snap]).await?;
    if warn.is_some()
        || !sessions
            .iter()
            .any(|s| s.commits.len() == 2 && s.qa_log.len() == 1)
    {
        return Err(
            "Cold restore of cpush-sessions.json did not recover 2 split commits and Q&A log"
                .into(),
        );
    }
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeCPushProof",
            "splitCommitsVerified": true,
            "unselectedStagedPreserved": true,
            "sessionPersistedAndRestored": true
        })
    );
    Ok(())
}

#[tauri::command]
fn prepare_m5_conflict_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .prepare_m5_conflict()
}

#[tauri::command]
fn verify_m5_conflict_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .verify_m5_conflict()?;
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeM5SyncProof",
            "remotePushVerified": true,
            "conflictAgentMergeVerified": true,
            "disjointDirtyPreservedAcrossRebase": true
        })
    );
    Ok(())
}

#[tauri::command]
async fn save_workbench_state(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    persistence: State<'_, WorkbenchPersistence>,
    request: WorkbenchSaveRequest,
) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    persistence.save(&data_dir, &store, request).await
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
        serde_json::json!({
            "type": "uiSmokeBranchProof",
            "branchSwitchVerified": true,
            "worktreeSwitchVerified": true,
            "stagingAndDraftsPreserved": true
        })
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
fn verify_repository_ide_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository IDE smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>().verify_ide()
}

#[tauri::command]
async fn verify_repository_persistence_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository persistence smoke mode".into());
    }
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let raw = std::fs::read_to_string(data_dir.join("workbench-state.json"))
        .map_err(|e| format!("Missing persisted workbench-state.json: {e}"))?;
    if raw.contains("commitToken") || raw.contains("branchToken") || raw.contains("apiKey") {
        return Err("Persisted workbench file leaked tokens or credential fields".into());
    }
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let restored = cold_persistence.restore(&data_dir, &cold_store).await?;
    let has_saved_draft = restored.worktrees.iter().any(|tree| {
        tree.draft_text == "persisted-across-restart: keep draft" && tree.selected_ids.len() == 1
    });
    if restored.editor != EditorKind::Vscode
        || restored.snapshots.len() != 1
        || !has_saved_draft
        || restored.warning.is_some()
    {
        return Err("Cold persistence restore did not match saved editor, repository, or worktree draft/selection".into());
    }
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeIdeAndPersistenceProof",
            "ideStagedDiffVerified": true,
            "persistenceRestoreVerified": true
        })
    );
    Ok(())
}

#[tauri::command]
async fn verify_restart_smoke(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().restart_smoke {
        return Err("Not running restart smoke mode".into());
    }
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let (restored_models, active_id, warn) = models.restore(&data_dir).await?;
    let active_id = active_id.ok_or("Restart smoke lost activeModelId")?;
    if restored_models.len() != 1 || warn.is_some() {
        return Err("Restart smoke failed to restore saved model catalog".into());
    }
    let resolved = models.resolve_runtime_spec(&data_dir, &active_id).await?;
    if !resolved.api_key.starts_with("smoke-token-") {
        return Err("Restart smoke Keychain lookup did not recover expected secret".into());
    }
    app.state::<DesktopLifecycle>().mark_restart_verified();
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeRestartProof",
            "coldProcessRepositoryHydrated": true,
            "coldProcessKeychainModelHydrated": true,
            "coldProcessSessionHistoryHydrated": true
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
        .manage(EditorLauncher::default())
        .manage(WorkbenchPersistence::default())
        .manage(ModelStore::default())
        .manage(ModelSmokeState::default())
        .manage(RepositoryPicker::default())
        .manage(repository::smoke::SmokeState::default())
        .manage(PiSupervisor::default())
        .manage(CPushCoordinator::default())
        .manage(DesktopLifecycle::with_flags(
            std::env::args().any(|arg| arg == "--foundation-ui-smoke"),
            std::env::args().any(|arg| arg == "--foundation-restart-smoke"),
            std::env::args().any(|arg| arg == "--keep-smoke-repo-once"),
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
            open_in_editor,
            load_workbench_state,
            save_workbench_state,
            test_model_connection,
            save_model,
            select_model,
            delete_model,
            generate_commit_message,
            review_selected_scope,
            cancel_model_request,
            start_cpush,
            respond_cpush,
            commit_and_push_repository,
            pull_rebase_repository,
            reconcile_cpush_session,
            start_model_smoke_server,
            verify_model_m3_smoke,
            prepare_cpush_m4_smoke,
            verify_cpush_m4_smoke,
            prepare_m5_conflict_smoke,
            verify_m5_conflict_smoke,
            verify_restart_smoke,
            verify_repository_read_smoke,
            verify_repository_branch_smoke,
            verify_repository_commit_smoke,
            verify_repository_ide_smoke,
            verify_repository_persistence_smoke
        ])
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && window.state::<DesktopLifecycle>().start_ui_smoke()
            {
                let event_name = if window.state::<DesktopLifecycle>().restart_smoke {
                    "picket:foundation-restart-smoke"
                } else {
                    "picket:foundation-smoke"
                };
                if let Err(error) =
                    window.eval(format!("window.dispatchEvent(new Event('{event_name}'))"))
                {
                    eprintln!("Cannot start UI smoke: {error}");
                    window.state::<DesktopLifecycle>().fail();
                    exit_after_cleanup(window.app_handle());
                }
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
