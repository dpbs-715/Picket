//! Persistent non-secret model catalog (`models.json`) paired with macOS Keychain
//! (`CredentialVault`) and Pi Core task execution for M3.
use super::{keychain::CredentialVault, types::*};
use crate::{
    foundation::pi::{PiEvent, PiPaths, PiSupervisor, PiTaskOutcome, RequestContext},
    repository::{self, RepositoryStore, ai_context::AiScopeMode},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
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

const STORAGE_VERSION: u8 = 1;
const MODELS_FILE: &str = "models.json";
const MAX_STORED_MODELS: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredModelRecord {
    id: String,
    name: String,
    provider: ProviderKind,
    model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential_ref: Option<String>,
}

impl StoredModelRecord {
    fn to_public(&self) -> SavedModelConfiguration {
        SavedModelConfiguration {
            id: self.id.clone(),
            name: self.name.clone(),
            provider: self.provider,
            model_id: self.model_id.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredModelsFile {
    version: u8,
    active_model_id: Option<String>,
    models: Vec<StoredModelRecord>,
}

#[derive(Clone, Debug)]
pub struct ResolvedRuntimeSpec {
    pub provider: ProviderKind,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Clone, Default)]
pub struct ModelStore {
    vault: CredentialVault,
    gate: Arc<tokio::sync::Mutex<()>>,
    counter: Arc<AtomicU64>,
    endpoint_overrides: Arc<Mutex<HashMap<ProviderKind, String>>>,
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

fn read_models_file(storage_dir: &Path) -> Result<(StoredModelsFile, Option<String>), String> {
    let path = storage_dir.join(MODELS_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                StoredModelsFile {
                    version: STORAGE_VERSION,
                    active_model_id: None,
                    models: Vec::new(),
                },
                None,
            ));
        }
        Err(_) => {
            return Ok((
                StoredModelsFile {
                    version: STORAGE_VERSION,
                    active_model_id: None,
                    models: Vec::new(),
                },
                Some("无法读取本地模型配置，已回退为零模型状态；原有文件未被覆盖。".into()),
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
            Ok((parsed, None))
        }
        _ => Ok((
            StoredModelsFile {
                version: STORAGE_VERSION,
                active_model_id: None,
                models: Vec::new(),
            },
            Some(
                "本地模型配置文件已损坏或版本不兼容，已回退为零模型状态；原有文件未被覆盖。".into(),
            ),
        )),
    }
}

fn write_models_file_atomic(storage_dir: &Path, payload: &StoredModelsFile) -> Result<(), String> {
    fs::create_dir_all(storage_dir).map_err(|_| "无法创建本地配置目录".to_string())?;
    fs::set_permissions(storage_dir, fs::Permissions::from_mode(0o700))
        .map_err(|_| "无法设置本地配置目录权限".to_string())?;
    let serialized =
        serde_json::to_vec_pretty(payload).map_err(|_| "无法序列化模型配置".to_string())?;
    let mut temp =
        NamedTempFile::new_in(storage_dir).map_err(|_| "无法创建模型配置临时文件".to_string())?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
        .map_err(|_| "无法设置模型配置文件权限".to_string())?;
    temp.write_all(&serialized)
        .map_err(|_| "无法写入模型配置文件".to_string())?;
    temp.as_file()
        .sync_all()
        .map_err(|_| "无法同步模型配置文件".to_string())?;
    temp.persist(storage_dir.join(MODELS_FILE))
        .map_err(|_| "无法原子保存模型配置文件".to_string())?;
    Ok(())
}

impl ModelStore {
    pub fn with_vault(vault: CredentialVault) -> Self {
        Self {
            vault,
            gate: Arc::new(tokio::sync::Mutex::new(())),
            counter: Arc::new(AtomicU64::new(0)),
            endpoint_overrides: Arc::new(Mutex::new(HashMap::new())),
        }
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
        let _guard = self.gate.lock().await;
        let (stored, warning) = read_models_file(storage_dir)?;
        let public_models = stored
            .models
            .iter()
            .map(StoredModelRecord::to_public)
            .collect();
        Ok((public_models, stored.active_model_id, warning))
    }

    pub async fn save_model(
        &self,
        storage_dir: &Path,
        input: &ModelInput,
    ) -> Result<ModelSaveReply, String> {
        let normalized = validate_and_normalize_model_input(input)?;
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir)?;
        if stored.models.len() >= MAX_STORED_MODELS {
            return Err("已保存的模型数量已达上限，请先删除不再使用的模型。".into());
        }

        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let id = short_token("model", seq, &normalized.model_id);
        let credential_ref = if normalized.provider.needs_key() {
            let account = short_token("cred", seq, &id);
            self.vault.put(&account, &normalized.api_key)?;
            Some(account)
        } else {
            None
        };

        let record = StoredModelRecord {
            id: id.clone(),
            name: normalized.name,
            provider: normalized.provider,
            model_id: normalized.model_id,
            base_url: normalized.base_url,
            credential_ref: credential_ref.clone(),
        };
        let public = record.to_public();
        stored.models.push(record);
        stored.active_model_id = Some(id.clone());

        if let Err(error) = write_models_file_atomic(storage_dir, &stored) {
            if let Some(account) = credential_ref {
                let _ = self.vault.delete(&account);
            }
            return Err(format!("{error}；没有保存模型配置，输入已保留。"));
        }

        Ok(ModelSaveReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            model: public,
            active_model_id: id,
        })
    }

    pub async fn select_model(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ModelMutateReply, String> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Err("未指定要切换的模型".into());
        }
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir)?;
        if !stored.models.iter().any(|model| model.id == trimmed) {
            return Err("所选模型不存在或已被删除".into());
        }
        stored.active_model_id = Some(trimmed.to_owned());
        write_models_file_atomic(storage_dir, &stored)?;
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            models: stored
                .models
                .iter()
                .map(StoredModelRecord::to_public)
                .collect(),
            active_model_id: stored.active_model_id,
        })
    }

    pub async fn delete_model(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ModelMutateReply, String> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Err("未指定要删除的模型".into());
        }
        let _guard = self.gate.lock().await;
        let (mut stored, _) = read_models_file(storage_dir)?;
        let Some(index) = stored.models.iter().position(|model| model.id == trimmed) else {
            return Err("要删除的模型不存在".into());
        };
        let removed = stored.models.remove(index);
        if stored.active_model_id.as_deref() == Some(trimmed) {
            stored.active_model_id = stored.models.first().map(|model| model.id.clone());
        }
        write_models_file_atomic(storage_dir, &stored)?;
        if let Some(account) = removed.credential_ref {
            let _ = self.vault.delete(&account);
        }
        Ok(ModelMutateReply {
            protocol_version: MODEL_PROTOCOL_VERSION,
            models: stored
                .models
                .iter()
                .map(StoredModelRecord::to_public)
                .collect(),
            active_model_id: stored.active_model_id,
        })
    }

    pub async fn resolve_runtime_spec(
        &self,
        storage_dir: &Path,
        model_id: &str,
    ) -> Result<ResolvedRuntimeSpec, String> {
        let _guard = self.gate.lock().await;
        let (stored, _) = read_models_file(storage_dir)?;
        let record = stored
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or("当前选中的模型已不存在，请在底栏或设置中重新选择模型；草稿与选择已保留。")?;
        let api_key = if record.provider.needs_key() {
            let account = record.credential_ref.as_deref().ok_or(
                "该模型缺少系统钥匙串凭据引用，请在设置中删除后重新添加；草稿与选择已保留。",
            )?;
            self.vault.get(account)?.ok_or(
                "该模型在 macOS 系统钥匙串中的 API Key 已缺失，请在设置中删除后重新添加模型；草稿与选择已保留。",
            )?
        } else {
            String::new()
        };
        let base_url = self.effective_base_url(record.provider, record.base_url.as_deref());
        Ok(ResolvedRuntimeSpec {
            provider: record.provider,
            model_id: record.model_id.clone(),
            base_url,
            api_key,
        })
    }

    pub async fn test_connection(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        input: &ModelInput,
    ) -> Result<ModelTestReply, String> {
        let normalized = validate_and_normalize_model_input(input)?;
        let base_url = self.effective_base_url(normalized.provider, normalized.base_url.as_deref());
        let seq = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let request_id = short_token("testconn", seq, &normalized.model_id);
        let context = RequestContext {
            repository_id: "settings".into(),
            worktree_id: "settings".into(),
            session_id: request_id.clone(),
            scope_version: "connection-test-v1".into(),
        };
        let command = serde_json::json!({
            "type": "testConnection",
            "requestId": request_id,
            "context": context,
            "model": {
                "provider": normalized.provider.as_id(),
                "modelId": normalized.model_id,
                "baseUrl": base_url,
                "apiKey": normalized.api_key,
            }
        });
        let started_at = Instant::now();
        let outcome = supervisor
            .run_task(
                paths,
                request_id,
                context,
                command,
                Duration::from_secs(20),
                Some(&normalized.api_key),
                |_| Ok(()),
            )
            .await?;
        let latency_ms = started_at.elapsed().as_millis() as u64;
        match outcome {
            PiTaskOutcome::Completed { .. } => Ok(ModelTestReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                provider: normalized.provider,
                model_id: normalized.model_id.clone(),
                summary: format!(
                    "连接成功：{} · {}（{} ms）",
                    normalized.provider.label(),
                    normalized.model_id,
                    latency_ms
                ),
                latency_ms,
            }),
            PiTaskOutcome::Cancelled => Err("连接测试已取消；当前输入已保留。".into()),
            PiTaskOutcome::ReviewCompleted { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. } => {
                Err("连接测试返回了未预期的事件类型".into())
            }
        }
    }

    pub async fn generate_commit(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        repo_store: &RepositoryStore,
        request: &GenerateCommitRequest,
        mut on_stream: impl FnMut(ModelStreamEvent) -> Result<(), String> + Send,
    ) -> Result<GenerateCommitReply, String> {
        repository::validate_request_id(&request.request_id)?;
        let spec = self
            .resolve_runtime_spec(storage_dir, &request.model_id)
            .await?;
        let scope = repo_store
            .extract_ai_scope(&request.target, &request.selections, AiScopeMode::Generate)
            .await?;
        if scope.files.is_empty() && scope.skipped_paths.is_empty() {
            return Err("所选范围内没有可用于生成提交说明的内容；当前草稿与选择已保留。".into());
        }
        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let command = serde_json::json!({
            "type": "generateCommit",
            "requestId": request.request_id,
            "context": context,
            "model": {
                "provider": spec.provider.as_id(),
                "modelId": spec.model_id,
                "baseUrl": spec.base_url,
                "apiKey": spec.api_key,
            },
            "branch": scope.branch,
            "recentCommits": scope.recent_commits,
            "draft": request.draft,
            "files": scope.files,
            "skippedPaths": scope.skipped_paths,
        });
        let stream_target = request.target.clone();
        let outcome = supervisor
            .run_task(
                paths,
                request.request_id.clone(),
                context,
                command,
                Duration::from_secs(60),
                Some(&spec.api_key),
                |event| match event {
                    PiEvent::Started { request_id, .. } => on_stream(ModelStreamEvent::Started {
                        request_id: request_id.clone(),
                        kind: "generate",
                        target: stream_target.clone(),
                    }),
                    PiEvent::TextDelta {
                        request_id, delta, ..
                    } => on_stream(ModelStreamEvent::Delta {
                        request_id: request_id.clone(),
                        kind: "generate",
                        target: stream_target.clone(),
                        delta: delta.clone(),
                    }),
                    _ => Ok(()),
                },
            )
            .await?;

        match outcome {
            PiTaskOutcome::Completed { text } => Ok(GenerateCommitReply {
                protocol_version: MODEL_PROTOCOL_VERSION,
                kind: "generate",
                request_id: request.request_id.clone(),
                target: request.target.clone(),
                model_id: request.model_id.clone(),
                text,
                skipped_paths: scope.skipped_paths,
            }),
            PiTaskOutcome::Cancelled => Err("已取消生成提交说明；当前草稿与选择已保留。".into()),
            PiTaskOutcome::ReviewCompleted { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. } => {
                Err("提交说明生成返回了未预期的响应类型；当前草稿与选择已保留。".into())
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
        repository::validate_request_id(&request.request_id)?;
        if let Some(changed) = &request.changed_paths
            && changed.iter().any(|path| path.trim().is_empty())
        {
            return Err("复审变动文件列表包含空路径".into());
        }
        let spec = self
            .resolve_runtime_spec(storage_dir, &request.model_id)
            .await?;
        let scope = repo_store
            .extract_ai_scope(&request.target, &request.selections, AiScopeMode::Review)
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
        let command = serde_json::json!({
            "type": "reviewScope",
            "requestId": request.request_id,
            "context": context,
            "model": {
                "provider": spec.provider.as_id(),
                "modelId": spec.model_id,
                "baseUrl": spec.base_url,
                "apiKey": spec.api_key,
            },
            "branch": scope.branch,
            "draft": request.draft,
            "files": scope.files,
            "skippedPaths": scope.skipped_paths,
        });
        let stream_target = request.target.clone();
        let outcome = supervisor
            .run_task(
                paths,
                request.request_id.clone(),
                context,
                command,
                Duration::from_secs(60),
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
                    skipped_paths: scope.skipped_paths,
                    reviewed_file_ids: scope.reviewed_file_ids,
                })
            }
            PiTaskOutcome::Cancelled => Err("已取消代码审查；当前草稿与选择已保留。".into()),
            PiTaskOutcome::Completed { .. }
            | PiTaskOutcome::CPushPlanCompleted { .. }
            | PiTaskOutcome::ConflictResolved { .. } => {
                Err("代码审查返回了未预期的响应类型；当前草稿与选择已保留。".into())
            }
        }
    }
}
