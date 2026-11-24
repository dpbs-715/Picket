use super::{keychain::CredentialVault, smoke::LocalModelSseServer, store::ModelStore, types::*};
use crate::{
    foundation::pi::{PiPaths, PiSupervisor},
    i18n::LanguageKind,
    repository::{
        RepositoryStore,
        smoke::{Fixture, git},
        types::{CommitSelection, ReadTarget},
    },
};
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
