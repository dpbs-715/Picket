//! Atomic JSON storage I/O (`action-sessions.json` v4 unified flat array), lossless
//! v3/v2/v1 & legacy `cpush-sessions.json` migration, and dismissal updates.

use super::schema::{
    ACTION_SESSIONS_FILE, ACTION_SESSIONS_VERSION, LEGACY_CPUSH_SESSIONS_FILE,
    LegacyActionSessionsFileV2, LegacyActionSessionsFileV3, LegacyCPushSessionsFile,
    LegacyWorkflowSessionV3, MAX_STORED_SESSIONS, StoredActionSessionsFileV4,
};
use crate::{
    actions::types::{ActionSessionStatus, StoredActionSession},
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

fn parse_sessions_file(bytes: &[u8]) -> Option<(Vec<StoredActionSession>, bool)> {
    let header: SessionsVersionHeader = serde_json::from_slice(bytes).ok()?;
    match header.version {
        ACTION_SESSIONS_VERSION => {
            let v4: StoredActionSessionsFileV4 = serde_json::from_slice(bytes).ok()?;
            Some((v4.sessions, false))
        }
        3 => {
            let v3: LegacyActionSessionsFileV3 = serde_json::from_slice(bytes).ok()?;
            if v3.version != 3 {
                return None;
            }
            let sessions = v3
                .sessions
                .into_iter()
                .map(|entry| match entry {
                    LegacyWorkflowSessionV3::CustomAction(a) => StoredActionSession::from(*a),
                    LegacyWorkflowSessionV3::Cpush(c) => StoredActionSession::from(*c),
                })
                .collect();
            Some((sessions, true))
        }
        1 | 2 => {
            let legacy: LegacyActionSessionsFileV2 = serde_json::from_slice(bytes).ok()?;
            if legacy.version != header.version {
                return None;
            }
            let mut sessions =
                Vec::with_capacity(legacy.sessions.len() + legacy.c_push_sessions.len());
            for action in legacy.sessions {
                sessions.push(StoredActionSession::from(action));
            }
            for cpush in legacy.c_push_sessions {
                sessions.push(StoredActionSession::from(cpush));
            }
            Some((sessions, true))
        }
        _ => None,
    }
}

pub(super) fn load_store_unlocked(
    storage_dir: &Path,
    locale: LanguageKind,
) -> (Vec<StoredActionSession>, Option<String>, bool) {
    let path = sessions_file_path(storage_dir);
    let mut warning: Option<String> = None;
    let mut needs_write = false;

    let mut sessions = match fs::read(&path) {
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
                Vec::new()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => {
            warning = Some(i18n::cpush::cpush_sessions_unreadable_warning(locale).into());
            Vec::new()
        }
    };

    // One-time graceful migration from legacy `~/.picket/cpush-sessions.json` (v1):
    let legacy_path = legacy_cpush_file_path(storage_dir);
    if legacy_path.exists() {
        match fs::read(&legacy_path) {
            Ok(bytes) => match serde_json::from_slice::<LegacyCPushSessionsFile>(&bytes) {
                Ok(legacy) if legacy.version == 1 => {
                    let existing_runs: HashSet<String> =
                        sessions.iter().map(|s| s.run_id.clone()).collect();
                    let existing_cpush_wts: HashSet<String> = sessions
                        .iter()
                        .filter(|s| s.profile.is_cpush_like())
                        .map(|s| s.worktree_id.clone())
                        .collect();
                    for item in legacy.sessions {
                        if !existing_runs.contains(&item.run_id)
                            && !existing_cpush_wts.contains(&item.worktree_id)
                        {
                            sessions.push(StoredActionSession::from(item));
                        }
                    }
                    needs_write = true;
                    if save_store_unlocked(storage_dir, sessions.clone()).is_ok() {
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

    (sessions, warning, needs_write)
}

pub(super) fn save_store_unlocked(
    storage_dir: &Path,
    mut sessions: Vec<StoredActionSession>,
) -> Result<(), String> {
    // Keep active/recoverable sessions ahead of older completed ones when truncating.
    sessions.sort_by_key(|s| (!s.status.is_active(), std::cmp::Reverse(s.updated_at_ms)));
    sessions.truncate(MAX_STORED_SESSIONS);

    let file = StoredActionSessionsFileV4 {
        version: ACTION_SESSIONS_VERSION,
        sessions,
    };

    storage::write_json_atomic(storage_dir, &sessions_file_path(storage_dir), &file, false)
}

pub(super) fn apply_dismiss_unlocked(
    sessions: &mut [StoredActionSession],
    repository_id: &str,
    worktree_id: &str,
    run_id: Option<&str>,
    message_id: &str,
) -> bool {
    let clean_run = run_id.map(str::trim).filter(|id| !id.is_empty());
    let clean_msg = message_id.trim();
    let mut updated = false;

    for session in sessions.iter_mut() {
        if session.repository_id != repository_id || session.worktree_id != worktree_id {
            continue;
        }
        if let Some(target_run) = clean_run
            && session.run_id != target_run
        {
            continue;
        }
        if session.status == ActionSessionStatus::Done {
            if let Some(msg) = &session.message
                && !clean_msg.is_empty()
                && msg.id != clean_msg
            {
                continue;
            }
            if session.notice_dismissed
                && session.message.is_none()
                && session.dismissed_message_id.as_deref() == Some(message_id)
            {
                continue;
            }
            session.notice_dismissed = true;
            session.dismissed_message_id = Some(message_id.to_owned());
            session.message = None;
            updated = true;
        } else if !session.status.is_active() {
            session.dismissed_message_id = Some(message_id.to_owned());
            if let Some(msg) = &session.message
                && (clean_msg.is_empty() || msg.id == clean_msg)
            {
                session.message = None;
            }
            updated = true;
        }
    }

    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{
        profile::ActionProfileKind,
        session_store::reconcile::reconcile_all_sessions_on_startup,
        types::{CPushLandedCommit, WorkflowUiMessage},
    };
    use serde_json::json;

    #[test]
    fn migrates_v2_and_v3_sessions_losslessly_to_v4() {
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
        assert_eq!(reconciled.public_sessions.len(), 3);
        assert!(reconciled.public_sessions.iter().any(|s| {
            s.run_id == "run-act-v2"
                && s.profile == ActionProfileKind::CustomAction
                && s.links.len() == 1
        }));
        assert!(reconciled.public_sessions.iter().any(|s| {
            s.run_id == "run-cpush-v2-pushfail"
                && s.profile == ActionProfileKind::Cpush
                && s.status == ActionSessionStatus::PushFailed
                && s.commits
                    == vec![CPushLandedCommit {
                        oid: "bbbb22223333".into(),
                        short_oid: "bbbb222".into(),
                        title: "feat: add payment guard".into(),
                        branch: "feat/pay".into(),
                        file_ids: vec!["f-1".into()],
                    }]
        }));
        assert!(reconciled.public_sessions.iter().any(|s| {
            s.run_id == "run-cpush-v2-conflict"
                && s.status == ActionSessionStatus::Conflict
                && matches!(
                    s.message,
                    Some(WorkflowUiMessage { ref id, .. }) if id == "conflict-run-cpush-v2-conflict"
                )
        }));

        // File on disk must now be upgraded cleanly to v4 flat array:
        let upgraded: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(ACTION_SESSIONS_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(upgraded["version"], json!(4));
        assert!(upgraded.get("cPushSessions").is_none());
        let sessions = upgraded["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 3);
        assert!(sessions.iter().all(|s| s.get("kind").is_none()));
        assert_eq!(
            sessions
                .iter()
                .filter(|s| s["profile"] == "customAction")
                .count(),
            1
        );
        assert_eq!(
            sessions.iter().filter(|s| s["profile"] == "cpush").count(),
            2
        );

        // Reloading the upgraded v4 file stays lossless and clean:
        let reloaded = reconcile_all_sessions_on_startup(dir.path(), &[], LanguageKind::En);
        assert_eq!(reloaded.warning, None);
        assert_eq!(reloaded.public_sessions.len(), 3);
    }

    #[test]
    fn migrates_v3_tagged_sessions_losslessly_to_v4() {
        let dir = tempfile::tempdir().unwrap();
        let v3_payload = json!({
            "version": 3,
            "sessions": [
                {
                    "kind": "customAction",
                    "runId": "run-v3-act",
                    "repositoryId": "repo-1",
                    "worktreeId": "wt-1",
                    "actionId": "act-review",
                    "actionLabel": "Code Review",
                    "actionPromptHash": "hash-v3",
                    "status": "done",
                    "updatedAtMs": 50000,
                    "summary": "No blocking findings",
                    "timeline": [],
                    "commits": [],
                    "links": []
                },
                {
                    "kind": "cpush",
                    "runId": "run-v3-cpush",
                    "repositoryId": "repo-2",
                    "worktreeId": "wt-2",
                    "sessionId": "sess-2",
                    "frozenScopeVersion": "sv-2",
                    "modelId": "m-1",
                    "branch": "main",
                    "initialHead": "11112222",
                    "prompt": "",
                    "commits": [],
                    "sessionStatus": "pushFailed",
                    "message": {
                        "id": "push-failed-v3",
                        "kind": "pushFailed",
                        "title": "Push failed",
                        "description": "Offline"
                    },
                    "canRetryRemaining": false,
                    "pushAuthorized": true,
                    "autoCollapse": false,
                    "noticeDismissed": false,
                    "locale": "en",
                    "commitLanguage": "en"
                }
            ]
        });
        fs::write(
            dir.path().join(ACTION_SESSIONS_FILE),
            serde_json::to_vec_pretty(&v3_payload).unwrap(),
        )
        .unwrap();

        let reconciled = reconcile_all_sessions_on_startup(dir.path(), &[], LanguageKind::En);
        assert_eq!(reconciled.warning, None);
        assert_eq!(reconciled.public_sessions.len(), 2);
        let upgraded: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(ACTION_SESSIONS_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(upgraded["version"], json!(4));
    }

    #[test]
    fn corrupt_v4_sessions_file_creates_backup_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(ACTION_SESSIONS_FILE),
            b"{ \"version\": 99, \"sessions\": \"broken\" }",
        )
        .unwrap();
        let (sessions, warning, _) = load_store_unlocked(dir.path(), LanguageKind::En);
        assert!(warning.is_some());
        assert!(sessions.is_empty());
        let has_backup = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt."));
        assert!(has_backup, "expected corrupt backup file to be preserved");
    }

    #[test]
    fn reconciles_interrupted_custom_action_preserves_deterministic_decision_and_prevents_done_resurrection()
     {
        let dir = tempfile::tempdir().unwrap();
        let v4_payload = json!({
            "version": 4,
            "sessions": [
                {
                    "runId": "run-live-q",
                    "repositoryId": "repo-1",
                    "worktreeId": "wt-1",
                    "sessionId": "s-1",
                    "actionId": "act-1",
                    "actionLabel": "Release",
                    "profile": "customAction",
                    "actionPromptHash": "h1",
                    "frozenScopeVersion": "sv-1",
                    "modelId": "m-1",
                    "branch": "main",
                    "status": "needsDecision",
                    "pendingCallId": "tc-dead",
                    "updatedAtMs": 2000,
                    "summary": "",
                    "message": {
                        "id": "ask-dead",
                        "kind": "question",
                        "title": "Stale live question",
                        "description": "Should be cleared",
                        "questionnaire": { "questions": [] }
                    },
                    "timeline": [],
                    "qaLog": [],
                    "commits": [],
                    "links": [],
                    "canRetryRemaining": false,
                    "pushAuthorized": false,
                    "autoCollapse": false,
                    "noticeDismissed": false,
                    "locale": "en",
                    "commitLanguage": "en"
                },
                {
                    "runId": "run-deterministic-upstream",
                    "repositoryId": "repo-2",
                    "worktreeId": "wt-2",
                    "sessionId": "s-2",
                    "actionId": "act-2",
                    "actionLabel": "Push Custom",
                    "profile": "customAction",
                    "actionPromptHash": "h2",
                    "frozenScopeVersion": "sv-2",
                    "modelId": "m-1",
                    "branch": "feat/x",
                    "status": "needsDecision",
                    "updatedAtMs": 3000,
                    "summary": "",
                    "message": {
                        "id": "missing-upstream-run-deterministic-upstream",
                        "kind": "question",
                        "title": "Choose upstream remote",
                        "description": "Select remote",
                        "options": [{ "id": "push-set-upstream:origin", "label": "origin" }]
                    },
                    "timeline": [],
                    "qaLog": [],
                    "commits": [],
                    "links": [],
                    "canRetryRemaining": false,
                    "pushAuthorized": true,
                    "autoCollapse": false,
                    "noticeDismissed": false,
                    "locale": "en",
                    "commitLanguage": "en"
                },
                {
                    "runId": "run-done-new",
                    "repositoryId": "repo-3",
                    "worktreeId": "wt-3",
                    "sessionId": "s-3",
                    "actionId": "act-3",
                    "actionLabel": "Review",
                    "profile": "customAction",
                    "actionPromptHash": "h3",
                    "frozenScopeVersion": "sv-3",
                    "modelId": "m-1",
                    "branch": "main",
                    "status": "done",
                    "updatedAtMs": 5000,
                    "summary": "New review done",
                    "noticeDismissed": true,
                    "dismissedMessageId": "done-new",
                    "timeline": [],
                    "qaLog": [],
                    "commits": [],
                    "links": [],
                    "canRetryRemaining": false,
                    "pushAuthorized": false,
                    "autoCollapse": false,
                    "locale": "en",
                    "commitLanguage": "en"
                },
                {
                    "runId": "run-done-old",
                    "repositoryId": "repo-3",
                    "worktreeId": "wt-3",
                    "sessionId": "s-3",
                    "actionId": "act-3",
                    "actionLabel": "Review",
                    "profile": "customAction",
                    "actionPromptHash": "h3",
                    "frozenScopeVersion": "sv-3",
                    "modelId": "m-1",
                    "branch": "main",
                    "status": "done",
                    "updatedAtMs": 1000,
                    "summary": "Old review done",
                    "noticeDismissed": false,
                    "timeline": [],
                    "qaLog": [],
                    "commits": [],
                    "links": [],
                    "canRetryRemaining": false,
                    "pushAuthorized": false,
                    "autoCollapse": false,
                    "locale": "en",
                    "commitLanguage": "en"
                }
            ]
        });
        fs::write(
            dir.path().join(ACTION_SESSIONS_FILE),
            serde_json::to_vec_pretty(&v4_payload).unwrap(),
        )
        .unwrap();

        let reconciled = reconcile_all_sessions_on_startup(dir.path(), &[], LanguageKind::En);
        assert_eq!(reconciled.warning, None);

        // 1. Questionnaire NeedsDecision becomes Interrupted and replaces stale question message:
        let interrupted = reconciled
            .public_sessions
            .iter()
            .find(|s| s.run_id == "run-live-q")
            .expect("interrupted custom action should surface");
        assert_eq!(interrupted.status, ActionSessionStatus::Interrupted);
        assert!(interrupted.pending_call_id.is_none());
        let msg = interrupted.message.as_ref().expect("interrupted message");
        assert_eq!(msg.kind, "error");
        assert!(msg.questionnaire.is_none());

        // 2. Deterministic missing-upstream NeedsDecision stays active:
        let det = reconciled
            .public_sessions
            .iter()
            .find(|s| s.run_id == "run-deterministic-upstream")
            .expect("deterministic custom action decision must stay active");
        assert_eq!(det.status, ActionSessionStatus::NeedsDecision);

        // 3. Dismissed newer Done session must NOT resurrect older Done session on repo-3/wt-3:
        assert!(
            !reconciled
                .public_sessions
                .iter()
                .any(|s| s.repository_id == "repo-3"),
            "dismissed newer done session must not resurrect older done session"
        );
    }

    #[test]
    fn migrating_dismissed_v2_custom_action_sets_notice_dismissed() {
        let dir = tempfile::tempdir().unwrap();
        let v2_payload = json!({
            "version": 2,
            "sessions": [{
                "runId": "run-v2-dismissed",
                "repositoryId": "repo-1",
                "worktreeId": "wt-1",
                "actionId": "act-1",
                "actionLabel": "Review",
                "actionPromptHash": "h1",
                "status": "done",
                "updatedAtMs": 42000,
                "summary": "All good",
                "dismissedMessageId": "done-act-1",
                "timeline": [],
                "commits": [],
                "links": []
            }],
            "cPushSessions": []
        });
        fs::write(
            dir.path().join(ACTION_SESSIONS_FILE),
            serde_json::to_vec_pretty(&v2_payload).unwrap(),
        )
        .unwrap();

        let reconciled = reconcile_all_sessions_on_startup(dir.path(), &[], LanguageKind::En);
        assert!(
            reconciled.public_sessions.is_empty(),
            "dismissed v2 custom action must stay dismissed after migration"
        );
    }
}
