//! Persistent non-secret model catalog (`~/.picket/models.json` v3: provider connections
//! and child models) paired with `~/.picket/credentials.json` (`CredentialStore`),
//! macOS `LocalAuthentication` for on-demand key reveal, and Pi Core task execution.
use super::{
    credentials::CredentialStore,
    defaults::resolve_default_token_limits,
    local_auth::{LocalAuthOutcome, LocalAuthenticator, MacOsLocalAuthenticator},
    types::*,
};
use crate::{
    foundation::pi::{
        PiCommand, PiEvent, PiPaths, PiProviderSpec, PiSupervisor, PiTaskOutcome, RequestContext,
    },
    i18n::{self, LanguageKind},
    repository::{self, RepositoryStore},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

const STORAGE_VERSION: u8 = 3;
const MODELS_FILE: &str = "models.json";
const MAX_STORED_CONNECTIONS: usize = 32;
const MAX_STORED_MODELS: usize = 64;
const TEST_CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(50);
const AI_TASK_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredConnectionRecord {
    id: String,
    name: String,
    provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_hint: Option<String>,
}

impl StoredConnectionRecord {
    fn to_public(&self) -> SavedProviderConnection {
        SavedProviderConnection {
            id: self.id.clone(),
            name: self.name.clone(),
            provider: self.provider,
            base_url: self.base_url.clone(),
            key_hint: if self.provider.needs_key() {
                self.key_hint.clone()
            } else {
                None
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredModelRecord {
    id: String,
    connection_id: String,
    name: String,
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_format: Option<ApiFormatKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

impl StoredModelRecord {
    fn effective_api_format(&self, provider: ProviderKind) -> ApiFormatKind {
        self.api_format
            .unwrap_or_else(|| provider.default_api_format())
    }

    fn effective_token_limits(&self, provider: ProviderKind) -> (u32, u32) {
        let format = self.effective_api_format(provider);
        let (default_ctx, default_max_out) = resolve_default_token_limits(provider, Some(format));
        (
            self.context_window.unwrap_or(default_ctx),
            self.max_output_tokens.unwrap_or(default_max_out),
        )
    }

    fn to_public(&self, conn: &StoredConnectionRecord) -> SavedModelConfiguration {
        let (context_window, max_output_tokens) = self.effective_token_limits(conn.provider);
        SavedModelConfiguration {
            id: self.id.clone(),
            connection_id: self.connection_id.clone(),
            name: self.name.clone(),
            provider: conn.provider,
            api_format: Some(self.effective_api_format(conn.provider)),
            model_id: self.model_id.clone(),
            base_url: conn.base_url.clone(),
            context_window: Some(context_window),
            max_output_tokens: Some(max_output_tokens),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredModelsFile {
    version: u8,
    active_model_id: Option<String>,
    connections: Vec<StoredConnectionRecord>,
    models: Vec<StoredModelRecord>,
}

impl StoredModelsFile {
    fn empty() -> Self {
        Self {
            version: STORAGE_VERSION,
            active_model_id: None,
            connections: Vec::new(),
            models: Vec::new(),
        }
    }

    fn connection(&self, connection_id: &str) -> Option<&StoredConnectionRecord> {
        self.connections.iter().find(|c| c.id == connection_id)
    }

    fn public_connections(&self) -> Vec<SavedProviderConnection> {
        self.connections
            .iter()
            .map(StoredConnectionRecord::to_public)
            .collect()
    }

    fn public_models(&self) -> Vec<SavedModelConfiguration> {
        let by_id: HashMap<&str, &StoredConnectionRecord> = self
            .connections
            .iter()
            .map(|c| (c.id.as_str(), c))
            .collect();
        self.models
            .iter()
            .filter_map(|model| {
                by_id
                    .get(model.connection_id.as_str())
                    .map(|conn| model.to_public(conn))
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedRuntimeSpec {
    pub provider: ProviderKind,
    pub api_format: ApiFormatKind,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
}

impl ResolvedRuntimeSpec {
    pub fn to_pi_spec(&self) -> PiProviderSpec {
        PiProviderSpec {
            provider: self.provider.as_id(),
            api_format: Some(self.api_format.as_id()),
            model_id: self.model_id.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            context_window: Some(self.context_window),
            max_output_tokens: Some(self.max_output_tokens),
        }
    }
}

pub struct RestoredCatalog {
    pub connections: Vec<SavedProviderConnection>,
    pub models: Vec<SavedModelConfiguration>,
    pub active_model_id: Option<String>,
    pub warning: Option<String>,
}

#[derive(Clone)]
pub struct ModelStore {
    credentials: CredentialStore,
    authenticator: Arc<Mutex<Arc<dyn LocalAuthenticator>>>,
    gate: Arc<tokio::sync::Mutex<()>>,
    counter: Arc<AtomicU64>,
    endpoint_overrides: Arc<Mutex<HashMap<ProviderKind, String>>>,
}

impl Default for ModelStore {
    fn default() -> Self {
        Self::with_credentials_and_authenticator(
            CredentialStore::default(),
            Arc::new(MacOsLocalAuthenticator),
        )
    }
}

fn short_token(prefix: &str, counter: u64, hint: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(counter.to_le_bytes());
    hasher.update(hint.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &hex[..16])
}

fn read_models_file(
    storage_dir: &Path,
    locale: LanguageKind,
) -> Result<(StoredModelsFile, Option<String>), String> {
    let path = storage_dir.join(MODELS_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((StoredModelsFile::empty(), None));
        }
        Err(_) => {
            return Ok((
                StoredModelsFile::empty(),
                Some(i18n::models_file_unreadable_warning(locale).into()),
            ));
        }
    };
    match serde_json::from_slice::<StoredModelsFile>(&bytes) {
        Ok(mut parsed) if parsed.version == STORAGE_VERSION => {
            let conn_ids: std::collections::HashSet<&str> =
                parsed.connections.iter().map(|c| c.id.as_str()).collect();
            parsed
                .models
                .retain(|model| conn_ids.contains(model.connection_id.as_str()));
            if parsed
                .active_model_id
                .as_ref()
                .is_some_and(|active| !parsed.models.iter().any(|m| &m.id == active))
            {
                parsed.active_model_id = parsed.models.first().map(|m| m.id.clone());
            }
            Ok((parsed, None))
        }
        _ => Ok((
            StoredModelsFile::empty(),
            Some(i18n::models_file_corrupted_warning(locale).into()),
        )),
    }
}

fn write_models_file_atomic(storage_dir: &Path, payload: &StoredModelsFile) -> Result<(), String> {
    fs::create_dir_all(storage_dir)
        .map_err(|_| "Cannot create local configuration directory".to_string())?;
    fs::set_permissions(storage_dir, fs::Permissions::from_mode(0o700))
        .map_err(|_| "Cannot set local configuration directory permissions".to_string())?;
    let serialized = serde_json::to_vec_pretty(payload)
        .map_err(|_| "Cannot serialize model configuration".to_string())?;
    let mut temp = NamedTempFile::new_in(storage_dir)
        .map_err(|_| "Cannot create temporary model configuration file".to_string())?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
        .map_err(|_| "Cannot set model configuration file permissions".to_string())?;
    temp.write_all(&serialized)
        .map_err(|_| "Cannot write model configuration file".to_string())?;
    temp.as_file()
        .sync_all()
        .map_err(|_| "Cannot sync model configuration file".to_string())?;
    temp.persist(storage_dir.join(MODELS_FILE))
        .map_err(|_| "Cannot atomically persist model configuration file".to_string())?;
    Ok(())
}

impl ModelStore {
    pub fn with_credentials_and_authenticator(
        credentials: CredentialStore,
        authenticator: Arc<dyn LocalAuthenticator>,
    ) -> Self {
        Self {
            credentials,
            authenticator: Arc::new(Mutex::new(authenticator)),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            counter: Arc::new(AtomicU64::new(0)),
            endpoint_overrides: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub fn credentials_store(&self) -> &CredentialStore {
        &self.credentials
    }

    pub fn set_authenticator(
        &self,
        authenticator: Arc<dyn LocalAuthenticator>,
    ) -> Result<(), String> {
        *self
            .authenticator
            .lock()
            .map_err(|_| "Authenticator lock is unavailable".to_string())? = authenticator;
        Ok(())
    }

    fn current_authenticator(&self) -> Result<Arc<dyn LocalAuthenticator>, String> {
        self.authenticator
            .lock()
            .map(|guard| Arc::clone(&*guard))
            .map_err(|_| "Authenticator lock is unavailable".to_string())
    }

    #[cfg(test)]
    pub fn set_endpoint_override(&self, provider: ProviderKind, base_url: String) {
        self.endpoint_overrides
            .lock()
            .unwrap()
            .insert(provider, base_url);
    }

    fn effective_base_url(&self, provider: ProviderKind, configured: Option<&str>) -> String {
        if let Some(url) = configured.filter(|s| !s.trim().is_empty()) {
            return url.trim().to_owned();
        }
        if let Ok(overrides) = self.endpoint_overrides.lock()
            && let Some(override_url) = overrides.get(&provider)
        {
            return override_url.clone();
        }
        provider.default_base_url().to_owned()
    }

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
        let (stored, warning) = read_models_file(storage_dir, locale)?;
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
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        if stored.connections.len() >= MAX_STORED_CONNECTIONS {
            return Err(i18n::provider_limit_reached_error(locale).into());
        }
        if stored.models.len() >= MAX_STORED_MODELS {
            return Err(i18n::model_limit_reached_error(locale).into());
        }

        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let conn_id = short_token("conn", seq, &normalized.connection_name);
        let model_id = short_token("model", seq, &normalized.model_id);
        let key_hint = if normalized.provider.needs_key() {
            self.credentials
                .put(storage_dir, &conn_id, &normalized.api_key)?;
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

        if let Err(error) = write_models_file_atomic(storage_dir, &stored) {
            if normalized.provider.needs_key() {
                let _ = self.credentials.delete(storage_dir, &conn_id);
            }
            return Err(i18n::model_save_aborted_error(locale, &error, false));
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
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let Some(index) = stored.connections.iter().position(|c| c.id == conn_id) else {
            return Err(i18n::provider_not_found_error(locale).into());
        };
        let existing = stored.connections[index].clone();
        let next_name = validate_connection_name(&input.name, existing.provider, locale)?;
        let next_key =
            validate_api_key(&input.api_key, existing.provider.needs_key(), true, locale)?;

        let mut previous_secret_for_rollback: Option<Option<String>> = None;
        let next_hint = if existing.provider.needs_key() && !next_key.is_empty() {
            let prev = self.credentials.get(storage_dir, &conn_id)?;
            self.credentials.put(storage_dir, &conn_id, &next_key)?;
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

        if let Err(error) = write_models_file_atomic(storage_dir, &stored) {
            if let Some(prev_opt) = previous_secret_for_rollback {
                match prev_opt {
                    Some(prev_secret) => {
                        let _ = self.credentials.put(storage_dir, &conn_id, &prev_secret);
                    }
                    None => {
                        let _ = self.credentials.delete(storage_dir, &conn_id);
                    }
                }
            }
            return Err(i18n::model_save_aborted_error(locale, &error, true));
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
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let Some(index) = stored.connections.iter().position(|c| c.id == conn_id) else {
            return Err(i18n::provider_not_found_error(locale).into());
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
        write_models_file_atomic(storage_dir, &stored)?;
        if removed_conn.provider.needs_key() {
            let _ = self.credentials.delete(storage_dir, &conn_id);
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
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let Some(conn) = stored.connection(&conn_id).cloned() else {
            return Err(i18n::provider_not_found_error(locale).into());
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
                return Err(i18n::model_not_found_edit_error(locale).into());
            };
            if stored.models[index].connection_id != conn_id {
                return Err(i18n::provider_not_found_error(locale).into());
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
            write_models_file_atomic(storage_dir, &stored)
                .map_err(|error| i18n::model_save_aborted_error(locale, &error, true))?;
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
            return Err(i18n::model_limit_reached_error(locale).into());
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
        write_models_file_atomic(storage_dir, &stored)
            .map_err(|error| i18n::model_save_aborted_error(locale, &error, false))?;
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
            let (stored, _) = read_models_file(storage_dir, locale)?;
            let conn = stored
                .connection(&conn_id)
                .ok_or_else(|| i18n::provider_not_found_error(locale))?;
            if !conn.provider.needs_key() {
                return Err(i18n::model_reveal_no_key_error(locale).into());
            }
            conn.name.clone()
        };

        let authenticator = self.current_authenticator()?;
        let reason = i18n::model_reveal_auth_reason(locale, &conn_name);
        match authenticator.authenticate(&reason, locale).await {
            LocalAuthOutcome::Cancelled => {
                return Ok(ProviderRevealKeyReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    connection_id: conn_id,
                    result: ProviderRevealKeyOutcome::Cancelled {
                        message: i18n::model_reveal_auth_cancelled_message(locale).into(),
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
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let Some(index) = stored.connections.iter().position(|c| c.id == conn_id) else {
            return Err(i18n::provider_not_found_error(locale).into());
        };
        if !stored.connections[index].provider.needs_key() {
            return Err(i18n::model_reveal_no_key_error(locale).into());
        }
        let secret = self
            .credentials
            .get(storage_dir, &conn_id)?
            .ok_or_else(|| i18n::model_missing_stored_key_error(locale))?;
        let hint = compute_key_hint(&secret);
        if stored.connections[index].key_hint.as_deref() != Some(hint.as_str()) {
            stored.connections[index].key_hint = Some(hint.clone());
            let _ = write_models_file_atomic(storage_dir, &stored);
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
            return Err(i18n::model_select_missing_id_error(locale).into());
        }
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        if !stored.models.iter().any(|model| model.id == trimmed) {
            return Err(i18n::model_selected_not_found_error(locale).into());
        }
        stored.active_model_id = Some(trimmed.to_owned());
        write_models_file_atomic(storage_dir, &stored)?;
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
            return Err(i18n::model_delete_missing_id_error(locale).into());
        }
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let Some(index) = stored.models.iter().position(|model| model.id == trimmed) else {
            return Err(i18n::model_delete_not_found_error(locale).into());
        };
        // Removing even the last model keeps the parent connection and its saved key intact.
        stored.models.remove(index);
        if stored.active_model_id.as_deref() == Some(trimmed) {
            stored.active_model_id = stored.models.first().map(|model| model.id.clone());
        }
        write_models_file_atomic(storage_dir, &stored)?;
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            connections: stored.public_connections(),
            models: stored.public_models(),
            active_model_id: stored.active_model_id,
        })
    }

    pub async fn resolve_runtime_spec(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ResolvedRuntimeSpec, String> {
        self.resolve_runtime_spec_with_locale(storage_dir, model_id, LanguageKind::En)
            .await
    }

    pub async fn resolve_runtime_spec_with_locale(
        &self,
        storage_dir: &Path,
        model_id: &str,
        locale: LanguageKind,
    ) -> Result<ResolvedRuntimeSpec, String> {
        let _guard = self.gate.lock().await;
        let (stored, _) = read_models_file(storage_dir, locale)?;
        let record = stored
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or_else(|| i18n::model_active_missing_runtime_error(locale))?;
        let conn = stored
            .connection(&record.connection_id)
            .ok_or_else(|| i18n::provider_not_found_error(locale))?;
        let api_key = if conn.provider.needs_key() {
            self.credentials
                .get(storage_dir, &conn.id)?
                .ok_or_else(|| i18n::model_missing_stored_key_error(locale))?
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
                        .ok_or_else(|| i18n::provider_not_found_error(*locale))?;
                    let key = if conn.provider.needs_key() {
                        self.credentials
                            .get(storage_dir, &conn.id)?
                            .ok_or_else(|| i18n::model_missing_stored_key_error(*locale))?
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
                        .ok_or_else(|| i18n::provider_not_found_error(*locale))?;
                    let candidate_key =
                        validate_api_key(api_key, conn.provider.needs_key(), true, *locale)?;
                    let effective_key = if !conn.provider.needs_key() {
                        String::new()
                    } else if !candidate_key.is_empty() {
                        candidate_key
                    } else {
                        self.credentials
                            .get(storage_dir, &conn.id)?
                            .ok_or_else(|| i18n::model_missing_stored_key_error(*locale))?
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
                        return Err(i18n::provider_test_needs_model_error(*locale).into());
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
                    summary: i18n::model_connection_summary(
                        locale,
                        spec.provider,
                        &spec.model_id,
                        latency_ms,
                    ),
                    latency_ms,
                })
            }
            PiTaskOutcome::Cancelled => Err(i18n::model_test_cancelled_error(locale).into()),
            PiTaskOutcome::ReviewCompleted { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. } => {
                Err("Connection test returned an unexpected event type".into())
            }
        }
    }

    pub async fn review_scope(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        repo_store: &RepositoryStore,
        request: &ReviewScopeRequest,
        mut on_stream: impl FnMut(ModelStreamEvent) -> Result<(), String> + Send,
    ) -> Result<ReviewScopeReply, String> {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        if let Some(changed) = &request.changed_paths
            && changed.iter().any(|path| path.trim().is_empty())
        {
            return Err("Changed paths list for re-review contains an empty path".into());
        }
        let spec = self
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let scope = repo_store
            .extract_ai_scope(&request.target, &request.selections, locale)
            .await?;
        if scope.files.is_empty() {
            return Ok(ReviewScopeReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                kind: "review",
                request_id: request.request_id.clone(),
                target: request.target.clone(),
                model_id: request.model_id.clone(),
                findings: Vec::new(),
                skipped_paths: scope.skipped_paths,
                reviewed_file_ids: Vec::new(),
            });
        }
        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let skipped_paths = scope.skipped_paths.clone();
        let command = PiCommand::ReviewScope {
            request_id: request.request_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: scope.branch,
            prompt: request.prompt.clone(),
            files: scope.files,
            skipped_paths: skipped_paths.clone(),
            locale,
        };
        let stream_target = request.target.clone();
        let outcome = supervisor
            .run_task(
                paths,
                request.request_id.clone(),
                context,
                &command,
                AI_TASK_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |event| match event {
                    PiEvent::Started { request_id, .. } => on_stream(ModelStreamEvent::Started {
                        request_id: request_id.clone(),
                        kind: "review",
                        target: stream_target.clone(),
                    }),
                    PiEvent::TextDelta {
                        request_id, delta, ..
                    } => on_stream(ModelStreamEvent::Delta {
                        request_id: request_id.clone(),
                        kind: "review",
                        target: stream_target.clone(),
                        delta: delta.clone(),
                    }),
                    _ => Ok(()),
                },
            )
            .await?;

        match outcome {
            PiTaskOutcome::ReviewCompleted { findings } => {
                let mapped = findings
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| ReviewFindingWire {
                        id: format!("finding-{}-{index}", request.request_id),
                        file_id: item.file_id,
                        path: item.path,
                        line: item.line.max(1),
                        severity: item.severity,
                        title: item.title,
                        reason: item.reason,
                    })
                    .collect();
                Ok(ReviewScopeReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    kind: "review",
                    request_id: request.request_id.clone(),
                    target: request.target.clone(),
                    model_id: request.model_id.clone(),
                    findings: mapped,
                    skipped_paths,
                    reviewed_file_ids: scope.reviewed_file_ids,
                })
            }
            PiTaskOutcome::Cancelled => Err(i18n::model_review_cancelled_error(locale).into()),
            PiTaskOutcome::Completed { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. } => Err(
                "Code review returned an unexpected event type; draft and selection were preserved."
                    .into(),
            ),
        }
    }
}
