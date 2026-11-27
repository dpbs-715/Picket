use super::{
    ModelStore,
    persistence::{
        ResolvedRuntimeSpec, TEST_CONNECTION_IDLE_TIMEOUT, read_models_file, short_token,
    },
};
use crate::{
    foundation::pi::{
        PiCommand, PiPaths, PiProviderSpec, PiSupervisor, PiTaskOutcome, RequestContext,
    },
    i18n,
    models::types::*,
};
use std::{path::Path, sync::atomic::Ordering, time::Instant};

impl ModelStore {
    pub async fn test_connection_with_storage(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        input: &TestConnectionInput,
    ) -> Result<ModelTestReply, String> {
        let (locale, spec) = match input {
            TestConnectionInput::NewConnection {
                provider,
                base_url,
                api_key,
                model_id,
                api_format,
                context_window,
                max_output_tokens,
                locale,
            } => {
                let valid_url = if provider.needs_url() {
                    Some(validate_http_base_url(base_url, *locale)?)
                } else {
                    None
                };
                let valid_key = validate_api_key(api_key, provider.needs_key(), false, *locale)?;
                let model = validate_model_fields(
                    *provider,
                    "",
                    model_id,
                    *api_format,
                    *context_window,
                    *max_output_tokens,
                    *locale,
                )?;
                let effective_url = self.effective_base_url(*provider, valid_url.as_deref());
                (
                    *locale,
                    ResolvedRuntimeSpec {
                        provider: *provider,
                        api_format: model.api_format,
                        model_id: model.model_id,
                        base_url: effective_url,
                        api_key: valid_key,
                        context_window: model.context_window,
                        max_output_tokens: model.max_output_tokens,
                    },
                )
            }
            TestConnectionInput::ConnectionModel {
                connection_id,
                model_id,
                api_format,
                context_window,
                max_output_tokens,
                locale,
            } => {
                let conn_id = validate_record_id(connection_id, *locale)?;
                let (conn, api_key) = {
                    let _guard = self.gate.lock().await;
                    let (stored, _) = read_models_file(storage_dir, *locale)?;
                    let conn = stored
                        .connection(&conn_id)
                        .cloned()
                        .ok_or_else(|| i18n::models::provider_not_found_error(*locale))?;
                    let key = if conn.provider.needs_key() {
                        self.credentials
                            .get(storage_dir, &conn.id)?
                            .ok_or_else(|| i18n::models::model_missing_stored_key_error(*locale))?
                    } else {
                        String::new()
                    };
                    (conn, key)
                };
                let model = validate_model_fields(
                    conn.provider,
                    "",
                    model_id,
                    *api_format,
                    *context_window,
                    *max_output_tokens,
                    *locale,
                )?;
                let effective_url =
                    self.effective_base_url(conn.provider, conn.base_url.as_deref());
                (
                    *locale,
                    ResolvedRuntimeSpec {
                        provider: conn.provider,
                        api_format: model.api_format,
                        model_id: model.model_id,
                        base_url: effective_url,
                        api_key,
                        context_window: model.context_window,
                        max_output_tokens: model.max_output_tokens,
                    },
                )
            }
            TestConnectionInput::ExistingConnection {
                connection_id,
                api_key,
                model_id,
                locale,
            } => {
                let conn_id = validate_record_id(connection_id, *locale)?;
                let (conn, chosen_model, effective_key) = {
                    let _guard = self.gate.lock().await;
                    let (stored, _) = read_models_file(storage_dir, *locale)?;
                    let conn = stored
                        .connection(&conn_id)
                        .cloned()
                        .ok_or_else(|| i18n::models::provider_not_found_error(*locale))?;
                    let candidate_key =
                        validate_api_key(api_key, conn.provider.needs_key(), true, *locale)?;
                    let effective_key = if !conn.provider.needs_key() {
                        String::new()
                    } else if !candidate_key.is_empty() {
                        candidate_key
                    } else {
                        self.credentials
                            .get(storage_dir, &conn.id)?
                            .ok_or_else(|| i18n::models::model_missing_stored_key_error(*locale))?
                    };
                    let child = if let Some(requested_id) =
                        model_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
                    {
                        stored
                            .models
                            .iter()
                            .find(|m| m.connection_id == conn_id && m.model_id == requested_id)
                            .cloned()
                            .or_else(|| {
                                stored
                                    .models
                                    .iter()
                                    .find(|m| m.connection_id == conn_id)
                                    .cloned()
                            })
                    } else {
                        stored
                            .models
                            .iter()
                            .find(|m| m.connection_id == conn_id)
                            .cloned()
                    };
                    (conn, child, effective_key)
                };
                let (target_model_id, api_format, context_window, max_output_tokens) =
                    if let Some(child) = chosen_model {
                        let format = child.effective_api_format(conn.provider);
                        let (ctx, max_out) = child.effective_token_limits(conn.provider);
                        (child.model_id, format, ctx, max_out)
                    } else if let Some(explicit) =
                        model_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
                    {
                        let normalized = validate_model_fields(
                            conn.provider,
                            "",
                            explicit,
                            None,
                            None,
                            None,
                            *locale,
                        )?;
                        (
                            normalized.model_id,
                            normalized.api_format,
                            normalized.context_window,
                            normalized.max_output_tokens,
                        )
                    } else {
                        return Err(i18n::models::provider_test_needs_model_error(*locale).into());
                    };
                let effective_url =
                    self.effective_base_url(conn.provider, conn.base_url.as_deref());
                (
                    *locale,
                    ResolvedRuntimeSpec {
                        provider: conn.provider,
                        api_format,
                        model_id: target_model_id,
                        base_url: effective_url,
                        api_key: effective_key,
                        context_window,
                        max_output_tokens,
                    },
                )
            }
        };

        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let request_id = short_token("testconn", seq, &spec.model_id);
        let context = RequestContext {
            repository_id: "settings".into(),
            worktree_id: "settings".into(),
            session_id: request_id.clone(),
            scope_version: "connection-test-v1".into(),
        };
        let command = PiCommand::TestConnection {
            request_id: request_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            locale,
        };
        let started_at = Instant::now();
        let outcome = supervisor
            .run_task(
                paths,
                request_id,
                context,
                &command,
                TEST_CONNECTION_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
            )
            .await?;
        let latency_ms = started_at.elapsed().as_millis() as u64;
        match outcome {
            PiTaskOutcome::Completed { text } => {
                let _ = text.len();
                Ok(ModelTestReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    provider: spec.provider,
                    model_id: spec.model_id.clone(),
                    summary: i18n::models::model_connection_summary(
                        locale,
                        spec.provider,
                        &spec.model_id,
                        latency_ms,
                    ),
                    latency_ms,
                })
            }
            PiTaskOutcome::Cancelled => {
                Err(i18n::models::model_test_cancelled_error(locale).into())
            }
            PiTaskOutcome::ModelsListed { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. }
            | PiTaskOutcome::CustomActionCompleted { .. }
            | PiTaskOutcome::CustomActionDrafted { .. } => {
                Err("Connection test returned an unexpected event type".into())
            }
        }
    }

    pub async fn list_provider_models(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        input: &ListModelsInput,
    ) -> Result<ModelListReply, String> {
        let (locale, provider, api_format, base_url, api_key) = match input {
            ListModelsInput::NewConnection {
                provider,
                base_url,
                api_key,
                api_format,
                locale,
            } => {
                let effective_format = api_format.unwrap_or_else(|| provider.default_api_format());
                let resolved_url = if provider.needs_url() {
                    Some(validate_http_base_url(base_url, *locale)?)
                } else {
                    None
                };
                let effective_key =
                    validate_api_key(api_key, provider.needs_key(), false, *locale)?;
                let effective_url = self.effective_base_url(*provider, resolved_url.as_deref());
                (
                    *locale,
                    *provider,
                    effective_format,
                    effective_url,
                    effective_key,
                )
            }
            ListModelsInput::ExistingConnection {
                connection_id,
                api_format,
                locale,
            } => {
                let conn_id = validate_record_id(connection_id, *locale)?;
                let (conn, effective_key) = {
                    let _guard = self.gate.lock().await;
                    let (stored, _) = read_models_file(storage_dir, *locale)?;
                    let conn = stored
                        .connection(&conn_id)
                        .cloned()
                        .ok_or_else(|| i18n::models::provider_not_found_error(*locale))?;
                    let key = if conn.provider.needs_key() {
                        self.credentials
                            .get(storage_dir, &conn.id)?
                            .ok_or_else(|| i18n::models::model_missing_stored_key_error(*locale))?
                    } else {
                        String::new()
                    };
                    (conn, key)
                };
                let effective_format =
                    api_format.unwrap_or_else(|| conn.provider.default_api_format());
                let effective_url =
                    self.effective_base_url(conn.provider, conn.base_url.as_deref());
                (
                    *locale,
                    conn.provider,
                    effective_format,
                    effective_url,
                    effective_key,
                )
            }
        };

        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let request_id = short_token("listmodels", seq, provider.as_id());
        let context = RequestContext {
            repository_id: "settings".into(),
            worktree_id: "settings".into(),
            session_id: request_id.clone(),
            scope_version: "model-list-v1".into(),
        };
        let command = PiCommand::ListModels {
            request_id: request_id.clone(),
            context: context.clone(),
            model: PiProviderSpec {
                provider: provider.as_id(),
                api_format: Some(api_format.as_id()),
                model_id: String::new(),
                base_url,
                api_key: api_key.clone(),
                context_window: None,
                max_output_tokens: None,
            },
            locale,
        };
        let outcome = supervisor
            .run_task(
                paths,
                request_id,
                context,
                &command,
                TEST_CONNECTION_IDLE_TIMEOUT,
                Some(&api_key),
                |_| Ok(()),
            )
            .await?;
        match outcome {
            PiTaskOutcome::ModelsListed { models } => Ok(ModelListReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                provider,
                models: models
                    .into_iter()
                    .map(|item| RemoteModelOption {
                        id: item.id,
                        display_name: item.display_name,
                        context_window: item.context_window,
                        max_output_tokens: item.max_output_tokens,
                    })
                    .collect(),
            }),
            PiTaskOutcome::Cancelled => {
                Err(i18n::models::model_test_cancelled_error(locale).into())
            }
            PiTaskOutcome::Completed { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. }
            | PiTaskOutcome::CustomActionCompleted { .. }
            | PiTaskOutcome::CustomActionDrafted { .. } => {
                Err("Model list request returned an unexpected event type".into())
            }
        }
    }
}
