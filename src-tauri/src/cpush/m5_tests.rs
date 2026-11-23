//! M5 end-to-end integration tests using disposable local bare remotes + two test clones
//! (`clone_a` and `clone_b`) + bundled Pi Core, verifying R02–R06, C04, and W04.
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
        remote::RebaseSyncOutcome,
        smoke::git,
        types::{CommitSelection, ReadTarget, Remote},
    },
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

struct BareRemotePair {
    _dir: tempfile::TempDir,
    bare: PathBuf,
    clone_a: PathBuf,
    clone_b: PathBuf,
}

fn text_git(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git(root, args).unwrap())
        .unwrap()
        .trim()
        .to_owned()
}

impl BareRemotePair {
    fn new(remote_name: &str, branch: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("remote.git");
        let clone_a = dir.path().join("clone-a");
        let clone_b = dir.path().join("clone-b");

        git(dir.path(), &["init", "--bare", bare.to_str().unwrap()]).unwrap();
        git(
            dir.path(),
            &["init", "-b", branch, clone_a.to_str().unwrap()],
        )
        .unwrap();
        for (k, v) in [
            ("user.name", "Clone A"),
            ("user.email", "a@picket.invalid"),
            ("commit.gpgSign", "false"),
        ] {
            git(&clone_a, &["config", k, v]).unwrap();
        }
        fs::create_dir_all(clone_a.join("src/service")).unwrap();
        fs::write(
            clone_a.join("src/service/Order.ts"),
            "export function handleOrder() {\n  return 'v1';\n}\n",
        )
        .unwrap();
        fs::write(clone_a.join("part.txt"), "first=1\nsecond=1\n").unwrap();
        git(&clone_a, &["add", "--", "src/service/Order.ts", "part.txt"]).unwrap();
        git(&clone_a, &["commit", "-m", "feat: initial seed"]).unwrap();
        git(
            &clone_a,
            &["remote", "add", remote_name, bare.to_str().unwrap()],
        )
        .unwrap();
        git(&clone_a, &["push", "-u", remote_name, branch]).unwrap();
        git(
            &bare,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        )
        .unwrap();

        git(
            dir.path(),
            &[
                "clone",
                "--origin",
                remote_name,
                "-b",
                branch,
                bare.to_str().unwrap(),
                clone_b.to_str().unwrap(),
            ],
        )
        .unwrap();
        for (k, v) in [
            ("user.name", "Clone B"),
            ("user.email", "b@picket.invalid"),
            ("commit.gpgSign", "false"),
        ] {
            git(&clone_b, &["config", k, v]).unwrap();
        }

        Self {
            _dir: dir,
            bare,
            clone_a,
            clone_b,
        }
    }
}

fn make_models(dir: &Path) -> ModelStore {
    let vault =
        CredentialVault::with_keychain_file(dir.join("m5-test.keychain-db"), "m5-test-keychain-pw");
    ModelStore::with_vault(vault)
}

#[tokio::test]
async fn manual_commit_and_push_handles_missing_upstream_push_failure_and_retry_without_recommit() {
    let temp = tempfile::tempdir().unwrap();
    let models = make_models(temp.path());
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();
    let repo_store = RepositoryStore::default();
    let coordinator = CPushCoordinator::default();

    // 1. R05 + A01: Zero-model manual `commit_and_push` on a branch without upstream and non-`origin` remote (`corp`):
    let pair = BareRemotePair::new("corp", "feat/pay");
    git(&pair.clone_a, &["switch", "-c", "feat/no-upstream"]).unwrap();
    fs::write(pair.clone_a.join("part.txt"), "first=2\nsecond=1\n").unwrap();

    let snap = repo_store.open(&pair.clone_a).await.unwrap();
    let wt = &snap.worktrees[0];
    assert!(matches!(wt.remote, Remote::Missing { .. }));
    let part = wt.files.iter().find(|f| f.path == "part.txt").unwrap();
    let target = ReadTarget {
        repository_id: wt.repository_id.clone(),
        worktree_id: wt.worktree_id.clone(),
        session_id: "sess-r05".into(),
        scope_version: wt.status_version.clone(),
    };

    let asked = coordinator
        .commit_and_push(
            temp.path(),
            &repo_store,
            &CommitAndPushRequest {
                request_id: "cp-no-upstream-1".into(),
                target: target.clone(),
                selections: vec![CommitSelection {
                    file_id: part.id.clone(),
                    commit_token: part.commit_token.clone(),
                }],
                message: "feat(part): update first key".into(),
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    // Local commit landed (`count == 2`), paused asking to confirm `corp/feat/no-upstream` (never hardcoded to `origin`!):
    assert_eq!(
        asked.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert_eq!(asked.session.commits.len(), 1);
    assert_eq!(
        text_git(&pair.clone_a, &["rev-list", "--count", "HEAD"]),
        "2"
    );
    let opts = asked
        .session
        .message
        .as_ref()
        .unwrap()
        .options
        .as_ref()
        .unwrap();
    assert!(
        opts.iter()
            .any(|o| o.id == "push-set-upstream:corp" && o.label.contains("corp/feat/no-upstream"))
    );
    assert!(!opts.iter().any(|o| o.label.contains("origin")));

    let pushed = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cp-confirm-corp".into(),
                target: target.clone(),
                run_id: asked.session.run_id.clone(),
                message_id: asked.session.message.unwrap().id,
                option_id: Some("push-set-upstream:corp".into()),
                text: None,
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(pushed.session.session_status, CPushSessionStatus::Done);
    assert_eq!(
        text_git(&pair.bare, &["rev-parse", "refs/heads/feat/no-upstream"]),
        pushed.session.commits[0].oid
    );

    // 2. R02: Local commit succeeds, push fails (remote pre-receive hook / outage), `retry-push` ONLY pushes without adding commits!
    let pre_receive = pair.bare.join("hooks/pre-receive");
    fs::write(
        &pre_receive,
        "#!/bin/sh\necho 'simulated network / pre-receive outage' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&pre_receive, fs::Permissions::from_mode(0o755)).unwrap();

    fs::write(pair.clone_a.join("part.txt"), "first=3\nsecond=1\n").unwrap();
    let snap2 = repo_store.refresh(&target).await.unwrap();
    let wt2 = &snap2.worktrees[0];
    let part2 = wt2.files.iter().find(|f| f.path == "part.txt").unwrap();
    let target2 = ReadTarget {
        scope_version: wt2.status_version.clone(),
        ..target.clone()
    };

    let failed = coordinator
        .commit_and_push(
            temp.path(),
            &repo_store,
            &CommitAndPushRequest {
                request_id: "cp-push-fail-1".into(),
                target: target2.clone(),
                selections: vec![CommitSelection {
                    file_id: part2.id.clone(),
                    commit_token: part2.commit_token.clone(),
                }],
                message: "feat(part): bump first to 3".into(),
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(
        failed.session.session_status,
        CPushSessionStatus::PushFailed
    );
    assert_eq!(
        failed.session.message.as_ref().unwrap().title,
        "已在本地提交，推送失败"
    );
    let committed_oid = failed.session.commits[0].oid.clone();
    assert_eq!(
        text_git(&pair.clone_a, &["rev-list", "--count", "HEAD"]),
        "3"
    );

    // Restore remote availability and retry ONLY push (`R02`):
    fs::remove_file(&pre_receive).unwrap();
    let retried = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cp-push-retry-1".into(),
                target: target2.clone(),
                run_id: failed.session.run_id.clone(),
                message_id: failed.session.message.unwrap().id,
                option_id: Some("retry-push".into()),
                text: None,
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(retried.session.session_status, CPushSessionStatus::Done);
    assert_eq!(
        text_git(&pair.clone_a, &["rev-list", "--count", "HEAD"]),
        "3"
    );
    assert_eq!(
        text_git(&pair.clone_a, &["rev-parse", "HEAD"]),
        committed_oid
    );
    assert_eq!(
        text_git(&pair.bare, &["rev-parse", "refs/heads/feat/no-upstream"]),
        committed_oid
    );
}

#[tokio::test]
async fn dirty_worktree_pull_rebase_preserves_partial_staging_pauses_on_overlap_and_guards_published_history()
 {
    let temp = tempfile::tempdir().unwrap();
    let repo_store = RepositoryStore::default();
    let coordinator = CPushCoordinator::default();
    let pair = BareRemotePair::new("origin", "feat/sync");

    // Clone B pushes a commit touching `remote-only.txt`:
    fs::write(pair.clone_b.join("remote-only.txt"), "from clone B\n").unwrap();
    git(&pair.clone_b, &["add", "--", "remote-only.txt"]).unwrap();
    git(
        &pair.clone_b,
        &["commit", "-m", "feat: add remote-only file"],
    )
    .unwrap();
    git(&pair.clone_b, &["push", "origin", "feat/sync"]).unwrap();

    // Clone A has 1 local unpushed commit (`ahead = 1`) AND a partially staged `part.txt` + untracked `notes.md`:
    fs::write(pair.clone_a.join("local-committed.txt"), "local ahead\n").unwrap();
    git(&pair.clone_a, &["add", "--", "local-committed.txt"]).unwrap();
    git(&pair.clone_a, &["commit", "-m", "feat: local ahead commit"]).unwrap();

    fs::write(pair.clone_a.join("part.txt"), "first=staged\nsecond=1\n").unwrap();
    git(&pair.clone_a, &["add", "--", "part.txt"]).unwrap();
    fs::write(
        pair.clone_a.join("part.txt"),
        "first=staged\nsecond=unstaged-draft\n",
    )
    .unwrap();
    fs::write(pair.clone_a.join("notes.md"), "untracked draft\n").unwrap();

    let snap = repo_store.open(&pair.clone_a).await.unwrap();
    let wt = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: wt.repository_id.clone(),
        worktree_id: wt.worktree_id.clone(),
        session_id: "sess-r06".into(),
        scope_version: wt.status_version.clone(),
    };

    // Standalone `pull_rebase` (`R06`): preserves BOTH staged & unstaged parts of `part.txt`,
    // marks remote `local_only == false`, and does NOT auto-push Clone A's local commit!
    let pulled = coordinator
        .pull_rebase(
            temp.path(),
            &repo_store,
            &PullRebaseRequest {
                request_id: "pull-disjoint-1".into(),
                target: target.clone(),
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(pulled.session.session_status, CPushSessionStatus::Idle);
    let updated_wt = &pulled.repository_snapshot.as_ref().unwrap().worktrees[0];
    match &updated_wt.remote {
        Remote::Tracked {
            ahead,
            behind,
            local_only,
            ..
        } => {
            assert_eq!(
                *ahead, 1,
                "Standalone pull must NEVER auto-push ahead commits"
            );
            assert_eq!(*behind, 0);
            assert!(
                !local_only,
                "Live fetch must mark remote verified (local_only = false)"
            );
        }
        other => panic!("Expected Tracked remote, got {other:?}"),
    }
    assert_eq!(
        text_git(&pair.clone_a, &["show", ":part.txt"]),
        "first=staged\nsecond=1"
    );
    assert_eq!(
        fs::read(pair.clone_a.join("part.txt")).unwrap(),
        b"first=staged\nsecond=unstaged-draft\n"
    );
    assert_eq!(
        fs::read(pair.clone_a.join("notes.md")).unwrap(),
        b"untracked draft\n"
    );
    assert!(pair.clone_a.join("remote-only.txt").exists());

    // Overlap check (`R06`): Clone B now modifies `part.txt` directly -> pull_rebase pauses BEFORE touching index/worktree!
    git(&pair.clone_b, &["pull", "--rebase", "origin", "feat/sync"]).unwrap();
    fs::write(
        pair.clone_b.join("part.txt"),
        "first=remote-conflict\nsecond=1\n",
    )
    .unwrap();
    git(&pair.clone_b, &["add", "--", "part.txt"]).unwrap();
    git(
        &pair.clone_b,
        &["commit", "-m", "feat: remote edits part.txt"],
    )
    .unwrap();
    git(&pair.clone_b, &["push", "origin", "feat/sync"]).unwrap();

    let overlap_err = coordinator
        .pull_rebase(
            temp.path(),
            &repo_store,
            &PullRebaseRequest {
                request_id: "pull-overlap-1".into(),
                target: target.clone(),
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(overlap_err.contains("part.txt"), "{overlap_err}");
    assert_eq!(
        text_git(&pair.clone_a, &["show", ":part.txt"]),
        "first=staged\nsecond=1"
    );
    assert_eq!(
        fs::read(pair.clone_a.join("part.txt")).unwrap(),
        b"first=staged\nsecond=unstaged-draft\n"
    );

    // Published history rewind check (`R05`): if remote branch is force-rewound below Clone A's base, pause!
    let root_oid = text_git(&pair.clone_b, &["rev-list", "--max-parents=0", "HEAD"]);
    git(&pair.clone_b, &["reset", "--hard", &root_oid]).unwrap();
    fs::write(pair.clone_b.join("rewound.txt"), "force-pushed history\n").unwrap();
    git(&pair.clone_b, &["add", "--", "rewound.txt"]).unwrap();
    git(
        &pair.clone_b,
        &["commit", "-m", "chore: rewritten upstream"],
    )
    .unwrap();
    git(&pair.clone_b, &["push", "--force", "origin", "feat/sync"]).unwrap();

    let rewind_outcome = repo_store.sync_rebase_onto_upstream(&target).await.unwrap();
    assert!(matches!(
        rewind_outcome,
        RebaseSyncOutcome::UnsafePublishedHistory { .. }
    ));
}

#[tokio::test]
async fn conflict_agent_merge_validation_manual_ide_return_and_crash_recovery_work_end_to_end() {
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
                name: "M5 Resolver".into(),
                model_id: "resolver-v1".into(),
                base_url: format!("{}/v1", server.base_url),
                api_key: "sk-m5-secret-7788".into(),
            },
        )
        .await
        .unwrap();

    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();
    let repo_store = RepositoryStore::default();
    let coordinator = CPushCoordinator::default();
    let pair = BareRemotePair::new("origin", "feat/order");

    // 1. R03: Clone B pushes conflicting edit to `src/service/Order.ts`:
    fs::write(
        pair.clone_b.join("src/service/Order.ts"),
        "export function handleOrder() {\n  logRemoteTrace();\n  return 'remote-v2';\n}\n",
    )
    .unwrap();
    git(&pair.clone_b, &["add", "--", "src/service/Order.ts"]).unwrap();
    git(
        &pair.clone_b,
        &["commit", "-m", "feat(order): add remote trace"],
    )
    .unwrap();
    git(&pair.clone_b, &["push", "origin", "feat/order"]).unwrap();

    // Clone A edits `src/service/Order.ts` AND keeps an unrelated unstaged `notes.txt` draft, then runs C-Push!
    fs::write(
        pair.clone_a.join("src/service/Order.ts"),
        "export function handleOrder() {\n  acquireIdempotentLock();\n  return 'local-v2';\n}\n",
    )
    .unwrap();
    fs::write(pair.clone_a.join("notes.txt"), "keep my scratch note\n").unwrap();

    let snap = repo_store.open(&pair.clone_a).await.unwrap();
    let wt = &snap.worktrees[0];
    let order_file = wt
        .files
        .iter()
        .find(|f| f.path == "src/service/Order.ts")
        .unwrap();
    let target = ReadTarget {
        repository_id: wt.repository_id.clone(),
        worktree_id: wt.worktree_id.clone(),
        session_id: "sess-r03".into(),
        scope_version: wt.status_version.clone(),
    };

    let conflicted = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-conflict-1".into(),
                target: target.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: order_file.id.clone(),
                    commit_token: order_file.commit_token.clone(),
                }],
                draft: String::new(),
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(
        conflicted.session.session_status,
        CPushSessionStatus::Conflict
    );
    let c_msg = conflicted.session.message.as_ref().unwrap();
    assert_eq!(c_msg.kind, "conflict");
    assert_eq!(
        c_msg.files.as_ref().unwrap()[0].label,
        "src/service/Order.ts"
    );

    // R03 validation failure guard: leftover conflict markers are rejected and pause back in `Conflict`!
    let marker_rejected = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-merge-bad-markers".into(),
                target: target.clone(),
                run_id: conflicted.session.run_id.clone(),
                message_id: c_msg.id.clone(),
                option_id: None,
                text: Some("模拟残留冲突标记".into()),
                model_id: Some(saved.model.id.clone()),
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        marker_rejected.session.session_status,
        CPushSessionStatus::Conflict
    );
    let rej_desc = &marker_rejected
        .session
        .message
        .as_ref()
        .unwrap()
        .description;
    assert!(rej_desc.contains("冲突标记"), "got: {rej_desc}");

    // R03 authorized Agent merge with custom requirement -> merges, finishes rebase, restores `notes.txt`, and pushes!
    let merged_and_pushed = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-merge-good".into(),
                target: target.clone(),
                run_id: conflicted.session.run_id.clone(),
                message_id: marker_rejected.session.message.unwrap().id,
                option_id: None,
                text: Some("附上合并说明注释".into()),
                model_id: Some(saved.model.id.clone()),
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(
        merged_and_pushed.session.session_status,
        CPushSessionStatus::Done
    );
    let head_order = text_git(&pair.clone_a, &["show", "HEAD:src/service/Order.ts"]);
    assert!(head_order.contains("logRemoteTrace()"));
    assert!(head_order.contains("acquireIdempotentLock()"));
    assert!(head_order.contains("// merged: 附上合并说明注释"));
    assert!(!head_order.contains("<<<<<<<"));
    assert_eq!(
        text_git(&pair.bare, &["rev-parse", "refs/heads/feat/order"]),
        text_git(&pair.clone_a, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read(pair.clone_a.join("notes.txt")).unwrap(),
        b"keep my scratch note\n"
    );

    // 2. R04: Manual IDE merge + return reconciliation (still conflicted -> resolved unpushed -> terminal pushed auto-collapse):
    git(&pair.clone_b, &["pull", "--rebase", "origin", "feat/order"]).unwrap();
    fs::write(
        pair.clone_b.join("src/service/Order.ts"),
        "export function handleOrder() {\n  return 'remote-v3';\n}\n",
    )
    .unwrap();
    git(&pair.clone_b, &["add", "--", "src/service/Order.ts"]).unwrap();
    git(&pair.clone_b, &["commit", "-m", "feat(order): remote v3"]).unwrap();
    git(&pair.clone_b, &["push", "origin", "feat/order"]).unwrap();

    fs::write(
        pair.clone_a.join("src/service/Order.ts"),
        "export function handleOrder() {\n  return 'local-v3';\n}\n",
    )
    .unwrap();
    let snap2 = repo_store.refresh(&target).await.unwrap();
    let wt2 = &snap2.worktrees[0];
    let order2 = wt2
        .files
        .iter()
        .find(|f| f.path == "src/service/Order.ts")
        .unwrap();
    let target2 = ReadTarget {
        scope_version: wt2.status_version.clone(),
        ..target.clone()
    };

    let conf2 = coordinator
        .start_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &StartCPushRequest {
                request_id: "cpush-manual-1".into(),
                target: target2.clone(),
                model_id: saved.model.id.clone(),
                selections: vec![CommitSelection {
                    file_id: order2.id.clone(),
                    commit_token: order2.commit_token.clone(),
                }],
                draft: String::new(),
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(conf2.session.session_status, CPushSessionStatus::Conflict);

    // User chooses `我手动合并`:
    let waiting = coordinator
        .respond_cpush(
            temp.path(),
            &paths,
            &supervisor,
            &models,
            &repo_store,
            &RespondCPushRequest {
                request_id: "cpush-choose-manual".into(),
                target: target2.clone(),
                run_id: conf2.session.run_id.clone(),
                message_id: conf2.session.message.unwrap().id,
                option_id: Some("manual-merge".into()),
                text: None,
                model_id: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(
        waiting.session.session_status,
        CPushSessionStatus::WaitingManual
    );

    // Return check A: still conflicted -> stays `WaitingManual`!
    let still_waiting = coordinator
        .reconcile_worktree(
            temp.path(),
            &repo_store,
            &ReconcileCPushRequest {
                request_id: "reconcile-still-conflicted".into(),
                target: target2.clone(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        still_waiting.session.session_status,
        CPushSessionStatus::WaitingManual
    );

    // User resolves conflict in IDE & finishes `git rebase --continue` without pushing:
    fs::write(
        pair.clone_a.join("src/service/Order.ts"),
        "export function handleOrder() {\n  return 'manual-merged-v3';\n}\n",
    )
    .unwrap();
    git(&pair.clone_a, &["add", "--", "src/service/Order.ts"]).unwrap();
    git(
        &pair.clone_a,
        &["-c", "core.editor=:", "rebase", "--continue"],
    )
    .unwrap();

    // Return check B: resolved & unpushed -> asks `"冲突已解决，是否继续推送？"` (`NeedsDecision` / `resolved`)!
    let resolved_unpushed = coordinator
        .reconcile_worktree(
            temp.path(),
            &repo_store,
            &ReconcileCPushRequest {
                request_id: "reconcile-resolved-unpushed".into(),
                target: target2.clone(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        resolved_unpushed.session.session_status,
        CPushSessionStatus::NeedsDecision
    );
    assert_eq!(
        resolved_unpushed.session.message.as_ref().unwrap().kind,
        "resolved"
    );
    assert_eq!(resolved_unpushed.session.auto_collapse, None);

    // Put session back into `WaitingManual` briefly or push in terminal while in `PushFailed` / `WaitingManual` to verify R04 terminal-pushed auto-collapse!
    // Let's also test terminal push while in `PushFailed` / `WaitingManual`:
    // Suppose user pushes from terminal right now:
    git(&pair.clone_a, &["push", "origin", "feat/order"]).unwrap();
    // Simulate returning when session was `WaitingManual`:
    let mut raw_file: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(temp.path().join("cpush-sessions.json")).unwrap())
            .unwrap();
    raw_file["sessions"][0]["sessionStatus"] = serde_json::json!("waitingManual");
    fs::write(
        temp.path().join("cpush-sessions.json"),
        serde_json::to_vec_pretty(&raw_file).unwrap(),
    )
    .unwrap();
    let cold_for_ext = CPushCoordinator::default();
    let live_snap = repo_store.refresh(&target2).await.unwrap();
    cold_for_ext
        .restore(temp.path(), &[live_snap])
        .await
        .unwrap();

    let ext_pushed = cold_for_ext
        .reconcile_worktree(
            temp.path(),
            &repo_store,
            &ReconcileCPushRequest {
                request_id: "reconcile-terminal-pushed".into(),
                target: target2.clone(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ext_pushed.session.session_status, CPushSessionStatus::Done);
    assert_eq!(ext_pushed.session.auto_collapse, Some(true));
    assert!(
        ext_pushed
            .session
            .message
            .as_ref()
            .unwrap()
            .title
            .contains("终端已完成")
    );

    // 3. W04: Simulated app crash during `Running` push phase restores as `PushFailed` requiring user confirmation, NEVER auto-pushing!
    raw_file["sessions"][0]["sessionStatus"] = serde_json::json!("running");
    raw_file["sessions"][0]["pushAuthorized"] = serde_json::json!(true);
    fs::write(
        temp.path().join("cpush-sessions.json"),
        serde_json::to_vec_pretty(&raw_file).unwrap(),
    )
    .unwrap();
    let crash_recovery = CPushCoordinator::default();
    let latest_snap = repo_store.refresh(&target2).await.unwrap();
    let (recovered_list, warn) = crash_recovery
        .restore(temp.path(), &[latest_snap])
        .await
        .unwrap();
    assert_eq!(warn, None);
    assert_eq!(
        recovered_list[0].session_status,
        CPushSessionStatus::PushFailed
    );
    assert!(
        recovered_list[0]
            .message
            .as_ref()
            .unwrap()
            .title
            .contains("中断")
    );
}
