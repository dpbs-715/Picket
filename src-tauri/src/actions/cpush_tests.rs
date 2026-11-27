//! End-to-end integration tests for the unified new-engine C-Push workflow:
//! 1-turn fast path, live `message_user` Q&A, Rust hard sensitive-file safety net,
//! split commits with step-2 hook failure recovery, Local Keep exclusion, local-only mode,
//! mid-flight cancellation, disabled built-in skills, and legacy `cpush-sessions.json` migration.

use super::{CustomActionCoordinator, types::*};
use crate::{
    i18n::LanguageKind,
    models::{
        smoke::LocalModelSseServer,
        store::ModelStore,
        types::{CreateProviderConnectionInput, ProviderKind},
    },
    pi_runtime::{PiPaths, PiSupervisor},
    repository::{
        RepositoryStore,
        smoke::{Fixture, git},
        types::{CommitSelection, ReadTarget},
    },
    skills::SkillManager,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn text_git(root: &std::path::Path, args: &[&str]) -> String {
    String::from_utf8(git(root, args).unwrap())
        .unwrap()
        .trim()
        .to_owned()
}

fn attach_bare_remote(root: &std::path::Path) -> std::path::PathBuf {
    root.parent().unwrap().join("origin.git")
}

fn make_option_reply(
    req_id: &str,
    run_id: &str,
    call_id: &str,
    option_id: Option<&str>,
    custom_text: Option<&str>,
) -> RespondCustomActionRequest {
    RespondCustomActionRequest {
        request_id: req_id.into(),
        run_id: run_id.into(),
        call_id: call_id.into(),
        response: MessageUserResponse {
            cancelled: false,
            answers: vec![MessageUserAnswer {
                question_index: 0,
                question_id: "q-0".into(),
                header: "Confirm".into(),
                selected_option_ids: option_id.map(|o| vec![o.to_owned()]).unwrap_or_default(),
                selected_labels: option_id.map(|o| vec![o.to_owned()]).unwrap_or_default(),
                custom_text: custom_text.map(str::to_owned),
            }],
            unanswered: vec![],
            global_note: None,
            dont_ask_again: None,
        },
        locale: LanguageKind::En,
    }
}

#[tokio::test]
async fn clear_scope_commits_in_one_turn_and_skip_option_leaves_skipped_file_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let secret = "sk-cpush-secret-never-persist-1122";
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: secret.into(),
                model_name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // 1. C01: Clear scope (`part.txt` only, `pnpm-lock.yaml` unselected) commits & pushes in EXACTLY 1 LLM turn:
    let fixture = Fixture::new().unwrap();
    let bare1 = attach_bare_remote(&fixture.root);
    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let part = tree.files.iter().find(|f| f.path == "part.txt").unwrap();
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-clear".into(),
        scope_version: tree.status_version.clone(),
    };

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();
    let progress_count = Arc::new(AtomicUsize::new(0));
    let pc = Arc::clone(&progress_count);
    let reqs_before = server.requests().len();

    let reply = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-clear-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: part.id.clone(),
                    commit_token: part.commit_token.clone(),
                }],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            move |_| {
                pc.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(
        server.requests().len() - reqs_before,
        1,
        "clean-scope C-Push must complete in 1 LLM HTTP request with zero extra summary turn"
    );
    assert!(progress_count.load(Ordering::SeqCst) >= 2);
    assert_eq!(reply.session.session_status, CPushSessionStatus::Done);
    assert_eq!(reply.session.commits.len(), 1);
    assert!(reply.session.qa_log.is_empty());
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "2"
    );
    assert_eq!(
        text_git(&fixture.root, &["show", "HEAD:part.txt"]),
        "first=2\nsecond=1"
    );
    assert_eq!(
        text_git(&bare1, &["rev-parse", "refs/heads/feat/read-proof"]),
        text_git(&fixture.root, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert!(text_git(&fixture.root, &["status", "--porcelain"]).contains("pnpm-lock.yaml"));

    // Persisted action-sessions.json never leaks API key or commitToken:
    let persisted_raw = fs::read_to_string(temp.path().join("action-sessions.json")).unwrap();
    assert!(!persisted_raw.contains(secret));
    assert!(!persisted_raw.contains(&part.commit_token));

    // 2. C02 live `message_user` `skip` option: commits code file and keeps `RELEASE_NOTES.md` untouched:
    let skip_fixture = Fixture::new().unwrap();
    let _bare2 = attach_bare_remote(&skip_fixture.root);
    fs::write(
        skip_fixture.root.join("RELEASE_NOTES.md"),
        "# v0.2\n- release note\n",
    )
    .unwrap();
    let skip_snap = repo_store.open(&skip_fixture.root).await.unwrap();
    let skip_tree = &skip_snap.worktrees[0];
    let skip_target = ReadTarget {
        repository_id: skip_tree.repository_id.clone(),
        worktree_id: skip_tree.worktree_id.clone(),
        session_id: "sess-skip".into(),
        scope_version: skip_tree.status_version.clone(),
    };
    let sel_of = |path: &str| {
        let f = skip_tree
            .files
            .iter()
            .find(|item| item.path == path)
            .unwrap();
        CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        }
    };

    let coord_cb = coordinator.clone();
    let answered = Arc::new(AtomicUsize::new(0));
    let ans_cb = Arc::clone(&answered);
    let skipped_done = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-skip-start".into(),
                target: skip_target,
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            move |ev| {
                if ev.session.session_status == CPushSessionStatus::NeedsDecision
                    && let Some(call_id) = ev.session.pending_call_id.clone()
                {
                    ans_cb.fetch_add(1, Ordering::SeqCst);
                    let c = coord_cb.clone();
                    let run_id = ev.session.run_id.clone();
                    tokio::spawn(async move {
                        let _ = c.respond_question(&make_option_reply(
                            "cpush-skip-ans",
                            &run_id,
                            &call_id,
                            Some("skip"),
                            None,
                        ));
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(answered.load(Ordering::SeqCst), 1);
    assert_eq!(
        skipped_done.session.session_status,
        CPushSessionStatus::Done
    );
    assert_eq!(skipped_done.session.commits.len(), 1);
    assert_eq!(skipped_done.session.skipped_file_ids.len(), 1);
    assert!(skip_fixture.root.join("RELEASE_NOTES.md").exists());
    assert!(
        text_git(&skip_fixture.root, &["status", "--porcelain"]).contains("?? RELEASE_NOTES.md")
    );
}

#[tokio::test]
async fn splits_commits_reconfirms_out_of_scope_recovers_from_step2_hook_failure_and_restores_safely()
 {
    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-cpush-split-9900".into(),
                model_name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fixture = Fixture::new().unwrap();
    let bare_split = attach_bare_remote(&fixture.root);
    fs::write(
        fixture.root.join("RELEASE_NOTES.md"),
        "# Release\n- document callback lock\n",
    )
    .unwrap();
    let hook_path = fixture.root.join(".git/hooks/commit-msg");
    fs::write(
        &hook_path,
        "#!/bin/sh\nif grep -q '^docs:' \"$1\"; then\n  echo 'docs hook blocked temporarily' >&2\n  exit 1\nfi\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-split".into(),
        scope_version: tree.status_version.clone(),
    };
    let sel_of = |path: &str| {
        let f = tree.files.iter().find(|item| item.path == path).unwrap();
        CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        }
    };

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // First question -> reply out-of-scope free-text -> model re-asks -> second question -> reply `split` ->
    // Commit 1 lands, Commit 2 hits commit-msg hook and pauses with `can_retry_remaining == true`!
    let coord_cb = coordinator.clone();
    let q_round = Arc::new(AtomicUsize::new(0));
    let q_cb = Arc::clone(&q_round);
    let start_req = StartCPushRequest {
        request_id: "cpush-split-1".into(),
        target: target.clone(),
        model_id: saved.model.id.clone(),
        selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
        prompt: "clarify callback idempotency guard".into(),
        push: None,
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
    };

    let paused = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &start_req,
            move |ev| {
                if ev.session.session_status == CPushSessionStatus::NeedsDecision
                    && let Some(call_id) = ev.session.pending_call_id.clone()
                {
                    let round = q_cb.fetch_add(1, Ordering::SeqCst);
                    let c = coord_cb.clone();
                    let run_id = ev.session.run_id.clone();
                    tokio::spawn(async move {
                        let reply = if round == 0 {
                            make_option_reply(
                                "cpush-ans-out-of-scope",
                                &run_id,
                                &call_id,
                                None,
                                Some("also include unselected pnpm-lock.yaml"),
                            )
                        } else {
                            make_option_reply(
                                "cpush-ans-split",
                                &run_id,
                                &call_id,
                                Some("split"),
                                None,
                            )
                        };
                        let _ = c.respond_question(&reply);
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(q_round.load(Ordering::SeqCst), 2);
    assert_eq!(
        paused.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert!(paused.session.can_retry_remaining);
    assert_eq!(paused.session.commits.len(), 1);
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "2"
    );

    // Duplicate request_id is idempotent; new request_id on active paused session is rejected:
    let dup = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &start_req,
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(dup.session.run_id, paused.session.run_id);

    let second_err = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-second-blocked".into(),
                ..start_req.clone()
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(second_err.contains("active C-Push session"), "{second_err}");

    let first_commit_oid = paused.session.commits[0].oid.clone();
    let err_msg_id = paused.session.message.as_ref().unwrap().id.clone();

    // Remove the temporary hook block and retry remaining steps (`C04`):
    fs::remove_file(&hook_path).unwrap();
    let finished = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-retry-step2".into(),
                target: target.clone(),
                run_id: paused.session.run_id.clone(),
                message_id: err_msg_id,
                option_id: Some("retry-remaining".into()),
                text: None,
                model_id: None,
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(finished.session.session_status, CPushSessionStatus::Done);
    assert_eq!(
        text_git(&bare_split, &["rev-parse", "refs/heads/feat/read-proof"]),
        text_git(&fixture.root, &["rev-parse", "HEAD"])
    );
    assert!(!finished.session.can_retry_remaining);
    assert_eq!(finished.session.commits.len(), 2);
    assert_eq!(finished.session.commits[0].oid, first_commit_oid);
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "3"
    );
    assert_eq!(
        text_git(&fixture.root, &["show", "HEAD:RELEASE_NOTES.md"]),
        "# Release\n- document callback lock"
    );
    assert_eq!(
        text_git(&fixture.root, &["show", "HEAD~1:part.txt"]),
        "first=2\nsecond=1"
    );
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert!(text_git(&fixture.root, &["status", "--porcelain"]).contains("pnpm-lock.yaml"));

    // Cold-restart recovers the completed session summary from action-sessions.json v2:
    let cold = CustomActionCoordinator::default();
    let live_snap = repo_store.refresh(&target).await.unwrap();
    let (restored_sessions, restore_warn) =
        cold.restore_cpush(temp.path(), &[live_snap]).await.unwrap();
    assert_eq!(restore_warn, None);
    assert_eq!(restored_sessions.len(), 1);
    assert_eq!(restored_sessions[0].commits.len(), 2);
    assert_eq!(restored_sessions[0].qa_log.len(), 2);
}

#[tokio::test]
async fn select_all_with_dotenv_redacts_secrets_asks_before_committing_and_skips_dotenv_on_confirm()
{
    assert!(crate::repository::ai_context::is_sensitive_local_file(
        ".env"
    ));
    assert!(crate::repository::ai_context::is_sensitive_local_file(
        "apps/web/.env.development.local"
    ));
    assert!(!crate::repository::ai_context::is_sensitive_local_file(
        ".env.example"
    ));
    assert!(!crate::repository::ai_context::is_sensitive_local_file(
        ".env.template"
    ));

    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-dotenv-test-key".into(),
                model_name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fixture = Fixture::new().unwrap();
    let bare = attach_bare_remote(&fixture.root);
    let local_db_secret = "postgres://dev:local_super_secret_98765@127.0.0.1:5432/app";
    fs::write(
        fixture.root.join(".env.local"),
        format!("# local test env\nDATABASE_URL={local_db_secret}\nEMPTY_VAR=\n"),
    )
    .unwrap();
    fs::write(
        fixture.root.join(".env.example"),
        "DATABASE_URL=postgres://localhost/example\n",
    )
    .unwrap();

    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-dotenv".into(),
        scope_version: tree.status_version.clone(),
    };

    let selections: Vec<CommitSelection> = tree
        .files
        .iter()
        .filter(|f| matches!(f.path.as_str(), "part.txt" | ".env.local" | ".env.example"))
        .map(|f| CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        })
        .collect();
    assert_eq!(selections.len(), 3);

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // Rust hard sensitive-file safety net intercepts commit_exact_scope and asks BEFORE committing,
    // then commits & pushes immediately on `skip` with ZERO extra LLM call!
    let coord_cb = coordinator.clone();
    let sensitive_asked = Arc::new(AtomicUsize::new(0));
    let sa_cb = Arc::clone(&sensitive_asked);
    let reqs_before = server.requests().len();

    let finished = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-dotenv-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections,
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            move |ev| {
                if ev.session.session_status == CPushSessionStatus::NeedsDecision
                    && let Some(call_id) = ev.session.pending_call_id.clone()
                {
                    assert!(ev.session.commits.is_empty());
                    sa_cb.fetch_add(1, Ordering::SeqCst);
                    let c = coord_cb.clone();
                    let run_id = ev.session.run_id.clone();
                    tokio::spawn(async move {
                        let _ = c.respond_question(&make_option_reply(
                            "cpush-dotenv-skip",
                            &run_id,
                            &call_id,
                            Some("skip"),
                            None,
                        ));
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(sensitive_asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        server.requests().len() - reqs_before,
        1,
        "Rust sensitive-file interception settles with zero extra LLM call"
    );
    let reqs = server.requests();
    for req in &reqs {
        assert!(
            !req.body.contains("local_super_secret_98765"),
            "plaintext .env secret must never be sent to external LLM"
        );
        assert!(req.body.contains("DATABASE_URL=[REDACTED_LOCAL_VALUE]"));
    }

    assert_eq!(finished.session.session_status, CPushSessionStatus::Done);
    assert_eq!(finished.session.commits.len(), 1);
    assert_eq!(finished.session.skipped_file_ids.len(), 1);
    assert!(text_git(&fixture.root, &["status", "--porcelain"]).contains(".env.local"));
    assert!(
        git(&bare, &["show", "refs/heads/feat/read-proof:.env.local"]).is_err(),
        ".env.local must not be committed or pushed to remote"
    );
    assert_eq!(
        String::from_utf8(
            git(&bare, &["show", "refs/heads/feat/read-proof:.env.example"]).unwrap()
        )
        .unwrap()
        .trim(),
        "DATABASE_URL=postgres://localhost/example"
    );
}

#[tokio::test]
async fn cpush_excludes_local_keep_by_default_rejects_stale_pre_keep_tokens_and_allows_explicit_checked_keep()
 {
    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-cpush-local-keep-7788".into(),
                model_name: "CPush LocalKeep Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fixture = Fixture::new().unwrap();
    let bare = attach_bare_remote(&fixture.root);
    fs::write(
        fixture.root.join("scratch.local.ts"),
        "export const localOnly = 'never-auto-select';\n",
    )
    .unwrap();
    git(&fixture.root, &["add", "--", "pnpm-lock.yaml"]).unwrap();

    let repo_store = RepositoryStore::default();
    let initial_snap = repo_store.open(&fixture.root).await.unwrap();
    let initial_tree = &initial_snap.worktrees[0];
    let stale_scratch = initial_tree
        .files
        .iter()
        .find(|f| f.path == "scratch.local.ts")
        .unwrap()
        .clone();

    let target = ReadTarget {
        repository_id: initial_tree.repository_id.clone(),
        worktree_id: initial_tree.worktree_id.clone(),
        session_id: "sess-cpush-lk".into(),
        scope_version: initial_tree.status_version.clone(),
    };

    let after_keep = repo_store
        .set_local_keep(
            &target,
            &["scratch.local.ts".into(), "pnpm-lock.yaml".into()],
            true,
            LanguageKind::En,
        )
        .await
        .unwrap();
    let kept_tree = &after_keep.worktrees[0];

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    let stale_err = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-lk-stale".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: stale_scratch.id.clone(),
                    commit_token: stale_scratch.commit_token.clone(),
                }],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(
        stale_err.contains("changed")
            || stale_err.contains("expired")
            || stale_err.contains("Refresh"),
        "expected stale scope rejection, got: {stale_err}"
    );

    let default_selections: Vec<CommitSelection> = kept_tree
        .files
        .iter()
        .filter(|f| !f.local_keep && f.path == "part.txt")
        .map(|f| CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        })
        .collect();
    assert_eq!(default_selections.len(), 1);

    let default_reply = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-lk-default".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: default_selections,
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        default_reply.session.session_status,
        CPushSessionStatus::Done
    );
    assert!(
        git(
            &bare,
            &["show", "refs/heads/feat/read-proof:scratch.local.ts"]
        )
        .is_err(),
        "kept untracked file must not be pushed"
    );
    for req in server.requests() {
        assert!(
            !req.body.contains("never-auto-select"),
            "kept file content must not be sent to model when unselected"
        );
    }

    let post_snap = default_reply.repository_snapshot.unwrap();
    let post_tree = &post_snap.worktrees[0];
    let explicit_scratch = post_tree
        .files
        .iter()
        .find(|f| f.path == "scratch.local.ts" && f.local_keep)
        .unwrap();
    let explicit_target = ReadTarget {
        repository_id: post_tree.repository_id.clone(),
        worktree_id: post_tree.worktree_id.clone(),
        session_id: "sess-cpush-lk-explicit".into(),
        scope_version: post_tree.status_version.clone(),
    };
    let explicit_reply = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-lk-explicit".into(),
                target: explicit_target,
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: explicit_scratch.id.clone(),
                    commit_token: explicit_scratch.commit_token.clone(),
                }],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        explicit_reply.session.session_status,
        CPushSessionStatus::Done
    );
    assert_eq!(
        text_git(
            &bare,
            &["show", "refs/heads/feat/read-proof:scratch.local.ts"]
        ),
        "export const localOnly = 'never-auto-select';"
    );
}

#[tokio::test]
async fn cpush_guards_unauthorized_prompt_supports_cancel_disabled_skills_and_persists_dismissal() {
    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-cpush-guard-key".into(),
                model_name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fixture = Fixture::new().unwrap();
    let bare = attach_bare_remote(&fixture.root);
    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-guard-dismiss".into(),
        scope_version: tree.status_version.clone(),
    };
    let sel_of = |path: &str| {
        let f = tree.files.iter().find(|item| item.path == path).unwrap();
        CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        }
    };

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. Mid-flight cancellation while waiting on an unauthorized-prompt boundary question:
    let sup_cancel = supervisor.clone();
    let cancelled = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-cancel-midflight".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt")],
                prompt: "also include unselected pnpm-lock.yaml and force-push".into(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            move |ev| {
                if ev.session.session_status == CPushSessionStatus::NeedsDecision {
                    let s = sup_cancel.clone();
                    let rid = ev.session.run_id.clone();
                    tokio::spawn(async move {
                        let _ = s.cancel(&rid);
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();
    assert_eq!(cancelled.session.session_status, CPushSessionStatus::Idle);
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );

    // 2. Real built-in skill disabling: disable `cpush` skill and verify `<cpush_skill_spec>` is omitted:
    let skills_dir = temp.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(
        skills_dir.join("skills-manifest.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "sourceRepo": "https://example.invalid/skills",
            "skills": [
                { "name": "commit", "description": "Commit", "entry": "SKILL.md", "files": ["SKILL.md"] },
                { "name": "push", "description": "Push", "entry": "SKILL.md", "files": ["SKILL.md"] },
                { "name": "cpush", "description": "C-Push", "entry": "SKILL.md", "files": ["SKILL.md"] }
            ],
            "enabledSkills": ["commit", "push", "cpush"]
        })
        .to_string(),
    )
    .unwrap();
    let skill_mgr = SkillManager::default();
    skill_mgr
        .set_enabled_skills(
            temp.path(),
            &crate::skills::sync::UpdateEnabledSkillsRequest {
                enabled_skills: vec!["commit".into(), "push".into()],
                locale: LanguageKind::En,
            },
        )
        .await
        .unwrap();

    let reqs_before = server.requests().len();
    let coord_cb = coordinator.clone();
    let boundary_asked = Arc::new(AtomicUsize::new(0));
    let ba_cb = Arc::clone(&boundary_asked);
    let executed = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-unauthorized-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt")],
                prompt: "also include unselected pnpm-lock.yaml and force-push".into(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            move |ev| {
                if ev.session.session_status == CPushSessionStatus::NeedsDecision
                    && let Some(call_id) = ev.session.pending_call_id.clone()
                {
                    ba_cb.fetch_add(1, Ordering::SeqCst);
                    let c = coord_cb.clone();
                    let run_id = ev.session.run_id.clone();
                    tokio::spawn(async move {
                        let _ = c.respond_question(&make_option_reply(
                            "cpush-unauthorized-ans",
                            &run_id,
                            &call_id,
                            Some("proceed-authorized"),
                            None,
                        ));
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(boundary_asked.load(Ordering::SeqCst), 1);
    let new_reqs = &server.requests()[reqs_before..];
    assert!(!new_reqs.is_empty());
    assert!(
        !new_reqs[0].body.contains("<cpush_skill_spec>"),
        "disabling built-in `cpush` skill must actually omit <cpush_skill_spec>"
    );
    assert!(new_reqs[0].body.contains("<commit_skill_spec>"));

    assert_eq!(executed.session.session_status, CPushSessionStatus::Done);
    assert_eq!(executed.session.commits.len(), 1);
    assert_eq!(
        text_git(&bare, &["rev-parse", "refs/heads/feat/read-proof"]),
        text_git(&fixture.root, &["rev-parse", "HEAD"])
    );

    // Cold restart BEFORE dismissal restores Done summary; unified `dismiss_done_notice_async` persists dismissal:
    let before_dismiss = CustomActionCoordinator::default()
        .restore_all_sessions_async(
            temp.path(),
            std::slice::from_ref(&snapshot),
            LanguageKind::En,
        )
        .await
        .unwrap()
        .1;
    assert_eq!(before_dismiss.len(), 1);
    assert_eq!(before_dismiss[0].session_status, CPushSessionStatus::Done);

    let done_msg_id = executed
        .session
        .message
        .as_ref()
        .expect("done session message")
        .id
        .clone();
    assert!(
        !coordinator
            .dismiss_done_notice_async(
                temp.path(),
                &DismissSessionDoneRequest {
                    request_id: "dismiss-wrong-run".into(),
                    target: target.clone(),
                    run_id: Some("other-run".into()),
                    message_id: done_msg_id.clone(),
                    locale: LanguageKind::En,
                },
            )
            .await
            .unwrap()
    );
    assert!(
        coordinator
            .dismiss_done_notice_async(
                temp.path(),
                &DismissSessionDoneRequest {
                    request_id: "dismiss-done-1".into(),
                    target: target.clone(),
                    run_id: Some(executed.session.run_id.clone()),
                    message_id: done_msg_id,
                    locale: LanguageKind::En,
                },
            )
            .await
            .unwrap()
    );

    let after_dismiss = CustomActionCoordinator::default()
        .restore_all_sessions_async(temp.path(), &[snapshot], LanguageKind::En)
        .await
        .unwrap()
        .1;
    assert!(
        after_dismiss.is_empty(),
        "Dismissed Done C-Push summary must not resurface after restarting Picket"
    );
}

#[tokio::test]
async fn cpush_supports_untouched_submodule_fails_fast_before_llm_and_omits_retry_on_unsupported_guard()
 {
    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Guard Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-cpush-guard-key".into(),
                model_name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. Fail-fast preflight (`index.lock`) refuses BEFORE calling Pi Core (0 LLM requests):
    let lock_fixture = Fixture::new().unwrap();
    let repo_store = RepositoryStore::default();
    let lock_snap = repo_store.open(&lock_fixture.root).await.unwrap();
    let lock_tree = &lock_snap.worktrees[0];
    let lock_part = lock_tree
        .files
        .iter()
        .find(|f| f.path == "part.txt")
        .unwrap();
    let lock_target = ReadTarget {
        repository_id: lock_tree.repository_id.clone(),
        worktree_id: lock_tree.worktree_id.clone(),
        session_id: "sess-fail-fast".into(),
        scope_version: lock_tree.status_version.clone(),
    };
    fs::write(lock_fixture.root.join(".git/index.lock"), "locked").unwrap();
    let requests_before = server.requests().len();
    let lock_err = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-fail-fast-lock".into(),
                target: lock_target,
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: lock_part.id.clone(),
                    commit_token: lock_part.commit_token.clone(),
                }],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(lock_err.contains("index.lock"), "{lock_err}");
    assert_eq!(server.requests().len(), requests_before);

    // 2. Repo containing an untouched tracked 160000 submodule succeeds end-to-end:
    let sub_fixture = Fixture::new().unwrap();
    let bare = attach_bare_remote(&sub_fixture.root);
    let sub_dir = sub_fixture.root.join("vendor/skill-sub");
    fs::create_dir_all(&sub_dir).unwrap();
    git(&sub_dir, &["init", "-b", "main"]).unwrap();
    git(&sub_dir, &["config", "user.name", "Sub Tester"]).unwrap();
    git(&sub_dir, &["config", "user.email", "sub@picket.invalid"]).unwrap();
    git(&sub_dir, &["config", "commit.gpgSign", "false"]).unwrap();
    fs::write(sub_dir.join("SKILL.md"), "# skill\n").unwrap();
    git(&sub_dir, &["add", "SKILL.md"]).unwrap();
    git(&sub_dir, &["commit", "-m", "feat: sub seed"]).unwrap();
    let sub_oid = text_git(&sub_dir, &["rev-parse", "HEAD"]);
    git(
        &sub_fixture.root,
        &[
            "add",
            "--",
            "part.txt",
            "pnpm-lock.yaml",
            "\u{65b0} \u{6587}\u{4ef6}.ts",
        ],
    )
    .unwrap();
    git(
        &sub_fixture.root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{sub_oid},vendor/skill-sub"),
        ],
    )
    .unwrap();
    git(&sub_fixture.root, &["commit", "-m", "chore: add submodule"]).unwrap();
    git(&sub_fixture.root, &["push", "origin", "feat/read-proof"]).unwrap();

    fs::write(sub_fixture.root.join("RULES.md"), "# rules\n").unwrap();
    let sub_snap = repo_store.open(&sub_fixture.root).await.unwrap();
    let sub_tree = &sub_snap.worktrees[0];
    let rules = sub_tree
        .files
        .iter()
        .find(|f| f.path == "RULES.md")
        .unwrap();
    let sub_target = ReadTarget {
        repository_id: sub_tree.repository_id.clone(),
        worktree_id: sub_tree.worktree_id.clone(),
        session_id: "sess-sub-ok".into(),
        scope_version: sub_tree.status_version.clone(),
    };
    let sub_reply = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-sub-ok".into(),
                target: sub_target,
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: rules.id.clone(),
                    commit_token: rules.commit_token.clone(),
                }],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(sub_reply.session.session_status, CPushSessionStatus::Done);
    assert_eq!(sub_reply.session.commits.len(), 1);
    assert_eq!(
        text_git(&bare, &["rev-parse", "refs/heads/feat/read-proof"]),
        text_git(&sub_fixture.root, &["rev-parse", "HEAD"])
    );
    assert!(
        text_git(&sub_fixture.root, &["ls-tree", "HEAD", "vendor/skill-sub"])
            .contains(&format!("160000 commit {sub_oid}"))
    );

    // 3. Non-retryable unsupported guard encountered mid-execution omits `retry-remaining`:
    let mid_fixture = Fixture::new().unwrap();
    fs::write(mid_fixture.root.join("RELEASE_NOTES.md"), "# Notes\n").unwrap();
    let post_commit_hook = mid_fixture.root.join(".git/hooks/post-commit");
    fs::write(
        &post_commit_hook,
        "#!/bin/sh\ngit config core.sparseCheckout true\n",
    )
    .unwrap();
    fs::set_permissions(&post_commit_hook, fs::Permissions::from_mode(0o755)).unwrap();

    let mid_snap = repo_store.open(&mid_fixture.root).await.unwrap();
    let mid_tree = &mid_snap.worktrees[0];
    let mid_part = mid_tree
        .files
        .iter()
        .find(|f| f.path == "part.txt")
        .unwrap();
    let mid_notes = mid_tree
        .files
        .iter()
        .find(|f| f.path == "RELEASE_NOTES.md")
        .unwrap();
    let mid_target = ReadTarget {
        repository_id: mid_tree.repository_id.clone(),
        worktree_id: mid_tree.worktree_id.clone(),
        session_id: "sess-non-retryable".into(),
        scope_version: mid_tree.status_version.clone(),
    };
    let coord_cb = coordinator.clone();
    let blocked = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-mid-non-retryable".into(),
                target: mid_target,
                model_id: saved.model.id.clone(),
                selections: vec![
                    CommitSelection {
                        file_id: mid_part.id.clone(),
                        commit_token: mid_part.commit_token.clone(),
                    },
                    CommitSelection {
                        file_id: mid_notes.id.clone(),
                        commit_token: mid_notes.commit_token.clone(),
                    },
                ],
                prompt: String::new(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            move |ev| {
                if ev.session.session_status == CPushSessionStatus::NeedsDecision
                    && let Some(call_id) = ev.session.pending_call_id.clone()
                {
                    let c = coord_cb.clone();
                    let run_id = ev.session.run_id.clone();
                    tokio::spawn(async move {
                        let _ = c.respond_question(&make_option_reply(
                            "cpush-mid-split",
                            &run_id,
                            &call_id,
                            Some("split"),
                            None,
                        ));
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();
    assert_eq!(
        blocked.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert!(!blocked.session.can_retry_remaining);
    assert_eq!(blocked.session.commits.len(), 1);
    let msg = blocked.session.message.as_ref().unwrap();
    let opts = msg.options.as_ref().unwrap();
    assert_eq!(opts.len(), 1);
    assert_eq!(opts[0].id, "dismiss-session");
    assert_eq!(opts[0].recommended, Some(true));
}

#[tokio::test]
async fn cpush_supports_local_only_commit_on_demand_diff_and_legacy_cpush_sessions_migration() {
    let temp = tempfile::tempdir().unwrap();
    let models = ModelStore::default();
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush LocalOnly Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-cpush-local-only-key".into(),
                model_name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                locale: LanguageKind::En,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fixture = Fixture::new().unwrap();
    let bare = attach_bare_remote(&fixture.root);
    let initial_remote_oid = text_git(&bare, &["rev-parse", "refs/heads/feat/read-proof"]);
    fs::write(fixture.root.join(".env.local"), "SECRET=123\n").unwrap();

    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let part = tree.files.iter().find(|f| f.path == "part.txt").unwrap();
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-local-only".into(),
        scope_version: tree.status_version.clone(),
    };
    let part_sel = vec![CommitSelection {
        file_id: part.id.clone(),
        commit_token: part.commit_token.clone(),
    }];

    // 1. On-demand read_cpush_scope_diff serves ONLY authorized selected files and blocks unselected/sensitive:
    let extracted = repo_store
        .extract_cpush_scope(&target, &part_sel, LanguageKind::En)
        .await
        .unwrap();
    let part_diff = repo_store
        .read_cpush_scope_diff(
            &target,
            extracted.initial_head.as_deref(),
            &extracted.files,
            "part.txt",
            Some(1),
            Some(50),
            LanguageKind::En,
        )
        .await
        .unwrap();
    assert!(part_diff.contains("Authorized diff: part.txt"));
    assert!(part_diff.contains("+first=2"));
    assert!(!part_diff.contains("second=3"));

    let unselected_err = repo_store
        .read_cpush_scope_diff(
            &target,
            extracted.initial_head.as_deref(),
            &extracted.files,
            "pnpm-lock.yaml",
            None,
            None,
            LanguageKind::En,
        )
        .await
        .unwrap_err();
    assert!(unselected_err.contains("not part of the authorized C-Push scope"));

    let env_file = tree.files.iter().find(|f| f.path == ".env.local").unwrap();
    let env_extracted = repo_store
        .extract_cpush_scope(
            &target,
            &[CommitSelection {
                file_id: env_file.id.clone(),
                commit_token: env_file.commit_token.clone(),
            }],
            LanguageKind::En,
        )
        .await
        .unwrap();
    let sensitive_err = repo_store
        .read_cpush_scope_diff(
            &target,
            env_extracted.initial_head.as_deref(),
            &env_extracted.files,
            ".env.local",
            None,
            None,
            LanguageKind::En,
        )
        .await
        .unwrap_err();
    assert!(sensitive_err.contains("sensitive"));

    let coordinator = CustomActionCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 2. Direct C-Push with `prompt: "commit locally without pushing"` commits locally without pushing:
    let local_done = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-local-only-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: part_sel,
                prompt: "commit locally without pushing".into(),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(local_done.session.session_status, CPushSessionStatus::Done);
    assert_eq!(local_done.session.commits.len(), 1);
    let done_msg = local_done.session.message.as_ref().unwrap();
    assert!(done_msg.id.starts_with("done-commit-only-"));
    assert_eq!(done_msg.title, "Committed 1 commit locally (not pushed)");
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "2"
    );
    assert_eq!(
        text_git(&bare, &["rev-parse", "refs/heads/feat/read-proof"]),
        initial_remote_oid,
        "local-only C-Push must NOT push commits to remote"
    );

    // 3. One-time migration from legacy `cpush-sessions.json` (v1) into unified `action-sessions.json` (v2):
    let legacy_dir = tempfile::tempdir().unwrap();
    let legacy_payload = serde_json::json!({
        "version": 1,
        "sessions": [{
            "runId": "legacy-run-1",
            "repositoryId": "repo-legacy",
            "worktreeId": "wt-legacy",
            "sessionId": "sess-legacy",
            "frozenScopeVersion": "sv-1",
            "modelId": "m-1",
            "branch": "main",
            "initialHead": null,
            "prompt": "",
            "recentCommits": [],
            "unselectedPaths": [],
            "frozenFiles": [],
            "plannedGroups": [],
            "skippedFileIds": [],
            "nextStepIndex": 0,
            "committedFileIds": [],
            "commits": [{
                "oid": "1234567890abcdef",
                "shortOid": "1234567",
                "title": "feat: legacy migrated commit",
                "branch": "main",
                "fileIds": []
            }],
            "qaLog": [],
            "timeline": [],
            "sessionStatus": "done",
            "message": {
                "id": "done-legacy-run-1",
                "kind": "done",
                "title": "Pushed 1 commit",
                "description": "Legacy done"
            },
            "canRetryRemaining": false,
            "pushAuthorized": true,
            "planOnly": false,
            "autoCollapse": false,
            "noticeDismissed": false,
            "locale": "en",
            "commitLanguage": "en"
        }]
    });
    fs::write(
        legacy_dir.path().join("cpush-sessions.json"),
        serde_json::to_vec_pretty(&legacy_payload).unwrap(),
    )
    .unwrap();

    let migrated_coord = CustomActionCoordinator::default();
    let (_, cpush_restored, warn) = migrated_coord
        .restore_all_sessions_async(legacy_dir.path(), &[], LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(warn, None);
    assert_eq!(cpush_restored.len(), 1);
    assert_eq!(cpush_restored[0].run_id, "legacy-run-1");
    assert_eq!(cpush_restored[0].commits.len(), 1);
    assert!(
        !legacy_dir.path().join("cpush-sessions.json").exists(),
        "legacy cpush-sessions.json should be removed after successful migration"
    );
    let unified_raw = fs::read_to_string(legacy_dir.path().join("action-sessions.json")).unwrap();
    assert!(unified_raw.contains("\"version\": 2"));
    assert!(unified_raw.contains("legacy-run-1"));
}
