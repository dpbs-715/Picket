use super::{
    editor::EditorLauncher,
    smoke::{Fixture, git},
    *,
};
use std::os::unix::fs::PermissionsExt;

fn active(snapshot: &RepositorySnapshot) -> &WorktreeSnapshot {
    snapshot
        .worktrees
        .iter()
        .find(|tree| tree.worktree_id == snapshot.active_worktree_id)
        .unwrap()
}

fn file_id(snapshot: &RepositorySnapshot, path: &str) -> String {
    active(snapshot)
        .files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("missing file {path}"))
        .id
        .clone()
}

fn open_request(
    snapshot: &RepositorySnapshot,
    path: &str,
    editor: EditorKind,
    line: Option<u32>,
) -> OpenFileRequest {
    static REQUEST: AtomicU64 = AtomicU64::new(0);
    let tree = active(snapshot);
    OpenFileRequest {
        request_id: format!("ide-test-{}", REQUEST.fetch_add(1, Ordering::SeqCst)),
        target: ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "ide-tests".into(),
            scope_version: tree.status_version.clone(),
        },
        file_id: Some(file_id(snapshot, path)),
        path: None,
        editor,
        line,
        locale: crate::i18n::LanguageKind::En,
    }
}

fn install_recording_clis(bin_dir: &Path, capture_dir: &Path) {
    fs::create_dir_all(bin_dir).unwrap();
    fs::create_dir_all(capture_dir).unwrap();
    for cli in ["idea", "rustrover", "webstorm", "code", "cursor"] {
        let script_path = bin_dir.join(cli);
        let script = format!(
            concat!(
                "#!/bin/sh\n",
                "set -e\n",
                "OUT='{capture}'\n",
                "rm -f \"$OUT/cli\" \"$OUT/cwd\" \"$OUT/args\" \"$OUT\"/file-*\n",
                "printf '%s' \"{cli}\" > \"$OUT/cli\"\n",
                "pwd > \"$OUT/cwd\"\n",
                "i=0\n",
                "for arg in \"$@\"; do\n",
                "  printf '%s\\0' \"$arg\" >> \"$OUT/args\"\n",
                "  if [ -f \"$arg\" ]; then\n",
                "    cat \"$arg\" > \"$OUT/file-$i\"\n",
                "  fi\n",
                "  i=$((i + 1))\n",
                "done\n"
            ),
            capture = capture_dir.display(),
            cli = cli
        );
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn recorded_args(capture_dir: &Path) -> Vec<String> {
    fs::read(capture_dir.join("args"))
        .unwrap()
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8(part.to_vec()).unwrap())
        .collect()
}

#[tokio::test]
async fn launches_all_four_editors_for_staged_diff_open_diff_goto_and_cleanup() {
    let fixture = Fixture::new().unwrap();
    let harness = tempfile::tempdir().unwrap();
    let bin_dir = harness.path().join("bin");
    let capture_dir = harness.path().join("capture");
    install_recording_clis(&bin_dir, &capture_dir);

    let launcher = EditorLauncher::with_search_dirs(vec![bin_dir]);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let canonical_root = fs::canonicalize(&fixture.root).unwrap();

    // 1. Partial file -> StagedDiff across all 4 editors (exports HEAD vs :0:, never unstaged draft):
    for (editor, expected_cli, expected_flag) in [
        (EditorKind::Idea, "idea", "diff"),
        (EditorKind::Webstorm, "webstorm", "diff"),
        (EditorKind::Vscode, "code", "--diff"),
        (EditorKind::Cursor, "cursor", "--diff"),
    ] {
        let outcome = store
            .open_in_editor(
                &launcher,
                &open_request(&snapshot, "part.txt", editor, None),
            )
            .await
            .unwrap();
        assert_eq!(outcome.action, OpenFileAction::StagedDiff);
        assert_eq!(
            fs::read_to_string(capture_dir.join("cli")).unwrap(),
            expected_cli
        );
        assert_eq!(
            fs::canonicalize(fs::read_to_string(capture_dir.join("cwd")).unwrap().trim()).unwrap(),
            canonical_root
        );
        let args = recorded_args(&capture_dir);
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], expected_flag);
        assert!(args[1].ends_with("/HEAD/part.txt"), "{args:?}");
        assert!(args[2].ends_with("/STAGED/part.txt"), "{args:?}");
        assert_eq!(
            fs::metadata(&args[1]).unwrap().permissions().mode() & 0o777,
            0o444
        );
        assert_eq!(
            fs::read(capture_dir.join("file-1")).unwrap(),
            b"first=1\nsecond=1\n"
        );
        assert_eq!(
            fs::read(capture_dir.join("file-2")).unwrap(),
            b"first=2\nsecond=1\n"
        );
    }

    // 2. Untracked new file with spaces and Unicode -> Open real file in worktree:
    let open_outcome = store
        .open_in_editor(
            &launcher,
            &open_request(
                &snapshot,
                "\u{65b0} \u{6587}\u{4ef6}.ts",
                EditorKind::Cursor,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(open_outcome.action, OpenFileAction::Open);
    let open_args = recorded_args(&capture_dir);
    assert_eq!(
        open_args,
        vec![
            canonical_root
                .join("\u{65b0} \u{6587}\u{4ef6}.ts")
                .to_str()
                .unwrap()
        ]
    );

    // 3. Unstaged modification -> Diff (BASE snapshot vs live worktree file):
    let diff_outcome = store
        .open_in_editor(
            &launcher,
            &open_request(&snapshot, "pnpm-lock.yaml", EditorKind::Vscode, None),
        )
        .await
        .unwrap();
    assert_eq!(diff_outcome.action, OpenFileAction::Diff);
    let diff_args = recorded_args(&capture_dir);
    assert_eq!(diff_args[0], "--diff");
    assert_eq!(
        fs::read(capture_dir.join("file-1")).unwrap(),
        b"lock: old\n"
    );
    assert_eq!(
        diff_args[2],
        canonical_root.join("pnpm-lock.yaml").to_str().unwrap()
    );

    // 4. Line goto -> --line for IDEA/WebStorm, --goto file:line:1 for VS Code/Cursor:
    let idea_goto = store
        .open_in_editor(
            &launcher,
            &open_request(&snapshot, "part.txt", EditorKind::Idea, Some(2)),
        )
        .await
        .unwrap();
    assert_eq!(idea_goto.action, OpenFileAction::Goto);
    assert_eq!(idea_goto.line, Some(2));
    assert_eq!(
        recorded_args(&capture_dir),
        vec![
            "--line".to_string(),
            "2".to_string(),
            canonical_root
                .join("part.txt")
                .to_str()
                .unwrap()
                .to_string()
        ]
    );

    let code_goto = store
        .open_in_editor(
            &launcher,
            &open_request(
                &snapshot,
                "\u{65b0} \u{6587}\u{4ef6}.ts",
                EditorKind::Vscode,
                Some(1),
            ),
        )
        .await
        .unwrap();
    assert_eq!(code_goto.action, OpenFileAction::Goto);
    assert_eq!(
        recorded_args(&capture_dir),
        vec![
            "--goto".to_string(),
            format!(
                "{}:1:1",
                canonical_root
                    .join("\u{65b0} \u{6587}\u{4ef6}.ts")
                    .display()
            )
        ]
    );

    fixture.verify_unchanged().unwrap();
    launcher.cleanup().unwrap();
    assert!(!Path::new(&diff_args[1]).exists());
}

#[tokio::test]
async fn handles_conflicts_renames_newlines_symlinks_linked_worktrees_and_cli_errors() {
    // 1. Rename with newline/Unicode + symlink pointing outside worktree:
    let fixture = Fixture::new().unwrap();
    let renamed = "\u{65b0} \u{540d}\n\u{5b57}.lock";
    git(&fixture.root, &["checkout", "--", "pnpm-lock.yaml"]).unwrap();
    git(&fixture.root, &["mv", "--", "pnpm-lock.yaml", renamed]).unwrap();
    std::os::unix::fs::symlink(
        "/definitely-missing-picket-outside-target",
        fixture.root.join("outside-link"),
    )
    .unwrap();

    let harness = tempfile::tempdir().unwrap();
    let bin_dir = harness.path().join("bin");
    let capture_dir = harness.path().join("capture");
    install_recording_clis(&bin_dir, &capture_dir);
    let launcher = EditorLauncher::with_search_dirs(vec![bin_dir]);
    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();

    let rename_outcome = store
        .open_in_editor(
            &launcher,
            &open_request(&snapshot, renamed, EditorKind::Webstorm, None),
        )
        .await
        .unwrap();
    assert_eq!(rename_outcome.action, OpenFileAction::Diff);
    assert_eq!(
        fs::read(capture_dir.join("file-1")).unwrap(),
        b"lock: old\n"
    );
    assert_eq!(
        fs::read(capture_dir.join("file-2")).unwrap(),
        b"lock: old\n"
    );

    let symlink_outcome = store
        .open_in_editor(
            &launcher,
            &open_request(&snapshot, "outside-link", EditorKind::Idea, None),
        )
        .await
        .unwrap();
    assert_eq!(symlink_outcome.action, OpenFileAction::Open);
    assert_eq!(
        fs::read(capture_dir.join("file-0")).unwrap(),
        b"/definitely-missing-picket-outside-target"
    );

    // 2. Linked worktree opens inside the linked worktree directory, never main worktree:
    let linked_dir =
        fs::canonicalize(fixture.root.parent().unwrap().join("linked worktree")).unwrap();
    fs::write(linked_dir.join("part.txt"), "linked-edit\n").unwrap();
    let linked_snap = store.open(&linked_dir).await.unwrap();
    store
        .open_in_editor(
            &launcher,
            &open_request(&linked_snap, "part.txt", EditorKind::Cursor, None),
        )
        .await
        .unwrap();
    assert_eq!(
        fs::canonicalize(fs::read_to_string(capture_dir.join("cwd")).unwrap().trim()).unwrap(),
        linked_dir
    );
    assert_eq!(
        recorded_args(&capture_dir)[2],
        linked_dir.join("part.txt").to_str().unwrap()
    );

    // 3. 3-way conflict merge exports ancestor, ours, theirs for JetBrains, and opens working file for VS Code:
    let conflict_fixture = Fixture::new().unwrap();
    git(
        &conflict_fixture.root,
        &[
            "add",
            "--",
            "part.txt",
            "pnpm-lock.yaml",
            "\u{65b0} \u{6587}\u{4ef6}.ts",
        ],
    )
    .unwrap();
    git(&conflict_fixture.root, &["commit", "-m", "test: base"]).unwrap();
    git(&conflict_fixture.root, &["branch", "side"]).unwrap();
    fs::write(conflict_fixture.root.join("part.txt"), "ours=1\n").unwrap();
    git(&conflict_fixture.root, &["commit", "-am", "test: ours"]).unwrap();
    git(&conflict_fixture.root, &["checkout", "side"]).unwrap();
    fs::write(conflict_fixture.root.join("part.txt"), "theirs=1\n").unwrap();
    git(&conflict_fixture.root, &["commit", "-am", "test: theirs"]).unwrap();
    assert!(git(&conflict_fixture.root, &["merge", "feat/read-proof"]).is_err());

    let conflict_snap = store.open(&conflict_fixture.root).await.unwrap();
    let idea_merge = store
        .open_in_editor(
            &launcher,
            &open_request(&conflict_snap, "part.txt", EditorKind::Idea, None),
        )
        .await
        .unwrap();
    assert_eq!(idea_merge.action, OpenFileAction::Merge);
    let merge_args = recorded_args(&capture_dir);
    assert_eq!(merge_args.len(), 5);
    assert_eq!(merge_args[0], "merge");
    assert_eq!(fs::read(capture_dir.join("file-1")).unwrap(), b"theirs=1\n");
    assert_eq!(fs::read(capture_dir.join("file-2")).unwrap(), b"ours=1\n");
    assert_eq!(
        fs::read(capture_dir.join("file-3")).unwrap(),
        b"first=2\nsecond=3\n"
    );

    let vscode_merge = store
        .open_in_editor(
            &launcher,
            &open_request(&conflict_snap, "part.txt", EditorKind::Vscode, None),
        )
        .await
        .unwrap();
    assert_eq!(vscode_merge.action, OpenFileAction::Merge);
    assert_eq!(recorded_args(&capture_dir).len(), 1);

    // 4. Missing CLI, failing CLI, line=0, and unknown JSON fields:
    let empty_launcher = EditorLauncher::with_search_dirs(vec![harness.path().join("missing-bin")]);
    let missing_err = store
        .open_in_editor(
            &empty_launcher,
            &open_request(&conflict_snap, "part.txt", EditorKind::Cursor, None),
        )
        .await
        .unwrap_err();
    assert!(
        missing_err.contains("Cursor") && missing_err.contains("cursor"),
        "{missing_err}"
    );

    let fail_bin = harness.path().join("fail-bin");
    fs::create_dir(&fail_bin).unwrap();
    let fail_code = fail_bin.join("code");
    fs::write(&fail_code, "#!/bin/sh\necho 'cli broken' >&2\nexit 2\n").unwrap();
    fs::set_permissions(&fail_code, fs::Permissions::from_mode(0o755)).unwrap();
    let fail_launcher = EditorLauncher::with_search_dirs(vec![fail_bin]);
    let cli_err = store
        .open_in_editor(
            &fail_launcher,
            &open_request(&conflict_snap, "part.txt", EditorKind::Vscode, None),
        )
        .await
        .unwrap_err();
    assert!(cli_err.contains("cli broken"), "{cli_err}");

    let zero_line_err = store
        .open_in_editor(
            &launcher,
            &open_request(&conflict_snap, "part.txt", EditorKind::Idea, Some(0)),
        )
        .await
        .unwrap_err();
    assert!(zero_line_err.contains("line number"), "{zero_line_err}");

    assert!(
        serde_json::from_value::<OpenFileRequest>(serde_json::json!({
            "requestId": "ide-wire",
            "target": { "repositoryId": "r", "worktreeId": "w", "sessionId": "s", "scopeVersion": "v" },
            "fileId": "f",
            "editor": "idea",
            "command": "rm -rf /"
        }))
        .is_err()
    );

    // 5. macOS .app bundle fallback paths resolve for all 5 supported IDEs and detect_installed_editors filters accurately:
    let apps_dir = harness.path().join("Applications");
    for (editor, rel_bundle) in [
        (EditorKind::Idea, "IntelliJ IDEA CE.app/Contents/MacOS/idea"),
        (
            EditorKind::Rustrover,
            "RustRover.app/Contents/MacOS/rustrover",
        ),
        (EditorKind::Webstorm, "WebStorm.app/Contents/MacOS/webstorm"),
        (
            EditorKind::Vscode,
            "Visual Studio Code.app/Contents/Resources/app/bin/code",
        ),
        (
            EditorKind::Cursor,
            "Cursor.app/Contents/Resources/app/bin/cursor",
        ),
    ] {
        let bundle_exec = apps_dir.join(rel_bundle);
        fs::create_dir_all(bundle_exec.parent().unwrap()).unwrap();
        fs::write(&bundle_exec, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&bundle_exec, fs::Permissions::from_mode(0o755)).unwrap();
        let bundle_launcher = EditorLauncher::with_search_dirs(vec![apps_dir.clone()]);
        assert_eq!(
            bundle_launcher.resolve_executable(editor).unwrap(),
            bundle_exec
        );
    }
    let partial_apps = harness.path().join("PartialApps");
    for rel in [
        "IntelliJ IDEA.app/Contents/MacOS/idea",
        "RustRover.app/Contents/MacOS/rustrover",
    ] {
        let exec = partial_apps.join(rel);
        fs::create_dir_all(exec.parent().unwrap()).unwrap();
        fs::write(&exec, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&exec, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let partial_launcher = EditorLauncher::with_search_dirs(vec![partial_apps]);
    assert_eq!(
        partial_launcher.detect_installed_editors(),
        vec![EditorKind::Idea, EditorKind::Rustrover]
    );
    assert!(empty_launcher.detect_installed_editors().is_empty());

    // 6. Direct repo-relative `path` open & goto works even on clean tracked files and blocks escapes / `.git`:
    let clean_fixture = Fixture::new().unwrap();
    git(
        &clean_fixture.root,
        &[
            "add",
            "--",
            "part.txt",
            "pnpm-lock.yaml",
            "\u{65b0} \u{6587}\u{4ef6}.ts",
        ],
    )
    .unwrap();
    git(&clean_fixture.root, &["commit", "-m", "test: clean"]).unwrap();
    let clean_snap = store.open(&clean_fixture.root).await.unwrap();
    let clean_tree = active(&clean_snap);
    assert!(clean_tree.files.is_empty());
    let clean_target = ReadTarget {
        repository_id: clean_tree.repository_id.clone(),
        worktree_id: clean_tree.worktree_id.clone(),
        session_id: "ide-path-tests".into(),
        scope_version: clean_tree.status_version.clone(),
    };
    let path_goto = store
        .open_in_editor(
            &launcher,
            &OpenFileRequest {
                request_id: "ide-path-goto".into(),
                target: clean_target.clone(),
                file_id: None,
                path: Some("./part.txt".into()),
                editor: EditorKind::Idea,
                line: Some(2),
                locale: crate::i18n::LanguageKind::En,
            },
        )
        .await
        .unwrap();
    assert_eq!(path_goto.action, OpenFileAction::Goto);
    assert_eq!(path_goto.file_path, "part.txt");
    assert_eq!(path_goto.line, Some(2));

    let path_open = store
        .open_in_editor(
            &launcher,
            &OpenFileRequest {
                request_id: "ide-path-open".into(),
                target: clean_target.clone(),
                file_id: None,
                path: Some("part.txt".into()),
                editor: EditorKind::Vscode,
                line: None,
                locale: crate::i18n::LanguageKind::En,
            },
        )
        .await
        .unwrap();
    assert_eq!(path_open.action, OpenFileAction::Open);

    // Reject `.git` and `..` path traversal on direct path requests:
    assert!(
        store
            .open_in_editor(
                &launcher,
                &OpenFileRequest {
                    request_id: "ide-path-git".into(),
                    target: clean_target.clone(),
                    file_id: None,
                    path: Some(".git/config".into()),
                    editor: EditorKind::Idea,
                    line: Some(1),
                    locale: crate::i18n::LanguageKind::En,
                },
            )
            .await
            .is_err()
    );
    assert!(
        store
            .open_in_editor(
                &launcher,
                &OpenFileRequest {
                    request_id: "ide-path-escape".into(),
                    target: clean_target,
                    file_id: None,
                    path: Some("../escape.txt".into()),
                    editor: EditorKind::Idea,
                    line: Some(1),
                    locale: crate::i18n::LanguageKind::En,
                },
            )
            .await
            .is_err()
    );

    launcher.cleanup().unwrap();
}
