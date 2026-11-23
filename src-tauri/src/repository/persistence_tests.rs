use super::{persistence::WorkbenchPersistence, smoke::Fixture, *};
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn persists_and_cold_restores_editor_repositories_and_per_worktree_drafts_without_tokens() {
    let fixture = Fixture::new().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let store = RepositoryStore::default();
    let persistence = WorkbenchPersistence::default();

    // Missing file returns clean empty state without warning:
    let initial = persistence.restore(storage.path(), &store).await.unwrap();
    assert_eq!(initial.editor, EditorKind::Idea);
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
                editor: EditorKind::Cursor,
                active_worktree_id: Some(linked_wt.worktree_id.clone()),
                empty_draft: "empty draft note".into(),
                empty_editing: true,
                worktrees: vec![
                    PersistedWorktreeState {
                        repository_id: main_wt.repository_id.clone(),
                        worktree_id: main_wt.worktree_id.clone(),
                        draft_text: "feat: main worktree draft".into(),
                        draft_editing: true,
                        draft_outdated: false,
                        selected_ids: vec![main_wt.files[0].id.clone()],
                        scope_version: "scope-main".into(),
                        tab: WorkbenchTab::Commit,
                    },
                    PersistedWorktreeState {
                        repository_id: linked_wt.repository_id.clone(),
                        worktree_id: linked_wt.worktree_id.clone(),
                        draft_text: "fix: linked worktree draft".into(),
                        draft_editing: true,
                        draft_outdated: true,
                        selected_ids: vec![],
                        scope_version: "scope-linked".into(),
                        tab: WorkbenchTab::Review,
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
                editor: EditorKind::Idea,
                active_worktree_id: None,
                empty_draft: "stale".into(),
                empty_editing: false,
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
        !raw.contains("commitToken") && !raw.contains("branchToken") && !raw.contains("apiKey")
    );

    // Cold restore into brand-new instances:
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let restored = cold_persistence
        .restore(storage.path(), &cold_store)
        .await
        .unwrap();
    assert_eq!(restored.editor, EditorKind::Cursor);
    assert_eq!(
        restored.active_worktree_id.as_deref(),
        Some(linked_wt.worktree_id.as_str())
    );
    assert_eq!(restored.empty_draft, "empty draft note");
    assert_eq!(restored.snapshots.len(), 1);
    assert_eq!(restored.snapshots[0].repository.id, opened.repository.id);
    assert_eq!(restored.worktrees.len(), 2);
    assert!(restored.warning.is_none());
}

#[tokio::test]
async fn preserves_unavailable_repository_drafts_handles_corrupted_storage_and_rejects_extra_fields()
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
                editor: EditorKind::Webstorm,
                active_worktree_id: Some(wt_id.clone()),
                empty_draft: String::new(),
                empty_editing: false,
                worktrees: vec![PersistedWorktreeState {
                    repository_id: opened.repository.id.clone(),
                    worktree_id: wt_id.clone(),
                    draft_text: "keep even when repo unmounted".into(),
                    draft_editing: true,
                    draft_outdated: false,
                    selected_ids: vec![],
                    scope_version: "v1".into(),
                    tab: WorkbenchTab::Commit,
                }],
            },
        )
        .await
        .unwrap();

    // Remove the repository directory and cold restore:
    drop(fixture);
    let cold_store = RepositoryStore::default();
    let cold_persistence = WorkbenchPersistence::default();
    let missing_restore = cold_persistence
        .restore(storage.path(), &cold_store)
        .await
        .unwrap();
    assert_eq!(missing_restore.editor, EditorKind::Webstorm);
    assert!(missing_restore.snapshots.is_empty());
    assert!(
        missing_restore
            .warning
            .as_deref()
            .unwrap()
            .contains("不可访问")
    );
    assert_eq!(missing_restore.worktrees.len(), 1);
    assert_eq!(
        missing_restore.worktrees[0].draft_text,
        "keep even when repo unmounted"
    );

    // Saving in a session where that repository wasn't opened retains its saved draft:
    cold_persistence
        .save(
            storage.path(),
            &cold_store,
            WorkbenchSaveRequest {
                revision: 1,
                editor: EditorKind::Vscode,
                active_worktree_id: None,
                empty_draft: "new empty draft".into(),
                empty_editing: true,
                worktrees: vec![],
            },
        )
        .await
        .unwrap();
    let second_restore = WorkbenchPersistence::default()
        .restore(storage.path(), &RepositoryStore::default())
        .await
        .unwrap();
    assert_eq!(second_restore.editor, EditorKind::Vscode);
    assert_eq!(second_restore.worktrees.len(), 1);
    assert_eq!(
        second_restore.worktrees[0].draft_text,
        "keep even when repo unmounted"
    );

    // Corrupted JSON file falls back safely with warning and does not overwrite the file on restore:
    let saved_path = storage.path().join("workbench-state.json");
    fs::write(&saved_path, b"{broken json").unwrap();
    let corrupted_restore = WorkbenchPersistence::default()
        .restore(storage.path(), &RepositoryStore::default())
        .await
        .unwrap();
    assert_eq!(corrupted_restore.editor, EditorKind::Idea);
    assert!(corrupted_restore.warning.is_some());
    assert_eq!(fs::read(&saved_path).unwrap(), b"{broken json");

    // Wire format rejects injected paths or tokens:
    assert!(
        serde_json::from_value::<WorkbenchSaveRequest>(serde_json::json!({
            "revision": 1,
            "editor": "idea",
            "activeWorktreeId": null,
            "emptyDraft": "",
            "emptyEditing": false,
            "worktrees": [],
            "repositories": [{ "rootPath": "/etc" }]
        }))
        .is_err()
    );
}
