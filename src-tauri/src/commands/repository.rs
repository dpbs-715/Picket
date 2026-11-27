//! Repository, branch, editor, local-keep, and workbench persistence IPC commands.

use super::{RepositoryPicker, picket_data_dir};
#[cfg(feature = "smoke-harness")]
use crate::lifecycle::DesktopLifecycle;
use crate::{
    actions::CustomActionCoordinator,
    i18n,
    models::store::ModelStore,
    repository::{
        self, RepositoryStore,
        editor::EditorLauncher,
        persistence::WorkbenchPersistence,
        types::{
            BranchReply, BranchRequest, LocalKeepReply, LocalKeepRequest, OpenFileReply,
            OpenFileRequest, OpenRequest, ReadReply, RefreshRequest, WorkbenchRestoreReply,
            WorkbenchSaveRequest,
        },
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn open_repository(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    picker: State<'_, RepositoryPicker>,
    request: OpenRequest,
) -> Result<ReadReply, String> {
    repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
    let _selection = picker
        .0
        .try_lock()
        .map_err(|_| i18n::desktop::picker_busy(request.locale))?;
    #[cfg(feature = "smoke-harness")]
    let path = if app.state::<DesktopLifecycle>().ui_smoke {
        let (root, editor_bin) = app
            .state::<repository::smoke::SmokeState>()
            .path_and_editor_bin()?;
        app.state::<EditorLauncher>()
            .set_search_dirs(Some(vec![editor_bin]))?;
        Some(root)
    } else {
        pick_repository_folder(&app, request.locale).await?
    };
    #[cfg(not(feature = "smoke-harness"))]
    let path = pick_repository_folder(&app, request.locale).await?;
    let snapshot = match path {
        Some(path) => Some(store.open_with_locale(&path, request.locale).await?),
        None => None,
    };
    if snapshot.is_some() {
        app.state::<crate::tray::TrayCoordinator>()
            .refresh_from_app(&app);
    }
    Ok(ReadReply {
        protocol_version: 1,
        request_id: request.request_id,
        target: None,
        snapshot,
    })
}

async fn pick_repository_folder(
    app: &tauri::AppHandle,
    locale: i18n::LanguageKind,
) -> Result<Option<std::path::PathBuf>, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| i18n::desktop::main_window_unavailable(locale))?;
    app.dialog()
        .file()
        .set_parent(&window)
        .set_title(i18n::desktop::select_git_working_directory_title(locale))
        .pick_folder(move |path| {
            let _ = send.send(path);
        });
    let path = receive
        .await
        .map_err(|_| i18n::desktop::picker_incomplete(locale))?
        .map(|path| {
            path.into_path()
                .map_err(|_| i18n::desktop::picker_must_be_local_dir(locale).to_string())
        })
        .transpose()?;
    Ok(path)
}

#[tauri::command]
pub async fn refresh_repository(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    request: RefreshRequest,
) -> Result<ReadReply, String> {
    repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
    let snapshot = store
        .refresh_with_locale(&request.target, request.locale)
        .await?;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    Ok(ReadReply {
        protocol_version: 1,
        request_id: request.request_id,
        target: Some(request.target),
        snapshot: Some(snapshot),
    })
}

#[tauri::command]
pub async fn set_repository_local_keep(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    persistence: State<'_, WorkbenchPersistence>,
    request: LocalKeepRequest,
) -> Result<LocalKeepReply, String> {
    repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
    let snapshot = store
        .set_local_keep(
            &request.target,
            &request.paths,
            request.keep,
            request.locale,
        )
        .await?;
    let data_dir = picket_data_dir()?;
    let removed_selection_paths = if request.keep {
        request.paths.as_slice()
    } else {
        &[]
    };
    persistence
        .sync_local_keep(
            &data_dir,
            &store,
            &request.target.repository_id,
            removed_selection_paths,
            request.locale,
        )
        .await?;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
    Ok(LocalKeepReply {
        protocol_version: 1,
        kind: "localKeep",
        request_id: request.request_id,
        target: request.target,
        snapshot,
    })
}

#[tauri::command]
pub async fn change_repository_branch(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    request: BranchRequest,
) -> Result<BranchReply, String> {
    repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
    let snapshot = store.change_branch(&request).await?;
    app.state::<crate::tray::TrayCoordinator>()
        .refresh_from_app(&app);
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
pub async fn open_in_editor(
    store: State<'_, RepositoryStore>,
    launcher: State<'_, EditorLauncher>,
    request: OpenFileRequest,
) -> Result<OpenFileReply, String> {
    repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
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
pub async fn load_workbench_state(
    #[cfg_attr(not(feature = "smoke-harness"), allow(unused_variables))] app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    launcher: State<'_, EditorLauncher>,
    persistence: State<'_, WorkbenchPersistence>,
    models: State<'_, ModelStore>,
    actions: State<'_, CustomActionCoordinator>,
) -> Result<WorkbenchRestoreReply, String> {
    let data_dir = picket_data_dir()?;
    #[cfg(feature = "smoke-harness")]
    {
        let lifecycle = app.state::<DesktopLifecycle>();
        if lifecycle.ui_smoke && !lifecycle.restart_smoke {
            let (_, editor_bin) = app
                .state::<repository::smoke::SmokeState>()
                .path_and_editor_bin()?;
            launcher.set_search_dirs(Some(vec![editor_bin]))?;
        }
    }
    let mut restored = persistence.restore(&data_dir, &store).await?;
    #[cfg(feature = "smoke-harness")]
    {
        let lifecycle = app.state::<DesktopLifecycle>();
        if lifecycle.restart_smoke
            && let Some(first_snap) = restored.snapshots.first()
            && let Some(first_wt) = first_snap.worktrees.first()
            && let Some(parent) = std::path::Path::new(&first_wt.path).parent()
        {
            launcher.set_search_dirs(Some(vec![parent.join("editor-bin")]))?;
        }
    }
    let installed = launcher.detect_installed_editors();
    if !installed.is_empty() {
        if !installed.contains(&restored.editor) {
            restored.editor = installed[0];
        }
        for item in &mut restored.repository_settings {
            if !installed.contains(&item.editor) {
                item.editor = installed[0];
            }
        }
    }
    restored.installed_editors = Some(installed);

    let catalog = models
        .restore_with_locale(&data_dir, restored.locale)
        .await?;
    restored.connections = Some(catalog.connections);
    restored.models = Some(catalog.models);
    restored.active_model_id = Some(catalog.active_model_id);
    if let Some(m_warn) = catalog.warning {
        restored.warning = Some(match restored.warning {
            Some(existing) => format!("{existing} {m_warn}"),
            None => m_warn,
        });
    }
    let (action_sessions, cpush_sessions, session_warning) = actions
        .restore_all_sessions_async(&data_dir, &restored.snapshots, restored.locale)
        .await?;
    restored.c_push_sessions = Some(cpush_sessions);
    restored.action_sessions = Some(action_sessions);
    if let Some(s_warn) = session_warning {
        restored.warning = Some(match restored.warning {
            Some(existing) => format!("{existing} {s_warn}"),
            None => s_warn,
        });
    }
    app.state::<crate::tray::TrayCoordinator>().set_ui_context(
        &app,
        Some(restored.locale),
        Some(restored.theme),
        Some(restored.active_worktree_id.clone()),
    );
    Ok(restored)
}

#[tauri::command]
pub async fn save_workbench_state(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    persistence: State<'_, WorkbenchPersistence>,
    request: WorkbenchSaveRequest,
) -> Result<(), String> {
    let locale = request.locale;
    let theme = request.theme;
    let active_wt = request.active_worktree_id.clone();
    let data_dir = picket_data_dir()?;
    persistence.save(&data_dir, &store, request).await?;
    app.state::<crate::tray::TrayCoordinator>().set_ui_context(
        &app,
        Some(locale),
        Some(theme),
        Some(active_wt),
    );
    Ok(())
}
