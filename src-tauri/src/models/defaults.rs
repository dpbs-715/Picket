use super::types::{ApiFormatKind, ProviderKind};
use serde::{Deserialize, de::IgnoredAny};
use std::{collections::HashMap, sync::OnceLock};

const RAW_MODEL_DEFAULTS_JSON: &str = include_str!("../../../shared/model-defaults.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelTokenLimits {
    pub min_context_window: u32,
    pub max_context_window: u32,
    pub min_max_output_tokens: u32,
    pub max_max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelGlobalDefaults {
    pub context_window: u32,
    pub max_output_tokens: u32,
    #[serde(rename = "testConnectionMaxOutputTokens")]
    _test_connection_max_output_tokens: IgnoredAny,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiFormatDefaultEntry {
    #[serde(rename = "label")]
    _label: IgnoredAny,
    #[serde(rename = "short")]
    _short: IgnoredAny,
    pub default_context_window: u32,
    pub default_max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDefaultEntry {
    pub label: String,
    pub default_api_format: ApiFormatKind,
    pub needs_key: bool,
    pub needs_url: bool,
    pub default_base_url: String,
    #[serde(rename = "placeholderUrl")]
    _placeholder_url: IgnoredAny,
    #[serde(rename = "exampleModelId")]
    _example_model_id: IgnoredAny,
    #[serde(rename = "exampleName")]
    _example_name: IgnoredAny,
    pub default_context_window: u32,
    pub default_max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDefaultsConfig {
    pub limits: ModelTokenLimits,
    pub defaults: ModelGlobalDefaults,
    pub api_formats: HashMap<String, ApiFormatDefaultEntry>,
    pub providers: HashMap<String, ProviderDefaultEntry>,
}

pub fn model_defaults() -> &'static ModelDefaultsConfig {
    static CONFIG: OnceLock<ModelDefaultsConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(RAW_MODEL_DEFAULTS_JSON)
            .expect("shared/model-defaults.json must match ModelDefaultsConfig schema")
    })
}

pub fn resolve_default_token_limits(
    provider: ProviderKind,
    api_format: Option<ApiFormatKind>,
) -> (u32, u32) {
    let cfg = model_defaults();
    let Some(provider_cfg) = cfg.providers.get(provider.as_id()) else {
        return (cfg.defaults.context_window, cfg.defaults.max_output_tokens);
    };
    let effective_format = api_format.unwrap_or(provider_cfg.default_api_format);
    if effective_format == provider_cfg.default_api_format {
        return (
            provider_cfg.default_context_window,
            provider_cfg.default_max_output_tokens,
        );
    }
    if let Some(format_cfg) = cfg.api_formats.get(effective_format.as_id()) {
        return (
            format_cfg.default_context_window,
            format_cfg.default_max_output_tokens,
        );
    }
    (
        provider_cfg.default_context_window,
        provider_cfg.default_max_output_tokens,
    )
}

pub fn validate_token_limits(context_window: u32, max_output_tokens: u32) -> Result<(), String> {
    let limits = &model_defaults().limits;
    if context_window < limits.min_context_window || context_window > limits.max_context_window {
        return Err(format!(
            "上下文长度须为 {} 至 {} 之间的整数",
            limits.min_context_window, limits.max_context_window
        ));
    }
    if max_output_tokens < limits.min_max_output_tokens
        || max_output_tokens > limits.max_max_output_tokens
    {
        return Err(format!(
            "最大输出 Token 须为 {} 至 {} 之间的整数",
            limits.min_max_output_tokens, limits.max_max_output_tokens
        ));
    }
    if max_output_tokens > context_window {
        return Err("最大输出 Token 不能超过上下文长度".into());
    }
    Ok(())
}
