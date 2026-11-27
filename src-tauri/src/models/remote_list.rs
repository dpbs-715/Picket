//! Pure-HTTP remote model list fetching executed directly in Rust via `reqwest`.
//!
//! Avoids spawning the Node/Pi Core sidecar process for lightweight metadata listing while
//! preserving provider URL/fallback rules, Anthropic cursor pagination, token limit validation,
//! localized structured errors (`AppError`), strict secret redaction, and cancellation.

use super::{
    defaults::model_defaults,
    types::{ApiFormatKind, ProviderKind, RemoteModelOption},
};
use crate::{
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
};
use reqwest::{
    Client, Url,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue},
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock, time::Duration};
use tokio::sync::oneshot;

const MAX_REMOTE_MODELS: usize = 300;
const MAX_REMOTE_MODEL_PAGES: usize = 3;
const LIST_MODELS_TIMEOUT: Duration = Duration::from_secs(30);
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Clone, Debug)]
pub struct RemoteListSpec {
    pub provider: ProviderKind,
    pub api_format: ApiFormatKind,
    pub base_url: String,
    pub api_key: String,
    pub locale: LanguageKind,
}

#[derive(Debug)]
struct ModelListAttempt {
    url: String,
    headers: HeaderMap,
    anthropic_paginated: bool,
}

#[derive(Debug)]
enum FetchFailure {
    Http {
        status: u16,
        reason: String,
        detail: String,
    },
    Timeout(String),
    Connect(String),
    Message(String),
}

fn shared_http_client() -> Result<&'static Client, AppError> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    let result = CLIENT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Client::builder()
            .timeout(LIST_MODELS_TIMEOUT)
            .build()
            .map_err(|err| format!("Failed to initialize HTTP client: {err}"))
    });
    result
        .as_ref()
        .map_err(|msg| AppError::model(AppErrorCode::ModelConnectionFailed, msg.clone()))
}

fn has_control_chars(text: &str) -> bool {
    text.bytes().any(|b| b < 0x20 || b == 0x7f)
}

fn read_positive_u32(value: Option<&Value>) -> Option<u32> {
    let raw = value?;
    if let Some(u) = raw.as_u64() {
        return (u > 0).then(|| u32::try_from(u).ok()).flatten();
    }
    if let Some(f) = raw.as_f64()
        && f.is_finite()
        && f >= 1.0
        && f <= u32::MAX as f64
    {
        return Some(f.trunc() as u32);
    }
    None
}

fn non_empty_trimmed(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut iter = text.chars();
    let prefix: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn merge_remote_model_options(options: Vec<RemoteModelOption>) -> Vec<RemoteModelOption> {
    let mut by_id: BTreeMap<String, RemoteModelOption> = BTreeMap::new();
    for item in options {
        if item.id.is_empty() || item.id.len() > 256 || has_control_chars(&item.id) {
            continue;
        }
        if let Some(existing) = by_id.get_mut(&item.id) {
            if existing.display_name.is_none() {
                existing.display_name = item.display_name;
            }
            if existing.context_window.is_none() {
                existing.context_window = item.context_window;
            }
            if existing.max_output_tokens.is_none() {
                existing.max_output_tokens = item.max_output_tokens;
            }
        } else {
            if by_id.len() >= MAX_REMOTE_MODELS {
                break;
            }
            by_id.insert(item.id.clone(), item);
        }
    }
    by_id.into_values().collect()
}

pub fn parse_remote_model_options(payload: &Value) -> Result<Vec<RemoteModelOption>, String> {
    let raw_list = if let Some(arr) = payload.as_array() {
        arr
    } else if let Some(arr) = payload.get("data").and_then(Value::as_array) {
        arr
    } else if let Some(arr) = payload.get("models").and_then(Value::as_array) {
        arr
    } else {
        return Err(
            "Unexpected model list response format (expected `data` or `models` array)".into(),
        );
    };

    let limits = &model_defaults().limits;
    let mut parsed = Vec::with_capacity(raw_list.len().min(MAX_REMOTE_MODELS));
    for raw_item in raw_list {
        let Some(item) = raw_item.as_object() else {
            continue;
        };
        let id_raw = non_empty_trimmed(item.get("id"))
            .or_else(|| non_empty_trimmed(item.get("name")))
            .or_else(|| non_empty_trimmed(item.get("model")));
        let Some(id_raw) = id_raw else {
            continue;
        };
        if id_raw.len() > 256 || has_control_chars(id_raw) {
            continue;
        }

        let display_candidate = non_empty_trimmed(item.get("display_name"))
            .or_else(|| non_empty_trimmed(item.get("displayName")))
            .or_else(|| {
                if item.get("id").and_then(Value::as_str).is_some() {
                    non_empty_trimmed(item.get("name")).filter(|name| *name != id_raw)
                } else {
                    None
                }
            });
        let display_name = display_candidate
            .filter(|cand| *cand != id_raw && !has_control_chars(cand))
            .map(|cand| cand.chars().take(128).collect::<String>());

        let top_provider = item.get("top_provider").and_then(Value::as_object);
        let raw_context = read_positive_u32(item.get("context_window"))
            .or_else(|| read_positive_u32(item.get("contextWindow")))
            .or_else(|| read_positive_u32(item.get("context_length")))
            .or_else(|| read_positive_u32(item.get("contextLength")))
            .or_else(|| read_positive_u32(item.get("max_context_length")))
            .or_else(|| read_positive_u32(item.get("input_token_limit")))
            .or_else(|| top_provider.and_then(|tp| read_positive_u32(tp.get("context_length"))));
        let context_window = raw_context
            .filter(|v| *v >= limits.min_context_window && *v <= limits.max_context_window);

        let raw_max_out = read_positive_u32(item.get("max_output_tokens"))
            .or_else(|| read_positive_u32(item.get("maxOutputTokens")))
            .or_else(|| read_positive_u32(item.get("max_completion_tokens")))
            .or_else(|| read_positive_u32(item.get("maxCompletionTokens")))
            .or_else(|| read_positive_u32(item.get("output_token_limit")))
            .or_else(|| {
                top_provider.and_then(|tp| read_positive_u32(tp.get("max_completion_tokens")))
            });
        let max_output_tokens = raw_max_out.filter(|v| {
            *v >= limits.min_max_output_tokens
                && *v <= limits.max_max_output_tokens
                && context_window.is_none_or(|cw| *v <= cw)
        });

        parsed.push(RemoteModelOption {
            id: id_raw.to_owned(),
            display_name,
            context_window,
            max_output_tokens,
        });
    }

    Ok(merge_remote_model_options(parsed))
}

/// Checks whether a URL path ends with a version segment like `/v1`, `/v2`, `/v1beta`, `/v1beta2`.
fn ends_with_version_segment(clean_path: &str) -> bool {
    let segment = clean_path.rsplit('/').next().unwrap_or("");
    let lower = segment.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix('v') else {
        return false;
    };
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return false;
    }
    let tail = &rest[digits_len..];
    if tail.is_empty() {
        return true;
    }
    let Some(beta_tail) = tail.strip_prefix("beta") else {
        return false;
    };
    beta_tail.bytes().all(|b| b.is_ascii_digit())
}

fn header_value(value: &str, locale: LanguageKind) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(value).map_err(|_| {
        AppError::model(
            AppErrorCode::ModelInvalidConfig,
            i18n::models::model_api_key_invalid_error(locale),
        )
    })
}

fn build_model_list_attempts(
    spec: &RemoteListSpec,
    base_url: &str,
    parsed_url: &Url,
) -> Result<Vec<ModelListAttempt>, AppError> {
    let clean_base = base_url.trim_end_matches('/');
    let clean_path = parsed_url.path().trim_end_matches('/');
    let ends_with_version = ends_with_version_segment(clean_path);
    let key = spec.api_key.trim();

    let mut json_headers = HeaderMap::new();
    json_headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

    if spec.provider == ProviderKind::Ollama {
        let origin = parsed_url.origin().ascii_serialization();
        let clean_origin = origin.trim_end_matches('/');
        let primary_url = format!("{clean_base}/models");
        let tags_url = format!("{clean_origin}/api/tags");
        let mut attempts = vec![ModelListAttempt {
            url: primary_url.clone(),
            headers: json_headers.clone(),
            anthropic_paginated: false,
        }];
        if tags_url != primary_url {
            attempts.push(ModelListAttempt {
                url: tags_url,
                headers: json_headers,
                anthropic_paginated: false,
            });
        }
        return Ok(attempts);
    }

    if spec.provider == ProviderKind::Anthropic {
        let models_url = if ends_with_version {
            format!("{clean_base}/models")
        } else {
            format!("{clean_base}/v1/models")
        };
        let mut headers = json_headers;
        headers.insert(
            HeaderName::from_static("x-api-key"),
            header_value(key, spec.locale)?,
        );
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        return Ok(vec![ModelListAttempt {
            url: models_url,
            headers,
            anthropic_paginated: true,
        }]);
    }

    if spec.provider == ProviderKind::Custom && spec.api_format == ApiFormatKind::AnthropicMessages
    {
        let primary_url = if ends_with_version {
            format!("{clean_base}/models")
        } else {
            format!("{clean_base}/v1/models")
        };
        let mut headers = json_headers;
        headers.insert(
            HeaderName::from_static("x-api-key"),
            header_value(key, spec.locale)?,
        );
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(
            AUTHORIZATION,
            header_value(&format!("Bearer {key}"), spec.locale)?,
        );
        let mut attempts = vec![ModelListAttempt {
            url: primary_url,
            headers: headers.clone(),
            anthropic_paginated: true,
        }];
        if !ends_with_version {
            attempts.push(ModelListAttempt {
                url: format!("{clean_base}/models"),
                headers,
                anthropic_paginated: false,
            });
        }
        return Ok(attempts);
    }

    let mut bearer_headers = json_headers;
    bearer_headers.insert(
        AUTHORIZATION,
        header_value(&format!("Bearer {key}"), spec.locale)?,
    );
    let mut attempts = vec![ModelListAttempt {
        url: format!("{clean_base}/models"),
        headers: bearer_headers.clone(),
        anthropic_paginated: false,
    }];
    if !ends_with_version {
        attempts.push(ModelListAttempt {
            url: format!("{clean_base}/v1/models"),
            headers: bearer_headers,
            anthropic_paginated: false,
        });
    }
    Ok(attempts)
}

fn percent_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let unreserved = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn redact_token_prefix(text: &str, prefix: &str, min_tail_len: usize) -> String {
    let bytes = text.as_bytes();
    let prefix_bytes = prefix.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(prefix_bytes) {
            let mut end = idx + prefix_bytes.len();
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'-')
            {
                end += 1;
            }
            if end - (idx + prefix_bytes.len()) >= min_tail_len {
                out.push_str("[REDACTED]");
                idx = end;
                continue;
            }
        }
        let ch = text[idx..].chars().next().unwrap();
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn redact_bearer_tokens(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < text.len() {
        if lower[idx..].starts_with("bearer ") {
            let after_bearer = idx + "bearer ".len();
            let ws_len = text[after_bearer..]
                .bytes()
                .take_while(u8::is_ascii_whitespace)
                .count();
            let token_start = after_bearer + ws_len;
            let token_len = text[token_start..]
                .bytes()
                .take_while(|b| {
                    !b.is_ascii_whitespace()
                        && !matches!(b, b'"' | b'\'' | b',' | b';' | b')' | b']')
                })
                .count();
            if token_len > 0 {
                out.push_str("Bearer [REDACTED]");
                idx = token_start + token_len;
                continue;
            }
        }
        let ch = text[idx..].chars().next().unwrap();
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn redact_url_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < text.len() {
        let lower_rest = text[idx..].to_ascii_lowercase();
        let scheme_len = if lower_rest.starts_with("https://") {
            Some("https://".len())
        } else if lower_rest.starts_with("http://") {
            Some("http://".len())
        } else {
            None
        };
        if let Some(slen) = scheme_len {
            let auth_start = idx + slen;
            let authority_end = text[auth_start..]
                .find(|c: char| c == '/' || c.is_ascii_whitespace())
                .map_or(text.len(), |pos| auth_start + pos);
            if let Some(at_rel) = text[auth_start..authority_end].find('@') {
                out.push_str(&text[idx..auth_start]);
                out.push_str("[REDACTED]@");
                idx = auth_start + at_rel + 1;
                continue;
            }
        }
        let ch = text[idx..].chars().next().unwrap();
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

pub fn redact_provider_error_text(raw: &str, api_key: &str) -> String {
    let mut text = raw.to_owned();
    let trimmed_key = api_key.trim();
    for secret in [api_key, trimmed_key] {
        if !secret.is_empty() {
            text = text.replace(secret, "[REDACTED]");
            if secret.len() >= 4 {
                let encoded = percent_encode_component(secret);
                if encoded != secret {
                    text = text.replace(&encoded, "[REDACTED]");
                    let lower_encoded = encoded.to_ascii_lowercase();
                    if lower_encoded != encoded {
                        text = text.replace(&lower_encoded, "[REDACTED]");
                    }
                }
            }
        }
    }
    text = redact_bearer_tokens(&text);
    text = redact_token_prefix(&text, "sk-ant-", 6);
    text = redact_token_prefix(&text, "sk-", 8);
    text = redact_url_userinfo(&text);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, 220)
}

fn classify_failure(failure: FetchFailure, api_key: &str, locale: LanguageKind) -> AppError {
    match failure {
        FetchFailure::Http {
            status,
            reason,
            detail,
        } => {
            let raw = if detail.is_empty() {
                format!("HTTP {status} {reason}").trim().to_owned()
            } else {
                format!("HTTP {status} {reason}: {detail}")
            };
            let compact = redact_provider_error_text(&raw, api_key);
            match status {
                401 | 403 => AppError::model(
                    AppErrorCode::ModelAuthFailed,
                    i18n::models::model_auth_failed_error(locale, &compact),
                ),
                404 => AppError::model(
                    AppErrorCode::ModelEndpointNotFound,
                    i18n::models::model_endpoint_not_found_error(locale, &compact),
                ),
                402 | 429 => AppError::model(
                    AppErrorCode::ModelRateLimited,
                    i18n::models::model_rate_limited_error(locale, &compact),
                ),
                500..=599 => AppError::model(
                    AppErrorCode::ModelServiceError,
                    i18n::models::model_service_error(locale, &compact),
                ),
                _ => AppError::model(
                    AppErrorCode::ModelRequestFailed,
                    i18n::models::model_request_failed_error(locale, &compact),
                ),
            }
        }
        FetchFailure::Timeout(raw) => {
            let compact = redact_provider_error_text(&raw, api_key);
            AppError::model(
                AppErrorCode::ModelTimeout,
                i18n::models::model_request_timed_out_error(locale, &compact),
            )
        }
        FetchFailure::Connect(raw) => {
            let compact = redact_provider_error_text(&raw, api_key);
            AppError::model(
                AppErrorCode::ModelConnectionFailed,
                i18n::models::model_connection_failed_error(locale, &compact),
            )
        }
        FetchFailure::Message(raw) => {
            let compact = redact_provider_error_text(&raw, api_key);
            AppError::model(
                AppErrorCode::ModelRequestFailed,
                i18n::models::model_request_failed_error(locale, &compact),
            )
        }
    }
}

async fn run_attempts(
    client: &Client,
    attempts: &[ModelListAttempt],
) -> Result<Vec<RemoteModelOption>, FetchFailure> {
    let mut last_error: Option<FetchFailure> = None;

    for (idx, attempt) in attempts.iter().enumerate() {
        let has_fallback = idx + 1 < attempts.len();
        let mut collected: Vec<RemoteModelOption> = Vec::new();
        let mut after_id: Option<String> = None;
        let mut fell_back = false;

        for _page in 0..MAX_REMOTE_MODEL_PAGES {
            let mut target_url = Url::parse(&attempt.url)
                .map_err(|err| FetchFailure::Message(format!("Invalid request URL: {err}")))?;
            if attempt.anthropic_paginated {
                let mut pairs = target_url.query_pairs_mut();
                pairs.append_pair("limit", "100");
                if let Some(cursor) = after_id.as_deref() {
                    pairs.append_pair("after_id", cursor);
                }
            }

            let response = match client
                .get(target_url)
                .headers(attempt.headers.clone())
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(err) => {
                    let failure = if err.is_timeout() {
                        FetchFailure::Timeout(err.to_string())
                    } else if err.is_connect() {
                        FetchFailure::Connect(err.to_string())
                    } else {
                        FetchFailure::Message(err.to_string())
                    };
                    if has_fallback {
                        last_error = Some(failure);
                        collected.clear();
                        fell_back = true;
                        break;
                    }
                    return Err(failure);
                }
            };

            let status = response.status();
            let body_text = match response.text().await {
                Ok(text) => text,
                Err(err) => {
                    let failure = if err.is_timeout() {
                        FetchFailure::Timeout(err.to_string())
                    } else {
                        FetchFailure::Message(err.to_string())
                    };
                    if has_fallback {
                        last_error = Some(failure);
                        collected.clear();
                        fell_back = true;
                        break;
                    }
                    return Err(failure);
                }
            };

            if !status.is_success() {
                let detail = truncate_chars(body_text.trim(), 200);
                let failure = FetchFailure::Http {
                    status: status.as_u16(),
                    reason: status.canonical_reason().unwrap_or("").to_owned(),
                    detail,
                };
                if has_fallback && matches!(status.as_u16(), 404 | 405) {
                    last_error = Some(failure);
                    collected.clear();
                    fell_back = true;
                    break;
                }
                return Err(failure);
            }

            let payload: Value = match serde_json::from_str(&body_text) {
                Ok(val) => val,
                Err(_) => {
                    let failure = FetchFailure::Message(
                        "Provider returned non-JSON response for model list".into(),
                    );
                    if has_fallback {
                        last_error = Some(failure);
                        collected.clear();
                        fell_back = true;
                        break;
                    }
                    return Err(failure);
                }
            };

            let page_models = match parse_remote_model_options(&payload) {
                Ok(models) => models,
                Err(msg) => {
                    let failure = FetchFailure::Message(msg);
                    if has_fallback {
                        last_error = Some(failure);
                        collected.clear();
                        fell_back = true;
                        break;
                    }
                    return Err(failure);
                }
            };
            collected.extend(page_models);

            let has_more = attempt.anthropic_paginated
                && payload.get("has_more").and_then(Value::as_bool) == Some(true)
                && collected.len() < MAX_REMOTE_MODELS;
            let next_cursor = if has_more {
                payload
                    .get("last_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            } else {
                None
            };
            if let Some(cursor) = next_cursor {
                after_id = Some(cursor);
            } else {
                return Ok(merge_remote_model_options(collected));
            }
        }

        if !fell_back && (!collected.is_empty() || !has_fallback) {
            return Ok(merge_remote_model_options(collected));
        }
    }

    Err(last_error.unwrap_or_else(|| {
        FetchFailure::Message("Unable to fetch model list from provider".into())
    }))
}

pub async fn fetch_provider_models(
    spec: &RemoteListSpec,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<Vec<RemoteModelOption>, AppError> {
    let locale = spec.locale;
    let base_url = if spec.base_url.trim().is_empty() {
        spec.provider.default_base_url().trim().to_owned()
    } else {
        spec.base_url.trim().to_owned()
    };
    if base_url.is_empty() {
        return Err(AppError::model(
            AppErrorCode::ModelInvalidConfig,
            i18n::models::url_required_error(locale),
        ));
    }
    let parsed_url = Url::parse(&base_url).map_err(|_| {
        AppError::model(
            AppErrorCode::ModelInvalidConfig,
            i18n::models::url_invalid_http_error(locale),
        )
    })?;
    if !matches!(parsed_url.scheme(), "http" | "https")
        || !parsed_url.username().is_empty()
        || parsed_url.password().is_some()
    {
        return Err(AppError::model(
            AppErrorCode::ModelInvalidConfig,
            i18n::models::url_no_userinfo_http_error(locale),
        ));
    }
    if spec.provider.needs_key() && spec.api_key.trim().is_empty() {
        return Err(AppError::model(
            AppErrorCode::ModelInvalidConfig,
            i18n::models::model_api_key_required_error(locale),
        ));
    }

    let attempts = build_model_list_attempts(spec, &base_url, &parsed_url)?;
    let client = shared_http_client()?;

    let outcome = tokio::select! {
        biased;
        cancel_res = cancel_rx => {
            if cancel_res.is_ok() {
                return Err(AppError::model(
                    AppErrorCode::ModelCancelled,
                    i18n::models::model_test_cancelled_error(locale),
                ));
            }
            tokio::time::timeout(LIST_MODELS_TIMEOUT, run_attempts(client, &attempts)).await
        }
        res = tokio::time::timeout(LIST_MODELS_TIMEOUT, run_attempts(client, &attempts)) => res,
    };

    match outcome {
        Ok(Ok(models)) => Ok(models),
        Ok(Err(failure)) => Err(classify_failure(failure, &spec.api_key, locale)),
        Err(_) => Err(classify_failure(
            FetchFailure::Timeout("request timed out after 30s".into()),
            &spec.api_key,
            locale,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn parses_openai_anthropic_ollama_and_gateway_token_metadata_with_bounds() {
        let parsed = parse_remote_model_options(&json!({
            "data": [
                {
                    "id": "kimi-k3.1-pro",
                    "display_name": "Kimi K3.1 Pro",
                    "context_length": 1_048_576,
                    "max_completion_tokens": 131_072
                },
                {
                    "id": "claude-sonnet-4-20250514",
                    "display_name": "Claude Sonnet 4"
                },
                {
                    "id": "bad-limits-model",
                    "context_window": 99_999_999,
                    "max_output_tokens": 10
                },
                {
                    "id": "kimi-k3.1-pro",
                    "context_length": 128_000
                }
            ]
        }))
        .unwrap();

        assert_eq!(
            parsed,
            vec![
                RemoteModelOption {
                    id: "bad-limits-model".into(),
                    display_name: None,
                    context_window: None,
                    max_output_tokens: None,
                },
                RemoteModelOption {
                    id: "claude-sonnet-4-20250514".into(),
                    display_name: Some("Claude Sonnet 4".into()),
                    context_window: None,
                    max_output_tokens: None,
                },
                RemoteModelOption {
                    id: "kimi-k3.1-pro".into(),
                    display_name: Some("Kimi K3.1 Pro".into()),
                    context_window: Some(1_048_576),
                    max_output_tokens: Some(131_072),
                },
            ]
        );

        let ollama_tags = parse_remote_model_options(&json!({
            "models": [
                { "name": "qwen2.5-coder:14b" },
                { "name": "deepseek-r1:8b" }
            ]
        }))
        .unwrap();
        let ids: Vec<&str> = ollama_tags.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["deepseek-r1:8b", "qwen2.5-coder:14b"]);
    }

    #[test]
    fn version_segment_and_secret_redaction_work_accurately() {
        assert!(ends_with_version_segment("/v1"));
        assert!(ends_with_version_segment("/openai/v2"));
        assert!(ends_with_version_segment("/api/v1beta"));
        assert!(ends_with_version_segment("/api/V1Beta2"));
        assert!(!ends_with_version_segment("/openai"));
        assert!(!ends_with_version_segment("/v1/models"));
        assert!(!ends_with_version_segment("/v"));

        let secret = "sk-moonshot-super-secret-9999";
        let redacted = redact_provider_error_text(
            &format!(
                "HTTP 401: invalid Bearer {secret} at https://user:pass@api.example.com with sk-ant-abcdef123456"
            ),
            secret,
        );
        assert!(!redacted.contains(secret));
        assert!(!redacted.contains("user:pass"));
        assert!(!redacted.contains("sk-ant-abcdef123456"));
        assert!(redacted.contains("Bearer [REDACTED]"));
        assert!(redacted.contains("https://[REDACTED]@api.example.com"));
    }

    async fn spawn_mock_http_server(
        secret: String,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen_auth = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_auth_clone = Arc::clone(&seen_auth);

        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let secret = secret.clone();
                let seen_auth = Arc::clone(&seen_auth_clone);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let Ok(read) = stream.read(&mut buf).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let req = String::from_utf8_lossy(&buf[..read]);
                    let first_line = req.lines().next().unwrap_or("");
                    let target = first_line.split_whitespace().nth(1).unwrap_or("/");
                    let (path, query) = target.split_once('?').unwrap_or((target, ""));

                    let mut authorization = None::<String>;
                    let mut x_api_key = None::<String>;
                    let mut anthropic_version = None::<String>;
                    for line in req.lines().skip(1) {
                        if let Some((name, val)) = line.split_once(':') {
                            let lower = name.trim().to_ascii_lowercase();
                            let trimmed = val.trim().to_owned();
                            match lower.as_str() {
                                "authorization" => authorization = Some(trimmed),
                                "x-api-key" => x_api_key = Some(trimmed),
                                "anthropic-version" => anthropic_version = Some(trimmed),
                                _ => {}
                            }
                        }
                    }

                    if path == "/slow/v1/models" {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let body = r#"{"data":[{"id":"late-model"}]}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }

                    let (status_line, body) = if path == "/openai/v1/models" {
                        if let Some(auth) = authorization {
                            seen_auth.lock().unwrap().push(auth);
                        }
                        (
                            "200 OK",
                            json!({
                                "data": [
                                    { "id": "kimi-k3.1-pro", "context_length": 1_048_576, "max_completion_tokens": 131_072 },
                                    { "id": "kimi-k2.8" }
                                ]
                            })
                            .to_string(),
                        )
                    } else if path == "/anthropic/v1/models" {
                        if x_api_key.as_deref() != Some(secret.as_str())
                            || anthropic_version.is_none()
                        {
                            (
                                "401 Unauthorized",
                                json!({ "error": format!("Invalid key {secret}") }).to_string(),
                            )
                        } else if query.contains("after_id=claude-sonnet-4-20250514") {
                            (
                                "200 OK",
                                json!({
                                    "data": [{ "id": "claude-opus-4-20250514", "display_name": "Claude Opus 4" }],
                                    "has_more": false
                                })
                                .to_string(),
                            )
                        } else {
                            (
                                "200 OK",
                                json!({
                                    "data": [{ "id": "claude-sonnet-4-20250514", "display_name": "Claude Sonnet 4" }],
                                    "has_more": true,
                                    "last_id": "claude-sonnet-4-20250514"
                                })
                                .to_string(),
                            )
                        }
                    } else if path == "/custom-anth/models" {
                        // Fallback when /custom-anth/v1/models returns 404:
                        (
                            "200 OK",
                            json!({ "data": [{ "id": "gateway-claude-sonnet" }] }).to_string(),
                        )
                    } else if path == "/api/tags" {
                        (
                            "200 OK",
                            json!({ "models": [{ "name": "qwen2.5-coder:14b" }] }).to_string(),
                        )
                    } else if path == "/fail/v1/models" {
                        (
                            "401 Unauthorized",
                            json!({ "error": format!("Rejected token {secret}") }).to_string(),
                        )
                    } else {
                        ("404 Not Found", json!({ "error": "not found" }).to_string())
                    };

                    let response = format!(
                        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });

        (format!("http://127.0.0.1:{port}"), seen_auth, handle)
    }

    #[tokio::test]
    async fn fetches_openai_anthropic_ollama_custom_anthropic_redacts_401_and_supports_cancel() {
        let secret = "sk-moonshot-super-secret-9999".to_owned();
        let (origin, seen_auth, server_task) = spawn_mock_http_server(secret.clone()).await;

        // 1. Custom OpenAI-compatible endpoint (auto-falls back from /openai/models -> /openai/v1/models):
        let (_tx1, rx1) = oneshot::channel();
        let openai_models = fetch_provider_models(
            &RemoteListSpec {
                provider: ProviderKind::Custom,
                api_format: ApiFormatKind::OpenaiCompletions,
                base_url: format!("{origin}/openai"),
                api_key: secret.clone(),
                locale: LanguageKind::En,
            },
            rx1,
        )
        .await
        .unwrap();
        assert_eq!(*seen_auth.lock().unwrap(), vec![format!("Bearer {secret}")]);
        assert_eq!(
            openai_models,
            vec![
                RemoteModelOption {
                    id: "kimi-k2.8".into(),
                    display_name: None,
                    context_window: None,
                    max_output_tokens: None,
                },
                RemoteModelOption {
                    id: "kimi-k3.1-pro".into(),
                    display_name: None,
                    context_window: Some(1_048_576),
                    max_output_tokens: Some(131_072),
                },
            ]
        );

        // 2. Anthropic paginated /v1/models:
        let (_tx2, rx2) = oneshot::channel();
        let anthropic_models = fetch_provider_models(
            &RemoteListSpec {
                provider: ProviderKind::Anthropic,
                api_format: ApiFormatKind::AnthropicMessages,
                base_url: format!("{origin}/anthropic"),
                api_key: secret.clone(),
                locale: LanguageKind::En,
            },
            rx2,
        )
        .await
        .unwrap();
        assert_eq!(
            anthropic_models,
            vec![
                RemoteModelOption {
                    id: "claude-opus-4-20250514".into(),
                    display_name: Some("Claude Opus 4".into()),
                    context_window: None,
                    max_output_tokens: None,
                },
                RemoteModelOption {
                    id: "claude-sonnet-4-20250514".into(),
                    display_name: Some("Claude Sonnet 4".into()),
                    context_window: None,
                    max_output_tokens: None,
                },
            ]
        );

        // 3. Custom Anthropic gateway fallback from /custom-anth/v1/models (404) -> /custom-anth/models:
        let (_tx3, rx3) = oneshot::channel();
        let custom_anth_models = fetch_provider_models(
            &RemoteListSpec {
                provider: ProviderKind::Custom,
                api_format: ApiFormatKind::AnthropicMessages,
                base_url: format!("{origin}/custom-anth"),
                api_key: secret.clone(),
                locale: LanguageKind::En,
            },
            rx3,
        )
        .await
        .unwrap();
        assert_eq!(custom_anth_models.len(), 1);
        assert_eq!(custom_anth_models[0].id, "gateway-claude-sonnet");

        // 4. Ollama keyless fallback from /v1/models (404) -> /api/tags:
        let (_tx4, rx4) = oneshot::channel();
        let ollama_models = fetch_provider_models(
            &RemoteListSpec {
                provider: ProviderKind::Ollama,
                api_format: ApiFormatKind::OpenaiCompletions,
                base_url: format!("{origin}/v1"),
                api_key: String::new(),
                locale: LanguageKind::En,
            },
            rx4,
        )
        .await
        .unwrap();
        assert_eq!(
            ollama_models,
            vec![RemoteModelOption {
                id: "qwen2.5-coder:14b".into(),
                display_name: None,
                context_window: None,
                max_output_tokens: None,
            }]
        );

        // 5. 401 error yields structured ModelAuthFailed and redacts echoed API key:
        let (_tx5, rx5) = oneshot::channel();
        let auth_err = fetch_provider_models(
            &RemoteListSpec {
                provider: ProviderKind::Custom,
                api_format: ApiFormatKind::OpenaiCompletions,
                base_url: format!("{origin}/fail/v1"),
                api_key: secret.clone(),
                locale: LanguageKind::En,
            },
            rx5,
        )
        .await
        .unwrap_err();
        assert_eq!(auth_err.category, ErrorCategory::Model);
        assert_eq!(auth_err.code, AppErrorCode::ModelAuthFailed);
        assert!(auth_err.message.contains("Authentication failed (401/403)"));
        assert!(!auth_err.message.contains(&secret));

        // 6. Cancellation terminates an in-flight request immediately with ModelCancelled:
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let slow_spec = RemoteListSpec {
            provider: ProviderKind::Custom,
            api_format: ApiFormatKind::OpenaiCompletions,
            base_url: format!("{origin}/slow/v1"),
            api_key: secret,
            locale: LanguageKind::Zh,
        };
        let slow_fut = fetch_provider_models(&slow_spec, cancel_rx);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = cancel_tx.send(());
        });
        let cancelled = slow_fut.await.unwrap_err();
        assert_eq!(cancelled.code, AppErrorCode::ModelCancelled);
        assert!(cancelled.message.contains("取消"));

        server_task.abort();
    }
}
