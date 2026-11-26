//! C-Push planning, execution, pull-with-rebase, and session reconciliation IPC commands.

use super::{picket_data_dir, runtime_paths};
use crate::{
    actions::CustomActionCoordinator,
    cpush::{
        CPushCoordinator,
        types::{
            CPushCommandReply, CPushProgressEvent, DismissCPushDoneRequest, PlanCPushRequest,
            PullRebaseRequest, ReconcileCPushRequest, RespondCPushRequest, StartCPushRequest,
        },
    },
    foundation::pi::PiSupervisor,
    i18n::{self, LanguageKind},
    models::store::ModelStore,
    repository::RepositoryStore,
};
use tauri::{State, ipc::Channel};

#[tauri::command]
pub async fn plan_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    cpush: State<'_, CPushCoordinator>,
    request: PlanCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    let data_dir = picket_data_dir()?;
    let paths = runtime_paths(&app)?;
    cpush
        .plan_cpush(
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
#[allow(clippy::too_many_arguments)]
pub async fn start_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    cpush: State<'_, CPushCoordinator>,
    actions: State<'_, CustomActionCoordinator>,
    request: StartCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    if actions.has_active_operation_for_repo(&request.target.repository_id) {
        return Err(i18n::actions::action_cpush_busy_error(request.locale).into());
    }
    let data_dir = picket_data_dir()?;
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
pub async fn respond_cpush(
    app: tauri::AppHandle,
    store: State<'_, RepositoryStore>,
    models: State<'_, ModelStore>,
    supervisor: State<'_, PiSupervisor>,
    cpush: State<'_, CPushCoordinator>,
    request: RespondCPushRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    let data_dir = picket_data_dir()?;
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
pub async fn pull_rebase_repository(
    store: State<'_, RepositoryStore>,
    cpush: State<'_, CPushCoordinator>,
    actions: State<'_, CustomActionCoordinator>,
    request: PullRebaseRequest,
    on_event: Channel<CPushProgressEvent>,
) -> Result<CPushCommandReply, String> {
    if actions.has_active_operation_for_repo(&request.target.repository_id) {
        return Err(i18n::actions::action_pull_busy_error(
            request.locale.unwrap_or(LanguageKind::En),
        )
        .into());
    }
    let data_dir = picket_data_dir()?;
    cpush
        .pull_rebase(&data_dir, &store, &request, |event| {
            let _ = on_event.send(event);
            Ok(())
        })
        .await
}

#[tauri::command]
pub async fn reconcile_cpush_session(
    store: State<'_, RepositoryStore>,
    cpush: State<'_, CPushCoordinator>,
    request: ReconcileCPushRequest,
) -> Result<Option<CPushProgressEvent>, String> {
    let data_dir = picket_data_dir()?;
    cpush.reconcile_worktree(&data_dir, &store, &request).await
}

#[tauri::command]
pub async fn dismiss_cpush_done(
    cpush: State<'_, CPushCoordinator>,
    request: DismissCPushDoneRequest,
) -> Result<bool, String> {
    let data_dir = picket_data_dir()?;
    cpush.dismiss_done_notice(&data_dir, &request).await
}
