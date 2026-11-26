use super::types::{ApiFormatKind, ProviderKind};
use crate::i18n::{self, LanguageKind};
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
pub struct ModelPresetEntry {
    pub pattern: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDefaultsConfig {
    pub limits: ModelTokenLimits,
    pub defaults: ModelGlobalDefaults,
    pub api_formats: HashMap<String, ApiFormatDefaultEntry>,
    pub providers: HashMap<String, ProviderDefaultEntry>,
    #[serde(default)]
    pub model_presets: Vec<ModelPresetEntry>,
}

pub fn model_defaults() -> &'static ModelDefaultsConfig {
    static CONFIG: OnceLock<ModelDefaultsConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(RAW_MODEL_DEFAULTS_JSON)
            .expect("shared/model-defaults.json must match ModelDefaultsConfig schema")
    })
}

fn matches_preset_pattern(candidate: &str, pattern: &str) -> bool {
    if candidate == pattern {
        return true;
    }
    let Some(rest) = candidate.strip_prefix(pattern) else {
        return false;
    };
    matches!(
        rest.as_bytes().first(),
        Some(b'-' | b'_' | b'.' | b':' | b'@')
    )
}

pub fn resolve_model_preset_token_limits(model_id: &str) -> Option<(u32, u32)> {
    let normalized = model_id.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let tail = normalized
        .rsplit_once('/')
        .map(|(_, segment)| segment)
        .filter(|segment| !segment.is_empty());
    for preset in &model_defaults().model_presets {
        let pattern = preset.pattern.trim().to_ascii_lowercase();
        if pattern.is_empty() {
            continue;
        }
        if matches_preset_pattern(&normalized, &pattern)
            || tail.is_some_and(|segment| matches_preset_pattern(segment, &pattern))
        {
            return Some((preset.context_window, preset.max_output_tokens));
        }
    }
    None
}

pub fn resolve_default_token_limits(
    provider: ProviderKind,
    api_format: Option<ApiFormatKind>,
    model_id: Option<&str>,
) -> (u32, u32) {
    if let Some(id) = model_id
        && let Some(preset) = resolve_model_preset_token_limits(id)
    {
        return preset;
    }
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

pub fn validate_token_limits(
    context_window: u32,
    max_output_tokens: u32,
    locale: LanguageKind,
) -> Result<(), String> {
    let limits = &model_defaults().limits;
    if context_window < limits.min_context_window || context_window > limits.max_context_window {
        return Err(i18n::models::token_context_range_error(
            locale,
            limits.min_context_window,
            limits.max_context_window,
        ));
    }
    if max_output_tokens < limits.min_max_output_tokens
        || max_output_tokens > limits.max_max_output_tokens
    {
        return Err(i18n::models::token_output_range_error(
            locale,
            limits.min_max_output_tokens,
            limits.max_max_output_tokens,
        ));
    }
    if max_output_tokens > context_window {
        return Err(i18n::models::token_output_exceeds_context_error(locale).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_kimi_model_presets_and_falls_back_cleanly() {
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("kimi-k3.1-pro")
            ),
            (1_048_576, 131_072)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::AnthropicMessages),
                Some("moonshotai/kimi-k3.1-pro-highspeed")
            ),
            (1_048_576, 131_072)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("kimi-k2.8")
            ),
            (1_048_576, 131_072)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("kimi-k2.7-code")
            ),
            (262_144, 32_768)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("kimi-k2.6")
            ),
            (262_144, 32_768)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("kimi-for-coding")
            ),
            (262_144, 32_768)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("moonshot-v1-8k-vision-preview")
            ),
            (8_192, 4_096)
        );
        assert_eq!(
            resolve_default_token_limits(
                ProviderKind::Custom,
                Some(ApiFormatKind::OpenaiCompletions),
                Some("moonshot-v1-80k")
            ),
            (128_000, 16_384)
        );
        // Every preset in shared/model-defaults.json must itself pass token limit validation:
        for preset in &model_defaults().model_presets {
            assert!(
                validate_token_limits(
                    preset.context_window,
                    preset.max_output_tokens,
                    LanguageKind::En
                )
                .is_ok(),
                "Preset {} has invalid token limits",
                preset.pattern
            );
        }
    }
}
