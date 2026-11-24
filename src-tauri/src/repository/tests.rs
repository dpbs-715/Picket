use super::smoke::{Fixture, git};
use super::*;

fn active(snapshot: &RepositorySnapshot) -> &WorktreeSnapshot {
    snapshot
        .worktrees
        .iter()
        .find(|tree| tree.worktree_id == snapshot.active_worktree_id)
        .unwrap()
}
#[tokio::test]
async fn reads_real_partial_range_without_changing_index_or_draft() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let tree = active(&snapshot);
    let partial = tree
        .files
        .iter()
        .find(|file| file.path == "part.txt")
        .unwrap();
    assert_eq!(partial.range.kind, "partial");
    assert_eq!((partial.range.added, partial.range.removed), (1, 1));
    assert!(
        tree.files
            .iter()
            .find(|file| file.path == "pnpm-lock.yaml")
            .unwrap()
            .ignored_for_review
    );
    assert_eq!(
        tree.files
            .iter()
            .find(|file| file.path == "\u{65b0} \u{6587}\u{4ef6}.ts")
            .unwrap()
            .range
            .added,
        1
    );
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "test".into(),
        scope_version: tree.status_version.clone(),
    };
    let refreshed = store.refresh(&target).await.unwrap();
    assert_eq!(active(&refreshed).status_version, tree.status_version);
    assert!(refreshed.read_sequence > snapshot.read_sequence);
    fixture.verify_unchanged().unwrap();
}
#[tokio::test]
async fn discovers_linked_worktrees_and_occupied_branches_with_stable_ids() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let first = store.open(&fixture.root).await.unwrap();
    let linked = first
        .worktrees
        .iter()
        .find(|tree| tree.branch == "feat/linked")
        .unwrap();
    assert!(linked.files.is_empty());
    assert_eq!(
        first
            .repository
            .branches
            .iter()
            .find(|branch| branch.name == "feat/linked")
            .unwrap()
            .worktree_id
            .as_deref(),
        Some(linked.worktree_id.as_str())
    );
    let second = store.open(Path::new(&linked.path)).await.unwrap();
    assert_eq!(first.repository.id, second.repository.id);
    assert_eq!(linked.worktree_id, second.active_worktree_id);
}
#[tokio::test]
async fn partial_scope_does_not_include_later_unstaged_draft_changes() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let first = store.open(&fixture.root).await.unwrap();
    fs::write(
        fixture.root.join("part.txt"),
        "first=2\nsecond=another-uncommitted-draft\nmore=draft\n",
    )
    .unwrap();
    let second = store.open(&fixture.root).await.unwrap();
    assert_eq!(
        active(&first)
            .files
            .iter()
            .find(|file| file.path == "part.txt")
            .unwrap()
            .content_version,
        active(&second)
            .files
            .iter()
            .find(|file| file.path == "part.txt")
            .unwrap()
            .content_version
    );
    assert_eq!(
        active(&first).status_version,
        active(&second).status_version
    );
}
#[tokio::test]
async fn staged_rename_keeps_both_paths_with_unicode_spaces_and_newlines() {
    let fixture = Fixture::new().unwrap();
    git(
        &fixture.root,
        &[
            "mv",
            "--",
            "pnpm-lock.yaml",
            "\u{65b0} \u{540d}\n\u{5b57}.lock",
        ],
    )
    .unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let renamed = active(&snapshot)
        .files
        .iter()
        .find(|file| file.status == "R")
        .unwrap();
    assert_eq!(renamed.path, "\u{65b0} \u{540d}\n\u{5b57}.lock");
    assert_eq!(renamed.previous_path.as_deref(), Some("pnpm-lock.yaml"));
}
#[tokio::test]
async fn binary_untracked_file_is_not_presented_as_reviewed_text() {
    let fixture = Fixture::new().unwrap();
    fs::write(fixture.root.join("binary.dat"), [1, 0, 2]).unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let binary = active(&snapshot)
        .files
        .iter()
        .find(|file| file.path == "binary.dat")
        .unwrap();
    assert!(binary.binary);
    assert_eq!(binary.range.added, 0);
}
#[tokio::test]
async fn preserves_symlinks_without_reading_their_outside_targets() {
    let fixture = Fixture::new().unwrap();
    std::os::unix::fs::symlink(
        "/definitely-missing-picket-outside-file",
        fixture.root.join("outside-link"),
    )
    .unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    assert_eq!(
        active(&snapshot)
            .files
            .iter()
            .find(|file| file.path == "outside-link")
            .unwrap()
            .range
            .added,
        1
    );
}
#[tokio::test]
async fn supports_unborn_branch_but_rejects_non_repository_and_bare_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let store = RepositoryStore::default();
    let non_git_err = store.open(temporary.path()).await.unwrap_err();
    assert!(non_git_err.contains("not managed by Git"), "{non_git_err}");
    git(temporary.path(), &["init", "-b", "main"]).unwrap();
    fs::write(temporary.path().join("new.txt"), "hello\n").unwrap();
    let snapshot = store.open(temporary.path()).await.unwrap();
    assert_eq!(active(&snapshot).branch, "main");
    assert!(active(&snapshot).head.is_none());
    assert_eq!(active(&snapshot).files.len(), 1);
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare"]).unwrap();
    let bare_err = store.open(bare.path()).await.unwrap_err();
    assert!(bare_err.contains("bare Git repository"), "{bare_err}");
}
#[tokio::test]
async fn missing_worktree_is_explicitly_unavailable_not_clean() {
    let fixture = Fixture::new().unwrap();
    fs::remove_dir_all(fixture.root.parent().unwrap().join("linked worktree")).unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    assert!(
        snapshot
            .worktrees
            .iter()
            .find(|tree| tree.branch == "feat/linked")
            .unwrap()
            .read_error
            .is_some()
    );
}
#[tokio::test]
async fn rejects_unknown_targets_and_rereads_reinitialized_repository() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let first = store.open(&fixture.root).await.unwrap();
    let target = ReadTarget {
        repository_id: "unknown".into(),
        worktree_id: first.active_worktree_id.clone(),
        session_id: "test".into(),
        scope_version: "test".into(),
    };
    assert!(store.refresh(&target).await.is_err());
    let target = ReadTarget {
        repository_id: first.repository.id,
        ..target
    };
    fs::rename(fixture.root.join(".git"), fixture.root.join("old-git")).unwrap();
    git(&fixture.root, &["init", "-b", "main"]).unwrap();
    // Same canonical .git location is the same local repository identity, but
    // real HEAD/branch/content versions still change; no write is authorized.
    let refreshed = store.refresh(&target).await.unwrap();
    assert_eq!(active(&refreshed).branch, "main");
}
#[tokio::test]
async fn reports_detached_head_without_inventing_a_branch_or_remote() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["checkout", "--detach"]).unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    assert!(active(&snapshot).detached);
    assert!(matches!(active(&snapshot).remote, Remote::Missing { .. }));
}
#[tokio::test]
async fn remote_counts_use_local_tracking_refs_without_fetch_or_origin_assumption() {
    let fixture = Fixture::new().unwrap();
    git(
        &fixture.root,
        &[
            "config",
            "remote.mirror.url",
            "https://no-network.picket.invalid/repository.git",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &[
            "config",
            "remote.mirror.fetch",
            "+refs/heads/*:refs/remotes/mirror/*",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &["config", "branch.feat/read-proof.remote", "mirror"],
    )
    .unwrap();
    git(
        &fixture.root,
        &[
            "config",
            "branch.feat/read-proof.merge",
            "refs/heads/feat/read-proof",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &["update-ref", "refs/remotes/mirror/feat/read-proof", "HEAD"],
    )
    .unwrap();
    git(
        &fixture.root,
        &["commit", "-m", "test: one local staged commit"],
    )
    .unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    match &active(&snapshot).remote {
        Remote::Tracked {
            branch,
            ahead,
            behind,
            local_only,
        } => {
            assert_eq!(branch, "mirror/feat/read-proof");
            assert_eq!((*ahead, *behind), (1, 0));
            assert!(local_only);
        }
        _ => panic!("Missing real tracking state"),
    }
    git(
        &fixture.root,
        &["update-ref", "-d", "refs/remotes/mirror/feat/read-proof"],
    )
    .unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    assert!(matches!(active(&snapshot).remote, Remote::Error { .. }));
}
#[tokio::test]
async fn detects_real_conflict_records() {
    let fixture = Fixture::new().unwrap();
    git(
        &fixture.root,
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
        &fixture.root,
        &["commit", "-m", "test: clean starting point"],
    )
    .unwrap();
    git(&fixture.root, &["branch", "side"]).unwrap();
    fs::write(fixture.root.join("part.txt"), "main-side\n").unwrap();
    git(&fixture.root, &["add", "--", "part.txt"]).unwrap();
    git(&fixture.root, &["commit", "-m", "test: main side"]).unwrap();
    git(&fixture.root, &["checkout", "side"]).unwrap();
    fs::write(fixture.root.join("part.txt"), "other-side\n").unwrap();
    git(&fixture.root, &["add", "--", "part.txt"]).unwrap();
    git(&fixture.root, &["commit", "-m", "test: other side"]).unwrap();
    assert!(git(&fixture.root, &["merge", "feat/read-proof"]).is_err());
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    assert!(
        active(&snapshot)
            .files
            .iter()
            .any(|file| file.status == "U")
    );
}
#[tokio::test]
async fn does_not_execute_external_diff_textconv_or_fsmonitor_helpers() {
    let fixture = Fixture::new().unwrap();
    let marker = fixture.root.join("must-not-run");
    let hook = fixture.root.join("hook.sh");
    fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    for setting in ["diff.external", "diff.proof.textconv", "core.fsmonitor"] {
        git(&fixture.root, &["config", setting, hook.to_str().unwrap()]).unwrap();
    }
    fs::write(fixture.root.join(".gitattributes"), "part.txt diff=proof\n").unwrap();
    RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    assert!(!marker.exists());
    fixture.verify_unchanged().unwrap();
}
#[tokio::test]
async fn no_new_git_process_starts_after_shutdown() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    store.shutdown().await;
    assert!(store.open(&fixture.root).await.is_err());
    fixture.verify_unchanged().unwrap();
}
#[tokio::test]
async fn wire_format_uses_the_shared_camel_case_contract_and_no_credentials() {
    let fixture = Fixture::new().unwrap();
    let reply = ReadReply {
        protocol_version: 1,
        request_id: "test-read".into(),
        target: None,
        snapshot: Some(
            RepositoryStore::default()
                .open(&fixture.root)
                .await
                .unwrap(),
        ),
    };
    let json = serde_json::to_value(reply).unwrap();
    assert_eq!(json["protocolVersion"], 1);
    assert_eq!(json["requestId"], "test-read");
    assert!(json["snapshot"]["worktrees"][0]["statusVersion"].is_string());
    assert!(json["snapshot"]["worktrees"][0]["readError"].is_null());
    assert!(!json.to_string().contains("apiKey"));
    assert!(
        serde_json::from_value::<OpenRequest>(
            serde_json::json!({"requestId":"test", "path":"/arbitrary"})
        )
        .is_err()
    );
}
#[tokio::test]
async fn shutdown_cancels_and_reaps_in_flight_git_reads() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let (read, ()) = tokio::join!(store.open(&fixture.root), async {
            tokio::task::yield_now().await;
            store.shutdown().await;
        });
        assert!(read.is_err());
    })
    .await
    .expect("Read shutdown must not leave a Git child waiting");
    fixture.verify_unchanged().unwrap();
}
#[test]
fn rejects_unsafe_paths_and_invalid_request_ids() {
    for path in ["../outside", "/absolute", "", "safe/../outside"] {
        assert!(parse::relative(path).is_err());
    }
    assert!(validate_request_id("request-123").is_ok());
    assert!(validate_request_id("../other").is_err());
}
