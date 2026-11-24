use super::{
    keychain::CredentialVault,
    local_auth::{LocalAuthOutcome, StaticLocalAuthenticator},
    smoke::LocalModelSseServer,
    store::ModelStore,
    types::*,
};
use crate::{
    foundation::pi::{PiPaths, PiSupervisor},
    i18n::LanguageKind,
    repository::{
        RepositoryStore,
        smoke::{Fixture, git},
        types::{CommitSelection, ReadTarget},
    },
};
use std::sync::Arc;
use std::{fs, os::unix::fs::PermissionsExt};

fn make_store(dir: &std::path::Path) -> ModelStore {
    let vault = CredentialVault::with_keychain_file(
        dir.join("picket-test.keychain-db"),
        "picket-test-keychain-pw",
    );
    ModelStore::with_vault(vault)
}

#[tokio::test]
async fn keychain_encrypts_secrets_persists_across_cold_restart_and_deletes_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let store = make_store(temp.path());
    let secret_claude = "sk-ant-real-keychain-secret-alpha-9001";
    let secret_custom = "sk-custom-keychain-secret-beta-9002";

    // Zero-model initial state:
    let (initial_models, initial_active, initial_warn) = store.restore(temp.path()).await.unwrap();
    assert!(initial_models.is_empty());
    assert_eq!(initial_active, None);
    assert_eq!(initial_warn, None);

    // Save Anthropic + Ollama + Custom models:
    let saved_anthropic = store
        .save_model(
            temp.path(),
            &ModelInput {
                id: None,
                provider: ProviderKind::Anthropic,
                api_format: None,
                name: "Claude Sonnet 4".into(),
                model_id: "claude-sonnet-4-20250514".into(),
                base_url: "https://ignored.example.com".into(),
                api_key: format!("  {secret_claude}  "),
                context_window: None,
                max_output_tokens: None,
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(saved_anthropic.model.base_url, None);
    assert_eq!(saved_anthropic.model.context_window, Some(200_000));
    assert_eq!(saved_anthropic.model.max_output_tokens, Some(16_384));
    assert_eq!(saved_anthropic.active_model_id, saved_anthropic.model.id);

    let saved_ollama = store
        .save_model(
            temp.path(),
            &ModelInput {
                id: None,
                provider: ProviderKind::Ollama,
                api_format: None,
                name: " ".into(),
                model_id: "qwen2.5-coder:14b".into(),
                base_url: "http://127.0.0.1:11434/v1/".into(),
                api_key: "must-be-discarded-for-ollama".into(),
                context_window: Some(32_768),
                max_output_tokens: Some(4_096),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(saved_ollama.model.name, "qwen2.5-coder:14b");
    assert_eq!(saved_ollama.model.context_window, Some(32_768));
    assert_eq!(saved_ollama.model.max_output_tokens, Some(4_096));
    assert_eq!(
        saved_ollama.model.base_url.as_deref(),
        Some("http://127.0.0.1:11434/v1")
    );

    let saved_custom = store
        .save_model(
            temp.path(),
            &ModelInput {
                id: None,
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::AnthropicMessages),
                name: "Gateway Model".into(),
                model_id: "coding-v3".into(),
                base_url: "https://gateway.example.com/v1".into(),
                api_key: secret_custom.into(),
                context_window: Some(256_000),
                max_output_tokens: Some(32_768),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        saved_custom.model.api_format,
        Some(ApiFormatKind::AnthropicMessages)
    );

    // S01 check on disk: models.json is 0600, has no plaintext secret; .keychain-db also has no plaintext secret!
    let models_json_path = temp.path().join("models.json");
    assert_eq!(
        fs::metadata(&models_json_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let raw_models_json = fs::read_to_string(&models_json_path).unwrap();
    assert!(!raw_models_json.contains(secret_claude));
    assert!(!raw_models_json.contains(secret_custom));
    assert!(!raw_models_json.contains("must-be-discarded-for-ollama"));
    assert!(!raw_models_json.contains("apiKey"));

    let raw_keychain_bytes = fs::read(temp.path().join("picket-test.keychain-db")).unwrap();
    assert!(
        !raw_keychain_bytes
            .windows(secret_claude.len())
            .any(|w| w == secret_claude.as_bytes()),
        "macOS Keychain file must encrypt password data at rest"
    );

    // Cold restart with brand new store & vault instances:
    let cold_store = make_store(temp.path());
    let (restored, active_id, warn) = cold_store.restore(temp.path()).await.unwrap();
    assert_eq!(warn, None);
    assert_eq!(restored.len(), 3);
    assert_eq!(active_id.as_deref(), Some(saved_custom.model.id.as_str()));

    let resolved_claude = cold_store
        .resolve_runtime_spec(temp.path(), &saved_anthropic.model.id)
        .await
        .unwrap();
    assert_eq!(resolved_claude.api_key, secret_claude);
    assert_eq!(resolved_claude.base_url, "https://api.anthropic.com");

    let resolved_ollama = cold_store
        .resolve_runtime_spec(temp.path(), &saved_ollama.model.id)
        .await
        .unwrap();
    assert_eq!(resolved_ollama.api_key, "");

    // Edit existing custom model without re-entering API key -> preserves Keychain secret & active selection:
    let edited_keep_key = cold_store
        .save_model(
            temp.path(),
            &ModelInput {
                id: Some(saved_custom.model.id.clone()),
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::OpenaiCompletions),
                name: "Gateway Model (Edited)".into(),
                model_id: "coding-v4".into(),
                base_url: "https://gateway2.example.com/v1".into(),
                api_key: String::new(),
                context_window: Some(128_000),
                max_output_tokens: Some(16_384),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(edited_keep_key.model.id, saved_custom.model.id);
    assert_eq!(edited_keep_key.model.name, "Gateway Model (Edited)");
    assert_eq!(edited_keep_key.model.model_id, "coding-v4");
    assert_eq!(
        edited_keep_key.model.api_format,
        Some(ApiFormatKind::OpenaiCompletions)
    );
    let resolved_after_keep = cold_store
        .resolve_runtime_spec(temp.path(), &saved_custom.model.id)
        .await
        .unwrap();
    assert_eq!(resolved_after_keep.api_key, secret_custom);
    assert_eq!(
        resolved_after_keep.api_format,
        ApiFormatKind::OpenaiCompletions
    );

    // Edit existing custom model with a NEW API key -> rotates Keychain entry and deletes old credential_ref:
    let before_rotate_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&models_json_path).unwrap()).unwrap();
    let old_cred_ref = before_rotate_json["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == saved_custom.model.id)
        .unwrap()["credentialRef"]
        .as_str()
        .unwrap()
        .to_owned();
    let rotated_secret = "sk-custom-rotated-secret-9999";
    let _ = cold_store
        .save_model(
            temp.path(),
            &ModelInput {
                id: Some(saved_custom.model.id.clone()),
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::OpenaiResponses),
                name: "Gateway Model (Rotated Key)".into(),
                model_id: "coding-v5".into(),
                base_url: "https://gateway2.example.com/v1".into(),
                api_key: rotated_secret.into(),
                context_window: None,
                max_output_tokens: None,
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let resolved_after_rotate = cold_store
        .resolve_runtime_spec(temp.path(), &saved_custom.model.id)
        .await
        .unwrap();
    assert_eq!(resolved_after_rotate.api_key, rotated_secret);
    let check_vault = CredentialVault::with_keychain_file(
        temp.path().join("picket-test.keychain-db"),
        "picket-test-keychain-pw",
    );
    assert_eq!(
        check_vault.get(&old_cred_ref).unwrap(),
        None,
        "old Keychain credential must be deleted when rotating key"
    );

    // Select & delete down to zero:
    let selected = cold_store
        .select_model(temp.path(), &saved_anthropic.model.id)
        .await
        .unwrap();
    assert_eq!(
        selected.active_model_id.as_deref(),
        Some(saved_anthropic.model.id.as_str())
    );

    let after_del_1 = cold_store
        .delete_model(temp.path(), &saved_anthropic.model.id)
        .await
        .unwrap();
    assert_eq!(
        after_del_1.active_model_id.as_deref(),
        Some(saved_ollama.model.id.as_str())
    );
    assert!(
        cold_store
            .resolve_runtime_spec(temp.path(), &saved_anthropic.model.id)
            .await
            .is_err()
    );

    let _ = cold_store
        .delete_model(temp.path(), &saved_ollama.model.id)
        .await
        .unwrap();
    let empty_after_all = cold_store
        .delete_model(temp.path(), &saved_custom.model.id)
        .await
        .unwrap();
    assert!(empty_after_all.models.is_empty());
    assert_eq!(empty_after_all.active_model_id, None);
}

#[tokio::test]
async fn validates_provider_fields_corrupted_storage_and_unknown_wire_fields() {
    for bad_url in [
        "",
        "file:///etc/passwd",
        "ftp://example.com/v1",
        "https://user:secret@example.com/v1",
        "http://:8080/v1",
        "http://example.com:0/v1",
        "http://example.com:99999/v1",
    ] {
        let err = validate_and_normalize_model_input(&ModelInput {
            id: None,
            provider: ProviderKind::Custom,
            api_format: None,
            name: "Bad".into(),
            model_id: "m1".into(),
            base_url: bad_url.into(),
            api_key: "sk-ok".into(),
            context_window: None,
            max_output_tokens: None,
            locale: LanguageKind::En,
            ..Default::default()
        })
        .unwrap_err();
        assert!(!err.is_empty(), "expected error for {bad_url}");
    }

    assert!(
        validate_and_normalize_model_input(&ModelInput {
            id: None,
            provider: ProviderKind::Openai,
            api_format: None,
            name: "GPT".into(),
            model_id: "gpt-4.1".into(),
            base_url: String::new(),
            api_key: "   ".into(),
            context_window: None,
            max_output_tokens: None,
            locale: LanguageKind::En,
            ..Default::default()
        })
        .is_err()
    );

    assert!(
        validate_and_normalize_model_input(&ModelInput {
            id: None,
            provider: ProviderKind::Openai,
            api_format: None,
            name: "GPT".into(),
            model_id: "gpt-4.1".into(),
            base_url: String::new(),
            api_key: "sk-ok".into(),
            context_window: Some(16_384),
            max_output_tokens: Some(32_768),
            locale: LanguageKind::En,
            ..Default::default()
        })
        .unwrap_err()
        .contains("cannot exceed context window")
    );

    // Localized Chinese validation error (`LanguageKind::Zh`):
    assert!(
        validate_and_normalize_model_input(&ModelInput {
            id: None,
            provider: ProviderKind::Openai,
            api_format: None,
            name: "GPT".into(),
            model_id: "gpt-4.1".into(),
            base_url: String::new(),
            api_key: "sk-ok".into(),
            context_window: Some(16_384),
            max_output_tokens: Some(32_768),
            locale: LanguageKind::Zh,
            ..Default::default()
        })
        .unwrap_err()
        .contains("\u{4e0d}\u{80fd}\u{8d85}\u{8fc7}\u{4e0a}\u{4e0b}\u{6587}\u{957f}\u{5ea6}")
    );

    assert!(
        serde_json::from_value::<ModelInput>(serde_json::json!({
            "provider": "openai",
            "name": "GPT",
            "modelId": "gpt-4.1",
            "baseUrl": "",
            "apiKey": "sk-1",
            "shellCommand": "whoami"
        }))
        .is_err()
    );

    // Corrupted models.json falls back cleanly without overwriting original bytes:
    let temp = tempfile::tempdir().unwrap();
    let corrupted_path = temp.path().join("models.json");
    fs::write(&corrupted_path, b"{broken-json").unwrap();
    let store = make_store(temp.path());
    let (models, active, warning) = store.restore(temp.path()).await.unwrap();
    assert!(models.is_empty());
    assert_eq!(active, None);
    assert!(warning.is_some());
    assert_eq!(fs::read(&corrupted_path).unwrap(), b"{broken-json");
}

#[tokio::test]
async fn runs_connection_tests_exact_partial_generation_review_filtering_and_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    let model_store = make_store(temp.path());
    let server = LocalModelSseServer::start().unwrap();
    model_store.set_endpoint_override(ProviderKind::Anthropic, server.base_url.clone());
    model_store.set_endpoint_override(ProviderKind::Openai, format!("{}/v1", server.base_url));
    model_store.set_endpoint_override(ProviderKind::Deepseek, server.base_url.clone());

    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. Connection tests across all 5 providers:
    for (provider, base_url, api_key) in [
        (ProviderKind::Anthropic, "", "sk-ant-conn-1"),
        (ProviderKind::Openai, "", "sk-openai-conn-2"),
        (ProviderKind::Deepseek, "", "ds-conn-3"),
        (ProviderKind::Ollama, &format!("{}/v1", server.base_url), ""),
        (
            ProviderKind::Custom,
            &format!("{}/v1", server.base_url),
            "custom-conn-4",
        ),
    ] {
        let reply = model_store
            .test_connection(
                &paths,
                &supervisor,
                &ModelInput {
                    id: None,
                    provider,
                    api_format: None,
                    name: String::new(),
                    model_id: "test-model".into(),
                    base_url: base_url.into(),
                    api_key: api_key.into(),
                    context_window: None,
                    max_output_tokens: None,
                    locale: LanguageKind::En,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(reply.summary.contains("Connected"), "{}", reply.summary);
    }

    // 2. HTTP 401 failure sanitizes echoed API key:
    let leaked_key = "sk-never-appear-in-error-776655";
    server.set_failure(Some((
        401,
        serde_json::json!({ "error": { "message": format!("Rejected key {leaked_key}") } })
            .to_string(),
    )));
    let auth_err = model_store
        .test_connection(
            &paths,
            &supervisor,
            &ModelInput {
                id: None,
                provider: ProviderKind::Custom,
                api_format: None,
                name: "Custom".into(),
                model_id: "m".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: leaked_key.into(),
                context_window: None,
                max_output_tokens: None,
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(!auth_err.contains(leaked_key), "{auth_err}");
    server.set_failure(None);

    // 3. Save model in Keychain and test review_scope partial diff scoping, prompt forwarding, filtering, and cancellation on real temporary Git:
    let saved = model_store
        .save_model(
            temp.path(),
            &ModelInput {
                id: None,
                provider: ProviderKind::Custom,
                api_format: None,
                name: "Local SSE Model".into(),
                model_id: "coder-v1".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-saved-in-keychain-only".into(),
                context_window: None,
                max_output_tokens: None,
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fixture = Fixture::new().unwrap();
    fs::write(fixture.root.join("logo.bin"), b"\x00\x01\x02\xff").unwrap();
    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "m3-session".into(),
        scope_version: tree.status_version.clone(),
    };
    let sel_of = |path: &str| -> CommitSelection {
        let file = tree.files.iter().find(|f| f.path == path).unwrap();
        CommitSelection {
            file_id: file.id.clone(),
            commit_token: file.commit_token.clone(),
        }
    };

    // Review with lockfile + binary + partial file + user prompt -> lockfile & binary skipped, partial staged diff reviewed with prompt forwarded:
    let mut streamed_deltas = 0usize;
    let rev_reply = model_store
        .review_scope(
            temp.path(),
            &paths,
            &supervisor,
            &repo_store,
            &ReviewScopeRequest {
                request_id: "rev-m3-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![
                    sel_of("part.txt"),
                    sel_of("pnpm-lock.yaml"),
                    sel_of("logo.bin"),
                ],
                prompt: "Focus on staged config key invariants".into(),
                changed_paths: None,
                locale: LanguageKind::En,
            },
            |event| {
                if matches!(event, ModelStreamEvent::Delta { .. }) {
                    streamed_deltas += 1;
                }
                Ok(())
            },
        )
        .await
        .unwrap();
    assert!(streamed_deltas >= 1);
    let last_rev_req = server.requests().pop().unwrap();
    assert!(
        last_rev_req.body.contains("+first=2")
            && !last_rev_req.body.contains("second=3")
            && last_rev_req
                .body
                .contains("Focus on staged config key invariants"),
        "partial file review must send ONLY staged diff plus user review prompt: {}",
        last_rev_req.body
    );
    assert_eq!(
        rev_reply.reviewed_file_ids,
        vec![sel_of("part.txt").file_id]
    );
    assert_eq!(rev_reply.findings.len(), 1);
    assert_eq!(rev_reply.findings[0].path, "part.txt");
    assert_eq!(rev_reply.findings[0].severity, "high");
    assert_eq!(rev_reply.skipped_paths.len(), 2);
    assert!(
        rev_reply
            .skipped_paths
            .iter()
            .any(|p| p.starts_with("pnpm-lock.yaml") && p.contains("lockfile"))
    );
    assert!(
        rev_reply
            .skipped_paths
            .iter()
            .any(|p| p.starts_with("logo.bin") && p.contains("binary file"))
    );

    // Selecting ONLY lockfile + binary makes 0 model HTTP calls and localizes skipped_paths for `LanguageKind::Zh`:
    let count_before = server.requests().len();
    let skip_only_reply = model_store
        .review_scope(
            temp.path(),
            &paths,
            &supervisor,
            &repo_store,
            &ReviewScopeRequest {
                request_id: "rev-skip-only".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("pnpm-lock.yaml"), sel_of("logo.bin")],
                prompt: String::new(),
                changed_paths: None,
                locale: LanguageKind::Zh,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(server.requests().len(), count_before);
    assert!(skip_only_reply.reviewed_file_ids.is_empty());
    assert_eq!(skip_only_reply.skipped_paths.len(), 2);
    assert!(
        skip_only_reply
            .skipped_paths
            .iter()
            .any(|p| p.starts_with("pnpm-lock.yaml") && p.contains("\u{9501}\u{6587}\u{4ef6}"))
    );
    assert!(skip_only_reply.skipped_paths.iter().any(
        |p| p.starts_with("logo.bin") && p.contains("\u{4e8c}\u{8fdb}\u{5236}\u{6587}\u{4ef6}")
    ));

    // Editing unstaged half of partial `part.txt` does NOT invalidate its staged scope,
    // whereas modifying selected unstaged `\u{65b0} \u{6587}\u{4ef6}.ts` DOES pause the stale scope:
    fs::write(fixture.root.join("part.txt"), "first=2\nsecond=999\n").unwrap();
    assert!(
        model_store
            .review_scope(
                temp.path(),
                &paths,
                &supervisor,
                &repo_store,
                &ReviewScopeRequest {
                    request_id: "rev-partial-draft-ok".into(),
                    target: target.clone(),
                    model_id: saved.model.id.clone(),
                    selections: vec![sel_of("part.txt")],
                    prompt: String::new(),
                    changed_paths: None,
                    locale: LanguageKind::En,
                },
                |_| Ok(()),
            )
            .await
            .is_ok()
    );

    fs::write(
        fixture.root.join("\u{65b0} \u{6587}\u{4ef6}.ts"),
        "export const changed = 2;\n",
    )
    .unwrap();
    let stale_err = model_store
        .review_scope(
            temp.path(),
            &paths,
            &supervisor,
            &repo_store,
            &ReviewScopeRequest {
                request_id: "rev-stale".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("\u{65b0} \u{6587}\u{4ef6}.ts")],
                prompt: String::new(),
                changed_paths: None,
                locale: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(stale_err.contains("has changed"), "{stale_err}");

    // Cancellation during slow stream settles cleanly:
    server.set_slow_delay_ms(3_000);
    let cancel_sup = supervisor.clone();
    let cancel_err = model_store
        .review_scope(
            temp.path(),
            &paths,
            &supervisor,
            &repo_store,
            &ReviewScopeRequest {
                request_id: "rev-cancel-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt")],
                prompt: String::new(),
                changed_paths: None,
                locale: LanguageKind::En,
            },
            move |event| {
                if matches!(event, ModelStreamEvent::Delta { .. }) {
                    let _ = cancel_sup.cancel("rev-cancel-1");
                }
                Ok(())
            },
        )
        .await
        .unwrap_err();
    assert!(cancel_err.contains("cancelled"), "{cancel_err}");

    // Ensure Git index remained completely untouched throughout all AI operations:
    assert_eq!(
        git(&fixture.root, &["show", ":part.txt"]).unwrap(),
        b"first=2\nsecond=1\n"
    );
}

#[test]
fn computes_key_hints_and_canonical_endpoints_safely() {
    assert_eq!(compute_key_hint(""), "••••");
    assert_eq!(compute_key_hint("short77"), "••••");
    assert_eq!(compute_key_hint("12345678"), "…78");
    assert_eq!(compute_key_hint("abcdefghijklmno"), "…no");
    assert_eq!(compute_key_hint("0123456789abcdef"), "0123…cdef");
    assert_eq!(
        compute_key_hint("  fake-shared-key-alpha-9876  "),
        "fake…9876"
    );
    assert_eq!(
        compute_key_hint("\u{5bc6}\u{94a5}\u{6d4b}\u{8bd5}abcdefghijkl1234"),
        "\u{5bc6}\u{94a5}\u{6d4b}\u{8bd5}…1234"
    );

    for len in 0..=64 {
        let sample: String = (0..len)
            .map(|idx| char::from(b'a' + (idx % 26) as u8))
            .collect();
        let hint = compute_key_hint(&sample);
        let visible_chars = hint.chars().filter(|ch| *ch != '•' && *ch != '…').count();
        assert!(
            visible_chars <= len / 2,
            "len={len} hint={hint} exposed {visible_chars} chars"
        );
    }

    assert!(same_credential_endpoint(
        ProviderKind::Openai,
        None,
        ProviderKind::Openai,
        Some("https://ignored.example.com"),
    ));
    assert!(!same_credential_endpoint(
        ProviderKind::Openai,
        None,
        ProviderKind::Deepseek,
        None,
    ));
    assert!(same_credential_endpoint(
        ProviderKind::Custom,
        Some("https://API.Example.com/v1/"),
        ProviderKind::Custom,
        Some("https://api.example.com/v1"),
    ));
    assert!(!same_credential_endpoint(
        ProviderKind::Custom,
        Some("https://api.example.com/v1"),
        ProviderKind::Custom,
        Some("https://other.example.com/v1"),
    ));
}

#[tokio::test]
async fn shares_credentials_refcounts_deletes_updates_peers_enforces_endpoints_and_rolls_back_atomically()
 {
    let temp = tempfile::tempdir().unwrap();
    let keychain_path = temp.path().join("picket-shared.keychain-db");
    let vault = CredentialVault::with_keychain_file(keychain_path.clone(), "picket-shared-pw");
    let store = ModelStore::with_vault(vault.clone());
    let server = LocalModelSseServer::start().unwrap();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    let fake_key_a = "fake-key-shared-gateway-0001";
    let base_url_a = format!("{}/v1", server.base_url);

    let created_a = store
        .save_model(
            temp.path(),
            &ModelInput {
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::OpenaiCompletions),
                name: "Gateway Primary".into(),
                model_id: "coder-primary".into(),
                base_url: format!("{base_url_a}/"),
                api_key: fake_key_a.into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(created_a.model.key_hint.as_deref(), Some("fake…0001"));
    assert_eq!(created_a.model.shared_key_count, Some(1));
    assert_eq!(created_a.active_model_id, created_a.model.id);

    // Cold restore must NOT read Keychain (`read_count() == 0`):
    let cold_vault = CredentialVault::with_keychain_file(keychain_path, "picket-shared-pw");
    let cold_store = ModelStore::with_vault(cold_vault.clone());
    let (restored_initial, _, _) = cold_store.restore(temp.path()).await.unwrap();
    assert_eq!(
        cold_vault.read_count(),
        0,
        "Opening settings / restoring catalog must never read Keychain"
    );
    assert_eq!(restored_initial[0].key_hint.as_deref(), Some("fake…0001"));

    // Duplicate B sharing A's key (whitespace/slash normalized URL, `keep_active_model: true`):
    let normalized_match_url = format!("  {base_url_a}///  ");
    let conn_shared = store
        .test_connection_with_storage(
            Some(temp.path()),
            &paths,
            &supervisor,
            &ModelInput {
                source_model_id: Some(created_a.model.id.clone()),
                keep_active_model: true,
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::OpenaiCompletions),
                name: "Gateway Primary Copy".into(),
                model_id: "coder-fast".into(),
                base_url: normalized_match_url.clone(),
                api_key: String::new(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(conn_shared.summary.contains("coder-fast"));

    let duplicated_b = store
        .save_model(
            temp.path(),
            &ModelInput {
                source_model_id: Some(created_a.model.id.clone()),
                keep_active_model: true,
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::OpenaiCompletions),
                name: "Gateway Primary Copy".into(),
                model_id: "coder-fast".into(),
                base_url: normalized_match_url,
                api_key: String::new(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        duplicated_b.active_model_id, created_a.model.id,
        "Duplicating a model must not change activeModelId"
    );
    assert_eq!(duplicated_b.models.len(), 2);
    assert!(
        duplicated_b
            .models
            .iter()
            .all(|m| m.shared_key_count == Some(2) && m.key_hint.as_deref() == Some("fake…0001"))
    );

    let disk_v2: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(temp.path().join("models.json")).unwrap())
            .unwrap();
    assert_eq!(disk_v2["version"], 2);
    assert_eq!(disk_v2["credentials"].as_array().unwrap().len(), 1);
    let cred_a = disk_v2["models"][0]["credentialRef"]
        .as_str()
        .unwrap()
        .to_owned();
    let cred_b = disk_v2["models"][1]["credentialRef"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(cred_a, cred_b);

    // Endpoint mismatch when duplicating (URL or provider changed with blank key) is rejected on both test & save:
    assert!(
        store
            .test_connection_with_storage(
                Some(temp.path()),
                &paths,
                &supervisor,
                &ModelInput {
                    source_model_id: Some(created_a.model.id.clone()),
                    provider: ProviderKind::Custom,
                    name: "Wrong Host".into(),
                    model_id: "coder-x".into(),
                    base_url: "https://other-host.example.com/v1".into(),
                    api_key: String::new(),
                    locale: LanguageKind::En,
                    ..Default::default()
                },
            )
            .await
            .is_err()
    );
    assert!(
        store
            .save_model(
                temp.path(),
                &ModelInput {
                    source_model_id: Some(created_a.model.id.clone()),
                    provider: ProviderKind::Custom,
                    name: "Wrong Host".into(),
                    model_id: "coder-x".into(),
                    base_url: "https://other-host.example.com/v1".into(),
                    api_key: String::new(),
                    locale: LanguageKind::En,
                    ..Default::default()
                },
            )
            .await
            .is_err()
    );
    assert!(
        store
            .save_model(
                temp.path(),
                &ModelInput {
                    source_model_id: Some(created_a.model.id.clone()),
                    provider: ProviderKind::Openai,
                    name: "Wrong Provider".into(),
                    model_id: "gpt-4.1".into(),
                    base_url: String::new(),
                    api_key: String::new(),
                    locale: LanguageKind::En,
                    ..Default::default()
                },
            )
            .await
            .is_err()
    );
    assert!(
        store
            .save_model(
                temp.path(),
                &ModelInput {
                    source_model_id: Some("model-does-not-exist".into()),
                    provider: ProviderKind::Custom,
                    name: "Missing Source".into(),
                    model_id: "coder-x".into(),
                    base_url: base_url_a.clone(),
                    api_key: String::new(),
                    locale: LanguageKind::En,
                    ..Default::default()
                },
            )
            .await
            .is_err()
    );

    // Edit shared model A with `UpdateAll` -> rotates key for BOTH A and B and removes old cred:
    let fake_key_rotated_all = "fake-key-shared-gateway-2222";
    let updated_all = store
        .save_model(
            temp.path(),
            &ModelInput {
                id: Some(created_a.model.id.clone()),
                shared_key_update_scope: Some(SharedKeyUpdateScope::UpdateAll),
                provider: ProviderKind::Custom,
                api_format: Some(ApiFormatKind::OpenaiCompletions),
                name: "Gateway Primary".into(),
                model_id: "coder-primary".into(),
                base_url: base_url_a.clone(),
                api_key: fake_key_rotated_all.into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        updated_all
            .models
            .iter()
            .all(|m| m.shared_key_count == Some(2) && m.key_hint.as_deref() == Some("fake…2222"))
    );
    assert_eq!(vault.get(&cred_a).unwrap(), None);
    assert_eq!(
        store
            .resolve_runtime_spec(temp.path(), &duplicated_b.model.id)
            .await
            .unwrap()
            .api_key,
        fake_key_rotated_all
    );

    // Editing shared model B changing endpoint without a new key is rejected;
    // changing endpoint WITH a new key forces `DetachCurrent` even if `UpdateAll` is sent:
    assert!(
        store
            .save_model(
                temp.path(),
                &ModelInput {
                    id: Some(duplicated_b.model.id.clone()),
                    provider: ProviderKind::Custom,
                    name: "Gateway Moved".into(),
                    model_id: "coder-fast".into(),
                    base_url: "https://moved-gateway.example.com/v1".into(),
                    api_key: String::new(),
                    locale: LanguageKind::En,
                    ..Default::default()
                },
            )
            .await
            .is_err()
    );
    let fake_key_detached = "fake-key-detached-only-3333";
    let after_endpoint_detach = store
        .save_model(
            temp.path(),
            &ModelInput {
                id: Some(duplicated_b.model.id.clone()),
                shared_key_update_scope: Some(SharedKeyUpdateScope::UpdateAll),
                provider: ProviderKind::Custom,
                name: "Gateway Moved".into(),
                model_id: "coder-fast".into(),
                base_url: "https://moved-gateway.example.com/v1".into(),
                api_key: fake_key_detached.into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        after_endpoint_detach
            .models
            .iter()
            .all(|m| m.shared_key_count == Some(1))
    );
    assert_eq!(
        store
            .resolve_runtime_spec(temp.path(), &created_a.model.id)
            .await
            .unwrap()
            .api_key,
        fake_key_rotated_all
    );
    assert_eq!(
        store
            .resolve_runtime_spec(temp.path(), &duplicated_b.model.id)
            .await
            .unwrap()
            .api_key,
        fake_key_detached
    );

    // Re-duplicate C from A (`shared_key_count == 2`), then test `DetachCurrent` on same endpoint & refcounted deletion:
    let duplicated_c = store
        .save_model(
            temp.path(),
            &ModelInput {
                source_model_id: Some(created_a.model.id.clone()),
                keep_active_model: true,
                provider: ProviderKind::Custom,
                name: "Gateway Primary Copy 2".into(),
                model_id: "coder-review".into(),
                base_url: base_url_a.clone(),
                api_key: String::new(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let duplicated_d = store
        .save_model(
            temp.path(),
            &ModelInput {
                source_model_id: Some(created_a.model.id.clone()),
                keep_active_model: true,
                provider: ProviderKind::Custom,
                name: "Gateway Primary Copy 3".into(),
                model_id: "coder-plan".into(),
                base_url: base_url_a.clone(),
                api_key: String::new(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let a_peer_count = duplicated_d
        .models
        .iter()
        .find(|m| m.id == created_a.model.id)
        .unwrap()
        .shared_key_count;
    assert_eq!(a_peer_count, Some(3));

    // Detach D on same endpoint:
    let fake_key_d = "fake-key-only-d-model-4444";
    let after_detach_d = store
        .save_model(
            temp.path(),
            &ModelInput {
                id: Some(duplicated_d.model.id.clone()),
                shared_key_update_scope: Some(SharedKeyUpdateScope::DetachCurrent),
                provider: ProviderKind::Custom,
                name: "Gateway Primary Copy 3".into(),
                model_id: "coder-plan".into(),
                base_url: base_url_a.clone(),
                api_key: fake_key_d.into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(after_detach_d.model.shared_key_count, Some(1));
    assert_eq!(
        after_detach_d
            .models
            .iter()
            .find(|m| m.id == created_a.model.id)
            .unwrap()
            .shared_key_count,
        Some(2)
    );

    // Delete A -> C still shares the credential (`shared_key_count == 1`) and still resolves `fake_key_rotated_all`:
    let shared_cred_ac = {
        let parsed: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(temp.path().join("models.json")).unwrap())
                .unwrap();
        parsed["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == created_a.model.id)
            .unwrap()["credentialRef"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let after_del_a = store
        .delete_model(temp.path(), &created_a.model.id)
        .await
        .unwrap();
    assert_eq!(
        after_del_a
            .models
            .iter()
            .find(|m| m.id == duplicated_c.model.id)
            .unwrap()
            .shared_key_count,
        Some(1)
    );
    assert_eq!(
        vault.get(&shared_cred_ac).unwrap().as_deref(),
        Some(fake_key_rotated_all)
    );

    // Delete C (the last holder of `shared_cred_ac`) -> Keychain item IS deleted!
    let _ = store
        .delete_model(temp.path(), &duplicated_c.model.id)
        .await
        .unwrap();
    assert_eq!(
        vault.get(&shared_cred_ac).unwrap(),
        None,
        "Keychain item must be deleted once reference count drops to zero"
    );

    // Rollback test: if persisting `models.json` fails, newly written Keychain item is rolled back:
    let accounts_before_rollback = vault.live_account_count();
    let broken_dir = temp.path().join("unwritable-storage");
    fs::create_dir_all(broken_dir.join("models.json")).unwrap();
    let rollback_err = store
        .save_model(
            &broken_dir,
            &ModelInput {
                provider: ProviderKind::Openai,
                name: "Should Roll Back".into(),
                model_id: "gpt-4.1".into(),
                base_url: String::new(),
                api_key: "fake-key-must-rollback-5555".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(!rollback_err.is_empty());
    assert_eq!(
        vault.live_account_count(),
        accounts_before_rollback,
        "Failed models.json write must roll back newly written Keychain item without orphans"
    );
    // Clean up remaining B and D and verify Keychain has zero residual `cred-` entries:
    let _ = store
        .delete_model(temp.path(), &duplicated_b.model.id)
        .await
        .unwrap();
    let _ = store
        .delete_model(temp.path(), &duplicated_d.model.id)
        .await
        .unwrap();
    assert_eq!(vault.live_account_count(), 0);
}

#[tokio::test]
async fn gates_reveal_model_api_key_behind_local_authentication_without_touching_keychain_on_cancel_or_failure()
 {
    let temp = tempfile::tempdir().unwrap();
    let keychain_path = temp.path().join("picket-reveal.keychain-db");
    let vault = CredentialVault::with_keychain_file(keychain_path, "picket-reveal-pw");
    let auth = Arc::new(StaticLocalAuthenticator::new(LocalAuthOutcome::Cancelled));
    let store = ModelStore::with_vault_and_authenticator(vault.clone(), auth.clone());

    let fake_secret = "fake-reveal-key-secret-778899";
    let saved = store
        .save_model(
            temp.path(),
            &ModelInput {
                provider: ProviderKind::Anthropic,
                name: "Claude Reveal Test".into(),
                model_id: "claude-sonnet-4-20250514".into(),
                base_url: String::new(),
                api_key: fake_secret.into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(vault.read_count(), 0);

    // 1. Cancelled authentication -> does NOT read Keychain and does NOT include secret:
    let cancelled = store
        .reveal_model_api_key(temp.path(), &saved.model.id, LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(vault.read_count(), 0);
    match &cancelled.result {
        ModelRevealKeyOutcome::Cancelled { message } => {
            assert!(!message.contains(fake_secret));
        }
        other => panic!("expected Cancelled, got {other:?}"),
    }
    let cancelled_json = serde_json::to_string(&cancelled).unwrap();
    assert!(!cancelled_json.contains("apiKey"));
    assert!(!cancelled_json.contains(fake_secret));

    // 2. Failed authentication (Chinese locale) -> does NOT read Keychain and localizes reason:
    auth.set_outcome(LocalAuthOutcome::Failed("Auth failed.".into()));
    let failed = store
        .reveal_model_api_key(temp.path(), &saved.model.id, LanguageKind::Zh)
        .await
        .unwrap();
    assert_eq!(vault.read_count(), 0);
    match &failed.result {
        ModelRevealKeyOutcome::Failed { message } => {
            assert_eq!(message, "Auth failed.");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    let calls = auth.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].0.contains("Claude Reveal Test"));
    assert_eq!(calls[0].1, LanguageKind::En);
    assert!(calls[1].0.contains("Claude Reveal Test") && calls[1].0.contains("\u{67e5}\u{770b}"));
    assert_eq!(calls[1].1, LanguageKind::Zh);

    // 3. Authenticated -> reads Keychain once and returns full secret + keyHint:
    auth.set_outcome(LocalAuthOutcome::Authenticated);
    let revealed = store
        .reveal_model_api_key(temp.path(), &saved.model.id, LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(vault.read_count(), 1);
    assert_eq!(
        revealed.result,
        ModelRevealKeyOutcome::Authenticated {
            api_key: fake_secret.into(),
            key_hint: "fake…8899".into(),
        }
    );
}
