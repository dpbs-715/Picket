use super::{CPushCoordinator, types::*};
use crate::{
    foundation::pi::{PiPaths, PiSupervisor},
    models::{
        keychain::CredentialVault,
        smoke::LocalModelSseServer,
        store::ModelStore,
        types::{ModelInput, ProviderKind},
    },
    repository::{
        RepositoryStore,
        smoke::{Fixture, git},
        types::{CommitSelection, ReadTarget},
    },
};
use std::{fs, os::unix::fs::PermissionsExt};

fn make_models(dir: &std::path::Path) -> ModelStore {
    let vault = CredentialVault::with_keychain_file(
        dir.join("cpush-test.keychain-db"),
        "cpush-test-keychain-pw",
    );
    ModelStore::with_vault(vault)
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
        .save_model(
            temp.path(),
            &ModelInput {
                id: None,
                provider: ProviderKind::Custom,
                api_format: None,
                name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: secret.into(),
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
                draft: String::new(),
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
                draft: String::new(),
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
        .save_model(
            temp.path(),
            &ModelInput {
                id: None,
                provider: ProviderKind::Custom,
                api_format: None,
                name: "CPush Planner".into(),
                model_id: "planner-v1".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-cpush-split-9900".into(),
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
        draft: "补充回调幂等校验说明".into(),
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
    assert!(second_err.contains("已有未结束"), "{second_err}");

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
                text: Some("把未选的 pnpm-lock.yaml 也一起提交".into()),
                model_id: None,
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
