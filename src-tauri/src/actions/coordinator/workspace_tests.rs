//! End-to-end regression tests for Custom Action workspace tools (`edit_repo_file`,
//! `run_repo_command`) and action-edited commit scope closure (`commit_exact_scope` with
//! `editedPaths`), verifying Tier-2 confirmation + prompt-hash trust, Local Keep & unselected
//! draft protection, tamper detection, and exact-scope Git commits.

use super::{
    CustomActionCoordinator, SessionAuthGrants,
    context::{ActionRunState, ActionToolContext},
    tools_cli::handle_read_scope_diff,
    tools_command::handle_run_repo_command,
    tools_commit::handle_commit_exact_scope,
    tools_sync::{handle_handoff_to_recovery, handle_sync_and_push},
    tools_workspace::{
        handle_edit_repo_file, handle_write_repo_file, hydrate_verified_edited_hashes, sha256_hex,
    },
};
use crate::{
    actions::{
        session_store::{
            find_session_by_run_id, format_resume_checkpoint_for_prompt, now_millis, upsert_session,
        },
        types::*,
    },
    i18n::LanguageKind,
    models::store::ModelStore,
    pi_runtime::{PiPaths, PiSupervisor},
    repository::{
        RepositoryStore,
        remote::RebaseSyncOutcome,
        types::{CommitSelection, ReadTarget},
    },
    skills::RuntimeSkillSnapshot,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
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
            let res = coordinator.respond_question(&RespondActionQuestionRequest {
                request_id: format!("resp-{call_id}"),
                run_id: run_id.clone(),
                call_id: call_id.clone(),
                response: MessageUserResponse {
                    cancelled: false,
                    answers: vec![MessageUserAnswer {
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

    let run_state = Arc::new(Mutex::new(ActionRunState::default()));
    let mut local_keep = HashSet::new();
    local_keep.insert("local.txt".to_owned());

    let mut ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        profile: ActionProfileKind::CustomAction.profile(),
        emit: Arc::new(|_, _, _, _| {}),
        raw_emit: Arc::new(|_| {}),
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
        request_id: "run-rel-1".into(),
        run_id: "run-rel-1".into(),
        action_id: "act-release".into(),
        action_label: "Patch Release".into(),
        expected_hash: "prompt-hash-abc".into(),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: ActionApprovalMode::Ask,
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

    // 3. User approves Tier-2 confirmation under Ask mode:
    let approve_task = respond_tier2(
        coordinator.clone(),
        "run-rel-1",
        "confirm-c-pkg",
        true,
        false,
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
    assert!(run_state.lock().unwrap().trusted_action_update.is_none());

    // 4. Under global Auto mode, remaining 3 routine manifest edits + repo command verification proceed without re-prompting:
    ctx.approval_mode = ActionApprovalMode::Auto;
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
        "commit-bad",
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
        "commit-tampered",
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
        "commit-ok",
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

    let run_state = Arc::new(Mutex::new(ActionRunState::default()));
    let local_keep = HashSet::from(["local.txt".to_owned()]);
    let mut ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        profile: ActionProfileKind::CustomAction.profile(),
        emit: Arc::new(|_, _, _, _| {}),
        raw_emit: Arc::new(|_| {}),
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
        request_id: "run-ws-closure".into(),
        run_id: "run-ws-closure".into(),
        action_id: "act-fmt".into(),
        action_label: "Format & Changelog".into(),
        expected_hash: "prompt-hash-fmt".into(),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: ActionApprovalMode::Ask,
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
        false,
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

    // 3. Under global Auto mode, run_repo_command auto-tracks clean/new files (`src/app.ts`, `generated/info.txt`)
    //    while EXCLUDING pre-existing unselected `draft.ts` and Local Keep `local.txt`:
    ctx.approval_mode = ActionApprovalMode::Auto;
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
        "commit-draft",
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
        "commit-fmt",
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
        StoredActionSession {
            run_id: "run-ws-closure".into(),
            repository_id: target.repository_id.clone(),
            worktree_id: target.worktree_id.clone(),
            session_id: target.session_id.clone(),
            action_id: "act-fmt".into(),
            action_label: "Format & Changelog".into(),
            profile: ActionProfileKind::CustomAction,
            action_prompt_hash: "prompt-hash-fmt".into(),
            frozen_scope_version: target.scope_version.clone(),
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
            updated_at_ms: now_millis(),
            summary: "Wrote draft release note.".into(),
            card: None,
            message: None,
            timeline: vec![WorkflowTimelineItem {
                state: "done".into(),
                label: "Created notes/release.md".into(),
            }],
            qa_log: vec![],
            commits: vec![],
            links: vec![],
            can_retry_remaining: false,
            push_authorized: false,
            auto_collapse: false,
            notice_dismissed: false,
            dismissed_message_id: None,
            edited_file_hashes: persisted_hashes.clone(),
            submit_ledger: Default::default(),
            locale: LanguageKind::En,
            commit_language: LanguageKind::En,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_action_supports_read_scope_diff_multigroup_commit_and_no_unrequested_sensitive_popup()
 {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let storage = temp.path().join("storage");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&storage).unwrap();

    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Picket Test"]);
    git(&root, &["config", "user.email", "test@picket.local"]);

    fs::write(root.join("src/a.ts"), "export const a = 1;\n").unwrap();
    fs::write(root.join("src/b.ts"), "export const b = 1;\n").unwrap();
    fs::write(root.join("src/unselected.ts"), "export const u = 1;\n").unwrap();
    fs::write(root.join("local.txt"), "keep=1\n").unwrap();
    git(
        &root,
        &[
            "add",
            "src/a.ts",
            "src/b.ts",
            "src/unselected.ts",
            "local.txt",
        ],
    );
    git(&root, &["commit", "-m", "chore: baseline"]);

    // Modify selected files (including explicitly selected `.env.local`), unselected file, and Local Keep:
    fs::write(root.join("src/a.ts"), "export const a = 2;\n").unwrap();
    fs::write(root.join("src/b.ts"), "export const b = 2;\n").unwrap();
    fs::write(root.join(".env.local"), "PUBLIC_FLAG=true\n").unwrap();
    fs::write(root.join("src/unselected.ts"), "export const u = 2;\n").unwrap();
    fs::write(root.join("local.txt"), "keep=2\n").unwrap();

    let store = RepositoryStore::default();
    let snap = store.open(&root).await.unwrap();
    let tree = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "ws-unified-multigroup".into(),
        scope_version: tree.status_version.clone(),
    };
    let mut selections = Vec::new();
    for path in ["src/a.ts", "src/b.ts", ".env.local"] {
        let change = tree.files.iter().find(|f| f.path == path).unwrap();
        selections.push(CommitSelection {
            file_id: change.id.clone(),
            commit_token: change.commit_token.clone(),
        });
    }
    let scope = store
        .extract_cpush_scope(&target, &selections, LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(scope.files.len(), 3);

    let coordinator = CustomActionCoordinator::default();
    let run_state = Arc::new(Mutex::new(ActionRunState {
        branch: scope.branch.clone(),
        initial_head: scope.initial_head.clone(),
        frozen_files: scope.files.clone(),
        ..Default::default()
    }));
    let local_keep = HashSet::from(["local.txt".to_owned()]);
    let ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        profile: ActionProfileKind::CustomAction.profile(),
        emit: Arc::new(|_, _, _, _| {}),
        raw_emit: Arc::new(|_| {}),
        run_state: Arc::clone(&run_state),
        grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
        skills: Arc::new(RuntimeSkillSnapshot::load_from_data_dir(&storage)),
        root: root.clone(),
        storage_dir: storage.clone(),
        bundled_node_path: storage.join("picket-node"),
        remotes: Vec::new(),
        frozen_selections: selections.clone(),
        live_changes: tree.files.clone(),
        local_keep_paths: local_keep,
        target: target.clone(),
        request_id: "run-multigroup-1".into(),
        run_id: "run-multigroup-1".into(),
        action_id: "act-split".into(),
        action_label: "Smart Split".into(),
        expected_hash: "hash-split".into(),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: ActionApprovalMode::Auto,
    };

    // 1. read_scope_diff works for selected files and refuses unselected files:
    let diff_ok = handle_read_scope_diff(
        &ctx,
        &json!({ "path": "src/a.ts", "startLine": 1, "endLine": 50 }),
        "Reading src/a.ts".into(),
    )
    .await;
    assert!(
        diff_ok.ok,
        "read_scope_diff should succeed: {}",
        diff_ok.content
    );
    assert!(diff_ok.content.contains("export const a = 2;"));

    let diff_blocked = handle_read_scope_diff(
        &ctx,
        &json!({ "path": "src/unselected.ts" }),
        "Reading unselected".into(),
    )
    .await;
    assert!(
        !diff_blocked.ok,
        "read_scope_diff must reject unselected files"
    );

    // 2. Create an action-authored file to mix with selected files across multi-group commit:
    let created = handle_write_repo_file(
        &ctx,
        "w-notes",
        &json!({ "path": "notes.md", "content": "# Notes\n" }),
    )
    .await;
    assert!(created.ok, "write_repo_file failed: {}", created.content);

    // 3. Multi-group commit_exact_scope under CustomAction:
    //    - Group 1 commits `src/a.ts` + explicitly selected `.env.local` WITHOUT triggering any
    //      sensitive-file confirmation popup (`SensitiveFileGuardPolicy::None`).
    //    - Group 2 commits `src/b.ts` + action-created `notes.md` (re-resolving commitToken after HEAD moves).
    let commit_res = handle_commit_exact_scope(
        &ctx,
        "cm-multi",
        &json!({
            "groups": [
                {
                    "fileIds": ["@1", "@3"],
                    "message": "feat(a): update a and explicitly selected local env\n\n- update src/a.ts and .env.local"
                },
                {
                    "fileIds": ["@2"],
                    "editedPaths": ["notes.md"],
                    "message": "feat(b): update b with release notes\n\n- update src/b.ts and add notes.md"
                }
            ],
            "push": false
        }),
    )
    .await;
    assert!(
        commit_res.ok,
        "multi-group custom action commit failed: {}",
        commit_res.content
    );
    let parsed: serde_json::Value = serde_json::from_str(&commit_res.content).unwrap();
    assert_eq!(parsed["committed"], json!(true));
    assert_eq!(parsed["commitCount"], json!(2));

    let head_files: HashSet<String> = git(&root, &["show", "--name-only", "--format=", "HEAD"])
        .lines()
        .map(str::to_owned)
        .collect();
    let prev_files: HashSet<String> = git(&root, &["show", "--name-only", "--format=", "HEAD~1"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        prev_files,
        HashSet::from(["src/a.ts".to_owned(), ".env.local".to_owned()])
    );
    assert_eq!(
        head_files,
        HashSet::from(["src/b.ts".to_owned(), "notes.md".to_owned()])
    );
    // Unselected draft and Local Keep stay completely untouched:
    assert_eq!(
        fs::read_to_string(root.join("src/unselected.ts")).unwrap(),
        "export const u = 2;\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("local.txt")).unwrap(),
        "keep=2\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_action_agent_first_recovery_handoff_and_zero_token_retry_push() {
    let temp = tempfile::tempdir().unwrap();
    let bare = temp.path().join("remote.git");
    let root = temp.path().join("repo");
    let storage = temp.path().join("storage");
    fs::create_dir_all(&bare).unwrap();
    fs::create_dir_all(&storage).unwrap();

    git(&bare, &["init", "--bare", "-b", "main"]);
    git(
        temp.path(),
        &["clone", bare.to_str().unwrap(), root.to_str().unwrap()],
    );
    git(&root, &["config", "user.name", "Picket Test"]);
    git(&root, &["config", "user.email", "test@picket.local"]);
    fs::write(root.join("app.ts"), "export const v = 1;\n").unwrap();
    git(&root, &["add", "app.ts"]);
    git(&root, &["commit", "-m", "chore: initial commit"]);
    git(&root, &["push", "-u", "origin", "main"]);

    // Block pushes on the bare remote to simulate a remote hook / network rejection:
    let hook = bare.join("hooks/pre-receive");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'remote rejected for test' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    fs::write(root.join("app.ts"), "export const v = 2;\n").unwrap();
    let store = RepositoryStore::default();
    let snap = store.open(&root).await.unwrap();
    let tree = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "ws-recovery-handoff".into(),
        scope_version: tree.status_version.clone(),
    };
    let app_change = tree.files.iter().find(|f| f.path == "app.ts").unwrap();
    let selections = vec![CommitSelection {
        file_id: app_change.id.clone(),
        commit_token: app_change.commit_token.clone(),
    }];
    let scope = store
        .extract_cpush_scope(&target, &selections, LanguageKind::En)
        .await
        .unwrap();

    let coordinator = CustomActionCoordinator::default();
    let run_state = Arc::new(Mutex::new(ActionRunState {
        branch: scope.branch.clone(),
        initial_head: scope.initial_head.clone(),
        frozen_files: scope.files.clone(),
        ..Default::default()
    }));
    let ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        profile: ActionProfileKind::CustomAction.profile(),
        emit: Arc::new(|_, _, _, _| {}),
        raw_emit: Arc::new(|_| {}),
        run_state: Arc::clone(&run_state),
        grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
        skills: Arc::new(RuntimeSkillSnapshot::load_from_data_dir(&storage)),
        root: root.clone(),
        storage_dir: storage.clone(),
        bundled_node_path: storage.join("picket-node"),
        remotes: Vec::new(),
        frozen_selections: selections,
        live_changes: tree.files.clone(),
        local_keep_paths: HashSet::new(),
        target: target.clone(),
        request_id: "run-agent-first-1".into(),
        run_id: "run-agent-first-1".into(),
        action_id: "act-push".into(),
        action_label: "Custom Push".into(),
        expected_hash: "hash-push".into(),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: ActionApprovalMode::Auto,
    };

    // 1. commit_exact_scope(push: true) under AgentFirst commits locally and returns a verified recoveryHandle without auto-stopping:
    let commit_push = handle_commit_exact_scope(
        &ctx,
        "cm-push",
        &json!({
            "fileIds": ["@1"],
            "message": "feat(app): bump v to 2\n\n- test agent-first handoff",
            "push": true
        }),
    )
    .await;
    assert!(
        commit_push.ok,
        "commit_exact_scope should return structured result: {}",
        commit_push.content
    );
    assert!(
        commit_push.details.is_none(),
        "AgentFirst must NOT set stopAgent before Agent decides"
    );
    let commit_val: serde_json::Value = serde_json::from_str(&commit_push.content).unwrap();
    assert_eq!(commit_val["committed"], json!(true));
    assert_eq!(commit_val["pushed"], json!(false));
    let handle_id = commit_val["recoveryHandle"]["handleId"]
        .as_str()
        .expect("recoveryHandle.handleId must be present")
        .to_owned();
    assert_eq!(commit_val["recoveryHandle"]["kind"], json!("pushFailed"));

    // 2. sync_and_push also issues a fresh host-verified recoveryHandle on failure:
    let sync_fail = handle_sync_and_push(&ctx, &json!({ "mode": "push" })).await;
    assert!(!sync_fail.ok);
    let sync_val: serde_json::Value = serde_json::from_str(&sync_fail.content).unwrap();
    assert_eq!(sync_val["recoveryHandle"]["kind"], json!("pushFailed"));

    // 3. Anti-hallucination: fabricated handleId is strictly rejected:
    let fake_handoff = handle_handoff_to_recovery(&ctx, &json!({ "handleId": "rh-999" })).await;
    assert!(!fake_handoff.ok);
    assert!(
        fake_handoff
            .content
            .contains("Invalid or expired recovery handle")
    );

    // 4. Genuine handleId transitions session to Host Recovery (PushFailed) and terminates the Agent loop cleanly:
    let handoff = handle_handoff_to_recovery(
        &ctx,
        &json!({
            "handleId": handle_id,
            "summary": "Committed locally; handing push failure to host recovery."
        }),
    )
    .await;
    assert!(
        handoff.ok,
        "handoff_to_recovery failed: {}",
        handoff.content
    );
    assert_eq!(
        handoff.details.as_ref().and_then(|d| d.get("stopAgent")),
        Some(&json!(true))
    );
    let (settled_status, handoff_msg_id) = {
        let st = run_state.lock().unwrap();
        (
            st.settled_reply.as_ref().map(|r| r.session.status),
            st.settled_reply
                .as_ref()
                .and_then(|r| r.session.message.as_ref())
                .map(|m| m.id.clone()),
        )
    };
    assert_eq!(settled_status, Some(ActionSessionStatus::PushFailed));
    assert!(handoff_msg_id.is_some());

    // 5. Remove the remote rejection and execute zero-token Host Recovery (`RetryPush`):
    fs::remove_file(&hook).unwrap();
    let models = ModelStore::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();
    let recovered = coordinator
        .run_session_recovery(
            &storage,
            &paths,
            &supervisor,
            &models,
            &store,
            &SessionRecoveryRequest {
                request_id: "recover-retry-push-1".into(),
                target: target.clone(),
                run_id: Some("run-agent-first-1".into()),
                message_id: handoff_msg_id,
                action: SessionRecoveryAction::RetryPush {
                    set_upstream_remote: None,
                },
                model_id: None,
                locale: Some(LanguageKind::En),
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(recovered.session.status, ActionSessionStatus::Done);
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/main"]),
        git(&root, &["rev-parse", "HEAD"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_action_final_turn_dangling_conflict_safety_net_surfaces_conflict_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let bare = temp.path().join("remote.git");
    let clone_a = temp.path().join("clone_a");
    let clone_b = temp.path().join("clone_b");
    let storage = temp.path().join("storage");
    fs::create_dir_all(&bare).unwrap();
    fs::create_dir_all(&storage).unwrap();

    git(&bare, &["init", "--bare", "-b", "main"]);
    git(
        temp.path(),
        &["clone", bare.to_str().unwrap(), clone_a.to_str().unwrap()],
    );
    git(&clone_a, &["config", "user.name", "Picket A"]);
    git(&clone_a, &["config", "user.email", "a@picket.local"]);
    fs::write(clone_a.join("shared.txt"), "line=base\n").unwrap();
    git(&clone_a, &["add", "shared.txt"]);
    git(&clone_a, &["commit", "-m", "chore: base"]);
    git(&clone_a, &["push", "-u", "origin", "main"]);

    git(
        temp.path(),
        &["clone", bare.to_str().unwrap(), clone_b.to_str().unwrap()],
    );
    git(&clone_b, &["config", "user.name", "Picket B"]);
    git(&clone_b, &["config", "user.email", "b@picket.local"]);
    fs::write(clone_b.join("shared.txt"), "line=remote\n").unwrap();
    git(&clone_b, &["add", "shared.txt"]);
    git(&clone_b, &["commit", "-m", "feat: remote edit"]);
    git(&clone_b, &["push", "origin", "main"]);

    fs::write(clone_a.join("shared.txt"), "line=local\n").unwrap();
    git(&clone_a, &["add", "shared.txt"]);
    git(&clone_a, &["commit", "-m", "feat: local edit"]);

    let store = RepositoryStore::default();
    let snap = store.open(&clone_a).await.unwrap();
    let wt = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: wt.repository_id.clone(),
        worktree_id: wt.worktree_id.clone(),
        session_id: "ws-conflict-safety".into(),
        scope_version: wt.status_version.clone(),
    };

    // Trigger a genuine rebase conflict in clone_a:
    match store
        .sync_rebase_onto_upstream_with_locale(&target, LanguageKind::En)
        .await
        .unwrap()
    {
        RebaseSyncOutcome::Conflict { .. } => {}
        other => panic!("Expected rebase conflict, got {other:?}"),
    }

    let coordinator = CustomActionCoordinator::default();
    let run_state = Mutex::new(ActionRunState {
        branch: "main".into(),
        model_id: "model-1".into(),
        wrote_repo: true,
        ..Default::default()
    });
    let build_stored = || {
        let meta = super::context::SessionPersistMeta {
            run_id: "run-dangling-conflict",
            repository_id: &target.repository_id,
            worktree_id: &target.worktree_id,
            session_id: &target.session_id,
            action_id: "act-sync",
            action_label: "Custom Sync",
            profile: ActionProfileKind::CustomAction,
            action_prompt_hash: "hash-sync",
            frozen_scope_version: &target.scope_version,
            locale: LanguageKind::En,
            commit_language: LanguageKind::En,
        };
        run_state.lock().ok().map(|st| st.to_stored_session(&meta))
    };
    let raw_emit: super::context::RawEventEmitFn = Arc::new(|_| {});

    let reply = coordinator
        .maybe_surface_dangling_conflict_recovery(
            &storage,
            &store,
            "run-dangling-conflict",
            &target,
            &run_state,
            &build_stored,
            &raw_emit,
            LanguageKind::En,
        )
        .await
        .expect("safety net must surface Conflict recovery when unresolved conflicts remain");
    assert_eq!(reply.session.status, ActionSessionStatus::Conflict);
    let msg = reply.session.message.expect("conflict message must exist");
    assert_eq!(msg.kind, "conflict");
    let option_ids: Vec<&str> = msg
        .options
        .as_ref()
        .unwrap()
        .iter()
        .map(|o| o.id.as_str())
        .collect();
    assert!(option_ids.contains(&"agent-merge"));
    assert!(option_ids.contains(&"manual-merge"));
    assert!(option_ids.contains(&"abort-rebase"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_approval_modes_enforce_ask_auto_allow_all_and_preserve_redlines() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let storage = temp.path().join("storage");
    let mock_bin = temp.path().join("bin");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&storage).unwrap();
    fs::create_dir_all(&mock_bin).unwrap();

    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Picket Test"]);
    git(&root, &["config", "user.email", "test@picket.local"]);
    fs::write(root.join("app.txt"), "version=1\n").unwrap();
    fs::write(root.join("draft.txt"), "draft=1\n").unwrap();
    fs::write(root.join("keep.txt"), "keep=1\n").unwrap();
    fs::write(root.join(".env"), "SECRET=1\n").unwrap();
    git(&root, &["add", "app.txt", "draft.txt", "keep.txt"]);
    git(&root, &["commit", "-m", "chore: init"]);

    // Pre-existing unselected draft & Local Keep file:
    fs::write(root.join("draft.txt"), "draft=2\n").unwrap();
    fs::write(root.join("keep.txt"), "keep=2\n").unwrap();

    let store = RepositoryStore::default();
    let snap = store.open(&root).await.unwrap();
    let tree = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "ws-approval-modes".into(),
        scope_version: tree.status_version.clone(),
    };
    let coordinator = CustomActionCoordinator::default();
    let run_state = Arc::new(Mutex::new(ActionRunState::default()));
    let mut ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        profile: ActionProfileKind::CustomAction.profile(),
        emit: Arc::new(|_, _, _, _| {}),
        raw_emit: Arc::new(|_| {}),
        run_state: Arc::clone(&run_state),
        grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
        skills: Arc::new(RuntimeSkillSnapshot::load_from_data_dir(&storage)),
        root: root.clone(),
        storage_dir: storage.clone(),
        bundled_node_path: mock_bin.join("picket-node"),
        remotes: Vec::new(),
        frozen_selections: Vec::new(),
        live_changes: tree.files.clone(),
        local_keep_paths: HashSet::from(["keep.txt".to_owned()]),
        target: target.clone(),
        request_id: "run-modes-1".into(),
        run_id: "run-modes-1".into(),
        action_id: "act-modes".into(),
        action_label: "Modes Test".into(),
        expected_hash: "hash-modes".into(),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: ActionApprovalMode::Ask,
    };

    // 1. Ask mode prompts even for AllowTier1 routine git tag creation:
    let decline_tag = respond_tier2(
        coordinator.clone(),
        "run-modes-1",
        "confirm-g-ask-tag",
        false,
        false,
    );
    let declined_tag = super::tools_cli::handle_run_git(
        &ctx,
        "g-ask-tag",
        &json!({ "args": ["tag", "v0.1.0"] }),
        "Tagging v0.1.0".into(),
    )
    .await;
    decline_tag.await.unwrap();
    assert!(!declined_tag.ok);

    // 2. Auto mode auto-allows AllowTier1 & routine edits, while STILL prompting HighRiskWrite (`tag -f`):
    ctx.approval_mode = ActionApprovalMode::Auto;
    let auto_edit = handle_edit_repo_file(
        &ctx,
        "e-auto",
        &json!({ "path": "app.txt", "oldText": "version=1", "newText": "version=2" }),
    )
    .await;
    assert!(
        auto_edit.ok,
        "Auto mode should allow routine edit_repo_file: {}",
        auto_edit.content
    );

    let auto_tag = super::tools_cli::handle_run_git(
        &ctx,
        "g-auto-tag",
        &json!({ "args": ["tag", "v0.1.0"] }),
        "Tagging v0.1.0".into(),
    )
    .await;
    assert!(
        auto_tag.ok,
        "Auto mode should allow AllowTier1 git tag: {}",
        auto_tag.content
    );

    let decline_high_risk = respond_tier2(
        coordinator.clone(),
        "run-modes-1",
        "confirm-g-auto-force-tag",
        false,
        false,
    );
    let auto_high_risk = super::tools_cli::handle_run_git(
        &ctx,
        "g-auto-force-tag",
        &json!({ "args": ["tag", "-f", "v0.1.0"] }),
        "Force tagging v0.1.0".into(),
    )
    .await;
    decline_high_risk.await.unwrap();
    assert!(
        !auto_high_risk.ok,
        "Auto mode must prompt on HighRiskWrite (`git tag -f`)"
    );

    // 3. AllowAll auto-allows HighRiskWrite (`git tag -f`) while preserving ALL hard redlines:
    ctx.approval_mode = ActionApprovalMode::AllowAll;
    let allow_all_high_risk = super::tools_cli::handle_run_git(
        &ctx,
        "g-allow-all-force-tag",
        &json!({ "args": ["tag", "-f", "v0.1.0"] }),
        "Force tagging v0.1.0".into(),
    )
    .await;
    assert!(
        allow_all_high_risk.ok,
        "AllowAll should auto-allow non-redline HighRiskWrite: {}",
        allow_all_high_risk.content
    );

    // Hard redlines remain unconditionally blocked under AllowAll:
    let env_blocked = handle_edit_repo_file(
        &ctx,
        "e-env-allowall",
        &json!({ "path": ".env", "oldText": "SECRET=1", "newText": "SECRET=2" }),
    )
    .await;
    assert!(!env_blocked.ok && env_blocked.content.contains("sensitive"));

    let keep_blocked = handle_edit_repo_file(
        &ctx,
        "e-keep-allowall",
        &json!({ "path": "keep.txt", "oldText": "keep=2", "newText": "keep=3" }),
    )
    .await;
    assert!(!keep_blocked.ok && keep_blocked.content.contains("Local Keep"));

    let draft_blocked = handle_edit_repo_file(
        &ctx,
        "e-draft-allowall",
        &json!({ "path": "draft.txt", "oldText": "draft=2", "newText": "draft=3" }),
    )
    .await;
    assert!(!draft_blocked.ok && draft_blocked.content.contains("unselected"));

    let rm_blocked = handle_run_repo_command(
        &ctx,
        "cmd-rm-allowall",
        &json!({ "command": "rm", "args": ["-rf", "app.txt"] }),
        "Running rm".into(),
    )
    .await;
    assert!(!rm_blocked.ok && rm_blocked.content.contains("hard redline"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_repo_file_rejects_staged_files_and_multigroup_commit_detects_external_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let storage = temp.path().join("storage");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&storage).unwrap();

    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Picket Test"]);
    git(&root, &["config", "user.email", "test@picket.local"]);
    fs::write(root.join("staged.txt"), "v1\n").unwrap();
    fs::write(root.join("a.txt"), "a1\n").unwrap();
    fs::write(root.join("b.txt"), "b1\n").unwrap();
    git(&root, &["add", "staged.txt", "a.txt", "b.txt"]);
    git(&root, &["commit", "-m", "chore: init"]);

    // Stage staged.txt (range.kind == "staged") and modify a.txt / b.txt unstaged:
    fs::write(root.join("staged.txt"), "v2-staged\n").unwrap();
    git(&root, &["add", "staged.txt"]);
    fs::write(root.join("a.txt"), "a2\n").unwrap();
    fs::write(root.join("b.txt"), "b2\n").unwrap();

    let store = RepositoryStore::default();
    let snap = store.open(&root).await.unwrap();
    let tree = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "ws-staged-and-mutate".into(),
        scope_version: tree.status_version.clone(),
    };
    let mut selections = Vec::new();
    for path in ["staged.txt", "a.txt", "b.txt"] {
        let change = tree.files.iter().find(|f| f.path == path).unwrap();
        selections.push(CommitSelection {
            file_id: change.id.clone(),
            commit_token: change.commit_token.clone(),
        });
    }
    let scope = store
        .extract_cpush_scope(&target, &selections, LanguageKind::En)
        .await
        .unwrap();

    let coordinator = CustomActionCoordinator::default();
    let run_state = Arc::new(Mutex::new(ActionRunState {
        branch: scope.branch.clone(),
        initial_head: scope.initial_head.clone(),
        frozen_files: scope.files.clone(),
        ..Default::default()
    }));
    let ctx = ActionToolContext {
        coordinator: coordinator.clone(),
        store: store.clone(),
        profile: ActionProfileKind::CustomAction.profile(),
        emit: Arc::new(|_, _, _, _| {}),
        raw_emit: Arc::new(|_| {}),
        run_state: Arc::clone(&run_state),
        grants: Arc::new(Mutex::new(SessionAuthGrants::default())),
        skills: Arc::new(RuntimeSkillSnapshot::load_from_data_dir(&storage)),
        root: root.clone(),
        storage_dir: storage.clone(),
        bundled_node_path: storage.join("picket-node"),
        remotes: Vec::new(),
        frozen_selections: selections,
        live_changes: tree.files.clone(),
        local_keep_paths: HashSet::new(),
        target: target.clone(),
        request_id: "run-staged-1".into(),
        run_id: "run-staged-1".into(),
        action_id: "act-staged".into(),
        action_label: "Staged Check".into(),
        expected_hash: "hash-staged".into(),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: ActionApprovalMode::Auto,
    };

    // 1. edit_repo_file must refuse editing a fully-staged file so index content isn't clobbered:
    let staged_edit = handle_edit_repo_file(
        &ctx,
        "e-staged",
        &json!({ "path": "staged.txt", "oldText": "v2-staged", "newText": "v3" }),
    )
    .await;
    assert!(!staged_edit.ok && staged_edit.content.contains("staged"));

    // 2. Commit group 1 (a.txt), then externally mutate b.txt before group 2 -> must reject stale contentVersion!
    let group1 = handle_commit_exact_scope(
        &ctx,
        "cm-g1",
        &json!({ "fileIds": ["@2"], "message": "feat(a): update a\n\n- commit a" }),
    )
    .await;
    assert!(group1.ok, "group 1 commit failed: {}", group1.content);

    fs::write(root.join("b.txt"), "b2-externally-mutated\n").unwrap();
    let group2 = handle_commit_exact_scope(
        &ctx,
        "cm-g2",
        &json!({ "fileIds": ["@3"], "message": "feat(b): update b\n\n- commit b" }),
    )
    .await;
    assert!(
        !group2.ok && group2.content.contains("changed externally"),
        "expected external mutation rejection, got: ok={}, content={}",
        group2.ok,
        group2.content
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_request_validation_and_readonly_action_conflict_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let bare = temp.path().join("remote.git");
    let clone_a = temp.path().join("clone_a");
    let clone_b = temp.path().join("clone_b");
    let storage = temp.path().join("storage");
    fs::create_dir_all(&bare).unwrap();
    fs::create_dir_all(&storage).unwrap();

    git(&bare, &["init", "--bare", "-b", "main"]);
    git(
        temp.path(),
        &["clone", bare.to_str().unwrap(), clone_a.to_str().unwrap()],
    );
    git(&clone_a, &["config", "user.name", "Picket A"]);
    git(&clone_a, &["config", "user.email", "a@picket.local"]);
    fs::write(clone_a.join("shared.txt"), "base\n").unwrap();
    git(&clone_a, &["add", "shared.txt"]);
    git(&clone_a, &["commit", "-m", "chore: base"]);
    git(&clone_a, &["push", "-u", "origin", "main"]);

    git(
        temp.path(),
        &["clone", bare.to_str().unwrap(), clone_b.to_str().unwrap()],
    );
    git(&clone_b, &["config", "user.name", "Picket B"]);
    git(&clone_b, &["config", "user.email", "b@picket.local"]);
    fs::write(clone_b.join("shared.txt"), "remote\n").unwrap();
    git(&clone_b, &["add", "shared.txt"]);
    git(&clone_b, &["commit", "-m", "feat: remote"]);
    git(&clone_b, &["push", "origin", "main"]);

    fs::write(clone_a.join("shared.txt"), "local\n").unwrap();
    git(&clone_a, &["add", "shared.txt"]);
    git(&clone_a, &["commit", "-m", "feat: local"]);

    let store = RepositoryStore::default();
    let snap = store.open(&clone_a).await.unwrap();
    let wt = &snap.worktrees[0];
    let target = ReadTarget {
        repository_id: wt.repository_id.clone(),
        worktree_id: wt.worktree_id.clone(),
        session_id: "ws-iso".into(),
        scope_version: wt.status_version.clone(),
    };
    let coordinator = CustomActionCoordinator::default();
    let models = ModelStore::default();
    let paths = PiPaths::development();
    let supervisor = PiSupervisor::default();

    // 1. start_action must reject unsupported profiles (`ResolveConflict` / `PullRebase`):
    for bad_profile in [
        ActionProfileKind::ResolveConflict,
        ActionProfileKind::PullRebase,
    ] {
        let err = coordinator
            .start_action(
                &storage,
                &paths,
                &supervisor,
                &models,
                &store,
                &StartActionRequest {
                    request_id: format!("req-bad-{bad_profile:?}"),
                    target: target.clone(),
                    model_id: "m-1".into(),
                    profile: bad_profile,
                    action: None,
                    selections: vec![],
                    extra_prompt: String::new(),
                    push: None,
                    follow_up_prompt: None,
                    resume_from_run_id: None,
                    locale: LanguageKind::En,
                    commit_language: LanguageKind::En,
                    approval_mode: ActionApprovalMode::Auto,
                },
                |_| {},
            )
            .await
            .unwrap_err();
        assert!(
            err.message.contains("Unsupported execution profile"),
            "unexpected error: {err}"
        );
    }

    // 2. Seed an active PushFailed session and verify run_session_recovery rejects empty run_id & missing message_id:
    let mut seeded = ActionRunState {
        branch: "main".into(),
        model_id: "m-1".into(),
        ..Default::default()
    }
    .to_stored_session(&super::context::SessionPersistMeta {
        run_id: "run-recovery-guard",
        repository_id: &target.repository_id,
        worktree_id: &target.worktree_id,
        session_id: &target.session_id,
        action_id: "act-push",
        action_label: "Custom Push",
        profile: ActionProfileKind::CustomAction,
        action_prompt_hash: "hash-push",
        frozen_scope_version: &target.scope_version,
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
    });
    super::session_helpers::enter_push_failed_state(&mut seeded, "origin/main", "rejected", None);
    coordinator.save_session_in_memory(&seeded).unwrap();

    let missing_run_err = coordinator
        .run_session_recovery(
            &storage,
            &paths,
            &supervisor,
            &models,
            &store,
            &SessionRecoveryRequest {
                request_id: "req-no-run".into(),
                target: target.clone(),
                run_id: Some("   ".into()),
                message_id: seeded.message.as_ref().map(|m| m.id.clone()),
                action: SessionRecoveryAction::RetryPush {
                    set_upstream_remote: None,
                },
                model_id: None,
                locale: Some(LanguageKind::En),
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        missing_run_err.code,
        crate::error::AppErrorCode::ActionInvalidInput
    );

    let missing_msg_err = coordinator
        .run_session_recovery(
            &storage,
            &paths,
            &supervisor,
            &models,
            &store,
            &SessionRecoveryRequest {
                request_id: "req-no-msg".into(),
                target: target.clone(),
                run_id: Some("run-recovery-guard".into()),
                message_id: None,
                action: SessionRecoveryAction::RetryPush {
                    set_upstream_remote: None,
                },
                model_id: None,
                locale: Some(LanguageKind::En),
            },
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        missing_msg_err.code,
        crate::error::AppErrorCode::ActionInvalidInput
    );

    // 3. Read-only Custom Action (`wrote_repo = false`, no conflict handle) must NOT be hijacked
    //    into Conflict recovery even if a rebase conflict already existed in the repo:
    match store
        .sync_rebase_onto_upstream_with_locale(&target, LanguageKind::En)
        .await
        .unwrap()
    {
        RebaseSyncOutcome::Conflict { .. } => {}
        other => panic!("Expected rebase conflict, got {other:?}"),
    }
    let readonly_state = Mutex::new(ActionRunState {
        branch: "main".into(),
        model_id: "m-1".into(),
        wrote_repo: false,
        ..Default::default()
    });
    let build_stored = || {
        let meta = super::context::SessionPersistMeta {
            run_id: "run-readonly",
            repository_id: &target.repository_id,
            worktree_id: &target.worktree_id,
            session_id: &target.session_id,
            action_id: "act-review",
            action_label: "Read-only Review",
            profile: ActionProfileKind::CustomAction,
            action_prompt_hash: "hash-review",
            frozen_scope_version: &target.scope_version,
            locale: LanguageKind::En,
            commit_language: LanguageKind::En,
        };
        readonly_state
            .lock()
            .ok()
            .map(|st| st.to_stored_session(&meta))
    };
    let raw_emit: super::context::RawEventEmitFn = Arc::new(|_| {});
    let hijacked = coordinator
        .maybe_surface_dangling_conflict_recovery(
            &storage,
            &store,
            "run-readonly",
            &target,
            &readonly_state,
            &build_stored,
            &raw_emit,
            LanguageKind::En,
        )
        .await;
    assert!(
        hijacked.is_none(),
        "read-only Custom Action must not hijack unrelated pre-existing conflicts"
    );
}
