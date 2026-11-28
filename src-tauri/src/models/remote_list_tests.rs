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
                    if x_api_key.as_deref() != Some(secret.as_str()) || anthropic_version.is_none()
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
