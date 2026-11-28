use super::{
    CustomActionCoordinator,
    cli_env::check_cli_env,
    executor::{exec_controlled_git, exec_controlled_platform_cli},
    policy::{CommandDecision, classify_git_command, classify_platform_cli_command},
    session_store::{
        ACTION_SESSIONS_FILE, SubmitGroupCheckpoint, SubmitSessionLedger,
        find_latest_resumable_session, format_resume_checkpoint_for_prompt, now_millis,
        upsert_session,
    },
    submit_cleanup::cleanup_verified_submit_duplicates,
    submit_worktree::{
        commit_in_isolated_worktree, create_isolated_submit_worktree, parse_worktree_list,
        remove_verified_submit_worktree,
    },
    types::{
        ActionProfileKind, ActionSessionStatus, CPushLandedCommit, CustomActionLink,
        DismissCustomActionDoneRequest, RemoteRepoIdentity, StoredActionSession,
        WorkflowTimelineItem,
    },
};
use crate::{
    i18n::LanguageKind,
    repository::{RepositoryStore, types::ReadTarget},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("/usr/bin/git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Picket Tester")
        .env("GIT_AUTHOR_EMAIL", "tester@picket.local")
        .env("GIT_COMMITTER_NAME", "Picket Tester")
        .env("GIT_COMMITTER_EMAIL", "tester@picket.local")
        .output()
        .expect("git command should execute");
    assert!(
        out.status.success(),
        "git {:?} failed in {}:\nstdout: {}\nstderr: {}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn setup_repo_with_bare_remote() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let bare = tmp.path().join("remote.git");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&bare).unwrap();
    fs::create_dir_all(&repo).unwrap();

    git(&bare, &["init", "--bare", "-b", "main"]);
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Picket Tester"]);
    git(&repo, &["config", "user.email", "tester@picket.local"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);

    fs::write(repo.join("auth.ts"), "export const auth = 1;\n").unwrap();
    fs::write(repo.join("billing.ts"), "export const billing = 1;\n").unwrap();
    fs::write(repo.join("partial.ts"), "line1\nline2\n").unwrap();
    fs::write(
        repo.join("unselected.ts"),
        "export const keepUnselected = 1;\n",
    )
    .unwrap();
    fs::write(repo.join("keep.env"), "SECRET_MODE=dev\n").unwrap();
    fs::write(repo.join("old-name.ts"), "export const renamed = true;\n").unwrap();

    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "chore: initial commit"]);
    git(&repo, &["push", "-u", "origin", "main"]);

    (tmp, bare, repo)
}

#[tokio::test]
async fn phase3_multi_group_isolated_worktree_partial_staging_and_step5_backup_cleanup() {
    let (tmp, bare, repo) = setup_repo_with_bare_remote();
    let storage = tmp.path().join("picket-data");
    fs::create_dir_all(&storage).unwrap();

    // Mutate working tree across two independent submit groups + partial staging + rename + unselected + Local Keep:
    // Group 1: auth.ts (unstaged mod), partial.ts (staged v2 + unstaged v3 draft)
    fs::write(repo.join("auth.ts"), "export const auth = 2;\n").unwrap();
    fs::write(repo.join("partial.ts"), "line1\nline2-staged\n").unwrap();
    git(&repo, &["add", "partial.ts"]);
    fs::write(
        repo.join("partial.ts"),
        "line1\nline2-staged\nline3-unstaged-draft\n",
    )
    .unwrap();

    // Group 2: billing.ts (fully staged), new-feature.ts (untracked), rename old-name.ts -> new-name.ts
    fs::write(repo.join("billing.ts"), "export const billing = 2;\n").unwrap();
    git(&repo, &["add", "billing.ts"]);
    fs::write(repo.join("new-feature.ts"), "export const added = 42;\n").unwrap();
    git(&repo, &["mv", "old-name.ts", "new-name.ts"]);

    // Off-limits changes that MUST NEVER be touched:
    fs::write(
        repo.join("unselected.ts"),
        "export const keepUnselected = 999;\n",
    )
    .unwrap();
    git(&repo, &["add", "unselected.ts"]);
    fs::write(repo.join("keep.env"), "SECRET_MODE=local-override\n").unwrap();

    let store = RepositoryStore::default();
    let initial_snap = store
        .open_with_locale(&repo, LanguageKind::En)
        .await
        .unwrap();
    let repo_id = initial_snap.repository.id.clone();
    let wt_id = initial_snap.active_worktree_id.clone();
    let target = ReadTarget {
        repository_id: repo_id.clone(),
        worktree_id: wt_id.clone(),
        session_id: "sess-phase3".into(),
        scope_version: initial_snap.worktrees[0].status_version.clone(),
    };
    let snap = store
        .set_local_keep(&target, &["keep.env".into()], true, LanguageKind::En)
        .await
        .unwrap();
    let files = &snap.worktrees[0].files;
    let find_change = |p: &str| {
        files
            .iter()
            .find(|c| c.path == p)
            .unwrap_or_else(|| panic!("missing change for {p}"))
            .clone()
    };

    let group1_changes = vec![find_change("auth.ts"), find_change("partial.ts")];
    assert_eq!(group1_changes[1].range.kind, "partial");

    let group2_changes = vec![
        find_change("billing.ts"),
        find_change("new-feature.ts"),
        find_change("new-name.ts"),
    ];
    assert_eq!(
        group2_changes[2].previous_path.as_deref(),
        Some("old-name.ts")
    );

    // 1. Create isolated worktree for Group 1:
    let created1 = create_isolated_submit_worktree(
        &storage,
        &repo_id,
        &repo,
        "group-1",
        "fix/issue-10-auth",
        "origin/main",
        false,
        &group1_changes,
    )
    .await
    .expect("group 1 worktree creation should succeed");

    let wt1_path = PathBuf::from(&created1.worktree_path);
    assert_eq!(
        fs::read_to_string(wt1_path.join("auth.ts")).unwrap(),
        "export const auth = 2;\n"
    );
    // CRITICAL INVARIANT: partial.ts in isolated worktree receives ONLY staged content, NOT unstaged draft!
    assert_eq!(
        fs::read_to_string(wt1_path.join("partial.ts")).unwrap(),
        "line1\nline2-staged\n"
    );
    // CRITICAL INVARIANT: unselected & Local Keep files in isolated worktree remain untouched at origin/main!
    assert_eq!(
        fs::read_to_string(wt1_path.join("unselected.ts")).unwrap(),
        "export const keepUnselected = 1;\n"
    );
    assert_eq!(
        fs::read_to_string(wt1_path.join("keep.env")).unwrap(),
        "SECRET_MODE=dev\n"
    );
    // Original checkout's unstaged draft on partial.ts is untouched byte-for-byte:
    assert_eq!(
        fs::read_to_string(repo.join("partial.ts")).unwrap(),
        "line1\nline2-staged\nline3-unstaged-draft\n"
    );

    let commit1 = commit_in_isolated_worktree(
        &wt1_path,
        &created1.transferred_paths,
        "fix(auth): tighten auth guard\n\n- update auth export and staged partial hunk",
        true,
        Some("origin"),
    )
    .await
    .expect("group 1 isolated commit + push should succeed");
    assert!(commit1.pushed);
    assert_eq!(commit1.branch, "fix/issue-10-auth");

    // 2. Create isolated worktree for Group 2:
    let created2 = create_isolated_submit_worktree(
        &storage,
        &repo_id,
        &repo,
        "group-2",
        "feat/issue-11-billing",
        "origin/main",
        false,
        &group2_changes,
    )
    .await
    .expect("group 2 worktree creation should succeed");
    let wt2_path = PathBuf::from(&created2.worktree_path);
    assert!(!wt2_path.join("old-name.ts").exists());
    assert!(wt2_path.join("new-name.ts").exists());
    assert!(wt2_path.join("new-feature.ts").exists());

    let commit2 = commit_in_isolated_worktree(
        &wt2_path,
        &created2.transferred_paths,
        "feat(billing): add billing feature and rename module\n\n- add new-feature module and rename old-name to new-name",
        true,
        Some("origin"),
    )
    .await
    .expect("group 2 isolated commit + push should succeed");
    assert!(commit2.pushed);

    // Confirm both branches exist on the bare remote:
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/fix/issue-10-auth"]),
        commit1.oid
    );
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/feat/issue-11-billing"]),
        commit2.oid
    );

    // 3. Simulate a user editing `billing.ts` AFTER preflight snapshot: Step 5 MUST retain it!
    fs::write(
        repo.join("billing.ts"),
        "export const billing = 2;\n// post-preflight local edit\n",
    )
    .unwrap();

    let mut grp1 = SubmitGroupCheckpoint {
        group_id: "group-1".into(),
        title: "Auth".into(),
        issue_url: Some("https://github.com/acme/demo/issues/10".into()),
        issue_number: Some("10".into()),
        issue_closed: false,
        branch: Some("fix/issue-10-auth".into()),
        base_ref: Some("origin/main".into()),
        remote: Some("origin".into()),
        verified_head_sha: Some(commit1.oid.clone()),
        worktree_path: Some(created1.worktree_path.clone()),
        worktree_removed: false,
        pr_url: None, // Deliberately missing PR URL first!
        pr_number: None,
        pr_state: None,
        transferred_paths: created1.transferred_paths.clone(),
        preflight_snapshots: created1.preflight_snapshots.clone(),
    };
    let grp2 = SubmitGroupCheckpoint {
        group_id: "group-2".into(),
        title: "Billing".into(),
        issue_url: Some("https://github.com/acme/demo/issues/11".into()),
        issue_number: Some("11".into()),
        issue_closed: false,
        branch: Some("feat/issue-11-billing".into()),
        base_ref: Some("origin/main".into()),
        remote: Some("origin".into()),
        verified_head_sha: Some(commit2.oid.clone()),
        worktree_path: Some(created2.worktree_path.clone()),
        worktree_removed: false,
        pr_url: Some("https://github.com/acme/demo/pull/21".into()),
        pr_number: Some("21".into()),
        pr_state: Some("open".into()),
        transferred_paths: created2.transferred_paths.clone(),
        preflight_snapshots: created2.preflight_snapshots.clone(),
    };

    // Must refuse Step 5 cleanup while Group 1 lacks a verified PR URL:
    let denied = cleanup_verified_submit_duplicates(
        &repo,
        &created1.original_branch,
        &created1.original_head,
        &[grp1.clone(), grp2.clone()],
    )
    .await;
    assert!(
        denied.is_err(),
        "Step 5 cleanup must refuse when a group has no verified PR/MR URL"
    );

    // Populate verified PR URL and run Step 5 backup + cleanup:
    grp1.pr_url = Some("https://github.com/acme/demo/pull/20".into());
    grp1.pr_number = Some("20".into());
    grp1.pr_state = Some("open".into());

    let cleanup = cleanup_verified_submit_duplicates(
        &repo,
        &created1.original_branch,
        &created1.original_head,
        &[grp1, grp2],
    )
    .await
    .expect("Step 5 cleanup should succeed once all groups have pushed remote tips + PR URLs");

    // Verify backup directory & manifest exist and are preserved under `.git/submit-backup/<UTC>/`:
    let backup_dir = PathBuf::from(cleanup.backup_dir.as_ref().expect("backup dir expected"));
    assert!(backup_dir.exists());
    assert!(backup_dir.join("manifest.json").exists());
    assert!(backup_dir.to_string_lossy().contains("submit-backup"));

    // Verified duplicate files cleaned from original checkout:
    assert!(cleanup.cleaned_paths.contains(&"auth.ts".to_string()));
    assert!(
        cleanup
            .cleaned_paths
            .contains(&"new-feature.ts".to_string())
    );
    assert!(cleanup.cleaned_paths.contains(&"new-name.ts".to_string()));
    assert!(cleanup.cleaned_paths.contains(&"old-name.ts".to_string()));
    assert_eq!(
        fs::read_to_string(repo.join("auth.ts")).unwrap(),
        "export const auth = 1;\n"
    );
    assert!(!repo.join("new-feature.ts").exists());
    assert!(!repo.join("new-name.ts").exists());
    assert!(repo.join("old-name.ts").exists());

    // Partial-staged file and post-preflight-edited file MUST be retained intact!
    assert!(
        cleanup
            .retained_paths
            .iter()
            .any(|r| r.path == "partial.ts"),
        "partial.ts must be retained safely"
    );
    assert!(
        cleanup
            .retained_paths
            .iter()
            .any(|r| r.path == "billing.ts"),
        "post-preflight modified billing.ts must be retained safely"
    );
    assert_eq!(
        fs::read_to_string(repo.join("partial.ts")).unwrap(),
        "line1\nline2-staged\nline3-unstaged-draft\n"
    );
    assert!(
        fs::read_to_string(repo.join("billing.ts"))
            .unwrap()
            .contains("post-preflight local edit")
    );

    // Unselected staged file and Local Keep file MUST be completely untouched:
    assert_eq!(
        fs::read_to_string(repo.join("unselected.ts")).unwrap(),
        "export const keepUnselected = 999;\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("keep.env")).unwrap(),
        "SECRET_MODE=local-override\n"
    );
    let staged_unselected = git(
        &repo,
        &["diff", "--cached", "--name-only", "--", "unselected.ts"],
    );
    assert_eq!(staged_unselected, "unselected.ts");
}

#[tokio::test]
async fn phase3_two_stage_merge_chain_mock_gh_and_verified_worktree_branch_cleanup() {
    let (tmp, bare, repo) = setup_repo_with_bare_remote();
    let storage = tmp.path().join("picket-data");
    let mock_bin = tmp.path().join("mock-bin");
    fs::create_dir_all(&storage).unwrap();
    fs::create_dir_all(&mock_bin).unwrap();

    // Create target branches `v1.9.0` and `test` on both local and bare remote:
    git(&repo, &["checkout", "-b", "v1.9.0"]);
    git(&repo, &["push", "-u", "origin", "v1.9.0"]);
    git(&repo, &["checkout", "-b", "test"]);
    git(&repo, &["push", "-u", "origin", "test"]);
    git(&repo, &["checkout", "v1.9.0"]);

    // Write an executable mock `gh` CLI that validates `--match-head-commit`, refuses `--admin`,
    // and actually fast-forwards target branches inside the local bare remote on `pr merge`:
    let gh_log = tmp.path().join("gh.log");
    let gh_script = mock_bin.join("gh");
    let script_body = format!(
        r#"#!/bin/sh
echo "gh $*" >> "{log}"
case "$1" in
  --version)
    echo "gh version 2.74.0 (2025-05-01)"
    exit 0
    ;;
  auth)
    echo "Logged in to github.com account picket-bot"
    exit 0
    ;;
  issue)
    if [ "$2" = "create" ]; then
      echo "https://github.com/acme/demo/issues/42"
      exit 0
    fi
    if [ "$2" = "close" ]; then
      echo "Closed issue #$3"
      exit 0
    fi
    ;;
  pr)
    if [ "$2" = "merge" ] && [ "$3" = "--help" ]; then
      echo "  --match-head-commit string   Commit SHA that the pull request head must match"
      exit 0
    fi
    if [ "$2" = "create" ]; then
      case "$*" in
        *"--base test"*)
          echo "https://github.com/acme/demo/pull/102"
          ;;
        *)
          echo "https://github.com/acme/demo/pull/101"
          ;;
      esac
      exit 0
    fi
    if [ "$2" = "merge" ]; then
      case "$*" in
        *"--admin"*)
          echo "ERROR: --admin forbidden" >&2
          exit 2
          ;;
        *"--match-head-commit"*)
          ;;
        *)
          echo "ERROR: missing --match-head-commit" >&2
          exit 2
          ;;
      esac
      if [ "$3" = "101" ]; then
        SHA=$(/usr/bin/git --git-dir="{bare}" rev-parse refs/heads/fix/issue-42)
        /usr/bin/git --git-dir="{bare}" update-ref refs/heads/v1.9.0 "$SHA"
        echo "Merged PR #101 into v1.9.0 ($SHA)"
        exit 0
      fi
      if [ "$3" = "102" ]; then
        SHA=$(/usr/bin/git --git-dir="{bare}" rev-parse refs/heads/v1.9.0)
        /usr/bin/git --git-dir="{bare}" update-ref refs/heads/test "$SHA"
        echo "Merged promotion PR #102 into test ($SHA)"
        exit 0
      fi
    fi
    ;;
esac
echo "Unexpected mock gh invocation: $*" >&2
exit 1
"#,
        log = gh_log.display(),
        bare = bare.display(),
    );
    fs::write(&gh_script, script_body).unwrap();
    fs::set_permissions(&gh_script, fs::Permissions::from_mode(0o755)).unwrap();

    let coordinator = CustomActionCoordinator::default();
    coordinator.set_override_cli_dirs(Some(vec![mock_bin.clone()]));

    let env_snap = check_cli_env(&repo, Some(std::slice::from_ref(&mock_bin))).await;
    assert!(env_snap.gh.installed);
    assert_eq!(env_snap.gh.authenticated, Some(true));
    assert_eq!(env_snap.gh.supports_head_sha_guard, Some(true));

    // Create a change, transfer into isolated submit worktree off origin/v1.9.0, commit & push:
    fs::write(repo.join("auth.ts"), "export const auth = 42;\n").unwrap();
    let store = RepositoryStore::default();
    let snap = store
        .open_with_locale(&repo, LanguageKind::En)
        .await
        .unwrap();
    let auth_change = snap.worktrees[0]
        .files
        .iter()
        .find(|f| f.path == "auth.ts")
        .unwrap()
        .clone();
    let created = create_isolated_submit_worktree(
        &storage,
        &snap.repository.id,
        &repo,
        "group-42",
        "fix/issue-42",
        "origin/v1.9.0",
        false,
        &[auth_change],
    )
    .await
    .unwrap();
    let committed = commit_in_isolated_worktree(
        Path::new(&created.worktree_path),
        &created.transferred_paths,
        "fix(auth): enforce issue 42 fix\n\n- update auth value to 42",
        true,
        Some("origin"),
    )
    .await
    .unwrap();
    assert!(committed.pushed);

    let remotes = vec![RemoteRepoIdentity {
        remote: "origin".into(),
        platform: "github",
        host: "github.com".into(),
        repo_path: "acme/demo".into(),
        url: "https://github.com/acme/demo.git".into(),
    }];

    // Verify policy enforces head-SHA guard and blocks --admin:
    assert!(matches!(
        classify_platform_cli_command(
            "gh",
            &[
                "pr".into(),
                "merge".into(),
                "101".into(),
                "--admin".into(),
                "--match-head-commit".into(),
                committed.oid.clone()
            ],
            &remotes
        ),
        CommandDecision::DenyRedline { .. }
    ));
    assert!(matches!(
        classify_platform_cli_command("gh", &["pr".into(), "merge".into(), "101".into()], &remotes),
        CommandDecision::DenyRedline { .. }
    ));
    assert_eq!(
        classify_platform_cli_command(
            "gh",
            &[
                "pr".into(),
                "merge".into(),
                "101".into(),
                "--squash".into(),
                "--match-head-commit".into(),
                committed.oid.clone()
            ],
            &remotes
        ),
        CommandDecision::AllowTier1
    );

    // Execute Issue #42 -> PR #101 -> merge PR #101 -> promotion PR #102 (v1.9.0 -> test) -> merge PR #102 -> close Issue #42:
    let issue_out = exec_controlled_platform_cli(
        "gh",
        &repo,
        &[
            "issue".into(),
            "create".into(),
            "--title".into(),
            "Issue 42".into(),
        ],
        Some("Details for issue 42"),
        Some("--body-file"),
        Some(std::slice::from_ref(&mock_bin)),
    )
    .await
    .unwrap();
    assert!(issue_out.success);
    assert!(
        issue_out
            .extracted_links
            .iter()
            .any(|l| l.url.ends_with("/issues/42"))
    );

    let pr1_out = exec_controlled_platform_cli(
        "gh",
        &repo,
        &[
            "pr".into(),
            "create".into(),
            "--base".into(),
            "v1.9.0".into(),
            "--head".into(),
            "fix/issue-42".into(),
            "--title".into(),
            "fix(auth): issue 42".into(),
        ],
        Some("Fixes #42"),
        Some("--body-file"),
        Some(std::slice::from_ref(&mock_bin)),
    )
    .await
    .unwrap();
    assert!(pr1_out.success);
    assert!(
        pr1_out
            .extracted_links
            .iter()
            .any(|l| l.url.ends_with("/pull/101"))
    );

    let merge1 = exec_controlled_platform_cli(
        "gh",
        &repo,
        &[
            "pr".into(),
            "merge".into(),
            "101".into(),
            "--squash".into(),
            "--match-head-commit".into(),
            committed.oid.clone(),
        ],
        None,
        None,
        Some(std::slice::from_ref(&mock_bin)),
    )
    .await
    .unwrap();
    assert!(merge1.success);

    let v190_sha = git(&bare, &["rev-parse", "refs/heads/v1.9.0"]);
    assert_eq!(v190_sha, committed.oid);

    let pr2_out = exec_controlled_platform_cli(
        "gh",
        &repo,
        &[
            "pr".into(),
            "create".into(),
            "--base".into(),
            "test".into(),
            "--head".into(),
            "v1.9.0".into(),
            "--title".into(),
            "chore: promote v1.9.0 to test".into(),
        ],
        Some("Promote v1.9.0 to test"),
        Some("--body-file"),
        Some(std::slice::from_ref(&mock_bin)),
    )
    .await
    .unwrap();
    assert!(pr2_out.success);
    assert!(
        pr2_out
            .extracted_links
            .iter()
            .any(|l| l.url.ends_with("/pull/102"))
    );

    let merge2 = exec_controlled_platform_cli(
        "gh",
        &repo,
        &[
            "pr".into(),
            "merge".into(),
            "102".into(),
            "--merge".into(),
            "--match-head-commit".into(),
            v190_sha.clone(),
        ],
        None,
        None,
        Some(std::slice::from_ref(&mock_bin)),
    )
    .await
    .unwrap();
    assert!(merge2.success);
    assert_eq!(git(&bare, &["rev-parse", "refs/heads/test"]), committed.oid);

    let close_out = exec_controlled_platform_cli(
        "gh",
        &repo,
        &["issue".into(), "close".into(), "42".into()],
        None,
        None,
        Some(std::slice::from_ref(&mock_bin)),
    )
    .await
    .unwrap();
    assert!(close_out.success);

    // Verified remote branch deletion using `--force-with-lease=<ref>:<40-sha> :refs/heads/<branch>`:
    let delete_args = vec![
        "push".into(),
        "origin".into(),
        format!(
            "--force-with-lease=refs/heads/fix/issue-42:{}",
            committed.oid
        ),
        ":refs/heads/fix/issue-42".into(),
    ];
    assert!(matches!(
        classify_git_command(&delete_args),
        CommandDecision::RequireConfirmTier2 { .. }
    ));
    let del_remote = exec_controlled_git(&repo, &delete_args, true, false)
        .await
        .unwrap();
    assert!(
        del_remote.success,
        "verified remote branch deletion failed: {}",
        del_remote.stderr
    );
    let remaining_refs = git(&bare, &["show-ref", "--heads"]);
    assert!(!remaining_refs.contains("refs/heads/fix/issue-42"));

    // Fast-forward local v1.9.0 after cleaning auth.ts so safe `git branch -d` recognizes merge:
    git(&repo, &["checkout", "--", "auth.ts"]);
    git(&repo, &["pull", "--ff-only", "origin", "v1.9.0"]);

    let group_rec = SubmitGroupCheckpoint {
        group_id: "group-42".into(),
        title: "Issue 42".into(),
        issue_url: Some("https://github.com/acme/demo/issues/42".into()),
        issue_number: Some("42".into()),
        issue_closed: true,
        branch: Some("fix/issue-42".into()),
        base_ref: Some("origin/v1.9.0".into()),
        remote: Some("origin".into()),
        verified_head_sha: Some(committed.oid.clone()),
        worktree_path: Some(created.worktree_path.clone()),
        worktree_removed: false,
        pr_url: Some("https://github.com/acme/demo/pull/101".into()),
        pr_number: Some("101".into()),
        pr_state: Some("merged".into()),
        transferred_paths: created.transferred_paths.clone(),
        preflight_snapshots: created.preflight_snapshots.clone(),
    };

    // 1) Refuses removal if temporary worktree has untracked file:
    let dirty_probe = Path::new(&created.worktree_path).join("stray.tmp");
    fs::write(&dirty_probe, "do not lose me").unwrap();
    let dirty_err =
        remove_verified_submit_worktree(&repo, &group_rec, Some(&committed.oid), true).await;
    assert!(
        dirty_err.is_err(),
        "must refuse to remove dirty temporary worktree"
    );
    fs::remove_file(&dirty_probe).unwrap();

    // 2) Refuses removal if expected HEAD SHA mismatches:
    let bad_sha = "ffffffffffffffffffffffffffffffffffffffff";
    let sha_err = remove_verified_submit_worktree(&repo, &group_rec, Some(bad_sha), true).await;
    assert!(
        sha_err.is_err(),
        "must refuse removal when HEAD SHA does not match"
    );

    // 3) Succeeds cleanly when verified clean + matching HEAD SHA, and safely deletes local branch via `git branch -d`:
    let list_before = parse_worktree_list(&repo, std::slice::from_ref(&group_rec))
        .await
        .unwrap();
    assert!(
        list_before
            .iter()
            .any(|w| w.owned_by_picket_submit && w.group_id.as_deref() == Some("group-42"))
    );

    let removed = remove_verified_submit_worktree(&repo, &group_rec, Some(&committed.oid), true)
        .await
        .expect("verified clean temporary worktree removal should succeed");
    assert!(removed.removed);
    assert!(
        removed.branch_deleted,
        "safe `git branch -d` should delete merged local branch: {:?}",
        removed.branch_note
    );
    assert!(!Path::new(&created.worktree_path).exists());
}

#[test]
fn phase3_session_persistence_cold_restart_reconciliation_and_dismissal() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().join(".picket");
    let fake_wt = tmp.path().join("live-submit-wt");
    fs::create_dir_all(&fake_wt).unwrap();

    let session = StoredActionSession {
        run_id: "run-crash-1".into(),
        repository_id: "repo-demo".into(),
        worktree_id: "wt-main".into(),
        session_id: "s1".into(),
        action_id: "act-submit".into(),
        action_label: "Submit -> v1.9.0 -> test".into(),
        profile: ActionProfileKind::CustomAction,
        action_prompt_hash: "abc123".into(),
        frozen_scope_version: "sv1".into(),
        model_id: String::new(),
        branch: "main".into(),
        initial_head: None,
        prompt: String::new(),
        recent_commits: vec![],
        unselected_paths: vec![],
        frozen_files: vec![],
        planned_groups: vec![],
        skipped_file_ids: vec![],
        next_step_index: 0,
        committed_file_ids: vec![],
        status: ActionSessionStatus::Running,
        pending_call_id: None,
        updated_at_ms: now_millis(),
        summary: String::new(),
        card: None,
        message: None,
        timeline: vec![
            WorkflowTimelineItem {
                state: "done".into(),
                label: "Created isolated submit worktree".into(),
            },
            WorkflowTimelineItem {
                state: "active".into(),
                label: "Running gh pr merge".into(),
            },
        ],
        qa_log: vec![],
        commits: vec![CPushLandedCommit {
            oid: "1111111111111111111111111111111111111111".into(),
            short_oid: "1111111".into(),
            title: "fix(auth): validate tokens".into(),
            branch: "fix/issue-42".into(),
            file_ids: vec![],
        }],
        links: vec![
            CustomActionLink {
                label: "Issue #42".into(),
                url: "https://github.com/acme/demo/issues/42".into(),
            },
            CustomActionLink {
                label: "PR #101".into(),
                url: "https://github.com/acme/demo/pull/101".into(),
            },
        ],
        can_retry_remaining: false,
        push_authorized: false,
        auto_collapse: false,
        notice_dismissed: false,
        dismissed_message_id: None,
        edited_file_hashes: Default::default(),
        submit_ledger: SubmitSessionLedger {
            workflow_kind: "submitPromote".into(),
            first_target: Some("v1.9.0".into()),
            final_target: Some("test".into()),
            original_branch: Some("main".into()),
            original_head: Some("0000000000000000000000000000000000000000".into()),
            groups: vec![SubmitGroupCheckpoint {
                group_id: "group-1".into(),
                title: "Auth".into(),
                issue_url: Some("https://github.com/acme/demo/issues/42".into()),
                issue_number: Some("42".into()),
                issue_closed: false,
                branch: Some("fix/issue-42".into()),
                base_ref: Some("origin/v1.9.0".into()),
                remote: Some("origin".into()),
                verified_head_sha: Some("1111111111111111111111111111111111111111".into()),
                worktree_path: Some(fake_wt.to_string_lossy().into_owned()),
                worktree_removed: false,
                pr_url: Some("https://github.com/acme/demo/pull/101".into()),
                pr_number: Some("101".into()),
                pr_state: Some("open".into()),
                transferred_paths: vec!["auth.ts".into()],
                preflight_snapshots: vec![],
            }],
            promotion: None,
            cleanup: None,
        },
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
    };
    upsert_session(&storage, session).unwrap();

    // Verify 0700 dir / 0600 file permissions and zero leaked secrets:
    let file_path = storage.join(ACTION_SESSIONS_FILE);
    assert_eq!(
        fs::metadata(&storage).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&file_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let raw = fs::read_to_string(&file_path).unwrap();
    assert!(!raw.contains("apiKey"));
    assert!(!raw.contains("commitToken"));
    assert!(!raw.contains("branchToken"));

    // Cold startup reconciliation marks stale "running" as "interrupted" without deleting worktree:
    let coordinator = CustomActionCoordinator::default();
    let restored = coordinator.restore_sessions(&storage, LanguageKind::En);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].status, ActionSessionStatus::Interrupted);
    assert_eq!(restored[0].timeline[1].state, "failed");
    assert!(
        fake_wt.exists(),
        "cold startup reconciliation must never delete isolated worktrees"
    );
    let msg = restored[0]
        .message
        .as_ref()
        .expect("restored interrupted message expected");
    assert_eq!(msg.id, "interrupted-action-run-crash-1");
    assert_eq!(restored[0].links.len(), 2);

    // Resume checkpoint prompt formatting preserves all verified IDs/URLs/worktree existence:
    let resumable =
        find_latest_resumable_session(&storage, "repo-demo", "wt-main", Some("act-submit"))
            .expect("interrupted session must be resumable");
    let prompt_block = format_resume_checkpoint_for_prompt(&resumable);
    assert!(prompt_block.contains("https://github.com/acme/demo/issues/42"));
    assert!(prompt_block.contains("https://github.com/acme/demo/pull/101"));
    assert!(prompt_block.contains("exists=true"));

    // Dismissing the restored UI card hides it on next startup while keeping the checkpoint for resume:
    coordinator
        .dismiss_done_notice(
            &storage,
            &DismissCustomActionDoneRequest {
                request_id: "req-dismiss-1".into(),
                target: ReadTarget {
                    repository_id: "repo-demo".into(),
                    worktree_id: "wt-main".into(),
                    session_id: "s1".into(),
                    scope_version: "sv1".into(),
                },
                run_id: Some("run-crash-1".into()),
                message_id: msg.id.clone(),
                locale: LanguageKind::En,
            },
        )
        .unwrap();
    assert!(
        coordinator
            .restore_sessions(&storage, LanguageKind::En)
            .is_empty()
    );
    assert!(
        find_latest_resumable_session(&storage, "repo-demo", "wt-main", Some("act-submit"))
            .is_some()
    );
}

#[tokio::test]
async fn phase1_action_sessions_handle_concurrent_upserts_and_unknown_disk_fields() {
    use crate::actions::session_store::{load_all_sessions, upsert_session_async};
    use std::collections::BTreeMap;

    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().to_path_buf();

    // 1. Concurrent async upserts across multiple repos/runs must never clobber one another:
    let mut handles = Vec::new();
    for idx in 0..12 {
        let dir = storage.clone();
        handles.push(tokio::spawn(async move {
            let record = StoredActionSession {
                run_id: format!("concurrent-run-{idx}"),
                repository_id: format!("repo-{idx}"),
                worktree_id: format!("wt-{idx}"),
                session_id: format!("sess-{idx}"),
                action_id: "act-1".into(),
                action_label: "Action".into(),
                profile: ActionProfileKind::CustomAction,
                action_prompt_hash: "h1".into(),
                frozen_scope_version: "sv1".into(),
                model_id: String::new(),
                branch: "main".into(),
                initial_head: None,
                prompt: String::new(),
                recent_commits: vec![],
                unselected_paths: vec![],
                frozen_files: vec![],
                planned_groups: vec![],
                skipped_file_ids: vec![],
                next_step_index: 0,
                committed_file_ids: vec![],
                status: ActionSessionStatus::Done,
                pending_call_id: None,
                updated_at_ms: 1_000 + idx as u64,
                summary: format!("Completed {idx}"),
                card: None,
                message: None,
                timeline: vec![],
                qa_log: vec![],
                commits: vec![],
                links: vec![],
                can_retry_remaining: false,
                push_authorized: false,
                auto_collapse: false,
                notice_dismissed: false,
                dismissed_message_id: None,
                edited_file_hashes: BTreeMap::new(),
                submit_ledger: SubmitSessionLedger::default(),
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            };
            upsert_session_async(&dir, record).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let all = load_all_sessions(&storage);
    assert_eq!(all.len(), 12);

    // 2. Forward-compatible unknown disk fields must load cleanly without wiping sessions:
    let file_path = storage.join("action-sessions.json");
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(&file_path).unwrap()).unwrap();
    raw["futureFileMeta"] = serde_json::json!("vNext");
    raw["sessions"][0]["futureSessionField"] = serde_json::json!({ "nested": true });
    fs::write(&file_path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
    assert_eq!(load_all_sessions(&storage).len(), 12);
}

#[test]
fn phase3_policy_hardening_redlines() {
    let sha = "1111111111111111111111111111111111111111";

    // Protected branch deletion blocked:
    assert!(matches!(
        classify_git_command(&["branch".into(), "-d".into(), "main".into()]),
        CommandDecision::DenyRedline { .. }
    ));
    assert!(matches!(
        classify_git_command(&["branch".into(), "-D".into(), "master".into()]),
        CommandDecision::DenyRedline { .. }
    ));

    // Worktree policy: list is Tier 0, remove --force is hard redline:
    assert_eq!(
        classify_git_command(&["worktree".into(), "list".into(), "--porcelain".into()]),
        CommandDecision::AllowTier0
    );
    assert!(matches!(
        classify_git_command(&[
            "worktree".into(),
            "remove".into(),
            "--force".into(),
            "/tmp/wt".into()
        ]),
        CommandDecision::DenyRedline { .. }
    ));

    // Push ref deletion ordering: raw --force and bare --force-with-lease MUST be blocked even on `:refs/heads/...`:
    assert!(matches!(
        classify_git_command(&[
            "push".into(),
            "origin".into(),
            "--force".into(),
            ":refs/heads/feat".into()
        ]),
        CommandDecision::DenyRedline { .. }
    ));
    assert!(matches!(
        classify_git_command(&[
            "push".into(),
            "origin".into(),
            "--force-with-lease".into(),
            ":refs/heads/feat".into()
        ]),
        CommandDecision::DenyRedline { .. }
    ));
    assert!(matches!(
        classify_git_command(&[
            "push".into(),
            "origin".into(),
            format!("--force-with-lease=refs/heads/main:{sha}"),
            ":refs/heads/main".into()
        ]),
        CommandDecision::DenyRedline { .. }
    ));
    assert!(matches!(
        classify_git_command(&[
            "push".into(),
            "origin".into(),
            format!("--force-with-lease=refs/heads/feat/ok:{sha}"),
            ":refs/heads/feat/ok".into()
        ]),
        CommandDecision::RequireConfirmTier2 { .. }
    ));

    // GitHub `gh issue develop` and GitLab `glab mr approvers` / `glab api` cross-repo guards:
    let gh_remotes = vec![RemoteRepoIdentity {
        remote: "origin".into(),
        platform: "github",
        host: "github.com".into(),
        repo_path: "acme/demo".into(),
        url: "https://github.com/acme/demo.git".into(),
    }];
    assert_eq!(
        classify_platform_cli_command(
            "gh",
            &[
                "issue".into(),
                "develop".into(),
                "--list".into(),
                "42".into()
            ],
            &gh_remotes
        ),
        CommandDecision::AllowTier0
    );
    assert_eq!(
        classify_platform_cli_command(
            "gh",
            &[
                "issue".into(),
                "develop".into(),
                "42".into(),
                "--name".into(),
                "fix/42".into()
            ],
            &gh_remotes
        ),
        CommandDecision::AllowTier1
    );

    let gl_remotes = vec![RemoteRepoIdentity {
        remote: "origin".into(),
        platform: "gitlab",
        host: "gitlab.com".into(),
        repo_path: "acme/demo".into(),
        url: "https://gitlab.com/acme/demo.git".into(),
    }];
    assert_eq!(
        classify_platform_cli_command(
            "glab",
            &["mr".into(), "approvers".into(), "12".into()],
            &gl_remotes
        ),
        CommandDecision::AllowTier0
    );
    assert_eq!(
        classify_platform_cli_command(
            "glab",
            &["api".into(), "projects/:fullpath/merge_requests/12".into()],
            &gl_remotes
        ),
        CommandDecision::AllowTier0
    );
    assert!(matches!(
        classify_platform_cli_command(
            "glab",
            &[
                "api".into(),
                "projects/evil%2Fother/merge_requests/12".into()
            ],
            &gl_remotes
        ),
        CommandDecision::DenyRedline { .. }
    ));
}
