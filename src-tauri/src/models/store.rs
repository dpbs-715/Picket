//! Persistent non-secret model catalog (`models.json` v2) paired with macOS Keychain
//! (`CredentialVault`), `LocalAuthentication`, reference-counted shared credentials,
//! and Pi Core task execution.
use super::{
    defaults::resolve_default_token_limits,
    keychain::CredentialVault,
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
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

const STORAGE_VERSION: u8 = 2;
const MODELS_FILE: &str = "models.json";
const MAX_STORED_MODELS: usize = 64;
const TEST_CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(50);
const AI_TASK_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCredentialRecord {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredModelRecord {
    id: String,
    name: String,
    provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_format: Option<ApiFormatKind>,
    model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential_ref: Option<String>,
}

impl StoredModelRecord {
    fn effective_api_format(&self) -> ApiFormatKind {
        self.api_format
            .unwrap_or_else(|| self.provider.default_api_format())
    }

    fn effective_token_limits(&self) -> (u32, u32) {
        let format = self.effective_api_format();
        let (default_ctx, default_max_out) =
            resolve_default_token_limits(self.provider, Some(format));
        (
            self.context_window.unwrap_or(default_ctx),
            self.max_output_tokens.unwrap_or(default_max_out),
        )
    }

    fn to_public(
        &self,
        key_hint: Option<String>,
        shared_key_count: Option<u32>,
    ) -> SavedModelConfiguration {
        let (context_window, max_output_tokens) = self.effective_token_limits();
        SavedModelConfiguration {
            id: self.id.clone(),
            name: self.name.clone(),
            provider: self.provider,
            api_format: Some(self.effective_api_format()),
            model_id: self.model_id.clone(),
            base_url: self.base_url.clone(),
            context_window: Some(context_window),
            max_output_tokens: Some(max_output_tokens),
            key_hint,
            shared_key_count,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredModelsFile {
    version: u8,
    active_model_id: Option<String>,
    #[serde(default)]
    credentials: Vec<StoredCredentialRecord>,
    models: Vec<StoredModelRecord>,
}

impl StoredModelsFile {
    fn empty() -> Self {
        Self {
            version: STORAGE_VERSION,
            active_model_id: None,
            credentials: Vec::new(),
            models: Vec::new(),
        }
    }

    fn set_credential_hint(&mut self, cred_id: &str, hint: String) {
        if let Some(existing) = self.credentials.iter_mut().find(|item| item.id == cred_id) {
            existing.key_hint = Some(hint);
        } else {
            self.credentials.push(StoredCredentialRecord {
                id: cred_id.to_owned(),
                key_hint: Some(hint),
            });
        }
    }

    fn has_credential_hint(&self, cred_id: &str) -> bool {
        self.credentials
            .iter()
            .any(|item| item.id == cred_id && item.key_hint.is_some())
    }

    fn reconcile_credentials(&mut self) {
        let referenced: HashSet<&str> = self
            .models
            .iter()
            .filter(|model| model.provider.needs_key())
            .filter_map(|model| model.credential_ref.as_deref())
            .collect();
        self.credentials
            .retain(|cred| referenced.contains(cred.id.as_str()));
        let mut known: HashSet<String> = self
            .credentials
            .iter()
            .map(|cred| cred.id.clone())
            .collect();
        for model in &self.models {
            if model.provider.needs_key()
                && let Some(cred_id) = model.credential_ref.as_deref()
                && known.insert(cred_id.to_owned())
            {
                self.credentials.push(StoredCredentialRecord {
                    id: cred_id.to_owned(),
                    key_hint: None,
                });
            }
        }
    }

    fn credential_ref_count(&self, cred_id: &str) -> usize {
        self.models
            .iter()
            .filter(|model| {
                model.provider.needs_key() && model.credential_ref.as_deref() == Some(cred_id)
            })
            .count()
    }

    fn public_models(&self) -> Vec<SavedModelConfiguration> {
        let mut counts: HashMap<&str, u32> = HashMap::new();
        for model in &self.models {
            if model.provider.needs_key()
                && let Some(cred_id) = model.credential_ref.as_deref()
            {
                *counts.entry(cred_id).or_insert(0) += 1;
            }
        }
        let hints: HashMap<&str, Option<&str>> = self
            .credentials
            .iter()
            .map(|cred| (cred.id.as_str(), cred.key_hint.as_deref()))
            .collect();

        self.models
            .iter()
            .map(|model| {
                if model.provider.needs_key()
                    && let Some(cred_id) = model.credential_ref.as_deref()
                {
                    let hint = hints.get(cred_id).and_then(|item| *item).map(str::to_owned);
                    let count = counts.get(cred_id).copied();
                    model.to_public(hint, count)
                } else {
                    model.to_public(None, None)
                }
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

#[derive(Clone)]
pub struct ModelStore {
    vault: CredentialVault,
    authenticator: Arc<Mutex<Arc<dyn LocalAuthenticator>>>,
    gate: Arc<tokio::sync::Mutex<()>>,
    counter: Arc<AtomicU64>,
    endpoint_overrides: Arc<Mutex<HashMap<ProviderKind, String>>>,
}

impl Default for ModelStore {
    fn default() -> Self {
        Self::with_vault(CredentialVault::default())
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
            if parsed
                .active_model_id
                .as_ref()
                .is_some_and(|active| !parsed.models.iter().any(|m| &m.id == active))
            {
                parsed.active_model_id = parsed.models.first().map(|m| m.id.clone());
            }
            parsed.reconcile_credentials();
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
    pub fn with_vault(vault: CredentialVault) -> Self {
        Self::with_vault_and_authenticator(vault, Arc::new(MacOsLocalAuthenticator))
    }

    pub fn with_vault_and_authenticator(
        vault: CredentialVault,
        authenticator: Arc<dyn LocalAuthenticator>,
    ) -> Self {
        Self {
            vault,
            authenticator: Arc::new(Mutex::new(authenticator)),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            counter: Arc::new(AtomicU64::new(0)),
            endpoint_overrides: Arc::new(Mutex::new(HashMap::new())),
        }
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

    pub fn use_isolated_keychain(
        &self,
        path: PathBuf,
        password: impl Into<String>,
    ) -> Result<(), String> {
        self.vault.set_keychain_file(path, password)
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

    pub async fn restore(
        &self,
        storage_dir: &Path,
    ) -> Result<(Vec<SavedModelConfiguration>, Option<String>, Option<String>), String> {
        self.restore_with_locale(storage_dir, LanguageKind::En)
            .await
    }

    pub async fn restore_with_locale(
        &self,
        storage_dir: &Path,
        locale: LanguageKind,
    ) -> Result<(Vec<SavedModelConfiguration>, Option<String>, Option<String>), String> {
        let _guard = self.gate.lock().await;
        let (stored, warning) = read_models_file(storage_dir, locale)?;
        Ok((stored.public_models(), stored.active_model_id, warning))
    }

    pub async fn save_model(
        &self,
        storage_dir: &Path,
        input: &ModelInput,
    ) -> Result<ModelSaveReply, String> {
        let locale = input.locale;
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;

        if let Some(target_id) = input.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let Some(index) = stored.models.iter().position(|m| m.id == target_id) else {
                return Err(i18n::model_not_found_edit_error(locale).into());
            };
            let existing = stored.models[index].clone();
            let existing_peer_count = existing
                .credential_ref
                .as_deref()
                .map(|cred_id| stored.credential_ref_count(cred_id))
                .unwrap_or(0);
            let normalized = validate_and_normalize_model_input_with_options(
                input,
                existing.credential_ref.is_some(),
            )?;
            let endpoint_changed = !same_credential_endpoint(
                existing.provider,
                existing.base_url.as_deref(),
                normalized.provider,
                normalized.base_url.as_deref(),
            );

            let mut newly_written_cred: Option<String> = None;
            let mut obsolete_cred: Option<String> = None;
            let mut update_all_old_cred: Option<(String, String)> = None;

            let next_credential_ref = if normalized.provider.needs_key() {
                if !normalized.api_key.is_empty() {
                    let new_account = short_token("cred", seq, &existing.id);
                    self.vault.put(&new_account, &normalized.api_key)?;
                    newly_written_cred = Some(new_account.clone());
                    stored.set_credential_hint(&new_account, compute_key_hint(&normalized.api_key));

                    let scope = input.shared_key_update_scope.unwrap_or_default();
                    let update_all_peers = existing_peer_count > 1
                        && !endpoint_changed
                        && scope == SharedKeyUpdateScope::UpdateAll;
                    if update_all_peers {
                        if let Some(old_cred) = existing.credential_ref.clone() {
                            update_all_old_cred = Some((old_cred.clone(), new_account.clone()));
                            obsolete_cred = Some(old_cred);
                        }
                    } else if existing_peer_count <= 1 {
                        obsolete_cred = existing.credential_ref.clone();
                    }
                    Some(new_account)
                } else {
                    if existing_peer_count > 1 && endpoint_changed {
                        return Err(i18n::model_shared_key_endpoint_mismatch_error(locale).into());
                    }
                    let old_account = existing
                        .credential_ref
                        .as_deref()
                        .ok_or_else(|| i18n::model_api_key_required_error(locale))?;
                    let Some(secret) = self.vault.get(old_account)? else {
                        return Err(i18n::model_keychain_secret_missing_edit_error(locale).into());
                    };
                    if !stored.has_credential_hint(old_account) {
                        stored.set_credential_hint(old_account, compute_key_hint(&secret));
                    }
                    existing.credential_ref.clone()
                }
            } else {
                if existing_peer_count <= 1 {
                    obsolete_cred = existing.credential_ref.clone();
                }
                None
            };

            let updated = StoredModelRecord {
                id: existing.id.clone(),
                name: normalized.name,
                provider: normalized.provider,
                api_format: Some(normalized.api_format),
                model_id: normalized.model_id,
                base_url: normalized.base_url,
                context_window: Some(normalized.context_window),
                max_output_tokens: Some(normalized.max_output_tokens),
                credential_ref: next_credential_ref.clone(),
            };
            stored.models[index] = updated;
            if let Some((old_cred, new_cred)) = update_all_old_cred {
                for peer in &mut stored.models {
                    if peer.provider.needs_key()
                        && peer.credential_ref.as_deref() == Some(old_cred.as_str())
                    {
                        peer.credential_ref = Some(new_cred.clone());
                    }
                }
            }
            stored.reconcile_credentials();
            if stored.active_model_id.is_none() {
                stored.active_model_id = Some(existing.id.clone());
            }

            if let Err(error) = write_models_file_atomic(storage_dir, &stored) {
                if let Some(account) = newly_written_cred {
                    let _ = self.vault.delete(&account);
                }
                return Err(i18n::model_save_aborted_error(locale, &error, true));
            }
            if let Some(old_account) = obsolete_cred
                && stored.credential_ref_count(&old_account) == 0
            {
                let _ = self.vault.delete(&old_account);
            }

            let public_models = stored.public_models();
            let public = public_models
                .iter()
                .find(|m| m.id == existing.id)
                .cloned()
                .ok_or_else(|| i18n::model_not_found_edit_error(locale).to_owned())?;
            let active_id = stored
                .active_model_id
                .clone()
                .unwrap_or_else(|| existing.id.clone());
            return Ok(ModelSaveReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                model: public,
                models: public_models,
                active_model_id: active_id,
            });
        }

        let source_id = input
            .source_model_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let allow_empty_key =
            source_id.is_some() && input.provider.needs_key() && input.api_key.trim().is_empty();
        let normalized = validate_and_normalize_model_input_with_options(input, allow_empty_key)?;
        if stored.models.len() >= MAX_STORED_MODELS {
            return Err(i18n::model_limit_reached_error(locale).into());
        }

        let id = short_token("model", seq, &normalized.model_id);
        let mut newly_written_cred: Option<String> = None;
        let credential_ref = if normalized.provider.needs_key() {
            if !normalized.api_key.is_empty() {
                let account = short_token("cred", seq, &id);
                self.vault.put(&account, &normalized.api_key)?;
                stored.set_credential_hint(&account, compute_key_hint(&normalized.api_key));
                newly_written_cred = Some(account.clone());
                Some(account)
            } else {
                let src_id = source_id.ok_or_else(|| i18n::model_api_key_required_error(locale))?;
                let source = stored
                    .models
                    .iter()
                    .find(|m| m.id == src_id)
                    .cloned()
                    .ok_or_else(|| i18n::model_source_not_found_error(locale))?;
                if !same_credential_endpoint(
                    source.provider,
                    source.base_url.as_deref(),
                    normalized.provider,
                    normalized.base_url.as_deref(),
                ) {
                    return Err(i18n::model_shared_key_endpoint_mismatch_error(locale).into());
                }
                let src_cred = source
                    .credential_ref
                    .ok_or_else(|| i18n::model_source_not_found_error(locale))?;
                let Some(secret) = self.vault.get(&src_cred)? else {
                    return Err(i18n::model_source_not_found_error(locale).into());
                };
                if !stored.has_credential_hint(&src_cred) {
                    stored.set_credential_hint(&src_cred, compute_key_hint(&secret));
                }
                Some(src_cred)
            }
        } else {
            None
        };

        let record = StoredModelRecord {
            id: id.clone(),
            name: normalized.name,
            provider: normalized.provider,
            api_format: Some(normalized.api_format),
            model_id: normalized.model_id,
            base_url: normalized.base_url,
            context_window: Some(normalized.context_window),
            max_output_tokens: Some(normalized.max_output_tokens),
            credential_ref,
        };
        stored.models.push(record);
        stored.reconcile_credentials();

        let preserve_active = input.keep_active_model || source_id.is_some();
        if !preserve_active || stored.active_model_id.is_none() {
            stored.active_model_id = Some(id.clone());
        }

        if let Err(error) = write_models_file_atomic(storage_dir, &stored) {
            if let Some(account) = newly_written_cred {
                let _ = self.vault.delete(&account);
            }
            return Err(i18n::model_save_aborted_error(locale, &error, false));
        }

        let public_models = stored.public_models();
        let public = public_models
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| "Saved model missing from catalog".to_string())?;
        let active_id = stored.active_model_id.clone().unwrap_or(id);

        Ok(ModelSaveReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            model: public,
            models: public_models,
            active_model_id: active_id,
        })
    }

    pub async fn reveal_model_api_key(
        &self,
        storage_dir: &Path,
        model_id: &str,
        locale: LanguageKind,
    ) -> Result<ModelRevealKeyReply, String> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Err(i18n::model_select_missing_id_error(locale).into());
        }
        let model_name = {
            let _guard = self.gate.lock().await;
            let (stored, _) = read_models_file(storage_dir, locale)?;
            let record = stored
                .models
                .iter()
                .find(|m| m.id == trimmed)
                .ok_or_else(|| i18n::model_selected_not_found_error(locale))?;
            if !record.provider.needs_key() || record.credential_ref.is_none() {
                return Err(i18n::model_reveal_no_key_error(locale).into());
            }
            record.name.clone()
        };

        let authenticator = self.current_authenticator()?;
        let reason = i18n::model_reveal_auth_reason(locale, &model_name);
        match authenticator.authenticate(&reason, locale).await {
            LocalAuthOutcome::Cancelled => {
                return Ok(ModelRevealKeyReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    model_id: trimmed.to_owned(),
                    result: ModelRevealKeyOutcome::Cancelled {
                        message: i18n::model_reveal_auth_cancelled_message(locale).into(),
                    },
                });
            }
            LocalAuthOutcome::Failed(message) => {
                return Ok(ModelRevealKeyReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    model_id: trimmed.to_owned(),
                    result: ModelRevealKeyOutcome::Failed { message },
                });
            }
            LocalAuthOutcome::Authenticated => {}
        }

        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir, locale)?;
        let record = stored
            .models
            .iter()
            .find(|m| m.id == trimmed)
            .ok_or_else(|| i18n::model_selected_not_found_error(locale))?;
        if !record.provider.needs_key() {
            return Err(i18n::model_reveal_no_key_error(locale).into());
        }
        let cred_id = record
            .credential_ref
            .clone()
            .ok_or_else(|| i18n::model_missing_cred_ref_error(locale))?;
        let secret = self
            .vault
            .get(&cred_id)?
            .ok_or_else(|| i18n::model_missing_keychain_key_error(locale))?;
        let hint = compute_key_hint(&secret);
        if !stored.has_credential_hint(&cred_id) {
            stored.set_credential_hint(&cred_id, hint.clone());
            let _ = write_models_file_atomic(storage_dir, &stored);
        }
        Ok(ModelRevealKeyReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            model_id: trimmed.to_owned(),
            result: ModelRevealKeyOutcome::Authenticated {
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
        let removed = stored.models.remove(index);
        if stored.active_model_id.as_deref() == Some(trimmed) {
            stored.active_model_id = stored.models.first().map(|model| model.id.clone());
        }
        let still_referenced = removed
            .credential_ref
            .as_deref()
            .is_some_and(|cred_id| stored.credential_ref_count(cred_id) > 0);
        stored.reconcile_credentials();
        write_models_file_atomic(storage_dir, &stored)?;
        if !still_referenced && let Some(account) = removed.credential_ref {
            let _ = self.vault.delete(&account);
        }
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
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
        let api_key = if record.provider.needs_key() {
            let account = record
                .credential_ref
                .as_deref()
                .ok_or_else(|| i18n::model_missing_cred_ref_error(locale))?;
            self.vault
                .get(account)?
                .ok_or_else(|| i18n::model_missing_keychain_key_error(locale))?
        } else {
            String::new()
        };
        let base_url = self.effective_base_url(record.provider, record.base_url.as_deref());
        let (context_window, max_output_tokens) = record.effective_token_limits();
        Ok(ResolvedRuntimeSpec {
            provider: record.provider,
            api_format: record.effective_api_format(),
            model_id: record.model_id.clone(),
            base_url,
            api_key,
            context_window,
            max_output_tokens,
        })
    }

    #[cfg(test)]
    pub async fn test_connection(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        input: &ModelInput,
    ) -> Result<ModelTestReply, String> {
        self.test_connection_with_storage(None, paths, supervisor, input)
            .await
    }

    pub async fn test_connection_with_storage(
        &self,
        storage_dir: Option<&Path>,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        input: &ModelInput,
    ) -> Result<ModelTestReply, String> {
        let locale = input.locale;
        let candidate_base_url = if input.provider.needs_url() {
            Some(validate_http_base_url(&input.base_url, locale)?)
        } else {
            None
        };
        let mut stored_api_key: Option<String> = None;
        if input.api_key.trim().is_empty()
            && input.provider.needs_key()
            && let Some(dir) = storage_dir
        {
            let _guard = self.gate.lock().await;
            let (stored, _) = read_models_file(dir, locale)?;
            if let Some(target_id) = input.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if let Some(existing) = stored.models.iter().find(|m| m.id == target_id)
                    && let Some(account) = existing.credential_ref.as_deref()
                {
                    let peer_count = stored.credential_ref_count(account);
                    if peer_count > 1
                        && !same_credential_endpoint(
                            existing.provider,
                            existing.base_url.as_deref(),
                            input.provider,
                            candidate_base_url.as_deref(),
                        )
                    {
                        return Err(i18n::model_shared_key_endpoint_mismatch_error(locale).into());
                    }
                    stored_api_key = self.vault.get(account)?;
                }
            } else if let Some(src_id) = input
                .source_model_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let source = stored
                    .models
                    .iter()
                    .find(|m| m.id == src_id)
                    .ok_or_else(|| i18n::model_source_not_found_error(locale))?;
                if !same_credential_endpoint(
                    source.provider,
                    source.base_url.as_deref(),
                    input.provider,
                    candidate_base_url.as_deref(),
                ) {
                    return Err(i18n::model_shared_key_endpoint_mismatch_error(locale).into());
                }
                let src_cred = source
                    .credential_ref
                    .as_deref()
                    .ok_or_else(|| i18n::model_source_not_found_error(locale))?;
                stored_api_key = Some(
                    self.vault
                        .get(src_cred)?
                        .ok_or_else(|| i18n::model_source_not_found_error(locale))?,
                );
            }
        }
        let normalized =
            validate_and_normalize_model_input_with_options(input, stored_api_key.is_some())?;
        let effective_key = if normalized.api_key.is_empty() {
            stored_api_key.unwrap_or_default()
        } else {
            normalized.api_key
        };
        let base_url = self.effective_base_url(normalized.provider, normalized.base_url.as_deref());
        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let request_id = short_token("testconn", seq, &normalized.model_id);
        let context = RequestContext {
            repository_id: "settings".into(),
            worktree_id: "settings".into(),
            session_id: request_id.clone(),
            scope_version: "connection-test-v1".into(),
        };
        let command = PiCommand::TestConnection {
            request_id: request_id.clone(),
            context: context.clone(),
            model: PiProviderSpec {
                provider: normalized.provider.as_id(),
                api_format: Some(normalized.api_format.as_id()),
                model_id: normalized.model_id.clone(),
                base_url,
                api_key: effective_key.clone(),
                context_window: Some(normalized.context_window),
                max_output_tokens: Some(normalized.max_output_tokens),
            },
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
                Some(&effective_key),
                |_| Ok(()),
            )
            .await?;
        let latency_ms = started_at.elapsed().as_millis() as u64;
        match outcome {
            PiTaskOutcome::Completed { text } => {
                let _ = text.len();
                Ok(ModelTestReply {
                    protocol_version: MODEL_PROTOCOL_VERSION,
                    provider: normalized.provider,
                    model_id: normalized.model_id.clone(),
                    summary: i18n::model_connection_summary(
                        locale,
                        normalized.provider,
                        &normalized.model_id,
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
