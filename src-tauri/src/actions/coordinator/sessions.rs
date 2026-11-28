//! Unified in-memory session cache, startup restoration, done-notice dismissal, and menu-bar tray projection.

use super::CustomActionCoordinator;
use crate::{
    actions::{session_store, types::*},
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
    repository::{self, types::RepositorySnapshot},
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

impl CustomActionCoordinator {
    pub(super) fn save_session_in_memory(
        &self,
        session: &StoredActionSession,
    ) -> Result<(), String> {
        self.inner
            .active_sessions
            .lock()
            .map_err(|_| "Session cache lock is unavailable")?
            .insert(session.worktree_id.clone(), session.clone());
        Ok(())
    }

    pub(super) fn remove_session_in_memory(&self, worktree_id: &str) -> Result<(), String> {
        self.inner
            .active_sessions
            .lock()
            .map_err(|_| "Session cache lock is unavailable")?
            .remove(worktree_id);
        Ok(())
    }

    pub(super) async fn persist_session(
        &self,
        storage_dir: &Path,
        session: &StoredActionSession,
    ) -> Result<(), String> {
        let mut to_store = session.clone();
        to_store.updated_at_ms = session_store::now_millis();
        session_store::upsert_session_async(storage_dir, to_store).await
    }

    pub(super) async fn remove_persisted_session(
        &self,
        storage_dir: &Path,
        worktree_id: &str,
        run_id: Option<&str>,
    ) -> Result<(), String> {
        session_store::remove_session_async(
            storage_dir,
            worktree_id.to_owned(),
            run_id.map(str::to_owned),
        )
        .await
    }

    pub(super) fn lookup_processed_request(
        &self,
        request_id: &str,
    ) -> Result<Option<ActionCommandReply>, String> {
        Ok(self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "Action request registry lock is unavailable")?
            .get(request_id)
            .cloned())
    }

    pub(super) fn record_processed_request(
        &self,
        request_id: &str,
        reply: &ActionCommandReply,
    ) -> Result<(), String> {
        self.inner
            .processed_requests
            .lock()
            .map_err(|_| "Action request registry lock is unavailable")?
            .insert(request_id.to_owned(), reply.clone());
        Ok(())
    }

    pub(super) fn ensure_no_busy_session(
        &self,
        worktree_id: &str,
        locale: LanguageKind,
        for_cpush: bool,
    ) -> Result<(), AppError> {
        let existing = self
            .inner
            .active_sessions
            .lock()
            .map_err(|_| AppError::internal("Session cache lock is unavailable"))?
            .get(worktree_id)
            .cloned();
        if let Some(active) = existing
            && active.status.is_active()
        {
            return Err(AppError::action(
                AppErrorCode::ActionBusy,
                i18n::cpush::cpush_active_session_busy_error(locale, for_cpush),
            )
            .with_retryable(true));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn restore_sessions(
        &self,
        storage_dir: &Path,
        locale: LanguageKind,
    ) -> Vec<ActionSessionSnapshot> {
        session_store::reconcile_sessions_on_startup(storage_dir, locale)
    }

    #[cfg(any(test, feature = "smoke-harness"))]
    pub async fn restore_cpush(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
    ) -> Result<(Vec<ActionSessionSnapshot>, Option<String>), String> {
        self.restore_all_sessions_async(storage_dir, snapshots, LanguageKind::En)
            .await
    }

    pub async fn restore_all_sessions_async(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
        locale: LanguageKind,
    ) -> Result<(Vec<ActionSessionSnapshot>, Option<String>), String> {
        let reconciled = session_store::reconcile_all_sessions_on_startup_async(
            storage_dir,
            snapshots.to_vec(),
            locale,
        )
        .await;
        {
            let mut memory = self
                .inner
                .active_sessions
                .lock()
                .map_err(|_| "Session cache lock is unavailable")?;
            memory.clear();
            // Load the highest-priority / newest session per worktree only if it is still visible,
            // never resurrecting older historical sessions behind a dismissed/cancelled/failed one.
            let mut ordered = reconciled.stored_sessions;
            ordered.sort_by_key(|s| (!s.status.is_active(), std::cmp::Reverse(s.updated_at_ms)));
            let mut seen_worktrees = HashSet::new();
            for session in ordered {
                if !seen_worktrees.insert(session.worktree_id.clone()) {
                    continue;
                }
                if session.status.is_active()
                    || (session.status == ActionSessionStatus::Done && !session.notice_dismissed)
                {
                    memory.insert(session.worktree_id.clone(), session);
                }
            }
        }
        Ok((reconciled.public_sessions, reconciled.warning))
    }

    #[cfg(test)]
    pub fn dismiss_done_notice(
        &self,
        storage_dir: &Path,
        request: &DismissSessionDoneRequest,
    ) -> Result<bool, AppError> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        self.mark_dismissed_in_memory(request)?;
        session_store::dismiss_session_message(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            request.run_id.as_deref(),
            &request.message_id,
        )
        .map_err(AppError::storage)
    }

    /// Aggregates live worktree session states directly in Rust for the menu bar tray
    /// from the single unified `active_sessions` cache without depending on the main WebView.
    pub fn tray_session_statuses(&self) -> HashMap<String, crate::tray::TrayWorktreeSessionInfo> {
        use crate::tray::{TraySessionStatus, TrayWorktreeSessionInfo};
        let mut out: HashMap<String, TrayWorktreeSessionInfo> = HashMap::new();
        if let Ok(sessions) = self.inner.active_sessions.lock() {
            for (wt_id, session) in sessions.iter() {
                if session.status == ActionSessionStatus::Done && session.notice_dismissed {
                    continue;
                }
                let status = session.status.to_tray_status();
                if status != TraySessionStatus::Idle {
                    let detail = session
                        .message
                        .as_ref()
                        .map(|m| m.title.clone())
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| {
                            (!session.summary.trim().is_empty()).then(|| session.summary.clone())
                        });
                    out.insert(wt_id.clone(), TrayWorktreeSessionInfo { status, detail });
                }
            }
        }
        out
    }

    pub async fn dismiss_done_notice_async(
        &self,
        storage_dir: &Path,
        request: &DismissSessionDoneRequest,
    ) -> Result<bool, AppError> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        let mem_updated = self.mark_dismissed_in_memory(request)?;
        let disk_updated = session_store::dismiss_session_message_async(
            storage_dir,
            request.target.repository_id.clone(),
            request.target.worktree_id.clone(),
            request.run_id.clone(),
            request.message_id.clone(),
        )
        .await
        .map_err(AppError::storage)?;
        Ok(mem_updated || disk_updated)
    }

    fn mark_dismissed_in_memory(
        &self,
        request: &DismissSessionDoneRequest,
    ) -> Result<bool, AppError> {
        let mut memory = self
            .inner
            .active_sessions
            .lock()
            .map_err(|_| AppError::internal("Session cache lock is unavailable"))?;
        let Some(session) = memory.get_mut(&request.target.worktree_id) else {
            return Ok(false);
        };
        if session.repository_id != request.target.repository_id || session.status.is_active() {
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
        session.dismissed_message_id = Some(request.message_id.clone());
        session.message = None;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{
        message_user::{MessageBlock, MessageFindingItem, MessageFindingSeverity},
        session_store::{now_millis, upsert_session},
    };

    #[test]
    fn message_user_card_validation_filters_unsafe_links_and_paths_and_restores_after_restart() {
        let raw = MessageUserRequest {
            title: Some("Found 1 blocking issue".into()),
            subtitle: Some("Reviewed 3 files".into()),
            tone: Some(MessageTone::Warning),
            blocks: Some(vec![MessageBlock::Findings {
                heading: None,
                items: vec![MessageFindingItem {
                    severity: MessageFindingSeverity::High,
                    tag: Some("P1".into()),
                    title: "Missing lock release".into(),
                    detail: "Use an RAII guard.".into(),
                    path: Some("./src/main.rs".into()),
                    line: Some(42),
                }],
                empty_text: None,
                skipped: None,
            }]),
            links: Some(vec![
                WorkflowLink {
                    label: "PR #42".into(),
                    url: "https://github.com/acme/picket/pull/42".into(),
                },
                WorkflowLink {
                    label: "Bad".into(),
                    url: "javascript:alert(1)".into(),
                },
            ]),
            questions: None,
            context: None,
        };
        let valid = raw.sanitize_and_validate().unwrap();
        assert!(!valid.has_questions());
        let card = valid.to_card("Fallback");
        assert_eq!(card.links.as_ref().map(Vec::len), Some(1));
        if let Some(MessageBlock::Findings { items, .. }) =
            card.blocks.as_ref().and_then(|b| b.first())
        {
            assert_eq!(items[0].path.as_deref(), Some("src/main.rs"));
        } else {
            panic!("expected findings block");
        }

        // Unsafe path traversal in findings must be rejected:
        let bad_path = MessageUserRequest {
            title: Some("Bad".into()),
            subtitle: None,
            tone: None,
            blocks: Some(vec![MessageBlock::Findings {
                heading: None,
                items: vec![MessageFindingItem {
                    severity: MessageFindingSeverity::High,
                    tag: None,
                    title: "Traversal".into(),
                    detail: "Blocked".into(),
                    path: Some("../secret.txt".into()),
                    line: Some(1),
                }],
                empty_text: None,
                skipped: None,
            }]),
            links: None,
            questions: None,
            context: None,
        };
        assert!(bad_path.sanitize_and_validate().is_err());

        // Cold restart restores persisted MessageUserCard intact:
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join(".picket");
        upsert_session(
            &storage,
            StoredActionSession {
                run_id: "run-card-1".into(),
                repository_id: "repo-1".into(),
                worktree_id: "wt-1".into(),
                session_id: "sess-1".into(),
                action_id: "act-dcr".into(),
                action_label: "Review Code".into(),
                profile: ActionProfileKind::CustomAction,
                action_prompt_hash: "hash-1".into(),
                frozen_scope_version: String::new(),
                model_id: "m-1".into(),
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
                summary: "Review complete".into(),
                card: Some(card.clone()),
                message: None,
                timeline: vec![],
                qa_log: vec![],
                commits: vec![],
                links: card.links.clone().unwrap_or_default(),
                can_retry_remaining: false,
                push_authorized: false,
                auto_collapse: false,
                notice_dismissed: false,
                dismissed_message_id: None,
                edited_file_hashes: Default::default(),
                submit_ledger: Default::default(),
                locale: LanguageKind::En,
                commit_language: LanguageKind::En,
            },
        )
        .unwrap();
        let restored = session_store::reconcile_sessions_on_startup(&storage, LanguageKind::En);
        assert_eq!(restored.len(), 1);
        let msg = restored[0].message.as_ref().expect("restored message");
        assert_eq!(msg.title, "Found 1 blocking issue");
        assert_eq!(msg.card.as_ref(), Some(&card));
    }
}
