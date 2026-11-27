use super::smoke::{Fixture, git};
use super::{RepositoryStore, types::RepositorySnapshot};
use std::fs;
fn active(snapshot: &RepositorySnapshot) -> &super::types::WorktreeSnapshot {
    snapshot
        .worktrees
        .iter()
        .find(|tree| tree.worktree_id == snapshot.active_worktree_id)
        .unwrap()
}

#[tokio::test]
async fn unstaged_deletion_is_reported_with_real_removed_lines() {
    let fixture = Fixture::new().unwrap();
    fs::remove_file(fixture.root.join("pnpm-lock.yaml")).unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let deleted = active(&snapshot)
        .files
        .iter()
        .find(|file| file.path == "pnpm-lock.yaml")
        .unwrap();
    assert_eq!(deleted.status, "D");
    assert_eq!(deleted.range.kind, "unstaged");
    assert_eq!((deleted.range.added, deleted.range.removed), (0, 1));
}
#[tokio::test]
async fn staged_modification_remains_the_effective_range_if_working_file_is_deleted() {
    let fixture = Fixture::new().unwrap();
    fs::remove_file(fixture.root.join("part.txt")).unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let partial = active(&snapshot)
        .files
        .iter()
        .find(|file| file.path == "part.txt")
        .unwrap();
    assert_eq!(partial.git_status, "MD");
    assert_eq!(partial.status, "M");
    assert_eq!(partial.range.kind, "partial");
    assert_eq!((partial.range.added, partial.range.removed), (1, 1));
}
#[tokio::test]
async fn staged_deletion_with_recreated_untracked_draft_is_one_partial_file() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["rm", "-f", "--", "part.txt"]).unwrap();
    fs::write(fixture.root.join("part.txt"), "a recreated draft\n").unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let files: Vec<_> = active(&snapshot)
        .files
        .iter()
        .filter(|file| file.path == "part.txt")
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].status, "D");
    assert_eq!(files[0].range.kind, "partial");
    assert_eq!((files[0].range.added, files[0].range.removed), (0, 2));
}
#[tokio::test]
async fn untracked_statistics_follow_git_attributes_and_keep_empty_files() {
    let fixture = Fixture::new().unwrap();
    fs::write(
        fixture.root.join(".gitattributes"),
        "text-as-binary.txt -diff\n",
    )
    .unwrap();
    fs::write(
        fixture.root.join("text-as-binary.txt"),
        "plain text but binary by Git policy\n",
    )
    .unwrap();
    fs::write(fixture.root.join("empty.txt"), "").unwrap();
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let binary = active(&snapshot)
        .files
        .iter()
        .find(|file| file.path == "text-as-binary.txt")
        .unwrap();
    assert!(binary.binary);
    assert_eq!(binary.range.added, 0);
    let empty = active(&snapshot)
        .files
        .iter()
        .find(|file| file.path == "empty.txt")
        .unwrap();
    assert_eq!((empty.range.added, empty.range.removed), (0, 0));
}
#[test]
fn no_index_numstat_only_accepts_the_known_dev_null_source() {
    let bytes = b"1\t0\t\0/dev/null\0new.ts\0";
    assert!(super::parse::numstat(bytes).is_err());
    assert_eq!(
        super::parse::untracked_numstat(bytes, "new.ts")
            .unwrap()
            .added,
        1
    );
    assert!(super::parse::untracked_numstat(b"1\t0\t\0/outside\0new.ts\0", "new.ts").is_err());
    assert!(super::parse::untracked_numstat(bytes, "other.ts").is_err());
}
#[tokio::test]
async fn non_utf8_paths_fail_explicitly_instead_of_dropping_files() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let fixture = Fixture::new().unwrap();
    // APFS rejects non-UTF-8 filenames, but an index imported from another
    // filesystem can still contain them. Create that real Git state instead.
    let staged =
        String::from_utf8(git(&fixture.root, &["ls-files", "--stage", "--", "part.txt"]).unwrap())
            .unwrap();
    let oid = staged.split_whitespace().nth(1).unwrap();
    let mut record = format!("100644 {oid}\tf").into_bytes();
    record.extend([0xff, 0]);
    let mut child = Command::new("/usr/bin/git")
        .current_dir(&fixture.root)
        .args(["update-index", "-z", "--index-info"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&record).unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    let result = RepositoryStore::default().open(&fixture.root).await;
    assert!(result.unwrap_err().contains("UTF-8"));
}

#[tokio::test]
async fn external_filter_configuration_only_blocks_files_that_need_conversion() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["config", "filter.proof.clean", "cat"]).unwrap();
    let store = RepositoryStore::default();
    store.open(&fixture.root).await.unwrap();
    fs::write(
        fixture.root.join(".gitattributes"),
        "part.txt filter=proof\n",
    )
    .unwrap();
    let error = store.open(&fixture.root).await.unwrap_err();
    assert!(error.contains("external Git filter"), "{error}");
    fixture.verify_unchanged().unwrap();
}

#[tokio::test]
async fn missing_partial_clone_objects_are_not_downloaded_on_read() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["config", "uploadpack.allowFilter", "true"]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    git(
        temp.path(),
        &[
            "clone",
            "--filter=blob:none",
            "--no-checkout",
            &format!("file://{}", fixture.root.display()),
            "partial",
        ],
    )
    .unwrap();
    let partial = temp.path().join("partial");
    let list_missing = || {
        git(
            &partial,
            &["rev-list", "--objects", "--all", "--missing=print"],
        )
        .unwrap()
    };
    let before = list_missing();
    assert!(
        before
            .split(|b| *b == b'\n')
            .any(|line| line.starts_with(b"?"))
    );
    let result = super::runner::GitReader::default()
        .run(&partial, &["cat-file", "blob", "HEAD:part.txt"])
        .await
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.success);
    assert_ne!(
        result.code,
        Some(129),
        "GitReader must not fail with CLI usage error: {stderr}"
    );
    assert!(
        !stderr.contains("unknown option"),
        "GitReader passed unsupported CLI flag: {stderr}"
    );
    assert_eq!(list_missing(), before);
}

#[tokio::test]
async fn untracked_rust_stats_match_git_diff_across_text_binary_attributes_and_symlinks() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new().unwrap();
    fs::write(
        fixture.root.join(".gitattributes"),
        "forced-bin.txt binary\nforced-text.bin diff\n",
    )
    .unwrap();
    fs::write(fixture.root.join("no-eof-nl.txt"), "one\ntwo").unwrap();
    fs::write(fixture.root.join("with-eof-nl.txt"), "one\ntwo\n").unwrap();
    fs::write(fixture.root.join("crlf.txt"), "a\r\nb\r\nc\r\n").unwrap();
    fs::write(fixture.root.join("forced-bin.txt"), "plain text\nline 2\n").unwrap();
    fs::write(
        fixture.root.join("forced-text.bin"),
        b"line1\0still-line1\nline2",
    )
    .unwrap();
    fs::write(fixture.root.join("nul-early.bin"), b"abc\0def\nline2\n").unwrap();

    // NUL byte after Git's 8,000-byte binary sniff threshold is treated as text by Git.
    let mut nul_late = vec![b'a'; 8_050];
    nul_late.push(b'\n');
    nul_late.push(0);
    nul_late.extend_from_slice(b"tail");
    fs::write(fixture.root.join("nul-late.txt"), &nul_late).unwrap();
    symlink("with-eof-nl.txt", fixture.root.join("untracked-link")).unwrap();

    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let files = &active(&snapshot).files;

    let reader = super::runner::GitReader::default();
    for path in [
        "no-eof-nl.txt",
        "with-eof-nl.txt",
        "crlf.txt",
        "forced-bin.txt",
        "forced-text.bin",
        "nul-early.bin",
        "nul-late.txt",
        "untracked-link",
    ] {
        let actual = files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("missing {path}"));
        let git_out = reader
            .run(
                &fixture.root,
                &[
                    "diff",
                    "--no-index",
                    "--numstat",
                    "-z",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    "/dev/null",
                    path,
                ],
            )
            .await
            .unwrap();
        let expected = super::parse::untracked_numstat(&git_out.bytes, path).unwrap();
        assert_eq!(
            (actual.binary, actual.range.added, actual.range.removed),
            (expected.binary, expected.added, expected.removed),
            "stats mismatch for {path}"
        );
    }
}

#[tokio::test]
async fn external_filter_blocks_untracked_file_when_configured_via_gitattributes() {
    let fixture = Fixture::new().unwrap();
    git(&fixture.root, &["config", "filter.proof.clean", "cat"]).unwrap();
    fs::write(
        fixture.root.join(".gitattributes"),
        "filtered-new.txt filter=proof\n",
    )
    .unwrap();
    fs::write(fixture.root.join("filtered-new.txt"), "hello\n").unwrap();
    let error = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap_err();
    assert!(error.contains("external Git filter"), "{error}");
}

#[tokio::test]
async fn batch_untracked_files_read_cleanly_without_per_file_git_spawns() {
    let fixture = Fixture::new().unwrap();
    let dir = fixture.root.join("generated-batch");
    fs::create_dir_all(&dir).unwrap();
    for idx in 0..320 {
        fs::write(
            dir.join(format!("item-{idx:03}.txt")),
            format!("line 1 of {idx}\nline 2 of {idx}\n"),
        )
        .unwrap();
    }
    let snapshot = RepositoryStore::default()
        .open(&fixture.root)
        .await
        .unwrap();
    let batch_files: Vec<_> = active(&snapshot)
        .files
        .iter()
        .filter(|f| f.path.starts_with("generated-batch/item-"))
        .collect();
    assert_eq!(batch_files.len(), 320);
    assert!(
        batch_files
            .iter()
            .all(|f| !f.binary && f.range.added == 2 && !f.commit_token.is_empty())
    );
}
