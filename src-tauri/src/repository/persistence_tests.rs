use super::{persistence::WorkbenchPersistence, smoke::Fixture, *};
use crate::i18n::LanguageKind;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn persists_and_cold_restores_editor_repositories_and_per_worktree_prompts_without_tokens() {
    let fixture = Fixture::new().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let store = RepositoryStore::default();
    let persistence = WorkbenchPersistence::default();

    // Missing file returns clean v2 empty state without warning:
    let initial = persistence.restore(storage.path(), &store).await.unwrap();
    assert_eq!(initial.protocol_version, 2);
    assert_eq!(initial.locale, LanguageKind::En);
    assert_eq!(initial.editor, EditorKind::Idea);
    assert_eq!(initial.default_commit_language, LanguageKind::En);
    assert!(initial.repository_settings.is_empty());
    assert!(initial.snapshots.is_empty());
    assert!(initial.worktrees.is_empty());
    assert!(initial.warning.is_none());

    let opened = store.open(&fixture.root).await.unwrap();
    let main_wt = &opened.worktrees[0];
    let linked_wt = &opened.worktrees[1];

    persistence
        .save(
            storage.path(),
            &store,
            WorkbenchSaveRequest {
                revision: 2,
                locale: LanguageKind::Zh,
                editor: EditorKind::Cursor,
                default_commit_language: LanguageKind::Zh,
                repository_settings: vec![PersistedRepositorySettings {
                    repository_id: opened.repository.id.clone(),
                    editor: EditorKind::Rustrover,
                    commit_language: LanguageKind::En,
                    local_keep_paths: vec![],
                    custom_actions: vec![],
                    active_action_id: None,
                }],
                window_preferences: Some(PersistedWindowPreferences {
                    pinned: true,
                    compact_mode: true,
                    pinned_expanded: false,
                    full_size: Some(WindowSize {
                        width: 420,
                        height: 680,
                    }),
                    full_position: Some(WindowPosition { x: 120, y: 80 }),
                    compact_position: Some(WindowPosition { x: 120, y: 80 }),
                }),
                active_worktree_id: Some(linked_wt.worktree_id.clone()),
                empty_prompt: "empty extra instruction".into(),
                worktrees: vec![
                    PersistedWorktreeState {
                        repository_id: main_wt.repository_id.clone(),
                        worktree_id: main_wt.worktree_id.clone(),
                        prompt_text: "split lockfile separately".into(),
                        selected_ids: vec![main_wt.files[0].id.clone()],
                        unselected_ids: vec![main_wt.files[1].id.clone()],
                        scope_version: "scope-main".into(),
                        tab: Some(WorkbenchTab::Commit),
                    },
                    PersistedWorktreeState {
                        repository_id: linked_wt.repository_id.clone(),
                        worktree_id: linked_wt.worktree_id.clone(),
                        prompt_text: "focus on null safety".into(),
                        selected_ids: vec![],
                        unselected_ids: vec![],
                        scope_version: "scope-linked".into(),
                        tab: Some(WorkbenchTab::Review),
                    },
                ],
            },
        )
        .await
        .unwrap();

    // Older out-of-order revision=1 must be ignored:
    persistence
        .save(
            storage.path(),
            &store,
            WorkbenchSaveRequest {
                revision: 1,
                locale: LanguageKind::En,
                editor: EditorKind::Idea,
                default_commit_language: LanguageKind::En,
                repository_settings: vec![],
                window_preferences: None,
                active_worktree_id: None,
                empty_prompt: "stale".into(),
                worktrees: vec![],
            },
        )
        .await
        .unwrap();

    let saved_file = storage.path().join("workbench-state.json");
    assert_eq!(
        fs::metadata(&saved_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let raw = fs::read_to_string(&saved_file).unwrap();
    assert!(
        !raw.contains("commitToken")
            && !raw.contains("branchToken")
            && !raw.contains("apiKey")
            && !raw.contains("draftText")
    );

    // Cold restore into brand-new instances:
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let restored = cold_persistence
        .restore(storage.path(), &cold_store)
        .await
        .unwrap();
    assert_eq!(restored.protocol_version, 2);
    assert_eq!(restored.locale, LanguageKind::Zh);
    assert_eq!(restored.editor, EditorKind::Cursor);
    assert_eq!(restored.default_commit_language, LanguageKind::Zh);
    assert_eq!(
        restored.repository_settings,
        vec![PersistedRepositorySettings {
            repository_id: opened.repository.id.clone(),
            editor: EditorKind::Rustrover,
            commit_language: LanguageKind::En,
            local_keep_paths: vec![],
            custom_actions: vec![],
            active_action_id: None,
        }]
    );
    assert_eq!(
        restored.window_preferences,
        Some(PersistedWindowPreferences {
            pinned: true,
            compact_mode: true,
            pinned_expanded: false,
            full_size: Some(WindowSize {
                width: 420,
                height: 680,
            }),
            full_position: Some(WindowPosition { x: 120, y: 80 }),
            compact_position: Some(WindowPosition { x: 120, y: 80 }),
        })
    );
    assert_eq!(
        restored.active_worktree_id.as_deref(),
        Some(linked_wt.worktree_id.as_str())
    );
    assert_eq!(restored.empty_prompt, "empty extra instruction");
    assert_eq!(restored.snapshots.len(), 1);
    assert_eq!(restored.snapshots[0].repository.id, opened.repository.id);
    assert_eq!(restored.worktrees.len(), 2);
    assert!(restored.warning.is_none());
}

#[tokio::test]
async fn preserves_unavailable_repository_prompts_upgrades_v1_discarding_legacy_drafts_and_rejects_extra_fields()
 {
    let fixture = Fixture::new().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let store = RepositoryStore::default();
    let persistence = WorkbenchPersistence::default();
    let opened = store.open(&fixture.root).await.unwrap();
    let wt_id = opened.active_worktree_id.clone();

    persistence
        .save(
            storage.path(),
            &store,
            WorkbenchSaveRequest {
                revision: 1,
                locale: LanguageKind::En,
                editor: EditorKind::Webstorm,
                default_commit_language: LanguageKind::Zh,
                repository_settings: vec![PersistedRepositorySettings {
                    repository_id: opened.repository.id.clone(),
                    editor: EditorKind::Webstorm,
                    commit_language: LanguageKind::Zh,
                    local_keep_paths: vec![],
                    custom_actions: vec![],
                    active_action_id: None,
                }],
                window_preferences: None,
                active_worktree_id: Some(wt_id.clone()),
                empty_prompt: String::new(),
                worktrees: vec![PersistedWorktreeState {
                    repository_id: opened.repository.id.clone(),
                    worktree_id: wt_id.clone(),
                    prompt_text: "keep instruction even when repo unmounted".into(),
                    selected_ids: vec![],
                    unselected_ids: vec![],
                    scope_version: "v1".into(),
                    tab: None,
                }],
            },
        )
        .await
        .unwrap();

    // Remove the repository directory and cold restore:
    let saved_repo_id = opened.repository.id.clone();
    drop(fixture);
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let missing_restore = cold_persistence
        .restore(storage.path(), &cold_store)
        .await
        .unwrap();
    assert_eq!(missing_restore.editor, EditorKind::Webstorm);
    assert_eq!(missing_restore.default_commit_language, LanguageKind::Zh);
    assert!(missing_restore.snapshots.is_empty());
    assert!(
        missing_restore
            .warning
            .as_deref()
            .unwrap()
            .contains("inaccessible")
    );
    assert_eq!(missing_restore.worktrees.len(), 1);
    assert_eq!(
        missing_restore.worktrees[0].prompt_text,
        "keep instruction even when repo unmounted"
    );

    // Saving in a session where that repository wasn't opened retains its saved prompt and per-repo settings:
    cold_persistence
        .save(
            storage.path(),
            &cold_store,
            WorkbenchSaveRequest {
                revision: 1,
                locale: LanguageKind::Zh,
                editor: EditorKind::Vscode,
                default_commit_language: LanguageKind::En,
                repository_settings: vec![],
                window_preferences: None,
                active_worktree_id: None,
                empty_prompt: "new empty instruction".into(),
                worktrees: vec![],
            },
        )
        .await
        .unwrap();
    let second_restore = WorkbenchPersistence::default()
        .restore(storage.path(), &RepositoryStore::default())
        .await
        .unwrap();
    assert_eq!(second_restore.locale, LanguageKind::Zh);
    assert_eq!(second_restore.editor, EditorKind::Vscode);
    assert_eq!(second_restore.default_commit_language, LanguageKind::En);
    assert_eq!(second_restore.repository_settings.len(), 1);
    assert_eq!(
        second_restore.repository_settings[0].repository_id,
        saved_repo_id
    );
    assert!(
        second_restore
            .warning
            .as_deref()
            .unwrap()
            .contains("\u{4e0d}\u{53ef}\u{8bbf}\u{95ee}")
    );
    assert_eq!(second_restore.worktrees.len(), 1);
    assert_eq!(
        second_restore.worktrees[0].prompt_text,
        "keep instruction even when repo unmounted"
    );

    // Duplicate or empty repository_id in repository_settings is rejected:
    assert!(
        WorkbenchPersistence::default()
            .save(
                storage.path(),
                &RepositoryStore::default(),
                WorkbenchSaveRequest {
                    revision: 2,
                    locale: LanguageKind::En,
                    editor: EditorKind::Idea,
                    default_commit_language: LanguageKind::En,
                    repository_settings: vec![
                        PersistedRepositorySettings {
                            repository_id: "dup".into(),
                            editor: EditorKind::Idea,
                            commit_language: LanguageKind::En,
                            local_keep_paths: vec![],
                            custom_actions: vec![],
                            active_action_id: None,
                        },
                        PersistedRepositorySettings {
                            repository_id: "dup".into(),
                            editor: EditorKind::Vscode,
                            commit_language: LanguageKind::Zh,
                            local_keep_paths: vec![],
                            custom_actions: vec![],
                            active_action_id: None,
                        },
                    ],
                    window_preferences: None,
                    active_worktree_id: None,
                    empty_prompt: String::new(),
                    worktrees: vec![],
                },
            )
            .await
            .is_err()
    );

    // Out-of-bounds window dimensions or coordinates are rejected:
    assert!(
        WorkbenchPersistence::default()
            .save(
                storage.path(),
                &RepositoryStore::default(),
                WorkbenchSaveRequest {
                    revision: 3,
                    locale: LanguageKind::En,
                    editor: EditorKind::Idea,
                    default_commit_language: LanguageKind::En,
                    repository_settings: vec![],
                    window_preferences: Some(PersistedWindowPreferences {
                        pinned: true,
                        compact_mode: false,
                        pinned_expanded: true,
                        full_size: Some(WindowSize {
                            width: 399,
                            height: 479,
                        }),
                        full_position: None,
                        compact_position: None,
                    }),
                    active_worktree_id: None,
                    empty_prompt: String::new(),
                    worktrees: vec![],
                },
            )
            .await
            .is_err()
    );

    // Corrupted JSON file falls back safely with warning and does not overwrite the file on restore:
    let saved_path = storage.path().join("workbench-state.json");
    fs::write(&saved_path, b"{broken json").unwrap();
    let corrupted_restore = WorkbenchPersistence::default()
        .restore(storage.path(), &RepositoryStore::default())
        .await
        .unwrap();
    assert_eq!(corrupted_restore.locale, LanguageKind::En);
    assert_eq!(corrupted_restore.editor, EditorKind::Idea);
    assert_eq!(corrupted_restore.default_commit_language, LanguageKind::En);
    assert!(corrupted_restore.warning.is_some());
    assert_eq!(fs::read(&saved_path).unwrap(), b"{broken json");

    // Upgrade migration from v1 workbench-state.json:
    // Silently discards legacy `emptyDraft` and per-worktree `draftText` with zero warning,
    // while preserving opened repositories, Local Keep, repository settings, selections, and active tab!
    let v1_fixture = Fixture::new().unwrap();
    let v1_store_seed = RepositoryStore::default();
    let v1_opened = v1_store_seed.open(&v1_fixture.root).await.unwrap();
    let v1_common_dir = fs::canonicalize(v1_fixture.root.join(".git"))
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let v1_wt = &v1_opened.worktrees[0];
    fs::write(
        &saved_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "locale": "zh",
            "editor": "rustrover",
            "defaultCommitLanguage": "zh",
            "repositorySettings": [{
                "repositoryId": v1_opened.repository.id,
                "editor": "cursor",
                "commitLanguage": "zh"
            }],
            "activeWorktreeId": v1_wt.worktree_id,
            "emptyDraft": "legacy v1 commit draft that must be cleared",
            "emptyEditing": true,
            "repositories": [{
                "repositoryId": v1_opened.repository.id,
                "rootPath": v1_fixture.root.to_str().unwrap()
            }],
            "localKeep": [{
                "commonDir": v1_common_dir,
                "paths": ["pnpm-lock.yaml"]
            }],
            "worktrees": [{
                "repositoryId": v1_opened.repository.id,
                "worktreeId": v1_wt.worktree_id,
                "draftText": "legacy v1 manual commit message must not become prompt",
                "draftEditing": true,
                "draftOutdated": false,
                "selectedIds": [v1_wt.files[0].id],
                "scopeVersion": "v1-scope",
                "tab": "review"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let v1_restore = WorkbenchPersistence::default()
        .restore(storage.path(), &RepositoryStore::default())
        .await
        .unwrap();
    assert_eq!(v1_restore.protocol_version, 2);
    assert_eq!(v1_restore.locale, LanguageKind::Zh);
    assert_eq!(v1_restore.editor, EditorKind::Rustrover);
    assert_eq!(v1_restore.default_commit_language, LanguageKind::Zh);
    assert_eq!(v1_restore.empty_prompt, "");
    assert_eq!(v1_restore.snapshots.len(), 1);
    assert_eq!(
        v1_restore.snapshots[0].repository.local_keep_paths,
        vec!["pnpm-lock.yaml".to_string()]
    );
    assert_eq!(v1_restore.worktrees.len(), 1);
    assert_eq!(v1_restore.worktrees[0].prompt_text, "");
    assert_eq!(
        v1_restore.worktrees[0].selected_ids,
        vec![v1_wt.files[0].id.clone()]
    );
    assert_eq!(v1_restore.worktrees[0].tab, Some(WorkbenchTab::Review));
    assert!(v1_restore.warning.is_none());

    // Wire format rejects injected paths or tokens:
    assert!(
        serde_json::from_value::<WorkbenchSaveRequest>(serde_json::json!({
            "revision": 1,
            "editor": "idea",
            "activeWorktreeId": null,
            "emptyPrompt": "",
            "worktrees": [],
            "repositories": [{ "rootPath": "/etc" }]
        }))
        .is_err()
    );
}

#[tokio::test]
async fn persists_local_keep_by_canonical_common_dir_across_worktrees_restarts_and_clean_cycles() {
    let fixture = Fixture::new().unwrap();
    let linked_root = fixture.root.parent().unwrap().join("linked worktree");
    let storage = tempfile::tempdir().unwrap();
    let store = RepositoryStore::default();
    let persistence = WorkbenchPersistence::default();

    // Isolate test repo from host-level global gitignore patterns:
    super::smoke::git(&fixture.root, &["config", "core.excludesFile", "/dev/null"]).unwrap();

    // Create an untracked local note in BOTH worktrees of the same repository:
    fs::write(fixture.root.join("dev-notes.txt"), "# main local note\n").unwrap();
    fs::write(linked_root.join("dev-notes.txt"), "# linked local note\n").unwrap();

    let exclude_path = fixture.root.join(".git/info/exclude");
    let orig_exclude = fs::read(&exclude_path).unwrap();
    let orig_index = super::smoke::git(&fixture.root, &["ls-files", "--stage", "-z"]).unwrap();

    let opened = store.open(&fixture.root).await.unwrap();
    let main_tree = opened
        .worktrees
        .iter()
        .find(|wt| wt.worktree_id == opened.active_worktree_id)
        .unwrap();
    let target = ReadTarget {
        repository_id: main_tree.repository_id.clone(),
        worktree_id: main_tree.worktree_id.clone(),
        session_id: "lk-persist".into(),
        scope_version: main_tree.status_version.clone(),
    };

    // Unsafe paths (`../`, `.git/`, leading `/`, empty) must be rejected:
    for bad in ["../escape.env", ".git/config", "/abs/file.ts", ""] {
        assert!(
            store
                .set_local_keep(&target, &[bad.into()], true, LanguageKind::En)
                .await
                .is_err(),
            "must reject unsafe path {bad:?}"
        );
    }

    // Mark both tracked (`part.txt`) and untracked (`dev-notes.txt`) paths as Local Keep:
    let kept_paths = vec!["dev-notes.txt".to_string(), "part.txt".to_string()];
    let updated = store
        .set_local_keep(&target, &kept_paths, true, LanguageKind::En)
        .await
        .unwrap();
    persistence
        .sync_local_keep(
            storage.path(),
            &store,
            &target.repository_id,
            &kept_paths,
            LanguageKind::En,
        )
        .await
        .unwrap();

    assert_eq!(
        updated.repository.local_keep_paths,
        vec!["dev-notes.txt".to_string(), "part.txt".to_string()]
    );
    // Both worktrees of the same repository share Local Keep immediately:
    assert_eq!(updated.worktrees.len(), 2);
    for wt in &updated.worktrees {
        let note = wt
            .files
            .iter()
            .find(|f| f.path == "dev-notes.txt")
            .unwrap_or_else(|| panic!("worktree {} missing dev-notes.txt", wt.label));
        assert!(
            note.local_keep,
            "worktree {} must share Local Keep",
            wt.label
        );
    }

    // Verify zero Git side effects (index, .git/info/exclude, .gitignore, skip-worktree / assume-unchanged):
    assert_eq!(fs::read(&exclude_path).unwrap(), orig_exclude);
    assert!(!fixture.root.join(".gitignore").exists());
    assert_eq!(
        super::smoke::git(&fixture.root, &["ls-files", "--stage", "-z"]).unwrap(),
        orig_index
    );
    let flags = super::smoke::git(&fixture.root, &["ls-files", "-v", "-z"]).unwrap();
    assert!(
        flags
            .split(|b| *b == 0)
            .all(|entry| entry.is_empty() || entry[0] == b'H'),
        "Local Keep must never set skip-worktree or assume-unchanged flags"
    );

    // Verify workbench-state.json keys `localKeep` strictly by canonical Git common dir (never worktree paths):
    let saved_json: serde_json::Value =
        serde_json::from_slice(&fs::read(storage.path().join("workbench-state.json")).unwrap())
            .unwrap();
    let expected_common_dir = fs::canonicalize(fixture.root.join(".git"))
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let stored_lk = saved_json["localKeep"].as_array().unwrap();
    assert_eq!(stored_lk.len(), 1);
    assert_eq!(
        stored_lk[0]["commonDir"].as_str(),
        Some(expected_common_dir.as_str())
    );
    assert_ne!(
        stored_lk[0]["commonDir"].as_str(),
        Some(fs::canonicalize(&fixture.root).unwrap().to_str().unwrap())
    );
    assert_ne!(
        stored_lk[0]["commonDir"].as_str(),
        Some(fs::canonicalize(&linked_root).unwrap().to_str().unwrap())
    );

    // Cold restart into brand-new store + persistence instances restores shared Local Keep across both worktrees:
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let restored = cold_persistence
        .restore(storage.path(), &cold_store)
        .await
        .unwrap();
    assert_eq!(restored.snapshots.len(), 1);
    let cold_snap = &restored.snapshots[0];
    assert_eq!(
        cold_snap.repository.local_keep_paths,
        vec!["dev-notes.txt".to_string(), "part.txt".to_string()]
    );
    for wt in &cold_snap.worktrees {
        let note = wt
            .files
            .iter()
            .find(|f| f.path == "dev-notes.txt")
            .unwrap_or_else(|| {
                panic!(
                    "missing notes.local.md in worktree {} (read_error={:?}, files={:?})",
                    wt.label,
                    wt.read_error,
                    wt.files.iter().map(|f| &f.path).collect::<Vec<_>>()
                )
            });
        assert!(note.local_keep);
    }

    // Remove `notes.local.md` so it has no Git changes -> hidden from `files`, retained in `local_keep_paths`;
    // recreate later -> automatically re-enters `local_keep == true`:
    fs::remove_file(fixture.root.join("dev-notes.txt")).unwrap();
    let clean_snap = cold_store.refresh(&target).await.unwrap();
    let clean_main = clean_snap
        .worktrees
        .iter()
        .find(|wt| wt.worktree_id == target.worktree_id)
        .unwrap();
    assert!(!clean_main.files.iter().any(|f| f.path == "dev-notes.txt"));
    assert!(
        clean_snap
            .repository
            .local_keep_paths
            .contains(&"dev-notes.txt".to_string())
    );

    fs::write(fixture.root.join("dev-notes.txt"), "# edited again later\n").unwrap();
    let reappeared_snap = cold_store.refresh(&target).await.unwrap();
    let reappeared_main = reappeared_snap
        .worktrees
        .iter()
        .find(|wt| wt.worktree_id == target.worktree_id)
        .unwrap();
    assert!(
        reappeared_main
            .files
            .iter()
            .find(|f| f.path == "dev-notes.txt")
            .unwrap()
            .local_keep
    );

    // Rename (`git mv pnpm-lock.yaml moved.lock` when `pnpm-lock.yaml` was kept): judged by new path without migrating old path:
    cold_store
        .set_local_keep(&target, &["pnpm-lock.yaml".into()], true, LanguageKind::En)
        .await
        .unwrap();
    super::smoke::git(&fixture.root, &["mv", "--", "pnpm-lock.yaml", "moved.lock"]).unwrap();
    let renamed_snap = cold_store.refresh(&target).await.unwrap();
    let renamed_main = renamed_snap
        .worktrees
        .iter()
        .find(|wt| wt.worktree_id == target.worktree_id)
        .unwrap();
    let moved = renamed_main
        .files
        .iter()
        .find(|f| f.path == "moved.lock")
        .unwrap();
    assert!(
        !moved.local_keep,
        "rename v1 must evaluate by new path without migrating old path"
    );
}

#[tokio::test]
async fn persists_custom_actions_cold_restores_and_invalidates_stale_write_trust() {
    let fixture = Fixture::new().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let store = RepositoryStore::default();
    let persistence = WorkbenchPersistence::default();
    let opened = store.open(&fixture.root).await.unwrap();

    let trusted_prompt = "git tag v0.2.0 && git push origin v0.2.0";
    let valid_hash = persistence::compute_custom_action_prompt_hash(trusted_prompt);

    persistence
        .save(
            storage.path(),
            &store,
            WorkbenchSaveRequest {
                revision: 1,
                locale: LanguageKind::En,
                editor: EditorKind::Idea,
                default_commit_language: LanguageKind::En,
                repository_settings: vec![PersistedRepositorySettings {
                    repository_id: opened.repository.id.clone(),
                    editor: EditorKind::Idea,
                    commit_language: LanguageKind::En,
                    local_keep_paths: vec![],
                    custom_actions: vec![
                        PersistedCustomAction {
                            id: "act-trusted".into(),
                            template_key: Some("tpl-tag".into()),
                            label: "Release Tag".into(),
                            prompt: trusted_prompt.into(),
                            skip_write_confirm: Some(true),
                            trusted_prompt_hash: Some(valid_hash.clone()),
                        },
                        PersistedCustomAction {
                            id: "act-edited".into(),
                            template_key: None,
                            label: "Edited Tag".into(),
                            prompt: "git tag v0.3.0 && git push origin v0.3.0".into(),
                            skip_write_confirm: Some(true),
                            trusted_prompt_hash: Some(valid_hash),
                        },
                    ],
                    active_action_id: Some("act-trusted".into()),
                }],
                window_preferences: None,
                active_worktree_id: Some(opened.active_worktree_id.clone()),
                empty_prompt: String::new(),
                worktrees: vec![],
            },
        )
        .await
        .unwrap();

    let restored = WorkbenchPersistence::default()
        .restore(storage.path(), &RepositoryStore::default())
        .await
        .unwrap();
    assert_eq!(restored.repository_settings.len(), 1);
    let repo_cfg = &restored.repository_settings[0];
    assert_eq!(repo_cfg.active_action_id.as_deref(), Some("act-trusted"));
    assert_eq!(repo_cfg.custom_actions.len(), 2);
    assert_eq!(repo_cfg.custom_actions[0].skip_write_confirm, Some(true));
    assert!(repo_cfg.custom_actions[0].trusted_prompt_hash.is_some());
    // Edited prompt hash mismatch must automatically revoke skip_write_confirm:
    assert_eq!(repo_cfg.custom_actions[1].skip_write_confirm, None);
    assert_eq!(repo_cfg.custom_actions[1].trusted_prompt_hash, None);
}
