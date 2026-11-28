//! In-memory session cache, startup restoration, done-notice dismissal, and menu-bar tray projection.

use super::{CustomActionCoordinator, cpush_helpers};
use crate::{
    actions::{
        session_store::{self, ActionSessionSnapshot},
        types::*,
    },
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
    repository::{
        self,
        types::{ReadTarget, RepositorySnapshot},
    },
};
use std::{collections::HashMap, path::Path};

impl CustomActionCoordinator {
    pub(super) fn save_cpush_in_memory(&self, session: &StoredCPushSession) -> Result<(), String> {
        self.inner
            .cpush_sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .insert(session.worktree_id.clone(), session.clone());
        Ok(())
    }

    pub(super) fn remove_cpush_in_memory(&self, worktree_id: &str) -> Result<(), String> {
        self.inner
            .cpush_sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .remove(worktree_id);
        Ok(())
    }

    pub(super) async fn persist_cpush_sessions(&self, storage_dir: &Path) -> Result<(), String> {
        let all: Vec<StoredCPushSession> = self
            .inner
            .cpush_sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .values()
            .cloned()
            .collect();
        session_store::save_cpush_sessions_async(storage_dir, all).await
    }

    pub(super) fn lookup_processed_cpush_request(
        &self,
        request_id: &str,
    ) -> Result<Option<CPushCommandReply>, String> {
        Ok(self
            .inner
            .processed_cpush_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .get(request_id)
            .cloned())
    }

    pub(super) fn record_processed_cpush_request(
        &self,
        request_id: &str,
        reply: &CPushCommandReply,
    ) -> Result<(), String> {
        self.inner
            .processed_cpush_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .insert(request_id.to_owned(), reply.clone());
        Ok(())
    }

    pub(super) fn ensure_no_busy_cpush_session(
        &self,
        worktree_id: &str,
        locale: LanguageKind,
        for_cpush: bool,
    ) -> Result<(), AppError> {
        let existing_active = self
            .inner
            .cpush_sessions
            .lock()
            .map_err(|_| AppError::internal("C-Push session cache lock is unavailable"))?
            .get(worktree_id)
            .cloned();
        if let Some(active) = existing_active
            && cpush_helpers::is_active_status(active.session_status)
        {
            return Err(AppError::action(
                AppErrorCode::ActionBusy,
                i18n::cpush::cpush_active_session_busy_error(locale, for_cpush),
            )
            .with_retryable(true));
        }
        Ok(())
    }

    pub(super) fn session_read_target(&self, session: &StoredCPushSession) -> ReadTarget {
        ReadTarget {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        }
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
    ) -> Result<(Vec<CPushSessionSnapshot>, Option<String>), String> {
        let (_, cpush, warn) = self
            .restore_all_sessions_async(storage_dir, snapshots, LanguageKind::En)
            .await?;
        Ok((cpush, warn))
    }

    pub async fn restore_all_sessions_async(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
        locale: LanguageKind,
    ) -> Result<
        (
            Vec<ActionSessionSnapshot>,
            Vec<CPushSessionSnapshot>,
            Option<String>,
        ),
        String,
    > {
        let reconciled = session_store::reconcile_all_sessions_on_startup_async(
            storage_dir,
            snapshots.to_vec(),
            locale,
        )
        .await;
        {
            let mut memory = self
                .inner
                .cpush_sessions
                .lock()
                .map_err(|_| "C-Push session cache lock is unavailable")?;
            memory.clear();
            for session in reconciled.cpush_stored {
                memory.insert(session.worktree_id.clone(), session);
            }
        }
        Ok((
            reconciled.action_sessions,
            reconciled.cpush_public,
            reconciled.warning,
        ))
    }

    #[cfg(test)]
    pub fn dismiss_done_notice(
        &self,
        storage_dir: &Path,
        request: &DismissSessionDoneRequest,
    ) -> Result<bool, AppError> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        self.mark_cpush_dismissed_in_memory(request)?;
        session_store::dismiss_session_message(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            request.run_id.as_deref(),
            &request.message_id,
        )
        .map_err(AppError::storage)
    }

    pub(super) fn set_action_tray_status(
        &self,
        worktree_id: &str,
        status: crate::tray::TraySessionStatus,
        detail: Option<String>,
    ) {
        if let Ok(mut map) = self.inner.active_action_tray.lock() {
            if matches!(status, crate::tray::TraySessionStatus::Idle) {
                map.remove(worktree_id);
            } else {
                map.insert(
                    worktree_id.to_owned(),
                    crate::tray::TrayWorktreeSessionInfo { status, detail },
                );
            }
        }
    }

    /// Aggregates live C-Push and Custom Action worktree session states directly in Rust for
    /// the menu bar tray without depending on the main WebView.
    pub fn tray_session_statuses(&self) -> HashMap<String, crate::tray::TrayWorktreeSessionInfo> {
        use crate::tray::{TraySessionStatus, TrayWorktreeSessionInfo};
        let mut out: HashMap<String, TrayWorktreeSessionInfo> = HashMap::new();
        if let Ok(cpush) = self.inner.cpush_sessions.lock() {
            for (wt_id, session) in cpush.iter() {
                let status = match session.session_status {
                    CPushSessionStatus::Idle => TraySessionStatus::Idle,
                    CPushSessionStatus::Running => TraySessionStatus::Running,
                    CPushSessionStatus::NeedsDecision => TraySessionStatus::NeedsDecision,
                    CPushSessionStatus::Conflict => TraySessionStatus::Conflict,
                    CPushSessionStatus::WaitingManual => TraySessionStatus::WaitingManual,
                    CPushSessionStatus::PushFailed => TraySessionStatus::PushFailed,
                    CPushSessionStatus::Done => TraySessionStatus::Done,
                };
                if status != TraySessionStatus::Idle {
                    let detail = session
                        .message
                        .as_ref()
                        .map(|m| m.title.clone())
                        .filter(|s| !s.trim().is_empty());
                    out.insert(wt_id.clone(), TrayWorktreeSessionInfo { status, detail });
                }
            }
        }
        if let Ok(actions) = self.inner.active_action_tray.lock() {
            for (wt_id, info) in actions.iter() {
                let replace = out
                    .get(wt_id)
                    .is_none_or(|prev| info.status.urgency_rank() < prev.status.urgency_rank());
                if replace {
                    out.insert(wt_id.clone(), info.clone());
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
        let mem_updated = self.mark_cpush_dismissed_in_memory(request)?;
        if let Ok(mut map) = self.inner.active_action_tray.lock() {
            map.remove(&request.target.worktree_id);
        }
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

    fn mark_cpush_dismissed_in_memory(
        &self,
        request: &DismissSessionDoneRequest,
    ) -> Result<bool, AppError> {
        let mut memory = self
            .inner
            .cpush_sessions
            .lock()
            .map_err(|_| AppError::internal("C-Push session cache lock is unavailable"))?;
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
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::message_user::{MessageBlock, MessageFindingItem, MessageFindingSeverity};
    use crate::actions::session_store::{PersistedActionSession, now_millis, upsert_session};

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
                CustomActionLink {
                    label: "PR #42".into(),
                    url: "https://github.com/acme/picket/pull/42".into(),
                },
                CustomActionLink {
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
            PersistedActionSession {
                run_id: "run-card-1".into(),
                repository_id: "repo-1".into(),
                worktree_id: "wt-1".into(),
                action_id: "act-dcr".into(),
                action_label: "Review Code".into(),
                action_prompt_hash: "hash-1".into(),
                status: "done".into(),
                updated_at_ms: now_millis(),
                summary: "Review complete".into(),
                card: Some(card.clone()),
                dismissed_message_id: None,
                timeline: vec![],
                commits: vec![],
                links: card.links.clone().unwrap_or_default(),
                edited_file_hashes: Default::default(),
                submit_ledger: Default::default(),
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
