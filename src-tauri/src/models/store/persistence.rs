use crate::{
    i18n::{self, LanguageKind},
    models::{
        defaults::resolve_default_token_limits,
        types::{ApiFormatKind, ProviderKind, SavedModelConfiguration, SavedProviderConnection},
    },
    pi_runtime::PiProviderSpec,
    storage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(super) const STORAGE_VERSION: u8 = 3;
const MODELS_FILE: &str = "models.json";
pub(super) const MAX_STORED_CONNECTIONS: usize = 32;
pub(super) const MAX_STORED_MODELS: usize = 64;
pub(super) const TEST_CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(50);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredConnectionRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) key_hint: Option<String>,
}

impl StoredConnectionRecord {
    pub(super) fn to_public(&self) -> SavedProviderConnection {
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
#[serde(rename_all = "camelCase")]
pub(super) struct StoredModelRecord {
    pub(super) id: String,
    pub(super) connection_id: String,
    pub(super) name: String,
    pub(super) model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) api_format: Option<ApiFormatKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_output_tokens: Option<u32>,
}

impl StoredModelRecord {
    pub(super) fn effective_api_format(&self, provider: ProviderKind) -> ApiFormatKind {
        self.api_format
            .unwrap_or_else(|| provider.default_api_format())
    }

    pub(super) fn effective_token_limits(&self, provider: ProviderKind) -> (u32, u32) {
        let format = self.effective_api_format(provider);
        let (default_ctx, default_max_out) =
            resolve_default_token_limits(provider, Some(format), Some(&self.model_id));
        (
            self.context_window.unwrap_or(default_ctx),
            self.max_output_tokens.unwrap_or(default_max_out),
        )
    }

    pub(super) fn to_public(&self, conn: &StoredConnectionRecord) -> SavedModelConfiguration {
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
#[serde(rename_all = "camelCase")]
pub(super) struct StoredModelsFile {
    pub(super) version: u8,
    pub(super) active_model_id: Option<String>,
    pub(super) connections: Vec<StoredConnectionRecord>,
    pub(super) models: Vec<StoredModelRecord>,
}

impl StoredModelsFile {
    pub(super) fn empty() -> Self {
        Self {
            version: STORAGE_VERSION,
            active_model_id: None,
            connections: Vec::new(),
            models: Vec::new(),
        }
    }

    pub(super) fn connection(&self, connection_id: &str) -> Option<&StoredConnectionRecord> {
        self.connections.iter().find(|c| c.id == connection_id)
    }

    pub(super) fn public_connections(&self) -> Vec<SavedProviderConnection> {
        self.connections
            .iter()
            .map(StoredConnectionRecord::to_public)
            .collect()
    }

    pub(super) fn public_models(&self) -> Vec<SavedModelConfiguration> {
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

pub(super) fn short_token(prefix: &str, counter: u64, hint: &str) -> String {
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

fn read_models_file_unlocked(
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
                Some(i18n::models::models_file_unreadable_warning(locale).into()),
            ));
        }
    };
    match serde_json::from_slice::<StoredModelsFile>(&bytes) {
        Ok(mut parsed) if parsed.version == STORAGE_VERSION => {
            let conn_ids: HashSet<&str> =
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
        _ => {
            storage::backup_corrupt_copy(&path, &bytes);
            Ok((
                StoredModelsFile::empty(),
                Some(i18n::models::models_file_corrupted_warning(locale).into()),
            ))
        }
    }
}

pub(super) async fn read_models_file_async(
    storage_dir: &Path,
    locale: LanguageKind,
) -> Result<(StoredModelsFile, Option<String>), String> {
    let dir = storage_dir.to_path_buf();
    let path = dir.join(MODELS_FILE);
    storage::run_blocking_locked(path, move || read_models_file_unlocked(&dir, locale)).await
}

pub(super) async fn write_models_file_atomic_async(
    storage_dir: &Path,
    payload: StoredModelsFile,
) -> Result<(), String> {
    let dir = storage_dir.to_path_buf();
    let target = dir.join(MODELS_FILE);
    storage::run_blocking_locked(target.clone(), move || {
        storage::write_json_atomic(&dir, &target, &payload, false)
    })
    .await
}
