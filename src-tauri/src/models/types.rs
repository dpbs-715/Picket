use crate::repository::types::{CommitSelection, ReadTarget};
use serde::{Deserialize, Serialize};

pub const MODEL_PROTOCOL_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiFormatKind {
    OpenaiCompletions,
    OpenaiResponses,
    AnthropicMessages,
}

impl ApiFormatKind {
    pub fn as_id(self) -> &'static str {
        match self {
            Self::OpenaiCompletions => "openai-completions",
            Self::OpenaiResponses => "openai-responses",
            Self::AnthropicMessages => "anthropic-messages",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Anthropic,
    Openai,
    Deepseek,
    Ollama,
    Custom,
}

impl ProviderKind {
    pub fn as_id(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Openai => "openai",
            Self::Deepseek => "deepseek",
            Self::Ollama => "ollama",
            Self::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::Openai => "OpenAI",
            Self::Deepseek => "DeepSeek",
            Self::Ollama => "Ollama（本地）",
            Self::Custom => "自定义兼容",
        }
    }

    pub fn default_api_format(self) -> ApiFormatKind {
        match self {
            Self::Anthropic => ApiFormatKind::AnthropicMessages,
            Self::Openai => ApiFormatKind::OpenaiResponses,
            Self::Deepseek | Self::Ollama | Self::Custom => ApiFormatKind::OpenaiCompletions,
        }
    }

    pub fn needs_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    pub fn needs_url(self) -> bool {
        matches!(self, Self::Ollama | Self::Custom)
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::Openai => "https://api.openai.com/v1",
            Self::Deepseek => "https://api.deepseek.com",
            Self::Ollama => "http://127.0.0.1:11434/v1",
            Self::Custom => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedModelConfiguration {
    pub id: String,
    pub name: String,
    pub provider: ProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<ApiFormatKind>,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelInput {
    #[serde(default)]
    pub id: Option<String>,
    pub provider: ProviderKind,
    #[serde(default)]
    pub api_format: Option<ApiFormatKind>,
    pub name: String,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub struct NormalizedModelInput {
    pub provider: ProviderKind,
    pub api_format: ApiFormatKind,
    pub name: String,
    pub model_id: String,
    pub base_url: Option<String>,
    pub api_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestReply {
    pub protocol_version: u8,
    pub provider: ProviderKind,
    pub model_id: String,
    pub summary: String,
    pub latency_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSaveReply {
    pub protocol_version: u8,
    pub model: SavedModelConfiguration,
    pub active_model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMutateReply {
    pub protocol_version: u8,
    pub models: Vec<SavedModelConfiguration>,
    pub active_model_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelIdRequest {
    pub model_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateCommitRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub selections: Vec<CommitSelection>,
    pub draft: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCommitReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub text: String,
    pub skipped_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewScopeRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub selections: Vec<CommitSelection>,
    pub draft: String,
    #[serde(default)]
    pub changed_paths: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFindingWire {
    pub id: String,
    pub file_id: String,
    pub path: String,
    pub line: u32,
    pub severity: String,
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewScopeReply {
    pub protocol_version: u8,
    pub kind: &'static str,
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub findings: Vec<ReviewFindingWire>,
    pub skipped_paths: Vec<String>,
    pub reviewed_file_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ModelStreamEvent {
    Started {
        request_id: String,
        kind: &'static str,
        target: ReadTarget,
    },
    Delta {
        request_id: String,
        kind: &'static str,
        target: ReadTarget,
        delta: String,
    },
}

pub fn validate_http_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请填写接口地址".into());
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or("接口地址须为不含用户名和密码的 HTTP / HTTPS 地址")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
    }
    if authority.contains('@') {
        return Err("接口地址须为不含用户名和密码的 HTTP / HTTPS 地址".into());
    }
    if let Some(after_open) = authority.strip_prefix('[') {
        let Some(close_idx) = after_open.find(']') else {
            return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
        };
        let host = &after_open[..close_idx];
        let remainder = &after_open[close_idx + 1..];
        if host.is_empty() {
            return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
        }
        if let Some(port) = remainder.strip_prefix(':') {
            if !port.parse::<u16>().is_ok_and(|p| p >= 1) {
                return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
            }
        } else if !remainder.is_empty() {
            return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
        }
    } else if let Some((host, port)) = authority.split_once(':')
        && (host.is_empty() || port.contains(':') || !port.parse::<u16>().is_ok_and(|p| p >= 1))
    {
        return Err("请填写有效的 HTTP / HTTPS 接口地址".into());
    }
    Ok(trimmed.trim_end_matches('/').to_owned())
}

pub fn validate_and_normalize_model_input(
    input: &ModelInput,
) -> Result<NormalizedModelInput, String> {
    validate_and_normalize_model_input_with_options(input, false)
}

pub fn validate_and_normalize_model_input_with_options(
    input: &ModelInput,
    allow_empty_key: bool,
) -> Result<NormalizedModelInput, String> {
    if let Some(raw) = &input.id
        && (raw.trim().len() > 128 || raw.chars().any(char::is_control))
    {
        return Err("模型标识无效".into());
    }

    let model_id = input.model_id.trim();
    if model_id.is_empty() {
        return Err("请填写模型 ID".into());
    }
    if model_id.len() > 256 || model_id.chars().any(char::is_control) {
        return Err("模型 ID 格式无效或长度超出限制".into());
    }
    let name_trimmed = input.name.trim();
    let name = if name_trimmed.is_empty() {
        model_id.to_owned()
    } else {
        if name_trimmed.chars().count() > 128 || name_trimmed.chars().any(char::is_control) {
            return Err("显示名称格式无效或长度超出限制".into());
        }
        name_trimmed.to_owned()
    };

    let base_url = if input.provider.needs_url() {
        Some(validate_http_base_url(&input.base_url)?)
    } else {
        None
    };

    let api_key = if input.provider.needs_key() {
        let trimmed = input.api_key.trim();
        if trimmed.is_empty() {
            if allow_empty_key {
                String::new()
            } else {
                return Err("请填写 API Key".into());
            }
        } else {
            if trimmed.len() > 4096 || trimmed.chars().any(char::is_control) {
                return Err("API Key 包含非法控制字符或长度超出限制".into());
            }
            trimmed.to_owned()
        }
    } else {
        String::new()
    };

    Ok(NormalizedModelInput {
        provider: input.provider,
        api_format: input
            .api_format
            .unwrap_or_else(|| input.provider.default_api_format()),
        name,
        model_id: model_id.to_owned(),
        base_url,
        api_key,
    })
}
