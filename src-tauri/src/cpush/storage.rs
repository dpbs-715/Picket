//! C-Push session persistence (`cpush-sessions.json`), startup recovery, and done-notice dismissal.

use super::{
    CPushCoordinator,
    helpers::{enter_conflict_state, summarize_commits},
    types::*,
};
use crate::{
    i18n::{self, LanguageKind},
    repository::{self, remote::ConflictedFileSummary, types::RepositorySnapshot},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write, os::unix::fs::PermissionsExt, path::Path};
use tempfile::NamedTempFile;

const CPUSH_STORAGE_VERSION: u8 = 1;
const CPUSH_STORAGE_FILE: &str = "cpush-sessions.json";
const MAX_STORED_SESSIONS: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCPushFile {
    version: u8,
    sessions: Vec<StoredCPushSession>,
}

pub(super) fn read_sessions_file(
    storage_dir: &Path,
    locale: LanguageKind,
) -> (Vec<StoredCPushSession>, Option<String>) {
    let path = storage_dir.join(CPUSH_STORAGE_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(_) => {
            return (
                Vec::new(),
                Some(i18n::cpush::cpush_sessions_unreadable_warning(locale).into()),
            );
        }
    };
    match serde_json::from_slice::<StoredCPushFile>(&bytes) {
        Ok(parsed) if parsed.version == CPUSH_STORAGE_VERSION => (parsed.sessions, None),
        _ => (
            Vec::new(),
            Some(i18n::cpush::cpush_sessions_corrupted_warning(locale).into()),
        ),
    }
}

pub(super) fn write_sessions_file_atomic(
    storage_dir: &Path,
    sessions: &[StoredCPushSession],
) -> Result<(), String> {
    fs::create_dir_all(storage_dir).map_err(|e| {
        format!("Cannot create application data directory for C-Push sessions: {e}")
    })?;
    let payload = serde_json::to_vec_pretty(&StoredCPushFile {
        version: CPUSH_STORAGE_VERSION,
        sessions: sessions.iter().take(MAX_STORED_SESSIONS).cloned().collect(),
    })
    .map_err(|e| format!("Cannot serialize C-Push session history: {e}"))?;

    let mut temp = NamedTempFile::new_in(storage_dir)
        .map_err(|e| format!("Cannot create temporary C-Push session file: {e}"))?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("Cannot set permissions on C-Push session file: {e}"))?;
    temp.write_all(&payload)
        .map_err(|e| format!("Failed to write C-Push session file: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync C-Push session file: {e}"))?;
    temp.persist(storage_dir.join(CPUSH_STORAGE_FILE))
        .map_err(|e| format!("Failed to atomically persist C-Push session file: {e}"))?;
    Ok(())
}

impl CPushCoordinator {
    pub async fn restore(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
    ) -> Result<(Vec<CPushSessionSnapshot>, Option<String>), String> {
        self.restore_with_locale(storage_dir, snapshots, LanguageKind::En)
            .await
    }

    pub async fn restore_with_locale(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
        locale: LanguageKind,
    ) -> Result<(Vec<CPushSessionSnapshot>, Option<String>), String> {
        let _guard = self.inner.storage_gate.lock().await;
        let (stored_list, warning) = read_sessions_file(storage_dir, locale);
        let live_trees: HashMap<&str, &repository::types::WorktreeSnapshot> = snapshots
            .iter()
            .flat_map(|snap| {
                snap.worktrees
                    .iter()
                    .map(|wt| (wt.worktree_id.as_str(), wt))
            })
            .collect();

        let mut memory = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?;
        let mut public_sessions = Vec::new();
        let mut dirty = false;

        for mut session in stored_list {
            session.auto_collapse = false;
            if session.plan_only && session.commits.is_empty() {
                dirty = true;
                continue;
            }
            // Interrupted `Running` sessions must NEVER auto-rerun commits, rebases, or pushes (`W04`):
            if session.session_status == CPushSessionStatus::Running {
                dirty = true;
                session.locale = locale;
                if session.commits.is_empty() && session.planned_groups.is_empty() {
                    let live_has_conflict = live_trees
                        .get(session.worktree_id.as_str())
                        .is_some_and(|wt| wt.files.iter().any(|f| f.status == "U"));
                    if !live_has_conflict {
                        continue;
                    }
                }

                if let Some(live_tree) = live_trees.get(session.worktree_id.as_str()) {
                    // 1. Check if an in-progress local commit step landed right before exit:
                    if session.next_step_index < session.planned_groups.len()
                        && let Some(live_head) = &live_tree.head
                    {
                        let expected_prev = if session.next_step_index == 0 {
                            session.initial_head.as_deref()
                        } else {
                            session
                                .commits
                                .get(session.next_step_index - 1)
                                .map(|c| c.oid.as_str())
                        };
                        if Some(live_head.as_str()) != expected_prev
                            && !session.commits.iter().any(|c| &c.oid == live_head)
                        {
                            let group = &session.planned_groups[session.next_step_index];
                            let first_line = group
                                .message
                                .lines()
                                .next()
                                .unwrap_or_else(|| {
                                    i18n::cpush::cpush_commit_completed_fallback(locale)
                                })
                                .to_owned();
                            session.commits.push(CPushLandedCommit {
                                oid: live_head.clone(),
                                short_oid: live_head.chars().take(7).collect(),
                                title: first_line,
                                branch: live_tree.branch.clone(),
                                file_ids: group.file_ids.clone(),
                            });
                            session.committed_file_ids.extend(group.file_ids.clone());
                            session.next_step_index += 1;
                        }
                    }

                    // 2. If live Git currently has conflicted files, surface `Conflict` directly:
                    let live_conflicts: Vec<ConflictedFileSummary> = live_tree
                        .files
                        .iter()
                        .filter(|f| f.status == "U")
                        .map(|f| ConflictedFileSummary {
                            file_id: f.id.clone(),
                            path: f.path.clone(),
                        })
                        .collect();
                    if !live_conflicts.is_empty() {
                        let upstream = match &live_tree.remote {
                            repository::types::Remote::Tracked { branch, .. } => branch.as_str(),
                            _ => i18n::repository::remote_branch_fallback(locale),
                        };
                        let has_model = !session.model_id.is_empty();
                        enter_conflict_state(
                            &mut session,
                            upstream,
                            &live_conflicts,
                            has_model,
                            Some(i18n::cpush::cpush_restart_conflict_reason(locale)),
                        );
                        public_sessions.push(session.to_public());
                        memory.insert(session.worktree_id.clone(), session);
                        continue;
                    }
                }

                let has_remaining_commits = session.next_step_index < session.planned_groups.len();
                let commit_summaries = summarize_commits(&session.commits);

                if !has_remaining_commits && !session.commits.is_empty() && session.push_authorized
                {
                    // All local commits landed, interrupted during remote sync / push (`W04`):
                    session.session_status = CPushSessionStatus::PushFailed;
                    session.can_retry_remaining = false;
                    session.message = Some(CPushUiMessage {
                        id: format!("interrupted-push-{}", session.run_id),
                        kind: "pushFailed".into(),
                        title: i18n::cpush::cpush_interrupted_push_title(locale).into(),
                        description: i18n::cpush::cpush_interrupted_push_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            CPushMessageOption {
                                id: "retry-push".into(),
                                label: i18n::cpush::cpush_option_retry_push(locale).into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
                                id: "dismiss-session".into(),
                                label: i18n::cpush::cpush_option_keep_local_end(locale).into(),
                                recommended: None,
                            },
                        ]),
                        progress: None,
                    });
                } else {
                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = has_remaining_commits;
                    let mut options = Vec::new();
                    if has_remaining_commits {
                        options.push(CPushMessageOption {
                            id: "retry-remaining".into(),
                            label: i18n::cpush::cpush_option_confirm_continue_remaining(locale)
                                .into(),
                            recommended: Some(true),
                        });
                    }
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush::cpush_option_dismiss_interrupted(
                            locale,
                            !session.commits.is_empty(),
                        )
                        .into(),
                        recommended: (!has_remaining_commits).then_some(true),
                    });
                    session.message = Some(CPushUiMessage {
                        id: format!("interrupted-{}", session.run_id),
                        kind: "error".into(),
                        title: i18n::cpush::cpush_interrupted_commits_title(
                            locale,
                            session.commits.len(),
                        ),
                        description: i18n::cpush::cpush_interrupted_commits_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(options),
                        progress: None,
                    });
                }
            }
            if session.session_status == CPushSessionStatus::Done && session.notice_dismissed {
                memory.insert(session.worktree_id.clone(), session);
                continue;
            }
            public_sessions.push(session.to_public());
            memory.insert(session.worktree_id.clone(), session);
        }
        let updated_all: Vec<StoredCPushSession> = memory.values().cloned().collect();
        drop(memory);
        if dirty {
            let _ = write_sessions_file_atomic(storage_dir, &updated_all);
        }
        Ok((public_sessions, warning))
    }

    /// Persist user acknowledgement ("Got it" / inline `×`) of a completed `Done` session
    /// so its completed summary bar does not resurface after restarting Picket.
    pub async fn dismiss_done_notice(
        &self,
        storage_dir: &Path,
        request: &DismissCPushDoneRequest,
    ) -> Result<bool, String> {
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)?;
        let _guard = self.inner.storage_gate.lock().await;
        let updated_all = {
            let mut memory = self
                .inner
                .sessions
                .lock()
                .map_err(|_| "C-Push session cache lock is unavailable")?;
            let Some(session) = memory.get_mut(&request.target.worktree_id) else {
                return Ok(false);
            };
            if session.repository_id != request.target.repository_id
                || session.session_status != CPushSessionStatus::Done
            {
                return Ok(false);
            }
            if let Some(run_id) = request
                .run_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                && session.run_id != run_id
            {
                return Ok(false);
            }
            if let Some(msg) = &session.message
                && !request.message_id.trim().is_empty()
                && msg.id != request.message_id.trim()
            {
                return Ok(false);
            }
            if session.notice_dismissed && session.message.is_none() {
                return Ok(false);
            }
            session.notice_dismissed = true;
            session.message = None;
            memory.values().cloned().collect::<Vec<_>>()
        };
        write_sessions_file_atomic(storage_dir, &updated_all)?;
        Ok(true)
    }
}
