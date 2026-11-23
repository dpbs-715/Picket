use super::smoke::{Fixture, git};
use super::*;
use std::os::unix::fs::PermissionsExt;

fn active(snapshot: &RepositorySnapshot) -> &WorktreeSnapshot {
    snapshot
        .worktrees
        .iter()
        .find(|tree| tree.worktree_id == snapshot.active_worktree_id)
        .unwrap()
}
fn request(snapshot: &RepositorySnapshot, branch: &str) -> BranchRequest {
    static REQUEST: AtomicU64 = AtomicU64::new(0);
    let tree = active(snapshot);
    BranchRequest {
        request_id: format!("branch-test-{}", REQUEST.fetch_add(1, Ordering::SeqCst)),
        target: ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "branch-tests".into(),
            scope_version: "not-a-write-authorization".into(),
        },
        action: BranchAction::SwitchBranch,
        branch: branch.into(),
        branch_token: tree.branch_token.clone().unwrap(),
    }
}
fn index(root: &Path) -> Vec<u8> {
    git(root, &["ls-files", "--stage", "-z"]).unwrap()
}
fn branch(root: &Path) -> String {
    String::from_utf8(git(root, &["symbolic-ref", "--short", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .into()
}
fn assert_drafts(fixture: &Fixture, expected_index: &[u8]) {
    assert_eq!(index(&fixture.root), expected_index);
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert_eq!(
        git(&fixture.root, &["show", ":part.txt"]).unwrap(),
        b"first=2\nsecond=1\n"
    );
    assert_eq!(
        fs::read(fixture.root.join("新 文件.ts")).unwrap(),
        b"export const proof = true;\n"
    );
    assert_eq!(
        fs::read(fixture.root.join("pnpm-lock.yaml")).unwrap(),
        b"lock: updated\n"
    );
}

#[tokio::test]
async fn switches_same_head_branch_with_mixed_staging_and_jumps_to_occupied_worktree() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["branch", "feat/existing"]).unwrap();
    let before_index = index(&fixture.root);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let changed = store
        .change_branch(&request(&snapshot, "feat/existing"))
        .await
        .unwrap();
    assert_eq!(active(&changed).branch, "feat/existing");
    assert_drafts(&fixture, &before_index);
    let jumped = store
        .change_branch(&request(&changed, "feat/linked"))
        .await
        .unwrap();
    assert_ne!(jumped.active_worktree_id, changed.active_worktree_id);
    assert_eq!(active(&jumped).branch, "feat/linked");
    assert_eq!(branch(&fixture.root), "feat/existing");
    assert_drafts(&fixture, &before_index);
}

#[tokio::test]
async fn clean_worktree_can_switch_to_a_different_commit() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["branch", "feat/old"]).unwrap();
    git(
        &fixture.root,
        &["add", "--", "part.txt", "pnpm-lock.yaml", "新 文件.ts"],
    )
    .unwrap();
    git(&fixture.root, &["commit", "-m", "test: clean newer head"]).unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let result = store
        .change_branch(&request(&snapshot, "feat/old"))
        .await
        .unwrap();
    assert_eq!(active(&result).branch, "feat/old");
    assert_ne!(active(&result).head, active(&snapshot).head);
    assert!(active(&result).files.is_empty());
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=1\nsecond=1\n"
    );
}

#[tokio::test]
async fn refuses_dirty_cross_commit_checkout_without_stash_or_reset() {
    let fixture = Fixture::new().unwrap();
    let linked = fixture.root.parent().unwrap().join("linked worktree");
    fs::write(linked.join("new.txt"), "other branch\n").unwrap();
    git(&linked, &["add", "new.txt"]).unwrap();
    git(&linked, &["commit", "-m", "test: different head"]).unwrap();
    git(&linked, &["branch", "feat/different"]).unwrap();
    let before_index = index(&fixture.root);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let error = store
        .change_branch(&request(&snapshot, "feat/different"))
        .await
        .unwrap_err();
    assert!(error.contains("不同提交"), "{error}");
    assert_eq!(branch(&fixture.root), "feat/read-proof");
    assert_drafts(&fixture, &before_index);
    assert!(git(&fixture.root, &["stash", "list"]).unwrap().is_empty());
}

#[tokio::test]
async fn rejects_stale_head_branch_index_and_unstaged_half_of_a_partial_file() {
    for change in ["head", "branch", "index", "draft"] {
        let fixture = Fixture::new().unwrap();
        git(&fixture.root, &["branch", "feat/target"]).unwrap();
        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let operation = request(&snapshot, "feat/target");
        match change {
            "head" => {
                git(&fixture.root, &["commit", "-m", "test: outside commit"]).unwrap();
            }
            "branch" => {
                git(&fixture.root, &["switch", "-c", "outside-branch"]).unwrap();
            }
            "index" => {
                git(&fixture.root, &["add", "part.txt"]).unwrap();
            }
            _ => fs::write(
                fixture.root.join("part.txt"),
                "first=2\nsecond=later-draft\n",
            )
            .unwrap(),
        }
        let before_index = index(&fixture.root);
        let before_draft = fs::read(fixture.root.join("part.txt")).unwrap();
        let error = store.change_branch(&operation).await.unwrap_err();
        assert!(error.contains("变化"), "{change}: {error}");
        assert_eq!(index(&fixture.root), before_index);
        assert_eq!(
            fs::read(fixture.root.join("part.txt")).unwrap(),
            before_draft
        );
    }
}

#[tokio::test]
async fn refuses_hooks_filters_locks_and_existing_git_operations_without_executing_helpers() {
    for guard in [
        "hook",
        "default-hook",
        "relative-hook",
        "filter",
        "index.lock",
        "MERGE_HEAD",
        "rebase-merge",
        "hidden-index",
    ] {
        let fixture = Fixture::new().unwrap();
        git(&fixture.root, &["branch", "feat/blocked"]).unwrap();
        let marker = fixture.root.parent().unwrap().join("must-not-run");
        if ["hook", "default-hook", "relative-hook", "filter"].contains(&guard) {
            let hooks = if guard == "default-hook" {
                fixture.root.join(".git/hooks")
            } else {
                fixture.root.parent().unwrap().join("custom hooks")
            };
            fs::create_dir_all(&hooks).unwrap();
            let script = hooks.join("post-checkout");
            fs::write(
                &script,
                format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            if guard == "hook" || guard == "relative-hook" {
                let configured = if guard == "relative-hook" {
                    "../custom hooks"
                } else {
                    hooks.to_str().unwrap()
                };
                git(&fixture.root, &["config", "core.hooksPath", configured]).unwrap();
            } else if guard == "filter" {
                git(
                    &fixture.root,
                    &["config", "filter.external.smudge", script.to_str().unwrap()],
                )
                .unwrap();
                fs::write(
                    fixture.root.join(".gitattributes"),
                    "part.txt filter=external\n",
                )
                .unwrap();
            }
        } else if guard == "hidden-index" {
            git(
                &fixture.root,
                &["update-index", "--assume-unchanged", "pnpm-lock.yaml"],
            )
            .unwrap();
        } else if guard == "rebase-merge" {
            fs::create_dir(fixture.root.join(".git/rebase-merge")).unwrap();
        } else {
            fs::write(fixture.root.join(".git").join(guard), "blocked\n").unwrap();
        }
        let expected_index = fs::read(fixture.root.join(".git/index")).unwrap();
        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        assert!(
            store
                .change_branch(&request(&snapshot, "feat/blocked"))
                .await
                .is_err(),
            "{guard}"
        );
        assert_eq!(branch(&fixture.root), "feat/read-proof");
        assert!(!marker.exists());
        assert_eq!(
            fs::read(fixture.root.join(".git/index")).unwrap(),
            expected_index
        );
        assert_eq!(
            fs::read(fixture.root.join("part.txt")).unwrap(),
            b"first=2\nsecond=3\n"
        );
    }
}

#[tokio::test]
async fn rejects_invalid_and_missing_branches() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    for name in [
        "",
        "-bad",
        "a b",
        "HEAD",
        "../outside",
        "feat/../outside",
        "@{-1}",
        "missing",
    ] {
        assert!(
            store
                .change_branch(&request(&snapshot, name))
                .await
                .is_err(),
            "{name}"
        );
    }
    fixture.verify_unchanged().unwrap();
}

#[tokio::test]
async fn repeated_and_concurrent_requests_never_write_twice() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["branch", "feat/once"]).unwrap();
    git(&fixture.root, &["branch", "feat/one"]).unwrap();
    git(&fixture.root, &["branch", "feat/two"]).unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let operation = request(&snapshot, "feat/once");
    let (first, duplicate) = tokio::join!(
        store.change_branch(&operation),
        store.change_branch(&operation)
    );
    assert_ne!(first.is_ok(), duplicate.is_ok());
    assert!(
        store
            .change_branch(&operation)
            .await
            .unwrap_err()
            .contains("不会重复")
    );
    let current = store.open(&fixture.root).await.unwrap();
    let one = request(&current, "feat/one");
    let two = request(&current, "feat/two");
    let (first, second) = tokio::join!(store.change_branch(&one), store.change_branch(&two));
    assert_ne!(first.is_ok(), second.is_ok());
}

#[tokio::test]
async fn requires_registered_target_and_rust_token_not_ui_scope_and_rejects_after_shutdown() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["branch", "feat/target"]).unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let mut operation = request(&snapshot, "feat/target");
    operation.branch_token = operation.target.scope_version.clone();
    assert!(store.change_branch(&operation).await.is_err());
    let mut operation = request(&snapshot, "feat/target");
    operation.target.repository_id = "other-repository".into();
    assert!(store.change_branch(&operation).await.is_err());
    store.shutdown().await;
    assert!(
        store
            .change_branch(&request(&snapshot, "feat/target"))
            .await
            .is_err()
    );
    fixture.verify_unchanged().unwrap();
}

#[tokio::test]
async fn branch_switch_preserves_renames_binary_files_and_symlinks() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["branch", "feat/mixed"]).unwrap();
    git(
        &fixture.root,
        &["mv", "--", "pnpm-lock.yaml", "新 名\n字.lock"],
    )
    .unwrap();
    fs::write(fixture.root.join("binary.dat"), [0, 1, 2]).unwrap();
    std::os::unix::fs::symlink(
        "/missing-external-target",
        fixture.root.join("outside-link"),
    )
    .unwrap();
    git(&fixture.root, &["add", "--", "binary.dat", "outside-link"]).unwrap();
    let expected_index = index(&fixture.root);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let changed = store
        .change_branch(&request(&snapshot, "feat/mixed"))
        .await
        .unwrap();
    assert_eq!(active(&changed).head, active(&snapshot).head);
    assert_eq!(index(&fixture.root), expected_index);
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert_eq!(
        fs::read(fixture.root.join("新 名\n字.lock")).unwrap(),
        b"lock: updated\n"
    );
    assert_eq!(
        fs::read(fixture.root.join("binary.dat")).unwrap(),
        [0, 1, 2]
    );
    assert_eq!(
        fs::read_link(fixture.root.join("outside-link")).unwrap(),
        Path::new("/missing-external-target")
    );
}

#[tokio::test]
async fn refuses_filter_needed_only_by_the_target_branch_but_allows_unused_filter_config() {
    let fixture = Fixture::new().unwrap();
    let linked = fixture.root.parent().unwrap().join("linked worktree");
    git(&linked, &["branch", "feat/unused-filter"]).unwrap();
    let marker = fixture.root.parent().unwrap().join("filter-must-not-run");
    let script = fixture.root.parent().unwrap().join("filter.sh");
    fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        fixture.root.join(".gitattributes"),
        "part.txt filter=external\n",
    )
    .unwrap();
    git(
        &fixture.root,
        &[
            "add",
            "--",
            "part.txt",
            "pnpm-lock.yaml",
            "新 文件.ts",
            ".gitattributes",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &["commit", "-m", "test: target needs filter"],
    )
    .unwrap();
    git(&fixture.root, &["branch", "feat/filtered"]).unwrap();
    git(
        &fixture.root,
        &["config", "filter.external.smudge", script.to_str().unwrap()],
    )
    .unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&linked).await.unwrap();
    let error = store
        .change_branch(&request(&snapshot, "feat/filtered"))
        .await
        .unwrap_err();
    assert!(error.contains("过滤器"), "{error}");
    assert_eq!(branch(&linked), "feat/linked");
    assert!(!marker.exists());
    // This worktree has no filter attributes at HEAD, index or in its files.
    let result = store
        .change_branch(&request(&snapshot, "feat/unused-filter"))
        .await
        .unwrap();
    assert_eq!(active(&result).branch, "feat/unused-filter");
    assert!(!marker.exists());
}

#[tokio::test]
async fn checkout_never_overwrites_an_ignored_user_file() {
    let fixture = Fixture::new().unwrap();
    let linked = fixture.root.parent().unwrap().join("linked worktree");
    fs::write(fixture.root.join("cache.txt"), "target branch content\n").unwrap();
    git(
        &fixture.root,
        &[
            "add",
            "--",
            "part.txt",
            "pnpm-lock.yaml",
            "新 文件.ts",
            "cache.txt",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &["commit", "-m", "test: target tracks an ignored file"],
    )
    .unwrap();
    git(&fixture.root, &["branch", "feat/with-cache"]).unwrap();
    fs::write(fixture.root.join(".git/info/exclude"), "cache.txt\n").unwrap();
    fs::write(linked.join("cache.txt"), "ignored user draft, must stay\n").unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&linked).await.unwrap();
    assert!(active(&snapshot).files.is_empty());
    assert!(
        store
            .change_branch(&request(&snapshot, "feat/with-cache"))
            .await
            .is_err()
    );
    assert_eq!(branch(&linked), "feat/linked");
    assert_eq!(
        fs::read(linked.join("cache.txt")).unwrap(),
        b"ignored user draft, must stay\n"
    );
}

#[tokio::test]
async fn can_switch_existing_branch_from_detached_head_and_explicitly_refuses_unborn_repository() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["branch", "feat/from-detached"]).unwrap();
    git(&fixture.root, &["checkout", "--detach"]).unwrap();
    let expected_index = index(&fixture.root);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let changed = store
        .change_branch(&request(&snapshot, "feat/from-detached"))
        .await
        .unwrap();
    assert!(!active(&changed).detached);
    assert_eq!(active(&changed).branch, "feat/from-detached");
    assert_drafts(&fixture, &expected_index);
    let unborn = tempfile::tempdir().unwrap();
    git(unborn.path(), &["init", "-b", "main"]).unwrap();
    fs::write(unborn.path().join("draft.txt"), "keep\n").unwrap();
    let snapshot = store.open(unborn.path()).await.unwrap();
    let error = store
        .change_branch(&request(&snapshot, "main"))
        .await
        .unwrap_err();
    assert!(error.contains("首次提交"), "{error}");
    assert_eq!(branch(unborn.path()), "main");
    assert_eq!(
        fs::read(unborn.path().join("draft.txt")).unwrap(),
        b"keep\n"
    );
}

#[tokio::test]
async fn refuses_custom_hooks_even_when_executable_hook_exists_only_in_target_tree() {
    let fixture = Fixture::new().unwrap();
    let linked = fixture.root.parent().unwrap().join("linked worktree");
    let marker = fixture
        .root
        .parent()
        .unwrap()
        .join("target-hook-must-not-run");
    fs::create_dir(fixture.root.join(".custom-hooks")).unwrap();
    let hook = fixture.root.join(".custom-hooks/post-checkout");
    fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &fixture.root,
        &[
            "add",
            "--",
            ".custom-hooks/post-checkout",
            "part.txt",
            "pnpm-lock.yaml",
            "新 文件.ts",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &["commit", "-m", "test: target introduces hook"],
    )
    .unwrap();
    git(&fixture.root, &["branch", "feat/target-hook"]).unwrap();
    let target_hooks = linked.join(".custom-hooks");
    git(
        &fixture.root,
        &["config", "core.hooksPath", target_hooks.to_str().unwrap()],
    )
    .unwrap();
    assert!(!target_hooks.exists());
    let store = RepositoryStore::default();
    let snapshot = store.open(&linked).await.unwrap();
    let error = store
        .change_branch(&request(&snapshot, "feat/target-hook"))
        .await
        .unwrap_err();
    assert!(error.contains("hooksPath"), "{error}");
    assert!(!marker.exists());
    assert_eq!(branch(&linked), "feat/linked");
    assert!(!target_hooks.exists());
}

#[tokio::test]
async fn lists_remote_branches_without_head_symref_and_checks_out_tracking_branch() {
    let fixture = Fixture::new().unwrap();
    let before_index = index(&fixture.root);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    assert!(
        !snapshot
            .repository
            .remote_branches
            .iter()
            .any(|item| item.name.ends_with("/HEAD") || item.local_name == "HEAD"),
        "remote_branches must exclude origin/HEAD symbolic ref: {:?}",
        snapshot.repository.remote_branches
    );
    assert!(
        snapshot.repository.remote_branches.contains(&RemoteBranch {
            name: "origin/feat/remote-checkout".into(),
            remote: "origin".into(),
            local_name: "feat/remote-checkout".into(),
        }),
        "missing origin/feat/remote-checkout in {:?}",
        snapshot.repository.remote_branches
    );

    let mut checkout_req = request(&snapshot, "origin/feat/remote-checkout");
    checkout_req.action = BranchAction::CheckoutRemoteBranch;
    let checked_out = store.change_branch(&checkout_req).await.unwrap();
    assert_eq!(active(&checked_out).branch, "feat/remote-checkout");
    assert_eq!(
        git(
            &fixture.root,
            &["config", "--get", "branch.feat/remote-checkout.remote"]
        )
        .unwrap(),
        b"origin\n"
    );
    assert_eq!(
        git(
            &fixture.root,
            &["config", "--get", "branch.feat/remote-checkout.merge"]
        )
        .unwrap(),
        b"refs/heads/feat/remote-checkout\n"
    );
    assert_drafts(&fixture, &before_index);

    // Checking out a remote branch whose local branch already exists switches to that local branch:
    let mut switch_existing_req = request(&checked_out, "origin/feat/branch-proof");
    switch_existing_req.action = BranchAction::CheckoutRemoteBranch;
    let switched_existing = store.change_branch(&switch_existing_req).await.unwrap();
    assert_eq!(active(&switched_existing).branch, "feat/branch-proof");
    assert_drafts(&fixture, &before_index);

    // Refuses symbolic HEAD remote checkout:
    let mut symref_req = request(&switched_existing, "origin/HEAD");
    symref_req.action = BranchAction::CheckoutRemoteBranch;
    assert!(store.change_branch(&symref_req).await.is_err());

    // Refuses dirty cross-commit remote checkout without stash or reset:
    let linked = fixture.root.parent().unwrap().join("linked worktree");
    fs::write(linked.join("remote-ahead.txt"), "ahead\n").unwrap();
    git(&linked, &["add", "remote-ahead.txt"]).unwrap();
    git(&linked, &["commit", "-m", "test: remote ahead"]).unwrap();
    git(
        &linked,
        &["push", "origin", "HEAD:refs/heads/feat/remote-different"],
    )
    .unwrap();
    git(&fixture.root, &["fetch", "origin"]).unwrap();
    let refreshed = store.open(&fixture.root).await.unwrap();
    let mut dirty_remote_req = request(&refreshed, "origin/feat/remote-different");
    dirty_remote_req.action = BranchAction::CheckoutRemoteBranch;
    let error = store.change_branch(&dirty_remote_req).await.unwrap_err();
    assert!(error.contains("不同提交"), "{error}");
    assert_eq!(branch(&fixture.root), "feat/branch-proof");
    assert_drafts(&fixture, &before_index);
}

#[test]
fn branch_wire_format_never_accepts_paths_commands_or_creation_actions() {
    let base = serde_json::json!({"requestId":"branch-request", "target":{"repositoryId":"r", "worktreeId":"w", "sessionId":"s", "scopeVersion":"ui"}, "action":"switchBranch", "branch":"feat/existing", "branchToken":"rust-token"});
    assert!(serde_json::from_value::<BranchRequest>(base.clone()).is_ok());
    let mut remote_checkout = base.clone();
    remote_checkout["action"] = serde_json::json!("checkoutRemoteBranch");
    remote_checkout["branch"] = serde_json::json!("origin/feat/existing");
    assert!(serde_json::from_value::<BranchRequest>(remote_checkout).is_ok());
    for action in ["createBranch", "createWorktree"] {
        let mut removed = base.clone();
        removed["action"] = serde_json::json!(action);
        assert!(serde_json::from_value::<BranchRequest>(removed).is_err());
    }
    for field in ["path", "parent", "shell", "force"] {
        let mut invalid = base.clone();
        invalid[field] = serde_json::json!("untrusted");
        assert!(serde_json::from_value::<BranchRequest>(invalid).is_err());
    }
}
