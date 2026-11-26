//! End-to-end regression tests for Custom Action workspace tools (`edit_repo_file`,
//! `run_repo_command`) and action-edited commit scope closure (`commit_exact_scope` with
//! `editedPaths`), verifying Tier-2 confirmation + prompt-hash trust, Local Keep & unselected
//! draft protection, tamper detection, and exact-scope Git commits.

use super::{
    CustomActionCoordinator, SessionAuthGrants,
    context::{ActionRunState, ActionToolContext},
    tools_command::handle_run_repo_command,
    tools_git_ops::handle_commit_exact_scope,
    tools_workspace::{
        handle_edit_repo_file, handle_write_repo_file, hydrate_verified_edited_hashes, sha256_hex,
    },
};
use crate::{
    actions::{
        session_store::{
            PersistedActionSession, find_session_by_run_id, format_resume_checkpoint_for_prompt,
            now_millis, upsert_session,
        },
        types::*,
    },
    i18n::LanguageKind,
    repository::{RepositoryStore, types::ReadTarget},
    skills::RuntimeSkillSnapshot,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("/usr/bin/git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn respond_tier2(
    coordinator: CustomActionCoordinator,
    run_id: &str,
    confirm_call_id: &str,
    approve: bool,
    dont_ask_again: bool,
) -> tokio::task::JoinHandle<()> {
    let run_id = run_id.to_owned();
    let call_id = confirm_call_id.to_owned();
    tokio::spawn(async move {
        for _ in 0..40 {
            let res = coordinator.respond_question(&RespondCustomActionRequest {
                request_id: format!("resp-{call_id}"),
                run_id: run_id.clone(),
                call_id: call_id.clone(),
                response: AskUserQuestionResponse {
                    cancelled: false,
                    answers: vec![AskUserQuestionAnswer {
                        question_index: 0,
                        question_id: "confirm-write".into(),
                        header: "Confirm".into(),
                        selected_option_ids: vec![if approve {
                            "approve-write".into()
                        } else {
                            "cancel-write".into()
                        }],
                        selected_labels: vec![if approve {
                            "Approve".into()
                        } else {
                            "Cancel".into()
                        }],
                        custom_text: None,
                    }],
                    unanswered: vec![],
                    global_note: None,
                    dont_ask_again: dont_ask_again.then_some(true),
                },
                locale: LanguageKind::En,
            });
            if res.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        panic!("Timed out delivering Tier-2 confirmation response for {call_id}");
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_command_and_commit_release_loop_preserves_unselected_and_local_keep() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let storage = temp.path().join("storage");
    let mock_bin = temp.path().join("bin");
    fs::create_dir_all(root.join("src-tauri")).unwrap();
    fs::create_dir_all(&storage).unwrap();
    fs::create_dir_all(&mock_bin).unwrap();

    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Picket Test"]);
    git(&root, &["config", "user.email", "test@picket.local"]);

    fs::write(
        root.join("package.json"),
        "{\n  \"name\": \"picket\",\n  \"version\": \"0.2.0\"\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src-tauri/tauri.conf.json"),
        "{\n  \"productName\": \"Picket\",\n  \"version\": \"0.2.0\"\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src-tauri/Cargo.toml"),
        "[package]\nname = \"picket\"\nversion = \"0.2.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src-tauri/Cargo.lock"),
        "[[package]]\nname = \"other\"\nversion = \"0.2.0\"\n\n[[package]]\nname = \"picket\"\nversion = \"0.2.0\"\n",
    )
    .unwrap();
    fs::write(root.join("draft.ts"), "export const v = 1;\n").unwrap();
    fs::write(root.join("local.txt"), "keep-me=1\n").unwrap();
    fs::write(root.join(".env"), "API_KEY=secret\n").unwrap();
    git(
        &root,
        &[
            "add",
            "package.json",
            "src-tauri/tauri.conf.json",
            "src-tauri/Cargo.toml",
            "src-tauri/Cargo.lock",
            "draft.ts",
            "local.txt",
        ],
    );
    git(&root, &["commit", "-m", "chore: initial release baseline"]);

    // Create a pre-existing unselected dirty draft and a Local Keep edit that must NEVER be modified or committed:
    fs::write(
        root.join("draft.ts"),
        "export const v = 2; // unselected user draft\n",
    )
    .unwrap();
    fs::write(root.join("local.txt"), "keep-me=2\n").unwrap();

    // Create a mock verifier executable inside mock_bin:
    let verifier = mock_bin.join("verify-versions");
    fs::write(
        &verifier,
        r#"#!/bin/sh
grep -q '"0.2.1"' package.json &&
grep -q '"0.2.1"' src-tauri/tauri.conf.json &&
grep -q 'version = "0.2.1"' src-tauri/Cargo.toml &&
grep -q 'version = "0.2.1"' src-tauri/Cargo.lock &&
echo "versionVerified=true tag=$1"
"#,
    )
    .unwrap();
    fs::set_permissions(&verifier, fs::Permissions::from_mode(0o755)).unwrap();

    let store = RepositoryStore::default();
    let snap = store.open(&root).await.unwrap();
    let tree = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "ws-test-session".into(),
        scope_version: tree.status_version.clone(),
    };
    let coordinator = CustomActionCoordinator::default();
    coordinator.set_override_cli_dirs(Some(vec![mock_bin.clone()]));

    let run_state = Arc::new(Mutex::new(ActionRunState {
        timeline: Vec::new(),
        commits: Vec::new(),
        links: Vec::new(),
        committed_file_ids: HashSet::new(),
        action_edited_hashes: HashMap::new(),
        committed_edited_paths: HashSet::new(),
        skip_write_confirm: false,
        trusted_action_update: None,
        wrote_repo: false,
        submit_ledger: Default::default(),
    }));
    let mut local_keep = HashSet::new();
    local_keep.insert("local.txt".to_owned());

    let ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        emit: Arc::new(|_, _, _, _| {}),
        persist: Arc::new(|_, _| {}),
        run_state: Arc::clone(&run_state),
        grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
        skills: Arc::new(RuntimeSkillSnapshot::load_from_data_dir(&storage)),
        root: root.clone(),
        storage_dir: storage.clone(),
        bundled_node_path: mock_bin.join("picket-node"),
        remotes: Vec::new(),
        frozen_selections: Vec::new(), // Simulates clicking release button with 0 pre-selected files
        live_changes: tree.files.clone(),
        local_keep_paths: local_keep,
        target: target.clone(),
        run_id: "run-rel-1".into(),
        action_id: "act-release".into(),
        action_label: "Patch Release".into(),
        expected_hash: "prompt-hash-abc".into(),
        locale: LanguageKind::En,
    };

    // 1. Guardrail rejections before any confirmation prompt:
    let env_res = handle_edit_repo_file(
        &ctx,
        "c-env",
        &json!({ "path": ".env", "oldText": "secret", "newText": "new" }),
    )
    .await;
    assert!(!env_res.ok && env_res.content.contains("sensitive"));

    let keep_res = handle_edit_repo_file(
        &ctx,
        "c-keep",
        &json!({ "path": "local.txt", "oldText": "keep-me=2", "newText": "keep-me=3" }),
    )
    .await;
    assert!(!keep_res.ok && keep_res.content.contains("Local Keep"));

    let draft_res = handle_edit_repo_file(
        &ctx,
        "c-draft",
        &json!({ "path": "draft.ts", "oldText": "v = 2", "newText": "v = 3" }),
    )
    .await;
    assert!(!draft_res.ok && draft_res.content.contains("unselected"));

    let non_unique_res = handle_edit_repo_file(
        &ctx,
        "c-dup",
        &json!({ "path": "src-tauri/Cargo.lock", "oldText": "version = \"0.2.0\"", "newText": "version = \"0.2.1\"" }),
    )
    .await;
    assert!(!non_unique_res.ok && non_unique_res.content.contains("matched 2 locations"));

    // 2. User declines Tier-2 confirmation -> file remains untouched:
    let decline_task = respond_tier2(
        coordinator.clone(),
        "run-rel-1",
        "confirm-c-decline",
        false,
        false,
    );
    let declined = handle_edit_repo_file(
        &ctx,
        "c-decline",
        &json!({
            "path": "package.json",
            "oldText": "\"version\": \"0.2.0\"",
            "newText": "\"version\": \"0.2.1\""
        }),
    )
    .await;
    decline_task.await.unwrap();
    assert!(!declined.ok);
    assert!(
        fs::read_to_string(root.join("package.json"))
            .unwrap()
            .contains("0.2.0")
    );

    // 3. User approves Tier-2 confirmation WITH `dont_ask_again: true`:
    let approve_task = respond_tier2(
        coordinator.clone(),
        "run-rel-1",
        "confirm-c-pkg",
        true,
        true,
    );
    let pkg_res = handle_edit_repo_file(
        &ctx,
        "c-pkg",
        &json!({
            "path": "package.json",
            "oldText": "\"version\": \"0.2.0\"",
            "newText": "\"version\": \"0.2.1\""
        }),
    )
    .await;
    approve_task.await.unwrap();
    assert!(pkg_res.ok, "package.json edit failed: {}", pkg_res.content);
    assert!(
        run_state
            .lock()
            .unwrap()
            .trusted_action_update
            .as_ref()
            .is_some_and(|u| u.trusted_prompt_hash == "prompt-hash-abc")
    );

    // 4. Remaining 3 manifest edits + repo command verification proceed without re-prompting:
    for (call_id, path, old_text, new_text) in [
        (
            "c-tauri",
            "src-tauri/tauri.conf.json",
            "\"version\": \"0.2.0\"",
            "\"version\": \"0.2.1\"",
        ),
        (
            "c-toml",
            "src-tauri/Cargo.toml",
            "version = \"0.2.0\"",
            "version = \"0.2.1\"",
        ),
        (
            "c-lock",
            "src-tauri/Cargo.lock",
            "name = \"picket\"\nversion = \"0.2.0\"",
            "name = \"picket\"\nversion = \"0.2.1\"",
        ),
    ] {
        let res = handle_edit_repo_file(
            &ctx,
            call_id,
            &json!({ "path": path, "oldText": old_text, "newText": new_text }),
        )
        .await;
        assert!(res.ok, "Edit of {path} failed: {}", res.content);
    }

    // 5. Hard redline still blocks forbidden binaries even after `skip_write_confirm` is true:
    let blocked_cmd = handle_run_repo_command(
        &ctx,
        "c-bad-cmd",
        &json!({ "command": "rm", "args": ["-rf", "package.json"] }),
        "Running rm".into(),
    )
    .await;
    assert!(!blocked_cmd.ok && blocked_cmd.content.contains("hard redline"));

    // 6. Valid local verification command runs cleanly:
    let verify_cmd = handle_run_repo_command(
        &ctx,
        "c-verify",
        &json!({ "command": "verify-versions", "args": ["v0.2.1"] }),
        "Running verify-versions".into(),
    )
    .await;
    assert!(
        verify_cmd.ok,
        "verify-versions failed: {}",
        verify_cmd.content
    );
    assert!(
        verify_cmd
            .content
            .contains("versionVerified=true tag=v0.2.1")
    );

    // 7. Committing an unedited/unselected file via `editedPaths` is rejected:
    let bad_commit = handle_commit_exact_scope(
        &ctx,
        &json!({
            "editedPaths": ["draft.ts"],
            "message": "chore: sneak in draft\n\n- should fail"
        }),
    )
    .await;
    assert!(
        !bad_commit.ok
            && bad_commit
                .content
                .contains("was not created or modified by this action")
    );

    // 8. Tampering with an edited file externally causes SHA-256 verification to abort commit:
    let saved_pkg = fs::read_to_string(root.join("package.json")).unwrap();
    fs::write(
        root.join("package.json"),
        format!("{saved_pkg}// tampered\n"),
    )
    .unwrap();
    let tampered_commit = handle_commit_exact_scope(
        &ctx,
        &json!({
            "editedPaths": ["package.json"],
            "message": "chore(release): bump version to 0.2.1\n\n- test tamper"
        }),
    )
    .await;
    assert!(
        !tampered_commit.ok
            && tampered_commit
                .content
                .contains("changed externally on disk")
    );
    fs::write(root.join("package.json"), saved_pkg).unwrap();

    // 9. Exact-scope commit of ONLY the 4 action-edited version files succeeds, leaving draft.ts & local.txt untouched:
    let commit_res = handle_commit_exact_scope(
        &ctx,
        &json!({
            "editedPaths": [
                "package.json",
                "src-tauri/tauri.conf.json",
                "src-tauri/Cargo.toml",
                "src-tauri/Cargo.lock"
            ],
            "message": "chore(release): bump version to 0.2.1\n\n- Sync all four version manifests to 0.2.1"
        }),
    )
    .await;
    assert!(
        commit_res.ok,
        "commit_exact_scope failed: {}",
        commit_res.content
    );

    let committed_files = git(&root, &["show", "--name-only", "--format=", "HEAD"]);
    let committed_set: HashSet<&str> = committed_files.lines().collect();
    assert_eq!(
        committed_set,
        HashSet::from([
            "package.json",
            "src-tauri/tauri.conf.json",
            "src-tauri/Cargo.toml",
            "src-tauri/Cargo.lock",
        ])
    );
    assert_eq!(
        fs::read_to_string(root.join("draft.ts")).unwrap(),
        "export const v = 2; // unselected user draft\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("local.txt")).unwrap(),
        "keep-me=2\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_repo_file_command_diff_tracking_and_persisted_hash_hydration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let storage = temp.path().join("storage");
    let mock_bin = temp.path().join("bin");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&storage).unwrap();
    fs::create_dir_all(&mock_bin).unwrap();

    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Picket Test"]);
    git(&root, &["config", "user.email", "test@picket.local"]);

    fs::write(root.join("src/app.ts"), "export const formatted = false;\n").unwrap();
    fs::write(root.join("draft.ts"), "export const draft = 1;\n").unwrap();
    fs::write(root.join("local.txt"), "keep=1\n").unwrap();
    git(&root, &["add", "src/app.ts", "draft.ts", "local.txt"]);
    git(&root, &["commit", "-m", "chore: baseline"]);

    // Pre-existing unselected user draft and Local Keep change:
    fs::write(root.join("draft.ts"), "export const draft = 2;\n").unwrap();
    fs::write(root.join("local.txt"), "keep=2\n").unwrap();

    // Mock command simulating a formatter/generator that updates clean `src/app.ts`, creates
    // `generated/info.txt`, and even happens to touch `draft.ts` and `local.txt` (both off-limits):
    let fmt_cmd = mock_bin.join("auto-fmt");
    fs::write(
        &fmt_cmd,
        r#"#!/bin/sh
printf 'export const formatted = true;\n' > src/app.ts
mkdir -p generated
printf 'generated=ok\n' > generated/info.txt
printf 'export const draft = 2; // formatted\n' > draft.ts
printf 'keep=2-touched\n' > local.txt
echo "formatted"
"#,
    )
    .unwrap();
    fs::set_permissions(&fmt_cmd, fs::Permissions::from_mode(0o755)).unwrap();

    let store = RepositoryStore::default();
    let snap = store.open(&root).await.unwrap();
    let tree = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "ws-closure-session".into(),
        scope_version: tree.status_version.clone(),
    };
    let coordinator = CustomActionCoordinator::default();
    coordinator.set_override_cli_dirs(Some(vec![mock_bin.clone()]));

    let run_state = Arc::new(Mutex::new(ActionRunState {
        timeline: Vec::new(),
        commits: Vec::new(),
        links: Vec::new(),
        committed_file_ids: HashSet::new(),
        action_edited_hashes: HashMap::new(),
        committed_edited_paths: HashSet::new(),
        skip_write_confirm: false,
        trusted_action_update: None,
        wrote_repo: false,
        submit_ledger: Default::default(),
    }));
    let local_keep = HashSet::from(["local.txt".to_owned()]);
    let ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        emit: Arc::new(|_, _, _, _| {}),
        persist: Arc::new(|_, _| {}),
        run_state: Arc::clone(&run_state),
        grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
        skills: Arc::new(RuntimeSkillSnapshot::load_from_data_dir(&storage)),
        root: root.clone(),
        storage_dir: storage.clone(),
        bundled_node_path: mock_bin.join("picket-node"),
        remotes: Vec::new(),
        frozen_selections: Vec::new(),
        live_changes: tree.files.clone(),
        local_keep_paths: local_keep.clone(),
        target: target.clone(),
        run_id: "run-ws-closure".into(),
        action_id: "act-fmt".into(),
        action_label: "Format & Changelog".into(),
        expected_hash: "prompt-hash-fmt".into(),
        locale: LanguageKind::En,
    };

    // 1. write_repo_file rejects existing files, sensitive files, and Local Keep:
    let existing_err = handle_write_repo_file(
        &ctx,
        "w-exist",
        &json!({ "path": "src/app.ts", "content": "overwrite" }),
    )
    .await;
    assert!(!existing_err.ok && existing_err.content.contains("already exists"));

    let env_err = handle_write_repo_file(
        &ctx,
        "w-env",
        &json!({ "path": ".env.local", "content": "SECRET=1\n" }),
    )
    .await;
    assert!(!env_err.ok && env_err.content.contains("sensitive"));

    let keep_err = handle_write_repo_file(
        &ctx,
        "w-keep",
        &json!({ "path": "local.txt", "content": "x" }),
    )
    .await;
    assert!(!keep_err.ok && keep_err.content.contains("Local Keep"));

    // 2. write_repo_file honors Tier-2 decline and creates new file atomically on approval:
    let decline_task = respond_tier2(
        coordinator.clone(),
        "run-ws-closure",
        "confirm-w-decl",
        false,
        false,
    );
    let declined = handle_write_repo_file(
        &ctx,
        "w-decl",
        &json!({ "path": "docs/CHANGELOG.md", "content": "# Changelog\n" }),
    )
    .await;
    decline_task.await.unwrap();
    assert!(!declined.ok);
    assert!(!root.join("docs/CHANGELOG.md").exists());

    let approve_task = respond_tier2(
        coordinator.clone(),
        "run-ws-closure",
        "confirm-w-ok",
        true,
        true,
    );
    let created = handle_write_repo_file(
        &ctx,
        "w-ok",
        &json!({ "path": "docs/CHANGELOG.md", "content": "# Changelog\n- Added release notes\n" }),
    )
    .await;
    approve_task.await.unwrap();
    assert!(created.ok, "write_repo_file failed: {}", created.content);
    assert_eq!(
        fs::read_to_string(root.join("docs/CHANGELOG.md")).unwrap(),
        "# Changelog\n- Added release notes\n"
    );

    // 3. run_repo_command auto-tracks clean/new files (`src/app.ts`, `generated/info.txt`)
    //    while EXCLUDING pre-existing unselected `draft.ts` and Local Keep `local.txt`:
    let fmt_res = handle_run_repo_command(
        &ctx,
        "cmd-fmt",
        &json!({ "command": "auto-fmt", "args": [] }),
        "Running auto-fmt".into(),
    )
    .await;
    assert!(fmt_res.ok, "auto-fmt failed: {}", fmt_res.content);
    assert!(fmt_res.content.contains("modifiedPaths"));
    assert!(fmt_res.content.contains("src/app.ts"));
    assert!(fmt_res.content.contains("generated/info.txt"));
    assert!(!fmt_res.content.contains("draft.ts"));
    assert!(!fmt_res.content.contains("local.txt"));

    // Attempting to commit draft.ts via editedPaths is still refused:
    let draft_commit = handle_commit_exact_scope(
        &ctx,
        &json!({
            "editedPaths": ["draft.ts"],
            "message": "chore: sneak draft\n\n- blocked"
        }),
    )
    .await;
    assert!(!draft_commit.ok);

    // Committing the 3 tracked paths succeeds cleanly without leaking draft.ts or local.txt:
    let commit_res = handle_commit_exact_scope(
        &ctx,
        &json!({
            "editedPaths": ["docs/CHANGELOG.md", "src/app.ts", "generated/info.txt"],
            "message": "chore: format code and add changelog\n\n- format src/app.ts\n- add docs/CHANGELOG.md and generated/info.txt"
        }),
    )
    .await;
    assert!(commit_res.ok, "commit failed: {}", commit_res.content);
    let committed_files = git(&root, &["show", "--name-only", "--format=", "HEAD"]);
    let committed_set: HashSet<&str> = committed_files.lines().collect();
    assert_eq!(
        committed_set,
        HashSet::from(["docs/CHANGELOG.md", "src/app.ts", "generated/info.txt"])
    );

    // 4. Persisted edited_file_hashes checkpoint lookup & verified hydration across follow-up runs:
    let followup_note = handle_write_repo_file(
        &ctx,
        "w-note",
        &json!({ "path": "notes/release.md", "content": "Draft release note\n" }),
    )
    .await;
    assert!(followup_note.ok);
    let note_hash = sha256_hex(b"Draft release note\n");

    let mut persisted_hashes = BTreeMap::new();
    persisted_hashes.insert("notes/release.md".to_owned(), note_hash.clone());
    persisted_hashes.insert("src/app.ts".to_owned(), "0000000000000000".to_owned());
    persisted_hashes.insert("local.txt".to_owned(), sha256_hex(b"keep=2-touched\n"));

    upsert_session(
        &storage,
        PersistedActionSession {
            run_id: "run-ws-closure".into(),
            repository_id: target.repository_id.clone(),
            worktree_id: target.worktree_id.clone(),
            action_id: "act-fmt".into(),
            action_label: "Format & Changelog".into(),
            action_prompt_hash: "prompt-hash-fmt".into(),
            status: "done".into(),
            updated_at_ms: now_millis(),
            summary: "Wrote draft release note.".into(),
            dismissed_message_id: None,
            timeline: vec![CustomActionTimelineItem {
                state: "done".into(),
                label: "Created notes/release.md".into(),
            }],
            commits: vec![],
            links: vec![],
            edited_file_hashes: persisted_hashes.clone(),
            submit_ledger: Default::default(),
        },
    )
    .unwrap();

    let found = find_session_by_run_id(
        &storage,
        &target.repository_id,
        &target.worktree_id,
        "run-ws-closure",
    )
    .expect("session should be found by run_id");
    let checkpoint_text = format_resume_checkpoint_for_prompt(&found);
    assert!(checkpoint_text.contains("notes/release.md"));
    assert!(checkpoint_text.contains("Created notes/release.md"));

    let hydrated = hydrate_verified_edited_hashes(&root, &found.edited_file_hashes, &local_keep);
    assert_eq!(hydrated.len(), 1);
    assert_eq!(hydrated.get("notes/release.md"), Some(&note_hash));
}
