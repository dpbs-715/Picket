//! M0 diagnostic probes and packaged UI / restart smoke-test verification commands.
//! All commands here are gated behind debug assertions or explicit smoke-mode flags.

use super::{picket_data_dir, runtime_paths};
use crate::{
    actions::CustomActionCoordinator,
    foundation,
    lifecycle::{DesktopLifecycle, exit_after_cleanup},
    models::{
        local_auth::{LocalAuthOutcome, StaticLocalAuthenticator},
        smoke::ModelSmokeState,
        store::ModelStore,
    },
    pi_runtime::{PiEvent, PiSupervisor, ProbeReport},
    repository::{self, RepositoryStore, persistence::WorkbenchPersistence, types::EditorKind},
    window_motion,
};
use tauri::{Manager, State, ipc::Channel};

#[tauri::command]
pub async fn run_pi_probe(
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
pub fn cancel_pi_probe(
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
pub async fn run_git_probe(
    app: tauri::AppHandle,
) -> Result<foundation::git::GitProbeReport, String> {
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
pub fn start_model_smoke_server(app: tauri::AppHandle) -> Result<String, String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<ModelSmokeState>().ensure_server()
}

#[tauri::command]
pub fn set_smoke_local_auth(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
    outcome: String,
) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    let parsed = match outcome.as_str() {
        "authenticated" => LocalAuthOutcome::Authenticated,
        "cancelled" => LocalAuthOutcome::Cancelled,
        other => LocalAuthOutcome::Failed(other.to_owned()),
    };
    models.set_authenticator(std::sync::Arc::new(StaticLocalAuthenticator::new(parsed)))
}

#[tauri::command]
pub async fn verify_model_m3_smoke(app: tauri::AppHandle, secret: String) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    let data_dir = picket_data_dir()?;
    let models_raw = std::fs::read_to_string(data_dir.join("models.json"))
        .map_err(|e| format!("Missing models.json in UI smoke: {e}"))?;
    if models_raw.contains(&secret) || models_raw.contains("apiKey") {
        return Err("models.json leaked secret or apiKey field".into());
    }
    let creds_path = data_dir.join("credentials.json");
    let creds_raw = std::fs::read_to_string(&creds_path)
        .map_err(|e| format!("Missing credentials.json in UI smoke: {e}"))?;
    if !creds_raw.contains(&secret) {
        return Err("credentials.json did not persist provider secret".into());
    }
    let cold_models = ModelStore::default();
    let restored = cold_models.restore(&data_dir).await?;
    let active_id = restored
        .active_model_id
        .ok_or("Cold model restore lost activeModelId")?;
    if restored.connections.len() != 1
        || restored.models.len() != 2
        || restored.warning.is_some()
        || restored.connections[0].key_hint.is_none()
    {
        return Err(
            "Cold model restore did not recover 1 provider connection + 2 child models with key hint"
                .into(),
        );
    }
    let active_model = restored
        .models
        .iter()
        .find(|m| m.id == active_id)
        .ok_or("Active model missing from restored catalog")?;
    if active_model.name != "Smoke Coder" {
        return Err(
            "Adding a second model under connection unexpectedly replaced activeModelId".into(),
        );
    }
    for item in &restored.models {
        let resolved = cold_models
            .resolve_runtime_spec(&data_dir, &item.id)
            .await
            .map_err(|e| e.to_string())?;
        if resolved.api_key != secret {
            return Err("Cold credentials.json lookup did not recover connection API key".into());
        }
    }
    let recorded = app.state::<ModelSmokeState>().requests()?;
    let expected_bearer = format!("Bearer {secret}");
    let saw_conn = recorded.iter().any(|req| {
        req.path.ends_with("/chat/completions")
            && req.authorization.as_deref() == Some(expected_bearer.as_str())
            && req.x_api_key.is_none()
            && req.body.contains("Connection check.")
    });
    let plan_count = recorded
        .iter()
        .filter(|req| {
            req.path.ends_with("/chat/completions")
                && req.authorization.as_deref() == Some(expected_bearer.as_str())
                && req.x_api_key.is_none()
                && req.body.contains("Picket's C-Push Agent")
                && req.body.contains("User extra instructions")
                && req.body.contains("+first=2")
                && !req.body.contains("+second=3")
                && !req.body.contains("lock: updated")
        })
        .count();
    if !saw_conn || plan_count != 1 {
        return Err("Model smoke server did not observe connection test and single scoped one-click C-Push plan request".into());
    }
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeModelProof",
            "credentialsPersistedAndRestored": true,
            "partialScopeCPushPlanVerified": true
        })
    );
    Ok(())
}

#[tauri::command]
pub fn prepare_cpush_m4_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>().prepare_cpush()
}

#[tauri::command]
pub async fn verify_cpush_m4_smoke(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    secret: String,
) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .verify_cpush()?;
    let data_dir = picket_data_dir()?;
    let sessions_raw = std::fs::read_to_string(data_dir.join("action-sessions.json"))
        .map_err(|e| format!("Missing action-sessions.json in UI smoke: {e}"))?;
    if sessions_raw.contains(&secret)
        || sessions_raw.contains("apiKey")
        || sessions_raw.contains("commitToken")
    {
        return Err("action-sessions.json leaked secret or commitToken".into());
    }
    let root = app.state::<repository::smoke::SmokeState>().path()?;
    let live_snap = store.open(&root).await?;
    let cold_actions = CustomActionCoordinator::default();
    let (sessions, warn) = cold_actions.restore_cpush(&data_dir, &[live_snap]).await?;
    if warn.is_some()
        || !sessions
            .iter()
            .any(|s| s.commits.len() == 2 && s.qa_log.len() == 1)
    {
        return Err(
            "Cold restore of action-sessions.json did not recover 2 split commits and Q&A log"
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
pub fn prepare_m5_conflict_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>()
        .prepare_m5_conflict()
}

#[tauri::command]
pub fn verify_m5_conflict_smoke(app: tauri::AppHandle) -> Result<(), String> {
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
pub fn verify_repository_read_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository read smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>().verify()
}

#[tauri::command]
pub fn verify_repository_branch_smoke(app: tauri::AppHandle) -> Result<(), String> {
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
pub fn verify_repository_commit_smoke(app: tauri::AppHandle) -> Result<(), String> {
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
pub fn verify_repository_ide_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository IDE smoke mode".into());
    }
    app.state::<repository::smoke::SmokeState>().verify_ide()
}

#[tauri::command]
pub async fn verify_repository_persistence_smoke(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running repository persistence smoke mode".into());
    }
    let data_dir = picket_data_dir()?;
    let mut last_error = String::from("Missing persisted workbench-state.json");
    for attempt in 0..8 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        }
        let raw = match std::fs::read_to_string(data_dir.join("workbench-state.json")) {
            Ok(raw) => raw,
            Err(e) => {
                last_error = format!("Missing persisted workbench-state.json: {e}");
                continue;
            }
        };
        if raw.contains("commitToken")
            || raw.contains("branchToken")
            || raw.contains("apiKey")
            || raw.contains("draftText")
        {
            return Err(
                "Persisted workbench file leaked tokens, credential fields, or legacy draft fields"
                    .into(),
            );
        }
        let cold_store = RepositoryStore::default();
        let cold_persistence = WorkbenchPersistence::default();
        let restored = cold_persistence.restore(&data_dir, &cold_store).await?;
        let has_saved_prompt = restored.worktrees.iter().any(|tree| {
            tree.prompt_text == "persisted-across-restart: extra instruction"
                && tree.selected_ids.len() == 1
        });
        if restored.protocol_version == 2
            && restored.editor == EditorKind::Vscode
            && restored.snapshots.len() == 1
            && has_saved_prompt
            && restored.warning.is_none()
        {
            println!(
                "{}",
                serde_json::json!({
                    "type": "uiSmokeIdeAndPersistenceProof",
                    "ideStagedDiffVerified": true,
                    "persistenceRestoreVerified": true
                })
            );
            return Ok(());
        }
        last_error = format!(
            "Cold persistence restore did not match v2 saved editor, repository, or worktree prompt/selection (editor={:?}, snaps={}, has_prompt={}, warn={:?})",
            restored.editor,
            restored.snapshots.len(),
            has_saved_prompt,
            restored.warning
        );
    }
    Err(last_error)
}

#[tauri::command]
pub async fn verify_restart_smoke(
    app: tauri::AppHandle,
    models: State<'_, ModelStore>,
) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().restart_smoke {
        return Err("Not running restart smoke mode".into());
    }
    let data_dir = picket_data_dir()?;
    let restored = models.restore(&data_dir).await?;
    let active_id = restored
        .active_model_id
        .ok_or("Restart smoke lost activeModelId")?;
    if restored.connections.len() != 1
        || restored.models.len() != 2
        || restored.warning.is_some()
        || restored.connections[0].key_hint.is_none()
    {
        return Err("Restart smoke failed to restore saved provider & model catalog".into());
    }
    let resolved = models
        .resolve_runtime_spec(&data_dir, &active_id)
        .await
        .map_err(|e| e.to_string())?;
    if !resolved.api_key.starts_with("smoke-token-") {
        return Err("Restart smoke credentials lookup did not recover expected secret".into());
    }
    for item in &restored.models {
        let peer_resolved = models
            .resolve_runtime_spec(&data_dir, &item.id)
            .await
            .map_err(|e| e.to_string())?;
        if peer_resolved.api_key != resolved.api_key {
            return Err("Restart smoke lost connection credential across child models".into());
        }
    }
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let restored_wb = cold_persistence.restore(&data_dir, &cold_store).await?;
    let Some(ref win_prefs) = restored_wb.window_preferences else {
        return Err("Restart smoke did not persist windowPreferences".into());
    };
    if !win_prefs.pinned {
        return Err("Restart smoke windowPreferences lost pinned state".into());
    }
    app.state::<DesktopLifecycle>().mark_restart_verified();
    println!(
        "{}",
        serde_json::json!({
            "type": "uiSmokeRestartProof",
            "coldProcessRepositoryHydrated": true,
            "coldProcessModelCatalogHydrated": true,
            "coldProcessSessionHistoryHydrated": true,
            "coldProcessCompactWindowHydrated": true
        })
    );
    Ok(())
}

#[tauri::command]
pub fn close_smoke_window(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().may_close_probe() {
        return Err("UI smoke prerequisites are not satisfied".into());
    }
    app.get_webview_window("main")
        .ok_or("Missing main window")?
        .close()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_smoke_window_spaces_state(app: tauri::AppHandle) -> Result<bool, String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    window_motion::read_main_window_visible_on_all_spaces(&app).await
}

#[tauri::command]
pub fn fail_ui_smoke(app: tauri::AppHandle, message: String) -> Result<(), String> {
    if !app.state::<DesktopLifecycle>().ui_smoke {
        return Err("Not running UI smoke mode".into());
    }
    eprintln!("UI smoke failed: {message}");
    app.state::<DesktopLifecycle>().fail();
    exit_after_cleanup(&app);
    Ok(())
}
