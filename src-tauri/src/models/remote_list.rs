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
#[path = "remote_list_tests.rs"]
mod tests;
