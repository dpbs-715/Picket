use super::{
    ModelStore,
    persistence::{
        MAX_STORED_CONNECTIONS, MAX_STORED_MODELS, ResolvedRuntimeSpec, RestoredCatalog,
        StoredConnectionRecord, StoredModelRecord, read_models_file_async, short_token,
        write_models_file_atomic_async,
    },
};
use crate::{
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
    models::{local_auth::LocalAuthOutcome, types::*},
};
use std::{path::Path, sync::atomic::Ordering};

impl ModelStore {
    #[cfg(any(test, feature = "smoke-harness"))]
    pub async fn restore(&self, storage_dir: &Path) -> Result<RestoredCatalog, String> {
        self.restore_with_locale(storage_dir, LanguageKind::En)
            .await
    }

    pub async fn restore_with_locale(
        &self,
        storage_dir: &Path,
        locale: LanguageKind,
    ) -> Result<RestoredCatalog, String> {
        let _guard = self.gate.lock().await;
        let (stored, warning) = read_models_file_async(storage_dir, locale).await?;
        Ok(RestoredCatalog {
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: stored.active_model_id,
            warning,
        })
    }

    pub async fn create_provider_connection(
        &self,
        storage_dir: &Path,
        input: &CreateProviderConnectionInput,
    ) -> Result<ProviderSaveReply, String> {
        let locale = input.locale;
        let normalized = validate_new_connection(input)?;
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        if stored.connections.len() >= MAX_STORED_CONNECTIONS {
            return Err(i18n::models::provider_limit_reached_error(locale).into());
        }
        if stored.models.len() >= MAX_STORED_MODELS {
            return Err(i18n::models::model_limit_reached_error(locale).into());
        }

        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let conn_id = short_token("conn", seq, &normalized.connection_name);
        let model_id = short_token("model", seq, &normalized.model_id);
        let key_hint = if normalized.provider.needs_key() {
            self.credentials
                .put_async(storage_dir, &conn_id, &normalized.api_key)
                .await?;
            Some(compute_key_hint(&normalized.api_key))
        } else {
            None
        };

        let conn_record = StoredConnectionRecord {
            id: conn_id.clone(),
            name: normalized.connection_name,
            provider: normalized.provider,
            base_url: normalized.base_url,
            key_hint,
        };
        let model_record = StoredModelRecord {
            id: model_id.clone(),
            connection_id: conn_id.clone(),
            name: normalized.model_name,
            model_id: normalized.model_id,
            api_format: Some(normalized.api_format),
            context_window: Some(normalized.context_window),
            max_output_tokens: Some(normalized.max_output_tokens),
        };
        let public_conn = conn_record.to_public();
        let public_model = model_record.to_public(&conn_record);

        stored.connections.push(conn_record);
        stored.models.push(model_record);
        if !input.keep_active_model || stored.active_model_id.is_none() {
            stored.active_model_id = Some(model_id.clone());
        }

        if let Err(error) = write_models_file_atomic_async(storage_dir, stored.clone()).await {
            if normalized.provider.needs_key() {
                let _ = self.credentials.delete_async(storage_dir, &conn_id).await;
            }
            return Err(i18n::models::model_save_aborted_error(
                locale, &error, false,
            ));
        }

        let active_id = stored.active_model_id.clone().unwrap_or(model_id);
        Ok(ProviderSaveReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connection: public_conn,
            model: public_model,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: active_id,
        })
    }

    pub async fn update_provider_connection(
        &self,
        storage_dir: &Path,
        input: &UpdateProviderConnectionInput,
    ) -> Result<ProviderUpdateReply, String> {
        let locale = input.locale;
        let conn_id = validate_record_id(&input.connection_id, locale)?;
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        let Some(index) = stored.connections.iter().position(|c| c.id == conn_id) else {
            return Err(i18n::models::provider_not_found_error(locale).into());
        };
        let existing = stored.connections[index].clone();
        let next_name = validate_connection_name(&input.name, existing.provider, locale)?;
        let next_key =
            validate_api_key(&input.api_key, existing.provider.needs_key(), true, locale)?;

        let mut previous_secret_for_rollback: Option<Option<String>> = None;
        let next_hint = if existing.provider.needs_key() && !next_key.is_empty() {
            let prev = self.credentials.get_async(storage_dir, &conn_id).await?;
            self.credentials
                .put_async(storage_dir, &conn_id, &next_key)
                .await?;
            previous_secret_for_rollback = Some(prev);
            Some(compute_key_hint(&next_key))
        } else {
            existing.key_hint.clone()
        };

        let updated = StoredConnectionRecord {
            id: existing.id.clone(),
            name: next_name,
            provider: existing.provider,
            base_url: existing.base_url.clone(),
            key_hint: next_hint,
        };
        let public_conn = updated.to_public();
        stored.connections[index] = updated;

        if let Err(error) = write_models_file_atomic_async(storage_dir, stored.clone()).await {
            if let Some(prev_opt) = previous_secret_for_rollback {
                match prev_opt {
                    Some(prev_secret) => {
                        let _ = self
                            .credentials
                            .put_async(storage_dir, &conn_id, &prev_secret)
                            .await;
                    }
                    None => {
                        let _ = self.credentials.delete_async(storage_dir, &conn_id).await;
                    }
                }
            }
            return Err(i18n::models::model_save_aborted_error(locale, &error, true));
        }

        Ok(ProviderUpdateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connection: public_conn,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: stored.active_model_id,
        })
    }

    pub async fn delete_provider_connection_with_locale(
        &self,
        storage_dir: &Path,
        connection_id: &str,
        locale: LanguageKind,
    ) -> Result<ModelMutateReply, String> {
        let conn_id = validate_record_id(connection_id, locale)?;
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        let Some(index) = stored.connections.iter().position(|c| c.id == conn_id) else {
            return Err(i18n::models::provider_not_found_error(locale).into());
        };
        let removed_conn = stored.connections.remove(index);
        stored.models.retain(|m| m.connection_id != conn_id);
        if stored
            .active_model_id
            .as_ref()
            .is_some_and(|active| !stored.models.iter().any(|m| &m.id == active))
        {
            stored.active_model_id = stored.models.first().map(|m| m.id.clone());
        }
        write_models_file_atomic_async(storage_dir, stored.clone()).await?;
        if removed_conn.provider.needs_key() {
            let _ = self.credentials.delete_async(storage_dir, &conn_id).await;
        }
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: stored.active_model_id,
        })
    }

    pub async fn save_model(
        &self,
        storage_dir: &Path,
        input: &SaveModelInput,
    ) -> Result<ModelSaveReply, String> {
        let locale = input.locale;
        let conn_id = validate_record_id(&input.connection_id, locale)?;
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        let Some(conn) = stored.connection(&conn_id).cloned() else {
            return Err(i18n::models::provider_not_found_error(locale).into());
        };
        let normalized = validate_model_fields(
            conn.provider,
            &input.name,
            &input.model_id,
            input.api_format,
            input.context_window,
            input.max_output_tokens,
            locale,
        )?;

        if let Some(target_id) = input.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let target_id = validate_record_id(target_id, locale)?;
            let Some(index) = stored.models.iter().position(|m| m.id == target_id) else {
                return Err(i18n::models::model_not_found_edit_error(locale).into());
            };
            if stored.models[index].connection_id != conn_id {
                return Err(i18n::models::provider_not_found_error(locale).into());
            }
            let updated = StoredModelRecord {
                id: target_id.clone(),
                connection_id: conn_id,
                name: normalized.name,
                model_id: normalized.model_id,
                api_format: Some(normalized.api_format),
                context_window: Some(normalized.context_window),
                max_output_tokens: Some(normalized.max_output_tokens),
            };
            let public_model = updated.to_public(&conn);
            stored.models[index] = updated;
            if stored.active_model_id.is_none() {
                stored.active_model_id = Some(target_id.clone());
            }
            write_models_file_atomic_async(storage_dir, stored.clone())
                .await
                .map_err(|error| i18n::models::model_save_aborted_error(locale, &error, true))?;
            let active_id = stored.active_model_id.clone().unwrap_or(target_id);
            return Ok(ModelSaveReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                model: public_model,
                connections: stored.public_connections(),
                models: stored.public_models(),
                active_model_id: active_id,
            });
        }

        if stored.models.len() >= MAX_STORED_MODELS {
            return Err(i18n::models::model_limit_reached_error(locale).into());
        }
        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let id = short_token("model", seq, &normalized.model_id);
        let record = StoredModelRecord {
            id: id.clone(),
            connection_id: conn_id,
            name: normalized.name,
            model_id: normalized.model_id,
            api_format: Some(normalized.api_format),
            context_window: Some(normalized.context_window),
            max_output_tokens: Some(normalized.max_output_tokens),
        };
        let public_model = record.to_public(&conn);
        stored.models.push(record);
        // Adding a model under an existing connection preserves activeModelId unless none was active.
        if stored.active_model_id.is_none() {
            stored.active_model_id = Some(id.clone());
        }
        write_models_file_atomic_async(storage_dir, stored.clone())
            .await
            .map_err(|error| i18n::models::model_save_aborted_error(locale, &error, false))?;
        let active_id = stored.active_model_id.clone().unwrap_or(id);
        Ok(ModelSaveReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            model: public_model,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: active_id,
        })
    }

    pub async fn reveal_provider_api_key(
        &self,
        storage_dir: &Path,
        connection_id: &str,
        locale: LanguageKind,
    ) -> Result<ProviderRevealKeyReply, String> {
        let conn_id = validate_record_id(connection_id, locale)?;
        let conn_name = {
            let _guard = self.gate.lock().await;
            let (stored, _) = read_models_file_async(storage_dir, locale).await?;
            let conn = stored
                .connection(&conn_id)
                .ok_or_else(|| i18n::models::provider_not_found_error(locale))?;
            if !conn.provider.needs_key() {
                return Err(i18n::models::model_reveal_no_key_error(locale).into());
            }
            conn.name.clone()
        };

        let authenticator = self.current_authenticator()?;
        let reason = i18n::models::model_reveal_auth_reason(locale, &conn_name);
        match authenticator.authenticate(&reason, locale).await {
            LocalAuthOutcome::Cancelled => {
                return Ok(ProviderRevealKeyReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    connection_id: conn_id,
                    result: ProviderRevealKeyOutcome::Cancelled {
                        message: i18n::models::model_reveal_auth_cancelled_message(locale).into(),
                    },
                });
            }
            LocalAuthOutcome::Failed(message) => {
                return Ok(ProviderRevealKeyReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    connection_id: conn_id,
                    result: ProviderRevealKeyOutcome::Failed { message },
                });
            }
            LocalAuthOutcome::Authenticated => {}
        }

        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        let Some(index) = stored.connections.iter().position(|c| c.id == conn_id) else {
            return Err(i18n::models::provider_not_found_error(locale).into());
        };
        if !stored.connections[index].provider.needs_key() {
            return Err(i18n::models::model_reveal_no_key_error(locale).into());
        }
        let secret = self
            .credentials
            .get_async(storage_dir, &conn_id)
            .await?
            .ok_or_else(|| i18n::models::model_missing_stored_key_error(locale))?;
        let hint = compute_key_hint(&secret);
        if stored.connections[index].key_hint.as_deref() != Some(hint.as_str()) {
            stored.connections[index].key_hint = Some(hint.clone());
            let _ = write_models_file_atomic_async(storage_dir, stored).await;
        }
        Ok(ProviderRevealKeyReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connection_id: conn_id,
            result: ProviderRevealKeyOutcome::Authenticated {
                api_key: secret,
                key_hint: hint,
            },
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn select_model(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ModelMutateReply, String> {
        self.select_model_with_locale(storage_dir, model_id, LanguageKind::En)
            .await
    }

    pub async fn select_model_with_locale(
        &self,
        storage_dir: &Path,
        model_id: &str,
        locale: LanguageKind,
    ) -> Result<ModelMutateReply, String> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Err(i18n::models::model_select_missing_id_error(locale).into());
        }
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        if !stored.models.iter().any(|model| model.id == trimmed) {
            return Err(i18n::models::model_selected_not_found_error(locale).into());
        }
        stored.active_model_id = Some(trimmed.to_owned());
        write_models_file_atomic_async(storage_dir, stored.clone()).await?;
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: stored.active_model_id,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn delete_model(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ModelMutateReply, String> {
        self.delete_model_with_locale(storage_dir, model_id, LanguageKind::En)
            .await
    }

    pub async fn delete_model_with_locale(
        &self,
        storage_dir: &Path,
        model_id: &str,
        locale: LanguageKind,
    ) -> Result<ModelMutateReply, String> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Err(i18n::models::model_delete_missing_id_error(locale).into());
        }
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file_async(storage_dir, locale).await?;
        let Some(index) = stored.models.iter().position(|model| model.id == trimmed) else {
            return Err(i18n::models::model_delete_not_found_error(locale).into());
        };
        // Removing even the last model keeps the parent connection and its saved key intact.
        stored.models.remove(index);
        if stored.active_model_id.as_deref() == Some(trimmed) {
            stored.active_model_id = stored.models.first().map(|model| model.id.clone());
        }
        write_models_file_atomic_async(storage_dir, stored.clone()).await?;
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: stored.active_model_id,
        })
    }

    #[cfg(any(test, feature = "smoke-harness"))]
    pub async fn resolve_runtime_spec(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ResolvedRuntimeSpec, AppError> {
        self.resolve_runtime_spec_with_locale(storage_dir, model_id, LanguageKind::En)
            .await
    }

    pub async fn resolve_runtime_spec_with_locale(
        &self,
        storage_dir: &Path,
        model_id: &str,
        locale: LanguageKind,
    ) -> Result<ResolvedRuntimeSpec, AppError> {
        let _guard = self.gate.lock().await;
        let (stored, _) = read_models_file_async(storage_dir, locale)
            .await
            .map_err(|msg| AppError::model(AppErrorCode::ModelInvalidConfig, msg))?;
        let record = stored
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or_else(|| {
                AppError::model(
                    AppErrorCode::ModelNotFound,
                    i18n::models::model_active_missing_runtime_error(locale),
                )
            })?;
        let conn = stored.connection(&record.connection_id).ok_or_else(|| {
            AppError::model(
                AppErrorCode::ModelNotFound,
                i18n::models::provider_not_found_error(locale),
            )
        })?;
        let api_key = if conn.provider.needs_key() {
            self.credentials
                .get_async(storage_dir, &conn.id)
                .await
                .map_err(|msg| AppError::model(AppErrorCode::ModelMissingCredential, msg))?
                .ok_or_else(|| {
                    AppError::model(
                        AppErrorCode::ModelMissingCredential,
                        i18n::models::model_missing_stored_key_error(locale),
                    )
                })?
        } else {
            String::new()
        };
        let base_url = self.effective_base_url(conn.provider, conn.base_url.as_deref());
        let (context_window, max_output_tokens) = record.effective_token_limits(conn.provider);
        Ok(ResolvedRuntimeSpec {
            provider: conn.provider,
            api_format: record.effective_api_format(conn.provider),
            model_id: record.model_id.clone(),
            base_url,
            api_key,
            context_window,
            max_output_tokens,
        })
    }
}
