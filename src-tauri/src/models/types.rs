use super::defaults::{model_defaults, resolve_default_token_limits, validate_token_limits};
use crate::{
    i18n::{self, LanguageKind, deserialize_language_or_default},
    repository::types::{CommitSelection, ReadTarget},
};
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
pub struct SavedModelConfiguration {
    pub id: String,
    pub name: String,
    pub provider: ProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<ApiFormatKind>,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
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
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug)]
pub struct NormalizedModelInput {
    pub provider: ProviderKind,
    pub api_format: ApiFormatKind,
    pub name: String,
    pub model_id: String,
    pub base_url: Option<String>,
    pub api_key: String,
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
        return Err(i18n::url_required_error(locale).into());
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(i18n::url_invalid_http_error(locale).into());
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| i18n::url_no_userinfo_http_error(locale))?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(i18n::url_invalid_http_error(locale).into());
    }
    if authority.contains('@') {
        return Err(i18n::url_no_userinfo_http_error(locale).into());
    }
    if let Some(after_open) = authority.strip_prefix('[') {
        let Some(close_idx) = after_open.find(']') else {
            return Err(i18n::url_invalid_http_error(locale).into());
        };
        let host = &after_open[..close_idx];
        let remainder = &after_open[close_idx + 1..];
        if host.is_empty() {
            return Err(i18n::url_invalid_http_error(locale).into());
        }
        if let Some(port) = remainder.strip_prefix(':') {
            if !port.parse::<u16>().is_ok_and(|p| p >= 1) {
                return Err(i18n::url_invalid_http_error(locale).into());
            }
        } else if !remainder.is_empty() {
            return Err(i18n::url_invalid_http_error(locale).into());
        }
    } else if let Some((host, port)) = authority.split_once(':')
        && (host.is_empty() || port.contains(':') || !port.parse::<u16>().is_ok_and(|p| p >= 1))
    {
        return Err(i18n::url_invalid_http_error(locale).into());
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
    let locale = input.locale;
    if let Some(raw) = &input.id
        && (raw.trim().len() > 128 || raw.chars().any(char::is_control))
    {
        return Err(i18n::model_invalid_id_record_error(locale).into());
    }

    let model_id = input.model_id.trim();
    if model_id.is_empty() {
        return Err(i18n::model_id_required_error(locale).into());
    }
    if model_id.len() > 256 || model_id.chars().any(char::is_control) {
        return Err(i18n::model_id_invalid_error(locale).into());
    }
    let name_trimmed = input.name.trim();
    let name = if name_trimmed.is_empty() {
        model_id.to_owned()
    } else {
        if name_trimmed.chars().count() > 128 || name_trimmed.chars().any(char::is_control) {
            return Err(i18n::model_display_name_invalid_error(locale).into());
        }
        name_trimmed.to_owned()
    };

    let base_url = if input.provider.needs_url() {
        Some(validate_http_base_url(&input.base_url, locale)?)
    } else {
        None
    };

    let api_key = if input.provider.needs_key() {
        let trimmed = input.api_key.trim();
        if trimmed.is_empty() {
            if allow_empty_key {
                String::new()
            } else {
                return Err(i18n::model_api_key_required_error(locale).into());
            }
        } else {
            if trimmed.len() > 4096 || trimmed.chars().any(char::is_control) {
                return Err(i18n::model_api_key_invalid_error(locale).into());
            }
            trimmed.to_owned()
        }
    } else {
        String::new()
    };

    let api_format = input
        .api_format
        .unwrap_or_else(|| input.provider.default_api_format());
    let (default_context_window, default_max_output_tokens) =
        resolve_default_token_limits(input.provider, Some(api_format));
    let context_window = input.context_window.unwrap_or(default_context_window);
    let max_output_tokens = input.max_output_tokens.unwrap_or(default_max_output_tokens);
    validate_token_limits(context_window, max_output_tokens, locale)?;

    Ok(NormalizedModelInput {
        provider: input.provider,
        api_format,
        name,
        model_id: model_id.to_owned(),
        base_url,
        api_key,
        context_window,
        max_output_tokens,
    })
}
