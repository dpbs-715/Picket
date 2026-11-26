use super::defaults::{model_defaults, resolve_default_token_limits, validate_token_limits};
use crate::{
    i18n::{self, LanguageKind, deserialize_language_or_default},
    repository::types::{CommitSelection, ReadTarget},
};
use serde::{Deserialize, Serialize};

pub const MODEL_PROTOCOL_VERSION: u8 = 2;

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
        model_defaults()
            .providers
            .get(self.as_id())
            .map(|entry| entry.label.as_str())
            .unwrap_or(self.as_id())
    }

    pub fn default_api_format(self) -> ApiFormatKind {
        model_defaults()
            .providers
            .get(self.as_id())
            .map(|entry| entry.default_api_format)
            .unwrap_or(ApiFormatKind::OpenaiCompletions)
    }

    pub fn needs_key(self) -> bool {
        model_defaults()
            .providers
            .get(self.as_id())
            .map(|entry| entry.needs_key)
            .unwrap_or(!matches!(self, Self::Ollama))
    }

    pub fn needs_url(self) -> bool {
        model_defaults()
            .providers
            .get(self.as_id())
            .map(|entry| entry.needs_url)
            .unwrap_or(matches!(self, Self::Ollama | Self::Custom))
    }

    pub fn default_base_url(self) -> &'static str {
        model_defaults()
            .providers
            .get(self.as_id())
            .map(|entry| entry.default_base_url.as_str())
            .unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedProviderConnection {
    pub id: String,
    pub name: String,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedModelConfiguration {
    pub id: String,
    pub connection_id: String,
    pub name: String,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<ApiFormatKind>,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateProviderConnectionInput {
    pub provider: ProviderKind,
    #[serde(default)]
    pub connection_name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model_name: String,
    pub model_id: String,
    #[serde(default)]
    pub api_format: Option<ApiFormatKind>,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub keep_active_model: bool,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

impl Default for CreateProviderConnectionInput {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            connection_name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model_name: String::new(),
            model_id: String::new(),
            api_format: None,
            context_window: None,
            max_output_tokens: None,
            keep_active_model: false,
            locale: LanguageKind::En,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateProviderConnectionInput {
    pub connection_id: String,
    pub name: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveModelInput {
    #[serde(default)]
    pub id: Option<String>,
    pub connection_id: String,
    #[serde(default)]
    pub name: String,
    pub model_id: String,
    #[serde(default)]
    pub api_format: Option<ApiFormatKind>,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TestConnectionInput {
    NewConnection {
        provider: ProviderKind,
        #[serde(default)]
        base_url: String,
        #[serde(default)]
        api_key: String,
        model_id: String,
        #[serde(default)]
        api_format: Option<ApiFormatKind>,
        #[serde(default)]
        context_window: Option<u32>,
        #[serde(default)]
        max_output_tokens: Option<u32>,
        #[serde(default, deserialize_with = "deserialize_language_or_default")]
        locale: LanguageKind,
    },
    ConnectionModel {
        connection_id: String,
        model_id: String,
        #[serde(default)]
        api_format: Option<ApiFormatKind>,
        #[serde(default)]
        context_window: Option<u32>,
        #[serde(default)]
        max_output_tokens: Option<u32>,
        #[serde(default, deserialize_with = "deserialize_language_or_default")]
        locale: LanguageKind,
    },
    ExistingConnection {
        connection_id: String,
        #[serde(default)]
        api_key: String,
        #[serde(default)]
        model_id: Option<String>,
        #[serde(default, deserialize_with = "deserialize_language_or_default")]
        locale: LanguageKind,
    },
}

#[derive(Clone, Debug)]
pub struct NormalizedNewConnection {
    pub provider: ProviderKind,
    pub connection_name: String,
    pub base_url: Option<String>,
    pub api_key: String,
    pub model_name: String,
    pub model_id: String,
    pub api_format: ApiFormatKind,
    pub context_window: u32,
    pub max_output_tokens: u32,
}

#[derive(Clone, Debug)]
pub struct NormalizedModelFields {
    pub name: String,
    pub model_id: String,
    pub api_format: ApiFormatKind,
    pub context_window: u32,
    pub max_output_tokens: u32,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteModelOption {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ListModelsInput {
    NewConnection {
        provider: ProviderKind,
        #[serde(default)]
        base_url: String,
        #[serde(default)]
        api_key: String,
        #[serde(default)]
        api_format: Option<ApiFormatKind>,
        #[serde(default, deserialize_with = "deserialize_language_or_default")]
        locale: LanguageKind,
    },
    ExistingConnection {
        connection_id: String,
        #[serde(default)]
        api_format: Option<ApiFormatKind>,
        #[serde(default, deserialize_with = "deserialize_language_or_default")]
        locale: LanguageKind,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListReply {
    pub protocol_version: u8,
    pub provider: ProviderKind,
    pub models: Vec<RemoteModelOption>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSaveReply {
    pub protocol_version: u8,
    pub connection: SavedProviderConnection,
    pub model: SavedModelConfiguration,
    pub connections: Vec<SavedProviderConnection>,
    pub models: Vec<SavedModelConfiguration>,
    pub active_model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpdateReply {
    pub protocol_version: u8,
    pub connection: SavedProviderConnection,
    pub connections: Vec<SavedProviderConnection>,
    pub models: Vec<SavedModelConfiguration>,
    pub active_model_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSaveReply {
    pub protocol_version: u8,
    pub model: SavedModelConfiguration,
    pub connections: Vec<SavedProviderConnection>,
    pub models: Vec<SavedModelConfiguration>,
    pub active_model_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProviderRevealKeyOutcome {
    Authenticated { api_key: String, key_hint: String },
    Cancelled { message: String },
    Failed { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRevealKeyReply {
    pub protocol_version: u8,
    pub connection_id: String,
    #[serde(flatten)]
    pub result: ProviderRevealKeyOutcome,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMutateReply {
    pub protocol_version: u8,
    pub connections: Vec<SavedProviderConnection>,
    pub models: Vec<SavedModelConfiguration>,
    pub active_model_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelIdRequest {
    pub model_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionIdRequest {
    pub connection_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewScopeRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    pub selections: Vec<CommitSelection>,
    pub prompt: String,
    #[serde(default)]
    pub changed_paths: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
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

pub fn validate_http_base_url(raw: &str, locale: LanguageKind) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(i18n::models::url_required_error(locale).into());
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(i18n::models::url_invalid_http_error(locale).into());
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| i18n::models::url_no_userinfo_http_error(locale))?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(i18n::models::url_invalid_http_error(locale).into());
    }
    if authority.contains('@') {
        return Err(i18n::models::url_no_userinfo_http_error(locale).into());
    }
    if let Some(after_open) = authority.strip_prefix('[') {
        let Some(close_idx) = after_open.find(']') else {
            return Err(i18n::models::url_invalid_http_error(locale).into());
        };
        let host = &after_open[..close_idx];
        let remainder = &after_open[close_idx + 1..];
        if host.is_empty() {
            return Err(i18n::models::url_invalid_http_error(locale).into());
        }
        if let Some(port) = remainder.strip_prefix(':') {
            if !port.parse::<u16>().is_ok_and(|p| p >= 1) {
                return Err(i18n::models::url_invalid_http_error(locale).into());
            }
        } else if !remainder.is_empty() {
            return Err(i18n::models::url_invalid_http_error(locale).into());
        }
    } else if let Some((host, port)) = authority.split_once(':')
        && (host.is_empty() || port.contains(':') || !port.parse::<u16>().is_ok_and(|p| p >= 1))
    {
        return Err(i18n::models::url_invalid_http_error(locale).into());
    }
    Ok(trimmed.trim_end_matches('/').to_owned())
}

pub fn validate_record_id(raw: &str, locale: LanguageKind) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed.chars().any(char::is_control) {
        return Err(i18n::models::model_invalid_id_record_error(locale).into());
    }
    Ok(trimmed.to_owned())
}

pub fn validate_connection_name(
    raw: &str,
    provider: ProviderKind,
    locale: LanguageKind,
) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(i18n::models::provider_label(locale, provider).to_owned());
    }
    if trimmed.chars().count() > 128 || trimmed.chars().any(char::is_control) {
        return Err(i18n::models::model_display_name_invalid_error(locale).into());
    }
    Ok(trimmed.to_owned())
}

pub fn validate_api_key(
    raw: &str,
    needs_key: bool,
    allow_empty: bool,
    locale: LanguageKind,
) -> Result<String, String> {
    if !needs_key {
        return Ok(String::new());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        if allow_empty {
            return Ok(String::new());
        }
        return Err(i18n::models::model_api_key_required_error(locale).into());
    }
    if trimmed.len() > 4096 || trimmed.chars().any(char::is_control) {
        return Err(i18n::models::model_api_key_invalid_error(locale).into());
    }
    Ok(trimmed.to_owned())
}

pub fn validate_model_fields(
    provider: ProviderKind,
    raw_name: &str,
    raw_model_id: &str,
    raw_api_format: Option<ApiFormatKind>,
    raw_context_window: Option<u32>,
    raw_max_output_tokens: Option<u32>,
    locale: LanguageKind,
) -> Result<NormalizedModelFields, String> {
    let model_id = raw_model_id.trim();
    if model_id.is_empty() {
        return Err(i18n::models::model_id_required_error(locale).into());
    }
    if model_id.len() > 256 || model_id.chars().any(char::is_control) {
        return Err(i18n::models::model_id_invalid_error(locale).into());
    }
    let name_trimmed = raw_name.trim();
    let name = if name_trimmed.is_empty() {
        model_id.to_owned()
    } else {
        if name_trimmed.chars().count() > 128 || name_trimmed.chars().any(char::is_control) {
            return Err(i18n::models::model_display_name_invalid_error(locale).into());
        }
        name_trimmed.to_owned()
    };
    let api_format = raw_api_format.unwrap_or_else(|| provider.default_api_format());
    let (default_context_window, default_max_output_tokens) =
        resolve_default_token_limits(provider, Some(api_format), Some(model_id));
    let context_window = raw_context_window.unwrap_or(default_context_window);
    let max_output_tokens = raw_max_output_tokens.unwrap_or(default_max_output_tokens);
    validate_token_limits(context_window, max_output_tokens, locale)?;
    Ok(NormalizedModelFields {
        name,
        model_id: model_id.to_owned(),
        api_format,
        context_window,
        max_output_tokens,
    })
}

pub fn validate_new_connection(
    input: &CreateProviderConnectionInput,
) -> Result<NormalizedNewConnection, String> {
    let locale = input.locale;
    let connection_name = validate_connection_name(&input.connection_name, input.provider, locale)?;
    let base_url = if input.provider.needs_url() {
        Some(validate_http_base_url(&input.base_url, locale)?)
    } else {
        None
    };
    let api_key = validate_api_key(&input.api_key, input.provider.needs_key(), false, locale)?;
    let model = validate_model_fields(
        input.provider,
        &input.model_name,
        &input.model_id,
        input.api_format,
        input.context_window,
        input.max_output_tokens,
        locale,
    )?;
    Ok(NormalizedNewConnection {
        provider: input.provider,
        connection_name,
        base_url,
        api_key,
        model_name: model.name,
        model_id: model.model_id,
        api_format: model.api_format,
        context_window: model.context_window,
        max_output_tokens: model.max_output_tokens,
    })
}

pub fn compute_key_hint(secret: &str) -> String {
    let trimmed = secret.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    if len < 8 {
        return "••••".to_owned();
    }
    let max_visible = len / 2;
    if len < 16 {
        let suffix_len = 2usize.min(max_visible);
        let suffix: String = chars[len - suffix_len..].iter().collect();
        return format!("…{suffix}");
    }
    let suffix_len = 4usize.min(max_visible);
    let prefix_len = 4usize.min(max_visible.saturating_sub(suffix_len));
    let prefix: String = chars[..prefix_len].iter().collect();
    let suffix: String = chars[len - suffix_len..].iter().collect();
    format!("{prefix}…{suffix}")
}
