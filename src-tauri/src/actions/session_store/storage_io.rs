//! Atomic JSON storage I/O (`action-sessions.json` v3), lossless v2/v1 & legacy
//! `cpush-sessions.json` migration, and dismissal updates.

use super::schema::{
    ACTION_SESSIONS_FILE, ACTION_SESSIONS_VERSION, LEGACY_CPUSH_SESSIONS_FILE,
    LegacyActionSessionsFileV2, LegacyCPushSessionsFile, MAX_STORED_CPUSH_SESSIONS,
    MAX_STORED_SESSIONS, PersistedWorkflowSession, StoredActionSessionsFile,
    StoredActionSessionsFileV3,
};
use crate::{
    actions::types::CPushSessionStatus,
    i18n::{self, LanguageKind},
    storage,
};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct SessionsVersionHeader {
    version: u8,
}

pub(super) fn sessions_file_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(ACTION_SESSIONS_FILE)
}

fn legacy_cpush_file_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(LEGACY_CPUSH_SESSIONS_FILE)
}

fn parse_sessions_file(bytes: &[u8]) -> Option<(StoredActionSessionsFile, bool)> {
    let header: SessionsVersionHeader = serde_json::from_slice(bytes).ok()?;
    match header.version {
        ACTION_SESSIONS_VERSION => {
            let v3: StoredActionSessionsFileV3 = serde_json::from_slice(bytes).ok()?;
            let mut state = StoredActionSessionsFile::default();
            for entry in v3.sessions {
                match entry {
                    PersistedWorkflowSession::CustomAction(action) => state.sessions.push(*action),
                    PersistedWorkflowSession::Cpush(cpush) => state.c_push_sessions.push(*cpush),
                }
            }
            Some((state, false))
        }
        1 | 2 => {
            let legacy: LegacyActionSessionsFileV2 = serde_json::from_slice(bytes).ok()?;
            if legacy.version != header.version {
                return None;
            }
            Some((
                StoredActionSessionsFile {
                    sessions: legacy.sessions,
                    c_push_sessions: legacy.c_push_sessions,
                },
                true,
            ))
        }
        _ => None,
    }
}

pub(super) fn load_store_unlocked(
    storage_dir: &Path,
    locale: LanguageKind,
) -> (StoredActionSessionsFile, Option<String>, bool) {
    let path = sessions_file_path(storage_dir);
    let mut warning: Option<String> = None;
    let mut needs_write = false;

    let mut store = match fs::read(&path) {
        Ok(bytes) => match parse_sessions_file(&bytes) {
            Some((parsed, migrated)) => {
                if migrated {
                    needs_write = true;
                }
                parsed
            }
            None => {
                storage::backup_corrupt_copy(&path, &bytes);
                warning = Some(i18n::cpush::cpush_sessions_corrupted_warning(locale).into());
                StoredActionSessionsFile::default()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            StoredActionSessionsFile::default()
        }
        Err(_) => {
            warning = Some(i18n::cpush::cpush_sessions_unreadable_warning(locale).into());
            StoredActionSessionsFile::default()
        }
    };

    // One-time graceful migration from legacy `~/.picket/cpush-sessions.json` (v1):
    let legacy_path = legacy_cpush_file_path(storage_dir);
    if legacy_path.exists() {
        match fs::read(&legacy_path) {
            Ok(bytes) => match serde_json::from_slice::<LegacyCPushSessionsFile>(&bytes) {
                Ok(legacy) if legacy.version == 1 => {
                    let existing_wt: HashSet<String> = store
                        .c_push_sessions
                        .iter()
                        .map(|s| s.worktree_id.clone())
                        .collect();
                    for item in legacy.sessions {
                        if !existing_wt.contains(&item.worktree_id) {
                            store.c_push_sessions.push(item);
                        }
                    }
                    needs_write = true;
                    if save_store_unlocked(storage_dir, store.clone()).is_ok() {
                        let _ = fs::remove_file(&legacy_path);
                        needs_write = false;
                    }
                }
                _ => {
                    storage::backup_corrupt_copy(&legacy_path, &bytes);
                    let _ = fs::remove_file(&legacy_path);
                    if warning.is_none() {
                        warning =
                            Some(i18n::cpush::cpush_sessions_corrupted_warning(locale).into());
                    }
                }
            },
            Err(_) => {
                if warning.is_none() {
                    warning = Some(i18n::cpush::cpush_sessions_unreadable_warning(locale).into());
                }
            }
        }
    }

    (store, warning, needs_write)
}

pub(super) fn save_store_unlocked(
    storage_dir: &Path,
    mut store: StoredActionSessionsFile,
) -> Result<(), String> {
    store
        .sessions
        .sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    store.sessions.truncate(MAX_STORED_SESSIONS);
    store.c_push_sessions.truncate(MAX_STORED_CPUSH_SESSIONS);

    let mut unified = Vec::with_capacity(store.sessions.len() + store.c_push_sessions.len());
    for action in store.sessions {
        unified.push(PersistedWorkflowSession::CustomAction(Box::new(action)));
    }
    for cpush in store.c_push_sessions {
        unified.push(PersistedWorkflowSession::Cpush(Box::new(cpush)));
    }
    let file = StoredActionSessionsFileV3 {
        version: ACTION_SESSIONS_VERSION,
        sessions: unified,
    };

    storage::write_json_atomic(storage_dir, &sessions_file_path(storage_dir), &file, false)
}

pub(super) fn apply_dismiss_unlocked(
    store: &mut StoredActionSessionsFile,
    repository_id: &str,
    worktree_id: &str,
    run_id: Option<&str>,
    message_id: &str,
) -> bool {
    let clean_run = run_id.map(str::trim).filter(|id| !id.is_empty());
    let clean_msg = message_id.trim();
    let mut updated = false;

    for session in &mut store.sessions {
        if session.repository_id != repository_id || session.worktree_id != worktree_id {
            continue;
        }
        if let Some(target_run) = clean_run
            && session.run_id != target_run
        {
            continue;
        }
        session.dismissed_message_id = Some(message_id.to_owned());
        updated = true;
    }

    for session in &mut store.c_push_sessions {
        if session.repository_id != repository_id
            || session.worktree_id != worktree_id
            || session.session_status != CPushSessionStatus::Done
        {
            continue;
        }
        if let Some(target_run) = clean_run
            && session.run_id != target_run
        {
            continue;
        }
        if let Some(msg) = &session.message
            && !clean_msg.is_empty()
            && msg.id != clean_msg
        {
            continue;
        }
        if session.notice_dismissed && session.message.is_none() {
            continue;
        }
        session.notice_dismissed = true;
        session.message = None;
        updated = true;
    }

    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{
        session_store::reconcile::reconcile_all_sessions_on_startup,
        types::{CPushLandedCommit, WorkflowUiMessage},
    };
    use serde_json::json;

    #[test]
    fn migrates_v2_action_and_cpush_sessions_losslessly_to_v3() {
        let dir = tempfile::tempdir().unwrap();
        let v2_payload = json!({
            "version": 2,
            "sessions": [{
                "runId": "run-act-v2",
                "repositoryId": "repo-1",
                "worktreeId": "wt-1",
                "actionId": "act-submit",
                "actionLabel": "Submit PR",
                "actionPromptHash": "hash-1",
                "status": "done",
                "updatedAtMs": 42000,
                "summary": "Opened PR #77",
                "timeline": [{ "state": "done", "label": "Created PR #77" }],
                "commits": [],
                "links": [{ "label": "PR #77", "url": "https://github.com/acme/demo/pull/77" }],
                "submitLedger": {
                    "workflowKind": "submit",
                    "groups": [{
                        "groupId": "g-1",
                        "title": "Feature A",
                        "issueUrl": "https://github.com/acme/demo/issues/12",
                        "prUrl": "https://github.com/acme/demo/pull/77"
                    }]
                }
            }],
            "cPushSessions": [{
                "runId": "run-cpush-v2-pushfail",
                "repositoryId": "repo-2",
                "worktreeId": "wt-2",
                "sessionId": "sess-2",
                "frozenScopeVersion": "sv-2",
                "modelId": "m-1",
                "branch": "feat/pay",
                "initialHead": "aaaa1111",
                "prompt": "keep split clean",
                "commits": [{
                    "oid": "bbbb22223333",
                    "shortOid": "bbbb222",
                    "title": "feat: add payment guard",
                    "branch": "feat/pay",
                    "fileIds": ["f-1"]
                }],
                "sessionStatus": "pushFailed",
                "message": {
                    "id": "push-failed-run-cpush-v2-pushfail",
                    "kind": "pushFailed",
                    "title": "Committed locally, push failed",
                    "description": "Network unreachable",
                    "options": [{ "id": "retry-push", "label": "Retry Push", "recommended": true }]
                },
                "canRetryRemaining": false,
                "pushAuthorized": true,
                "autoCollapse": false,
                "noticeDismissed": false,
                "locale": "en",
                "commitLanguage": "en"
            }, {
                "runId": "run-cpush-v2-conflict",
                "repositoryId": "repo-3",
                "worktreeId": "wt-3",
                "sessionId": "sess-3",
                "frozenScopeVersion": "sv-3",
                "modelId": "m-1",
                "branch": "feat/sync",
                "initialHead": "cccc3333",
                "prompt": "",
                "commits": [],
                "sessionStatus": "conflict",
                "message": {
                    "id": "conflict-run-cpush-v2-conflict",
                    "kind": "conflict",
                    "title": "Rebase conflict",
                    "description": "Resolve conflict in order.ts"
                },
                "canRetryRemaining": false,
                "pushAuthorized": true,
                "autoCollapse": false,
                "noticeDismissed": false,
                "locale": "en",
                "commitLanguage": "en"
            }]
        });
        fs::write(
            dir.path().join(ACTION_SESSIONS_FILE),
            serde_json::to_vec_pretty(&v2_payload).unwrap(),
        )
        .unwrap();

        let reconciled = reconcile_all_sessions_on_startup(dir.path(), &[], LanguageKind::En);
        assert_eq!(reconciled.warning, None);
        assert_eq!(reconciled.action_sessions.len(), 1);
        assert_eq!(reconciled.action_sessions[0].run_id, "run-act-v2");
        assert_eq!(reconciled.action_sessions[0].links.len(), 1);
        assert_eq!(reconciled.cpush_public.len(), 2);
        assert!(reconciled.cpush_public.iter().any(|s| {
            s.run_id == "run-cpush-v2-pushfail"
                && s.session_status == CPushSessionStatus::PushFailed
                && s.commits
                    == vec![CPushLandedCommit {
                        oid: "bbbb22223333".into(),
                        short_oid: "bbbb222".into(),
                        title: "feat: add payment guard".into(),
                        branch: "feat/pay".into(),
                        file_ids: vec!["f-1".into()],
                    }]
        }));
        assert!(reconciled.cpush_public.iter().any(|s| {
            s.run_id == "run-cpush-v2-conflict"
                && s.session_status == CPushSessionStatus::Conflict
                && matches!(
                    s.message,
                    Some(WorkflowUiMessage { ref id, .. }) if id == "conflict-run-cpush-v2-conflict"
                )
        }));

        // File on disk must now be upgraded cleanly to v3 tagged array:
        let upgraded: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(ACTION_SESSIONS_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(upgraded["version"], json!(3));
        assert!(upgraded.get("cPushSessions").is_none());
        let sessions = upgraded["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 3);
        assert_eq!(
            sessions
                .iter()
                .filter(|s| s["kind"] == "customAction")
                .count(),
            1
        );
        assert_eq!(sessions.iter().filter(|s| s["kind"] == "cpush").count(), 2);

        // Reloading the upgraded v3 file stays lossless and clean:
        let reloaded = reconcile_all_sessions_on_startup(dir.path(), &[], LanguageKind::En);
        assert_eq!(reloaded.warning, None);
        assert_eq!(reloaded.action_sessions.len(), 1);
        assert_eq!(reloaded.cpush_public.len(), 2);
    }

    #[test]
    fn corrupt_v3_sessions_file_creates_backup_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(ACTION_SESSIONS_FILE),
            b"{ \"version\": 99, \"sessions\": \"broken\" }",
        )
        .unwrap();
        let (store, warning, _) = load_store_unlocked(dir.path(), LanguageKind::En);
        assert!(warning.is_some());
        assert!(store.sessions.is_empty());
        assert!(store.c_push_sessions.is_empty());
        let has_backup = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt."));
        assert!(has_backup, "expected corrupt backup file to be preserved");
    }
}
