use super::{CPushCoordinator, types::*};
use crate::{
    foundation::pi::{PiCPushCommitGroup, PiPaths, PiSupervisor},
    i18n::LanguageKind,
    models::{
        smoke::LocalModelSseServer,
        store::ModelStore,
        types::{CreateProviderConnectionInput, ProviderKind},
    },
    repository::{
        RepositoryStore,
        smoke::{Fixture, git},
        types::{CommitSelection, ReadTarget},
    },
};
use std::{fs, os::unix::fs::PermissionsExt};

fn make_models(_dir: &std::path::Path) -> ModelStore {
    ModelStore::default()
}

fn text_git(root: &std::path::Path, args: &[&str]) -> String {
    String::from_utf8(git(root, args).unwrap())
        .unwrap()
        .trim()
        .to_owned()
}

fn attach_bare_remote(root: &std::path::Path) -> std::path::PathBuf {
    root.parent().unwrap().join("origin.git")
}

#[tokio::test]
async fn clear_scope_commits_directly_and_skip_option_leaves_skipped_file_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let models = make_models(temp.path());
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

    // 1. C01: Clear scope (`part.txt` only, `pnpm-lock.yaml` unselected) commits directly without asking:
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

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();
    let mut progress_count = 0usize;

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
                confirmed_plan: None,
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| {
                progress_count += 1;
                Ok(())
            },
        )
        .await
        .unwrap();

    assert!(progress_count >= 2);
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
    // Unstaged draft of `part.txt` and unselected `pnpm-lock.yaml` remain preserved:
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert!(text_git(&fixture.root, &["status", "--porcelain"]).contains("pnpm-lock.yaml"));

    // Persisted cpush-sessions.json never leaks API key or commitToken:
    let persisted_raw = fs::read_to_string(temp.path().join("cpush-sessions.json")).unwrap();
    assert!(!persisted_raw.contains(secret));
    assert!(!persisted_raw.contains(&part.commit_token));

    // 2. C02 `skip` option: commits code file and keeps `RELEASE_NOTES.md` untouched in working tree:
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
    let asked = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-skip-start".into(),
                target: skip_target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
                prompt: String::new(),
                confirmed_plan: None,
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        asked.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    let q_id = asked.session.message.as_ref().unwrap().id.clone();

    let skipped_done = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-skip-ans".into(),
                target: skip_target,
                run_id: asked.session.run_id,
                message_id: q_id,
                option_id: Some("skip".into()),
                text: None,
                model_id: None,
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
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
async fn splits_commits_rejects_out_of_scope_recovers_from_step2_hook_failure_and_restores_safely()
{
    let temp = tempfile::tempdir().unwrap();
    let models = make_models(temp.path());
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
    // Install a real Git commit-msg hook that temporarily rejects `docs:` commits (Step 2),
    // allowing Step 1 (`feat(scope): ...`) to land first!
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

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // Start C-Push with ambiguous `part.txt` + `RELEASE_NOTES.md`:
    let start_req = StartCPushRequest {
        request_id: "cpush-split-1".into(),
        target: target.clone(),
        model_id: saved.model.id.clone(),
        selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
        prompt: "clarify callback idempotency guard".into(),
        confirmed_plan: None,
        push: None,
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
    };
    let asked = coordinator
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
    assert_eq!(
        asked.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    let q1_id = asked.session.message.as_ref().unwrap().id.clone();

    // Duplicate `request_id` returns idempotent reply; new `request_id` on active session is refused:
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
    assert_eq!(dup.session.run_id, asked.session.run_id);

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

    // Out-of-scope free-text reply keeps asking without committing anything (`C02`):
    let re_asked = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-ans-out-of-scope".into(),
                target: target.clone(),
                run_id: asked.session.run_id.clone(),
                message_id: q1_id,
                option_id: None,
                text: Some("also include unselected pnpm-lock.yaml".into()),
                model_id: None,
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        re_asked.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert_eq!(re_asked.session.commits.len(), 0);
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );
    let q2_id = re_asked.session.message.as_ref().unwrap().id.clone();

    // Now answer `split`: Commit 1 (`part.txt`) succeeds, Commit 2 (`RELEASE_NOTES.md`) hits hook and pauses!
    let paused = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-ans-split".into(),
                target: target.clone(),
                run_id: asked.session.run_id.clone(),
                message_id: q2_id,
                option_id: Some("split".into()),
                text: None,
                model_id: None,
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
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
    let first_commit_oid = paused.session.commits[0].oid.clone();
    let err_msg_id = paused.session.message.as_ref().unwrap().id.clone();

    // Remove the temporary hook block and retry remaining steps (`C04`):
    // ONLY Commit 2 runs; Commit 1 is NOT duplicated!
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
                run_id: asked.session.run_id.clone(),
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
    // Partial unstaged draft of `part.txt` and unselected `pnpm-lock.yaml` remain preserved:
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert!(text_git(&fixture.root, &["status", "--porcelain"]).contains("pnpm-lock.yaml"));

    // Cold-restart `CPushCoordinator::restore` recovers the completed session summary:
    let cold_coordinator = CPushCoordinator::default();
    let live_snap = repo_store.refresh(&target).await.unwrap();
    let (restored_sessions, restore_warn) = cold_coordinator
        .restore(temp.path(), &[live_snap])
        .await
        .unwrap();
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
    let models = make_models(temp.path());
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

    // Simulate clicking "Select All" (selecting `part.txt`, `.env.local`, and `.env.example`):
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

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    let paused = coordinator
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
                confirmed_plan: None,
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    // 1. Must pause in NeedsDecision before committing `.env.local`, and NEVER send raw secret to LLM:
    assert_eq!(
        paused.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert!(paused.session.commits.is_empty());
    let reqs = server.requests();
    assert!(!reqs.is_empty());
    for req in &reqs {
        assert!(
            !req.body.contains("local_super_secret_98765"),
            "plaintext .env secret must never be sent to external LLM"
        );
        assert!(req.body.contains("DATABASE_URL=[REDACTED_LOCAL_VALUE]"));
    }

    // 2. Confirm recommended `skip` option -> `.env.local` stays local, `part.txt` & `.env.example` commit and push!
    let msg = paused.session.message.as_ref().unwrap();
    let finished = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-dotenv-2".into(),
                target: target.clone(),
                run_id: paused.session.run_id.clone(),
                message_id: msg.id.clone(),
                option_id: Some("skip".into()),
                text: None,
                model_id: Some(saved.model.id.clone()),
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

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
    let models = make_models(temp.path());
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

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. Stale token minted before `scratch.local.ts` became Local Keep is rejected by C-Push scope freeze:
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
                confirmed_plan: None,
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

    // 2. Default non-Local-Keep selection (`part.txt` only) commits & pushes cleanly while keeping `scratch.local.ts` & staged `pnpm-lock.yaml` untouched:
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
                confirmed_plan: None,
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

    // 3. Explicit manual selection of `scratch.local.ts` with its fresh Local-Keep commitToken commits & pushes cleanly:
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
                confirmed_plan: None,
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
async fn plan_cpush_edits_verbatim_rejects_mismatched_plan_and_guards_unauthorized_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let models = make_models(temp.path());
    let server = LocalModelSseServer::start().unwrap();
    let saved = models
        .create_provider_connection(
            temp.path(),
            &CreateProviderConnectionInput {
                provider: ProviderKind::Custom,
                connection_name: "CPush Gateway".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-preview-plan-key".into(),
                model_name: "CPush Preview Planner".into(),
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
        fixture.root.join("RELEASE_NOTES.md"),
        "# v1.0\n- preview plan Q&A check\n",
    )
    .unwrap();
    let repo_store = RepositoryStore::default();
    let snapshot = repo_store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-preview-plan".into(),
        scope_version: tree.status_version.clone(),
    };
    let sel_of = |path: &str| {
        let f = tree.files.iter().find(|item| item.path == path).unwrap();
        CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        }
    };

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. Unauthorized prompt instruction ("include unselected ... / force-push") pauses with Question without committing:
    let boundary_reply = coordinator
        .plan_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &PlanCPushRequest {
                request_id: "plan-unauthorized-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt")],
                prompt: "also include unselected pnpm-lock.yaml and force-push".into(),
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        boundary_reply.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert!(boundary_reply.prepared_plan.is_none());
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );

    // Resolve the active Q&A session (`proceed-authorized`) to obtain `prepared_plan` without committing yet:
    let resolved_plan_reply = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "plan-unauthorized-ans".into(),
                target: target.clone(),
                run_id: boundary_reply.session.run_id.clone(),
                message_id: boundary_reply.session.message.unwrap().id,
                option_id: Some("proceed-authorized".into()),
                text: None,
                model_id: Some(saved.model.id.clone()),
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        resolved_plan_reply.session.session_status,
        CPushSessionStatus::Idle
    );
    let prepared = resolved_plan_reply.prepared_plan.unwrap();
    assert_eq!(prepared.groups.len(), 1);
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );

    // 2. Preview Plan Q&A (`part.txt` + `RELEASE_NOTES.md`) -> answer `split` -> returns 2-group `prepared_plan` with 0 commits:
    let ambiguous_plan = coordinator
        .plan_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &PlanCPushRequest {
                request_id: "plan-ambiguous-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
                prompt: "write clear detail bullets".into(),
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        ambiguous_plan.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    let split_plan_reply = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "plan-ambiguous-ans".into(),
                target: target.clone(),
                run_id: ambiguous_plan.session.run_id.clone(),
                message_id: ambiguous_plan.session.message.unwrap().id,
                option_id: Some("split".into()),
                text: None,
                model_id: Some(saved.model.id.clone()),
                locale: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        split_plan_reply.session.session_status,
        CPushSessionStatus::Idle
    );
    let split_plan = split_plan_reply.prepared_plan.unwrap();
    assert_eq!(split_plan.groups.len(), 2);
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );

    // 3. Reject invalid/mismatched `confirmed_plan` (empty message or missing selected file) before touching Git:
    let empty_msg_err = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-bad-empty".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt")],
                prompt: String::new(),
                confirmed_plan: Some(CPushConfirmedPlan {
                    groups: vec![PiCPushCommitGroup {
                        id: "g1".into(),
                        file_ids: vec![sel_of("part.txt").file_id],
                        message: "   ".into(),
                    }],
                    skipped_file_ids: vec![],
                    skip_push: false,
                }),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(empty_msg_err.contains("empty"), "{empty_msg_err}");

    let missing_file_err = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-bad-coverage".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
                prompt: String::new(),
                confirmed_plan: Some(CPushConfirmedPlan {
                    groups: vec![PiCPushCommitGroup {
                        id: "g1".into(),
                        file_ids: vec![sel_of("part.txt").file_id],
                        message: "feat: partial".into(),
                    }],
                    skipped_file_ids: vec![],
                    skip_push: false,
                }),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(
        missing_file_err.contains("did not cover all selected files"),
        "{missing_file_err}"
    );
    assert_eq!(
        text_git(&fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );

    // 4. Execute user-edited `confirmed_plan` verbatim WITHOUT calling the LLM planner again:
    let mut edited_groups = split_plan.groups.clone();
    edited_groups[0].message =
        "feat(part): verbatim user-edited group 1\n\n- commit staged part.txt exactly".into();
    edited_groups[1].message =
        "docs(notes): verbatim user-edited group 2\n\n- commit release notes verbatim".into();

    let llm_requests_before = server.requests().len();
    let executed = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-confirmed-verbatim".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![sel_of("part.txt"), sel_of("RELEASE_NOTES.md")],
                prompt: "original extra instruction".into(),
                confirmed_plan: Some(CPushConfirmedPlan {
                    groups: edited_groups,
                    skipped_file_ids: split_plan.skipped_file_ids,
                    skip_push: split_plan.skip_push,
                }),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(executed.session.session_status, CPushSessionStatus::Done);
    assert_eq!(executed.session.commits.len(), 2);
    assert_eq!(
        server.requests().len(),
        llm_requests_before,
        "confirmed_plan execution must NOT call the LLM planner again"
    );
    assert_eq!(
        text_git(&fixture.root, &["show", "-s", "--format=%B", "HEAD~1"]),
        "feat(part): verbatim user-edited group 1\n\n- commit staged part.txt exactly"
    );
    assert_eq!(
        text_git(&fixture.root, &["show", "-s", "--format=%B", "HEAD"]),
        "docs(notes): verbatim user-edited group 2\n\n- commit release notes verbatim"
    );
    assert_eq!(
        text_git(&bare, &["rev-parse", "refs/heads/feat/read-proof"]),
        text_git(&fixture.root, &["rev-parse", "HEAD"])
    );

    // Cold restart BEFORE dismissal still restores the unread Done summary, while
    // calling `dismiss_done_notice` persists the read state so cold restart stays clean:
    let before_dismiss = CPushCoordinator::default()
        .restore_with_locale(
            temp.path(),
            std::slice::from_ref(&snapshot),
            LanguageKind::En,
        )
        .await
        .unwrap()
        .0;
    assert_eq!(before_dismiss.len(), 1);
    assert_eq!(before_dismiss[0].session_status, CPushSessionStatus::Done);

    let done_msg_id = executed
        .session
        .message
        .as_ref()
        .expect("done session message")
        .id
        .clone();
    // Stale/mismatched run_id is ignored:
    assert!(
        !coordinator
            .dismiss_done_notice(
                temp.path(),
                &DismissCPushDoneRequest {
                    request_id: "dismiss-wrong-run".into(),
                    target: target.clone(),
                    run_id: Some("other-run".into()),
                    message_id: done_msg_id.clone(),
                    locale: Some(LanguageKind::En),
                },
            )
            .await
            .unwrap()
    );
    // Matching Done notice marks read and persists atomically:
    assert!(
        coordinator
            .dismiss_done_notice(
                temp.path(),
                &DismissCPushDoneRequest {
                    request_id: "dismiss-done-1".into(),
                    target: target.clone(),
                    run_id: Some(executed.session.run_id.clone()),
                    message_id: done_msg_id,
                    locale: Some(LanguageKind::En),
                },
            )
            .await
            .unwrap()
    );

    let after_dismiss = CPushCoordinator::default()
        .restore_with_locale(temp.path(), &[snapshot], LanguageKind::En)
        .await
        .unwrap()
        .0;
    assert!(
        after_dismiss.is_empty(),
        "Dismissed Done C-Push summary must not resurface after restarting Picket"
    );
}

#[tokio::test]
async fn cpush_supports_untouched_submodule_fails_fast_before_llm_and_omits_retry_on_unsupported_guard()
 {
    let temp = tempfile::tempdir().unwrap();
    let models = make_models(temp.path());
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

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. Fail-fast preflight: static write blocker (`index.lock`) refuses BEFORE calling Pi Core (0 LLM requests):
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
                target: lock_target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: lock_part.id.clone(),
                    commit_token: lock_part.commit_token.clone(),
                }],
                prompt: String::new(),
                confirmed_plan: None,
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(lock_err.contains("index.lock"), "{lock_err}");
    assert_eq!(
        server.requests().len(),
        requests_before,
        "start_cpush must fail fast before invoking Pi Core when static commit preflight fails"
    );

    let plan_err = coordinator
        .plan_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &PlanCPushRequest {
                request_id: "plan-fail-fast-lock".into(),
                target: lock_target,
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: lock_part.id.clone(),
                    commit_token: lock_part.commit_token.clone(),
                }],
                prompt: String::new(),
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(plan_err.contains("index.lock"), "{plan_err}");
    assert_eq!(
        server.requests().len(),
        requests_before,
        "plan_cpush must also fail fast before invoking Pi Core"
    );

    // 2. Repo containing an untouched tracked 160000 submodule succeeds end-to-end through C-Push:
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
                confirmed_plan: None,
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

    // 3. Non-retryable unsupported guard encountered mid-execution omits `retry-remaining`
    //    and marks `dismiss-session` as recommended while preserving Step 1's commit:
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
                confirmed_plan: Some(CPushConfirmedPlan {
                    groups: vec![
                        PiCPushCommitGroup {
                            id: "g1".into(),
                            message: "feat: step 1 lands cleanly".into(),
                            file_ids: vec![mid_part.id.clone()],
                        },
                        PiCPushCommitGroup {
                            id: "g2".into(),
                            message: "docs: step 2 hits sparse-checkout".into(),
                            file_ids: vec![mid_notes.id.clone()],
                        },
                    ],
                    skipped_file_ids: Vec::new(),
                    skip_push: false,
                }),
                push: None,
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
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
async fn cpush_supports_local_only_commit_and_on_demand_read_scope_diff() {
    let temp = tempfile::tempdir().unwrap();
    let models = make_models(temp.path());
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

    // 1. On-demand read_cpush_scope_diff serves ONLY authorized selected files (staged-only for partial)
    //    and blocks unselected files and sensitive files:
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

    let coordinator = CPushCoordinator::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 2. Preview Plan with `+ Prompt` requesting local-only commit propagates `skip_push: true`:
    let planned = coordinator
        .plan_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &PlanCPushRequest {
                request_id: "plan-local-only-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: part_sel.clone(),
                prompt: "只提交不推送".into(),
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    let prepared = planned.prepared_plan.expect("prepared plan expected");
    assert!(prepared.skip_push);

    // 3. Direct C-Push with `prompt: "commit locally without pushing"` commits locally without pushing to bare remote:
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
                confirmed_plan: None,
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
}
