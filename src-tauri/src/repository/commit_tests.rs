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

fn select(snapshot: &RepositorySnapshot, paths: &[&str]) -> Vec<CommitSelection> {
    let tree = active(snapshot);
    paths
        .iter()
        .map(|path| {
            let file = tree
                .files
                .iter()
                .find(|file| file.path == *path)
                .unwrap_or_else(|| panic!("missing file {path} in snapshot"));
            CommitSelection {
                file_id: file.id.clone(),
                commit_token: file.commit_token.clone(),
            }
        })
        .collect()
}

fn commit_request(snapshot: &RepositorySnapshot, paths: &[&str], message: &str) -> CommitRequest {
    static REQUEST: AtomicU64 = AtomicU64::new(0);
    let tree = active(snapshot);
    CommitRequest {
        request_id: format!("commit-test-{}", REQUEST.fetch_add(1, Ordering::SeqCst)),
        target: ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "commit-tests".into(),
            scope_version: "ui-scope-is-never-authorization".into(),
        },
        selections: select(snapshot, paths),
        message: message.into(),
        push: false,
    }
}

fn text(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git(root, args).unwrap())
        .unwrap()
        .trim_end_matches('\n')
        .to_owned()
}

#[tokio::test]
async fn commits_selected_partial_and_untracked_scope_while_preserving_unselected_staged_and_draft_bytes()
 {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["add", "--", "pnpm-lock.yaml"]).unwrap();
    git(
        &fixture.root,
        &[
            "config",
            "remote.origin.url",
            "https://no-network.picket.invalid/repository.git",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    )
    .unwrap();
    git(
        &fixture.root,
        &["update-ref", "refs/remotes/origin/feat/read-proof", "HEAD"],
    )
    .unwrap();
    git(
        &fixture.root,
        &["config", "branch.feat/read-proof.remote", "origin"],
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

    let store = RepositoryStore::default();
    let before = store.open(&fixture.root).await.unwrap();
    let parent = active(&before).head.clone();
    let req = commit_request(
        &before,
        &["part.txt", "新 文件.ts"],
        "feat(proof): 提交已暂存部分与新文件\n\n保留未选已暂存锁文件和未暂存草稿",
    );
    let (after, commit) = store.commit(&req).await.unwrap();

    assert_eq!(commit.parent, parent);
    assert_eq!(commit.branch, "feat/read-proof");
    assert_eq!(commit.title, "feat(proof): 提交已暂存部分与新文件");
    assert_eq!(commit.oid, text(&fixture.root, &["rev-parse", "HEAD"]));
    assert_eq!(text(&fixture.root, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(
        text(&fixture.root, &["show", "HEAD:part.txt"]),
        "first=2\nsecond=1"
    );
    assert_eq!(
        text(&fixture.root, &["show", ":part.txt"]),
        "first=2\nsecond=1"
    );
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=3\n"
    );
    assert_eq!(
        text(&fixture.root, &["show", "HEAD:新 文件.ts"]),
        "export const proof = true;"
    );
    assert_eq!(
        text(&fixture.root, &["show", "HEAD:pnpm-lock.yaml"]),
        "lock: old"
    );
    assert_eq!(
        text(&fixture.root, &["show", ":pnpm-lock.yaml"]),
        "lock: updated"
    );
    assert!(
        text(&fixture.root, &["log", "-g", "-1", "--oneline", "HEAD"])
            .contains("commit: feat(proof): 提交已暂存部分与新文件")
    );

    let updated_tree = active(&after);
    assert_eq!(updated_tree.head.as_deref(), Some(commit.oid.as_str()));
    match &updated_tree.remote {
        Remote::Tracked { ahead, behind, .. } => assert_eq!((*ahead, *behind), (1, 0)),
        other => panic!("expected tracked remote ahead=1, got {other:?}"),
    }
    let remaining_part = updated_tree
        .files
        .iter()
        .find(|file| file.path == "part.txt")
        .unwrap();
    assert_eq!(remaining_part.range.kind, "unstaged");
    assert_eq!(
        (remaining_part.range.added, remaining_part.range.removed),
        (1, 1)
    );
    let remaining_lock = updated_tree
        .files
        .iter()
        .find(|file| file.path == "pnpm-lock.yaml")
        .unwrap();
    assert_eq!(remaining_lock.range.kind, "staged");
    assert!(
        !updated_tree
            .files
            .iter()
            .any(|file| file.path == "新 文件.ts")
    );
}

#[tokio::test]
async fn commits_mixed_deletions_renames_symlinks_executables_binary_and_glob_paths() {
    let fixture = Fixture::new().unwrap();
    fs::write(fixture.root.join("delete-unstaged.txt"), "remove me\n").unwrap();
    git(&fixture.root, &["add", "--", "delete-unstaged.txt"]).unwrap();
    git(&fixture.root, &["commit", "-m", "test: add delete target"]).unwrap();

    fs::remove_file(fixture.root.join("delete-unstaged.txt")).unwrap();
    git(&fixture.root, &["rm", "-f", "--", "part.txt"]).unwrap();
    fs::write(fixture.root.join("part.txt"), "recreated draft\n").unwrap();
    let renamed = "新 名\n字.lock";
    git(&fixture.root, &["mv", "--", "pnpm-lock.yaml", renamed]).unwrap();
    let script = fixture.root.join("run [glob].sh");
    fs::write(&script, "#!/bin/sh\necho ok\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(fixture.root.join("payload.bin"), [0_u8, 159, 146, 150]).unwrap();
    std::os::unix::fs::symlink("run [glob].sh", fixture.root.join("script-link")).unwrap();

    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let (after, _) = store
        .commit(&commit_request(
            &snapshot,
            &[
                "delete-unstaged.txt",
                "part.txt",
                renamed,
                "run [glob].sh",
                "payload.bin",
                "script-link",
            ],
            "#42 feat: 支持混合增删重命名与特殊路径",
        ))
        .await
        .unwrap();

    assert_eq!(
        text(&fixture.root, &["show", "-s", "--format=%s", "HEAD"]),
        "#42 feat: 支持混合增删重命名与特殊路径"
    );
    assert!(git(&fixture.root, &["show", "HEAD:delete-unstaged.txt"]).is_err());
    assert!(git(&fixture.root, &["show", "HEAD:part.txt"]).is_err());
    assert!(git(&fixture.root, &["show", "HEAD:pnpm-lock.yaml"]).is_err());
    assert_eq!(
        git(&fixture.root, &["show", &format!("HEAD:{renamed}")]).unwrap(),
        b"lock: old\n"
    );
    assert_eq!(
        git(&fixture.root, &["show", "HEAD:payload.bin"]).unwrap(),
        vec![0_u8, 159, 146, 150]
    );
    let tree_entries = String::from_utf8(
        git(
            &fixture.root,
            &["ls-tree", "HEAD", "--", "run [glob].sh", "script-link"],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(tree_entries.contains("100755 blob "));
    assert!(tree_entries.contains("120000 blob "));
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"recreated draft\n"
    );
    assert_eq!(
        fs::read(fixture.root.join(renamed)).unwrap(),
        b"lock: updated\n"
    );
    let remaining_paths: Vec<_> = active(&after)
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.git_status.as_str()))
        .collect();
    assert_eq!(
        remaining_paths,
        vec![("part.txt", "??"), (renamed, ".M"), ("新 文件.ts", "??")]
    );
}

#[tokio::test]
async fn refuses_protected_branches_and_detached_head_then_commits_after_creating_branch() {
    for protected in ["main", "master"] {
        let fixture = Fixture::new().unwrap();
        git(&fixture.root, &["branch", "-m", protected]).unwrap();
        let before_index = git(&fixture.root, &["ls-files", "--stage", "-z"]).unwrap();
        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let error = store
            .commit(&commit_request(
                &snapshot,
                &["part.txt"],
                "feat: blocked on main",
            ))
            .await
            .unwrap_err();
        assert!(error.contains("保护分支"), "{error}");
        assert_eq!(text(&fixture.root, &["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(
            git(&fixture.root, &["ls-files", "--stage", "-z"]).unwrap(),
            before_index
        );

        let switched = store
            .change_branch(
                &BranchRequest {
                    request_id: format!("branch-from-{protected}"),
                    target: ReadTarget {
                        repository_id: active(&snapshot).repository_id.clone(),
                        worktree_id: active(&snapshot).worktree_id.clone(),
                        session_id: "w01".into(),
                        scope_version: "ui".into(),
                    },
                    action: BranchAction::CreateBranch,
                    branch: "feat/safe-dev".into(),
                    branch_token: active(&snapshot).branch_token.clone().unwrap(),
                },
                None,
            )
            .await
            .unwrap();
        let (_, commit) = store
            .commit(&commit_request(
                &switched,
                &["part.txt"],
                "feat: commit on dev branch",
            ))
            .await
            .unwrap();
        assert_eq!(commit.branch, "feat/safe-dev");
        assert_eq!(
            text(&fixture.root, &["show", "HEAD:part.txt"]),
            "first=2\nsecond=1"
        );
        assert_eq!(
            fs::read(fixture.root.join("part.txt")).unwrap(),
            b"first=2\nsecond=3\n"
        );
    }

    let detached_fixture = Fixture::new().unwrap();
    git(&detached_fixture.root, &["checkout", "--detach"]).unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&detached_fixture.root).await.unwrap();
    let error = store
        .commit(&commit_request(&snapshot, &["part.txt"], "feat: detached"))
        .await
        .unwrap_err();
    assert!(error.contains("分离 HEAD"), "{error}");
    assert_eq!(
        text(&detached_fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );
}

#[tokio::test]
async fn unrelated_changes_and_partial_draft_edits_succeed_while_stale_selected_scope_pauses() {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    // Unrelated unselected file staged + new untracked file + unstaged half of partial file edited:
    fs::write(
        fixture.root.join("pnpm-lock.yaml"),
        "lock: newer-unrelated\n",
    )
    .unwrap();
    git(&fixture.root, &["add", "--", "pnpm-lock.yaml"]).unwrap();
    fs::write(fixture.root.join("unrelated.md"), "keep me\n").unwrap();
    fs::write(
        fixture.root.join("part.txt"),
        "first=2\nsecond=updated-draft\n",
    )
    .unwrap();

    store
        .commit(&commit_request(
            &snapshot,
            &["part.txt"],
            "feat: commit partial only",
        ))
        .await
        .unwrap();
    assert_eq!(
        text(&fixture.root, &["show", "HEAD:part.txt"]),
        "first=2\nsecond=1"
    );
    assert_eq!(
        fs::read(fixture.root.join("part.txt")).unwrap(),
        b"first=2\nsecond=updated-draft\n"
    );
    assert_eq!(
        text(&fixture.root, &["show", ":pnpm-lock.yaml"]),
        "lock: newer-unrelated"
    );
    assert_eq!(
        text(&fixture.root, &["show", "HEAD:pnpm-lock.yaml"]),
        "lock: old"
    );

    for stale_case in [
        "unstaged-edit",
        "missing-literal",
        "chmod",
        "staged-index",
        "staged-becomes-partial",
        "head",
        "branch",
    ] {
        let fixture = Fixture::new().unwrap();
        if stale_case == "missing-literal" {
            fs::write(fixture.root.join("literal-missing.txt"), "missing").unwrap();
        } else if stale_case == "staged-becomes-partial" {
            fs::write(fixture.root.join("part.txt"), "first=2\nsecond=1\n").unwrap();
        }
        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let target_path = if stale_case == "missing-literal" {
            "literal-missing.txt"
        } else if stale_case == "unstaged-edit" || stale_case == "chmod" {
            "新 文件.ts"
        } else {
            "part.txt"
        };
        let operation = commit_request(&snapshot, &[target_path], "feat: must pause");
        match stale_case {
            "unstaged-edit" => {
                fs::write(
                    fixture.root.join("新 文件.ts"),
                    "export const proof = false;\n",
                )
                .unwrap();
            }
            "missing-literal" => {
                fs::remove_file(fixture.root.join("literal-missing.txt")).unwrap();
            }
            "chmod" => {
                fs::set_permissions(
                    fixture.root.join("新 文件.ts"),
                    fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            "staged-index" => {
                git(&fixture.root, &["add", "--", "part.txt"]).unwrap();
            }
            "staged-becomes-partial" => {
                fs::write(fixture.root.join("part.txt"), "first=2\nsecond=later\n").unwrap();
            }
            "head" => {
                git(&fixture.root, &["commit", "-m", "test: external commit"]).unwrap();
            }
            "branch" => {
                git(&fixture.root, &["switch", "-c", "feat/external"]).unwrap();
            }
            _ => unreachable!(),
        }
        let expected_commits = text(&fixture.root, &["rev-list", "--count", "HEAD"]);
        let error = store.commit(&operation).await.unwrap_err();
        assert!(
            error.contains("变化") || error.contains("暂停"),
            "{stale_case}: {error}"
        );
        assert_eq!(
            text(&fixture.root, &["rev-list", "--count", "HEAD"]),
            expected_commits,
            "{stale_case} unexpectedly wrote a commit"
        );
        assert!(!fixture.root.join(".git/index.lock").exists());
    }
}

#[tokio::test]
async fn executes_real_hooks_including_custom_hooks_path_and_aborts_on_rejection_or_hook_mutation()
{
    // 1. Passing pre-commit (with glob pathspec check on staged scope), commit-msg trailer, and post-commit:
    let fixture = Fixture::new().unwrap();
    let hooks_dir = fixture.root.join(".git/hooks");
    let pre_commit = hooks_dir.join("pre-commit");
    fs::write(
        &pre_commit,
        concat!(
            "#!/bin/sh\n",
            "set -e\n",
            "STAGED=$(/usr/bin/git diff --cached --name-only -- '*.txt')\n",
            "[ \"$STAGED\" = \"part.txt\" ]\n",
            "CONTENT=$(/usr/bin/git show :part.txt)\n",
            "[ \"$CONTENT\" = \"$(printf 'first=2\\nsecond=1')\" ]\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&pre_commit, fs::Permissions::from_mode(0o755)).unwrap();

    let commit_msg = hooks_dir.join("commit-msg");
    fs::write(
        &commit_msg,
        "#!/bin/sh\nprintf '\\nSigned-off-by: Hook <hook@picket.invalid>\\n' >> \"$1\"\n",
    )
    .unwrap();
    fs::set_permissions(&commit_msg, fs::Permissions::from_mode(0o755)).unwrap();

    let post_marker = fixture.root.parent().unwrap().join("post-commit-ran");
    let post_commit = hooks_dir.join("post-commit");
    fs::write(
        &post_commit,
        format!(
            "#!/bin/sh\n/usr/bin/git rev-parse HEAD > '{}'\n",
            post_marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&post_commit, fs::Permissions::from_mode(0o755)).unwrap();

    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let (_, created) = store
        .commit(&commit_request(
            &snapshot,
            &["part.txt"],
            "feat: verified by hooks",
        ))
        .await
        .unwrap();
    let body = text(&fixture.root, &["log", "-1", "--format=%B", "HEAD"]);
    assert!(
        body.contains("Signed-off-by: Hook <hook@picket.invalid>"),
        "{body}"
    );
    assert_eq!(
        fs::read_to_string(&post_marker).unwrap().trim(),
        created.oid
    );

    // 2. Custom relative core.hooksPath (.husky):
    let husky_fixture = Fixture::new().unwrap();
    let husky = husky_fixture.root.join(".husky");
    fs::create_dir(&husky).unwrap();
    fs::write(husky.join("pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(husky.join("pre-commit"), fs::Permissions::from_mode(0o755)).unwrap();
    git(&husky_fixture.root, &["config", "core.hooksPath", ".husky"]).unwrap();
    git(&husky_fixture.root, &["add", "--", ".husky/pre-commit"]).unwrap();
    git(&husky_fixture.root, &["commit", "-m", "chore: add husky"]).unwrap();
    let husky_store = RepositoryStore::default();
    let husky_snap = husky_store.open(&husky_fixture.root).await.unwrap();
    husky_store
        .commit(&commit_request(
            &husky_snap,
            &["新 文件.ts"],
            "feat: husky pass",
        ))
        .await
        .unwrap();
    assert_eq!(
        text(&husky_fixture.root, &["show", "HEAD:新 文件.ts"]),
        "export const proof = true;"
    );

    // 3. Hook rejection (pre-commit & commit-msg) keeps index, drafts, and HEAD intact:
    let reject_fixture = Fixture::new().unwrap();
    let reject_hook = reject_fixture.root.join(".git/hooks/commit-msg");
    fs::write(
        &reject_hook,
        "#!/bin/sh\necho 'missing ticket id' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&reject_hook, fs::Permissions::from_mode(0o755)).unwrap();
    let before_index = git(&reject_fixture.root, &["ls-files", "--stage", "-z"]).unwrap();
    let reject_store = RepositoryStore::default();
    let reject_snap = reject_store.open(&reject_fixture.root).await.unwrap();
    let error = reject_store
        .commit(&commit_request(
            &reject_snap,
            &["part.txt"],
            "feat: rejected",
        ))
        .await
        .unwrap_err();
    assert!(
        error.contains("commit-msg") && error.contains("missing ticket id"),
        "{error}"
    );
    assert_eq!(
        text(&reject_fixture.root, &["rev-list", "--count", "HEAD"]),
        "1"
    );
    assert_eq!(
        git(&reject_fixture.root, &["ls-files", "--stage", "-z"]).unwrap(),
        before_index
    );

    // 4. Hook mutating files + git add + exiting 0 MUST NOT write an unauthorized commit:
    for exit_code in [0, 1] {
        let mutating_fixture = Fixture::new().unwrap();
        let mutating_hook = mutating_fixture.root.join(".git/hooks/pre-commit");
        fs::write(
            &mutating_hook,
            format!(
                "#!/bin/sh\nprintf 'hook-injected\\n' >> part.txt\n/usr/bin/git add part.txt\nexit {exit_code}\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&mutating_hook, fs::Permissions::from_mode(0o755)).unwrap();
        let orig_index = git(&mutating_fixture.root, &["ls-files", "--stage", "-z"]).unwrap();
        let mutating_store = RepositoryStore::default();
        let mutating_snap = mutating_store.open(&mutating_fixture.root).await.unwrap();
        let error = mutating_store
            .commit(&commit_request(
                &mutating_snap,
                &["part.txt"],
                "feat: hook mutation",
            ))
            .await
            .unwrap_err();
        assert!(
            error.contains("钩子") && error.contains("修改"),
            "{exit_code}: {error}"
        );
        assert_eq!(
            text(&mutating_fixture.root, &["rev-list", "--count", "HEAD"]),
            "1",
            "hook exit {exit_code} must never advance HEAD when mutating content"
        );
        assert_eq!(
            git(&mutating_fixture.root, &["ls-files", "--stage", "-z"]).unwrap(),
            orig_index,
            "real .git/index must remain untouched when hook mutates selected.index"
        );
    }
}

#[tokio::test]
async fn refuses_locks_ongoing_operations_conflicts_submodules_sparse_flags_and_clean_filters() {
    for marker in ["index.lock", "HEAD.lock", "MERGE_HEAD", "rebase-merge"] {
        let fixture = Fixture::new().unwrap();
        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let marker_path = fixture.root.join(".git").join(marker);
        if marker == "rebase-merge" {
            fs::create_dir(&marker_path).unwrap();
        } else {
            fs::write(&marker_path, "locked").unwrap();
        }
        let error = store
            .commit(&commit_request(&snapshot, &["part.txt"], "feat: blocked"))
            .await
            .unwrap_err();
        assert!(error.contains(marker), "{marker}: {error}");
        assert_eq!(text(&fixture.root, &["rev-list", "--count", "HEAD"]), "1");
    }

    // Real conflict:
    let conflict_fixture = Fixture::new().unwrap();
    git(
        &conflict_fixture.root,
        &["add", "--", "part.txt", "pnpm-lock.yaml", "新 文件.ts"],
    )
    .unwrap();
    git(&conflict_fixture.root, &["commit", "-m", "test: base"]).unwrap();
    git(&conflict_fixture.root, &["branch", "other"]).unwrap();
    fs::write(conflict_fixture.root.join("part.txt"), "ours\n").unwrap();
    git(&conflict_fixture.root, &["commit", "-am", "test: ours"]).unwrap();
    git(&conflict_fixture.root, &["checkout", "other"]).unwrap();
    fs::write(conflict_fixture.root.join("part.txt"), "theirs\n").unwrap();
    git(&conflict_fixture.root, &["commit", "-am", "test: theirs"]).unwrap();
    assert!(git(&conflict_fixture.root, &["merge", "feat/read-proof"]).is_err());
    fs::write(conflict_fixture.root.join("clean.txt"), "clean\n").unwrap();
    let conflict_store = RepositoryStore::default();
    let conflict_snap = conflict_store.open(&conflict_fixture.root).await.unwrap();
    let conflict_err = conflict_store
        .commit(&commit_request(
            &conflict_snap,
            &["clean.txt"],
            "feat: blocked by conflict",
        ))
        .await
        .unwrap_err();
    assert!(
        conflict_err.contains("冲突") || conflict_err.contains("MERGE_HEAD"),
        "{conflict_err}"
    );

    // Unselected rename source slot collision:
    let rename_fixture = Fixture::new().unwrap();
    git(
        &rename_fixture.root,
        &["mv", "--", "pnpm-lock.yaml", "moved.lock"],
    )
    .unwrap();
    fs::write(
        rename_fixture.root.join("pnpm-lock.yaml"),
        "recreated at old slot\n",
    )
    .unwrap();
    let rename_store = RepositoryStore::default();
    let rename_snap = rename_store.open(&rename_fixture.root).await.unwrap();
    let slot_err = rename_store
        .commit(&commit_request(
            &rename_snap,
            &["pnpm-lock.yaml"],
            "feat: colliding slot",
        ))
        .await
        .unwrap_err();
    assert!(slot_err.contains("重命名"), "{slot_err}");

    // Unused global-like filter config is allowed, but selected unstaged file needing clean filter is refused:
    let filter_fixture = Fixture::new().unwrap();
    git(&filter_fixture.root, &["config", "filter.lfs.clean", "cat"]).unwrap();
    let filter_store = RepositoryStore::default();
    let filter_snap = filter_store.open(&filter_fixture.root).await.unwrap();
    fs::write(
        filter_fixture.root.join(".gitattributes"),
        "*.ts filter=lfs\n",
    )
    .unwrap();
    let filter_err = filter_store
        .commit(&commit_request(
            &filter_snap,
            &["新 文件.ts"],
            "feat: filter blocked",
        ))
        .await
        .unwrap_err();
    assert!(filter_err.contains("过滤器"), "{filter_err}");

    // Sparse / skip-worktree flag and tracked submodule entry are refused:
    let sparse_fixture = Fixture::new().unwrap();
    let sparse_store = RepositoryStore::default();
    let sparse_snap = sparse_store.open(&sparse_fixture.root).await.unwrap();
    git(
        &sparse_fixture.root,
        &["update-index", "--skip-worktree", "--", "pnpm-lock.yaml"],
    )
    .unwrap();
    let sparse_err = sparse_store
        .commit(&commit_request(&sparse_snap, &["part.txt"], "feat: sparse"))
        .await
        .unwrap_err();
    assert!(sparse_err.contains("隐藏变更标记"), "{sparse_err}");

    let sub_fixture = Fixture::new().unwrap();
    let sub_store = RepositoryStore::default();
    let sub_snap = sub_store.open(&sub_fixture.root).await.unwrap();
    let head_oid = text(&sub_fixture.root, &["rev-parse", "HEAD"]);
    git(
        &sub_fixture.root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head_oid},vendor/submodule"),
        ],
    )
    .unwrap();
    let sub_err = sub_store
        .commit(&commit_request(&sub_snap, &["part.txt"], "feat: submodule"))
        .await
        .unwrap_err();
    assert!(sub_err.contains("子模块"), "{sub_err}");
}

#[tokio::test]
async fn deduplicates_identical_requests_rejects_mismatched_or_interrupted_records_and_enforces_tokens()
 {
    let fixture = Fixture::new().unwrap();
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let mut operation = commit_request(&snapshot, &["part.txt"], "feat: idempotent commit");
    operation.request_id = "idempotent-req-1".into();

    let (first_res, second_res) = tokio::join!(store.commit(&operation), store.commit(&operation));
    let (_, first_commit) = first_res.unwrap();
    let (_, second_commit) = second_res.unwrap();
    assert_eq!(first_commit, second_commit);
    assert_eq!(text(&fixture.root, &["rev-list", "--count", "HEAD"]), "2");

    // Reusing same request_id with a different message must fail:
    let mut reused = operation.clone();
    reused.message = "feat: different message".into();
    assert!(store.commit(&reused).await.unwrap_err().contains("占用"));

    // Interrupted journal record must never auto-repeat:
    let next_snap = store.open(&fixture.root).await.unwrap();
    let mut interrupted = commit_request(&next_snap, &["新 文件.ts"], "feat: interrupted");
    interrupted.request_id = "interrupted-req-1".into();
    let journal = fixture.root.join(".git/picket/commits");
    fs::create_dir_all(&journal).unwrap();
    fs::write(
        journal.join("interrupted-req-1.json"),
        serde_json::to_vec(&serde_json::json!({
            "repositoryId": interrupted.target.repository_id,
            "worktreeId": interrupted.target.worktree_id,
            "parent": active(&next_snap).head,
            "branch": active(&next_snap).branch,
            "tree": "deadbeef",
            "paths": ["新 文件.ts"],
            "selections": interrupted.selections,
            "rawMessage": interrupted.message,
            "commit": null
        }))
        .unwrap(),
    )
    .unwrap();
    let interrupted_err = store.commit(&interrupted).await.unwrap_err();
    assert!(interrupted_err.contains("中断"), "{interrupted_err}");
    assert_eq!(text(&fixture.root, &["rev-list", "--count", "HEAD"]), "2");

    // Forged tokens, branchToken, UI scopeVersion, empty message, and M5 push=true are rejected:
    let mut bad_token = commit_request(&next_snap, &["新 文件.ts"], "feat: forged");
    bad_token.selections[0].commit_token = active(&next_snap).branch_token.clone().unwrap();
    assert!(store.commit(&bad_token).await.unwrap_err().contains("凭证"));

    let mut push_req = commit_request(&next_snap, &["新 文件.ts"], "feat: push in m2");
    push_req.push = true;
    assert!(store.commit(&push_req).await.unwrap_err().contains("M5"));

    let mut empty_msg = commit_request(&next_snap, &["新 文件.ts"], "   \n\t  ");
    empty_msg.request_id = "empty-msg-1".into();
    assert!(
        store
            .commit(&empty_msg)
            .await
            .unwrap_err()
            .contains("提交说明")
    );

    // Wire format rejects arbitrary paths, flags, or shell fields:
    for extra in ["path", "paths", "shell", "noVerify", "amend"] {
        assert!(
            serde_json::from_value::<CommitRequest>(serde_json::json!({
                "requestId": "wire-1",
                "target": {
                    "repositoryId": "r",
                    "worktreeId": "w",
                    "sessionId": "s",
                    "scopeVersion": "v"
                },
                "selections": [{ "fileId": "f", "commitToken": "t" }],
                "message": "feat: wire",
                "push": false,
                extra: "forbidden"
            }))
            .is_err()
        );
    }
}

#[tokio::test]
async fn supports_unborn_branch_initial_commit_linked_worktrees_and_missing_git_identity_error() {
    // 1. Unborn branch root commit + missing user.name/email recovery:
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "feat/initial"]).unwrap();
    git(temp.path(), &["config", "user.name", ""]).unwrap();
    git(temp.path(), &["config", "user.email", ""]).unwrap();
    git(temp.path(), &["config", "user.useConfigOnly", "true"]).unwrap();
    fs::write(temp.path().join("first.ts"), "export const a = 1;\n").unwrap();
    fs::write(
        temp.path().join("keep-untracked.ts"),
        "export const b = 2;\n",
    )
    .unwrap();

    let store = RepositoryStore::default();
    let snap = store.open(temp.path()).await.unwrap();
    let missing_ident_err = store
        .commit(&commit_request(
            &snap,
            &["first.ts"],
            "feat: initial root commit",
        ))
        .await
        .unwrap_err();
    assert!(
        missing_ident_err.contains("user.name"),
        "{missing_ident_err}"
    );
    assert!(git(temp.path(), &["rev-parse", "--verify", "HEAD"]).is_err());

    git(temp.path(), &["config", "user.name", "Picket Init"]).unwrap();
    git(
        temp.path(),
        &["config", "user.email", "init@picket.invalid"],
    )
    .unwrap();
    let (after_init, root_commit) = store
        .commit(&commit_request(
            &snap,
            &["first.ts"],
            "feat: initial root commit",
        ))
        .await
        .unwrap();
    assert!(root_commit.parent.is_none());
    assert_eq!(text(temp.path(), &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(
        text(temp.path(), &["show", "HEAD:first.ts"]),
        "export const a = 1;"
    );
    assert!(git(temp.path(), &["show", "HEAD:keep-untracked.ts"]).is_err());
    assert_eq!(active(&after_init).files.len(), 1);
    assert_eq!(active(&after_init).files[0].path, "keep-untracked.ts");

    // 2. Linked worktree commit leaves main worktree untouched:
    let fixture = Fixture::new().unwrap();
    let linked = fixture.root.parent().unwrap().join("linked worktree");
    fs::write(linked.join("linked-only.ts"), "export const linked = 1;\n").unwrap();
    let wt_store = RepositoryStore::default();
    let wt_snap = wt_store.open(&linked).await.unwrap();
    let (_, wt_commit) = wt_store
        .commit(&commit_request(
            &wt_snap,
            &["linked-only.ts"],
            "feat: linked worktree commit",
        ))
        .await
        .unwrap();
    assert_eq!(wt_commit.branch, "feat/linked");
    assert_eq!(
        text(&linked, &["show", "HEAD:linked-only.ts"]),
        "export const linked = 1;"
    );
    fixture.verify_unchanged().unwrap();
}
