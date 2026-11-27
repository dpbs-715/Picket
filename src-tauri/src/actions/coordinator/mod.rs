//! Unified Agent Action & C-Push coordinator:
//! manages same-repo write exclusion, real-time bidirectional `message_user` cards & oneshot
//! Q&A round-trips, unified C-Push / Pull-Rebase / tool-driven conflict resolution,
//! Rust-verified `install_platform_cli` grants, 3-tier controlled CLI policy enforcement,
//! and direct reuse of `RepositoryStore::commit` / `push_branch` / `sync_rebase_onto_upstream`.

mod conflict_flow;
mod context;
mod cpush_flow;
pub(super) mod cpush_helpers;
mod generator;
mod git_sync;
mod reconcile;
mod runner;
mod tools_cli;
mod tools_command;
mod tools_git_ops;
mod tools_submit;
mod tools_workspace;
#[cfg(test)]
mod workspace_tests;

use super::{
    session_store::{self, ActionSessionSnapshot},
    types::*,
};
use crate::{
    bounded_cache::BoundedMap,
    i18n::{self, LanguageKind},
    repo_lock::{RepoWriteCoordinator, RepoWriteKind, RepoWriteLease},
    repository::{
        self,
        types::{ReadTarget, RepositorySnapshot},
    },
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

pub(super) const CUSTOM_ACTION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub(super) const CPUSH_AI_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_PROCESSED_CPUSH_REQUESTS: usize = 128;

#[derive(Default)]
pub(super) struct SessionAuthGrants {
    pub(super) authorized_installs: HashSet<String>,
}

struct PendingQuestion {
    sender: oneshot::Sender<MessageUserResponse>,
    grants: Arc<Mutex<SessionAuthGrants>>,
    install_options: HashMap<String, String>,
}

pub(super) struct CoordinatorInner {
    pub(super) write_coordinator: RepoWriteCoordinator,
    active_operations: AtomicUsize,
    pending_questions: Mutex<HashMap<(String, String), PendingQuestion>>,
    override_cli_dirs: Mutex<Option<Vec<PathBuf>>>,
    pub(super) seq: AtomicU64,
    worktree_gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) cpush_sessions: Mutex<HashMap<String, StoredCPushSession>>,
    pub(super) active_action_tray: Mutex<HashMap<String, crate::tray::TrayWorktreeSessionInfo>>,
    processed_cpush_requests: Mutex<BoundedMap<String, CPushCommandReply>>,
}

impl Default for CoordinatorInner {
    fn default() -> Self {
        Self::with_write_coordinator(RepoWriteCoordinator::default())
    }
}

impl CoordinatorInner {
    fn with_write_coordinator(write_coordinator: RepoWriteCoordinator) -> Self {
        Self {
            write_coordinator,
            active_operations: AtomicUsize::default(),
            pending_questions: Mutex::default(),
            override_cli_dirs: Mutex::default(),
            seq: AtomicU64::default(),
            worktree_gates: Mutex::default(),
            cpush_sessions: Mutex::default(),
            active_action_tray: Mutex::default(),
            processed_cpush_requests: Mutex::new(BoundedMap::with_capacity(
                MAX_PROCESSED_CPUSH_REQUESTS,
            )),
        }
    }
}

#[derive(Clone, Default)]
pub struct CustomActionCoordinator {
    pub(super) inner: Arc<CoordinatorInner>,
}

pub(crate) struct ActiveOperationGuard {
    inner: Arc<CoordinatorInner>,
}

impl Drop for ActiveOperationGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) struct ActiveWriteRepoGuard {
    inner: Arc<CoordinatorInner>,
    _repo_lease: RepoWriteLease,
}

impl Drop for ActiveWriteRepoGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) struct PendingQuestionWaiter {
    rx: oneshot::Receiver<MessageUserResponse>,
    _cleanup: PendingQuestionGuard,
}

impl PendingQuestionWaiter {
    pub(super) async fn wait(self) -> Result<MessageUserResponse, String> {
        let Self { rx, _cleanup } = self;
        rx.await
            .map_err(|_| "Interactive question channel closed before a response arrived.".into())
    }
}

struct PendingQuestionGuard {
    inner: Arc<CoordinatorInner>,
    key: (String, String),
}

impl Drop for PendingQuestionGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.inner.pending_questions.lock() {
            map.remove(&self.key);
        }
    }
}

fn detect_install_option_tool(option: &MessageQuestionOption) -> Option<&'static str> {
    let id_lower = option.id.to_ascii_lowercase();
    let preview_lower = option.preview.as_deref().unwrap_or("").to_ascii_lowercase();
    let label_lower = option.label.to_ascii_lowercase();
    let zh_install = i18n::actions::action_install_label_keyword_zh();
    if id_lower.contains("install-gh")
        || id_lower == "install_gh"
        || preview_lower.contains("brew install gh")
        || (label_lower.contains("install")
            && label_lower.contains("gh")
            && !label_lower.contains("glab"))
        || (option.label.contains(zh_install)
            && label_lower.contains("gh")
            && !label_lower.contains("glab"))
    {
        return Some("gh");
    }
    if id_lower.contains("install-glab")
        || id_lower == "install_glab"
        || preview_lower.contains("brew install glab")
        || (label_lower.contains("install") && label_lower.contains("glab"))
        || (option.label.contains(zh_install) && label_lower.contains("glab"))
    {
        return Some("glab");
    }
    None
}

impl CustomActionCoordinator {
    pub fn with_write_coordinator(write_coordinator: RepoWriteCoordinator) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner::with_write_coordinator(write_coordinator)),
        }
    }

    pub fn has_active_operation(&self) -> bool {
        self.inner.active_operations.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn track_operation(&self) -> ActiveOperationGuard {
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        ActiveOperationGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    pub(super) fn worktree_gate(
        &self,
        worktree_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
        Ok(self
            .inner
            .worktree_gates
            .lock()
            .map_err(|_| "Session coordinator lock is unavailable")?
            .entry(worktree_id.to_owned())
            .or_default()
            .clone())
    }

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
    ) -> Result<(), String> {
        let existing_active = self
            .inner
            .cpush_sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(worktree_id)
            .cloned();
        if let Some(active) = existing_active
            && cpush_helpers::is_active_status(active.session_status)
        {
            return Err(i18n::cpush::cpush_active_session_busy_error(locale, for_cpush).into());
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

    pub(super) fn override_cli_dirs(&self) -> Option<Vec<PathBuf>> {
        self.inner
            .override_cli_dirs
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    #[cfg(test)]
    pub(crate) fn set_override_cli_dirs(&self, dirs: Option<Vec<PathBuf>>) {
        if let Ok(mut guard) = self.inner.override_cli_dirs.lock() {
            *guard = dirs;
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
    ) -> Result<bool, String> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
        self.mark_cpush_dismissed_in_memory(request)?;
        session_store::dismiss_session_message(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            request.run_id.as_deref(),
            &request.message_id,
        )
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
    ) -> Result<bool, String> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
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
        .await?;
        Ok(mem_updated || disk_updated)
    }

    fn mark_cpush_dismissed_in_memory(
        &self,
        request: &DismissSessionDoneRequest,
    ) -> Result<bool, String> {
        let mut memory = self
            .inner
            .cpush_sessions
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
        Ok(true)
    }

    pub(super) fn acquire_repo_write(
        &self,
        repository_id: &str,
        locale: LanguageKind,
    ) -> Result<ActiveWriteRepoGuard, String> {
        let lease = self.inner.write_coordinator.try_acquire(
            repository_id,
            RepoWriteKind::CustomAction,
            locale,
        )?;
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        Ok(ActiveWriteRepoGuard {
            inner: Arc::clone(&self.inner),
            _repo_lease: lease,
        })
    }

    pub fn respond_question(&self, request: &RespondCustomActionRequest) -> Result<bool, String> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
        let key = (request.run_id.clone(), request.call_id.clone());
        let entry = {
            let mut map = self
                .inner
                .pending_questions
                .lock()
                .map_err(|_| "Question registry is unavailable")?;
            map.remove(&key)
        };
        let Some(pending) = entry else {
            return Err(i18n::actions::action_question_inactive_error(request.locale).into());
        };

        // Hard-record any sensitive install authorization based strictly on the real optionId clicked by the user.
        if !request.response.cancelled
            && let Ok(mut grants) = pending.grants.lock()
        {
            for answer in &request.response.answers {
                for opt_id in &answer.selected_option_ids {
                    if let Some(tool) = pending.install_options.get(opt_id) {
                        grants.authorized_installs.insert(tool.clone());
                    }
                }
            }
        }

        pending
            .sender
            .send(request.response.clone())
            .map_err(|_| i18n::actions::action_question_settled_error(request.locale).to_owned())?;
        Ok(true)
    }

    /// Registers a pending question synchronously BEFORE emitting the UI progress event,
    /// eliminating any race where a fast user/test reply calls `respond_question` before the
    /// coordinator has inserted the oneshot sender.
    pub(super) fn register_pending_question(
        &self,
        run_id: &str,
        call_id: &str,
        request: &MessageUserRequest,
        grants: Arc<Mutex<SessionAuthGrants>>,
    ) -> Result<PendingQuestionWaiter, String> {
        let mut install_options = HashMap::new();
        for question in request.questions_slice() {
            for option in &question.options {
                if let Some(tool) = detect_install_option_tool(option) {
                    install_options.insert(option.id.clone(), tool.to_owned());
                }
            }
        }
        let (tx, rx) = oneshot::channel();
        let key = (run_id.to_owned(), call_id.to_owned());
        {
            let mut map = self
                .inner
                .pending_questions
                .lock()
                .map_err(|_| "Question registry is unavailable")?;
            map.insert(
                key.clone(),
                PendingQuestion {
                    sender: tx,
                    grants,
                    install_options,
                },
            );
        }
        Ok(PendingQuestionWaiter {
            rx,
            _cleanup: PendingQuestionGuard {
                inner: Arc::clone(&self.inner),
                key,
            },
        })
    }

    #[cfg(test)]
    pub(super) async fn suspend_for_question(
        &self,
        run_id: &str,
        call_id: &str,
        request: &MessageUserRequest,
        grants: Arc<Mutex<SessionAuthGrants>>,
    ) -> Result<MessageUserResponse, String> {
        self.register_pending_question(run_id, call_id, request, grants)?
            .wait()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn message_user_oneshot_records_install_grant_only_when_user_approves() {
        let coordinator = CustomActionCoordinator::default();
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let req = MessageUserRequest {
            title: Some("Install gh".into()),
            subtitle: None,
            tone: Some(MessageTone::Warning),
            blocks: None,
            links: None,
            questions: Some(vec![MessageQuestionItem {
                id: "q-1".into(),
                header: "Install gh".into(),
                question: "GitHub CLI is missing. Install via Homebrew?".into(),
                multi_select: None,
                options: vec![
                    MessageQuestionOption {
                        id: "install-gh".into(),
                        label: "Install gh via Homebrew".into(),
                        description: "Runs brew install gh".into(),
                        recommended: Some(true),
                        preview: Some("brew install gh".into()),
                    },
                    MessageQuestionOption {
                        id: "skip".into(),
                        label: "Skip".into(),
                        description: "Cancel installation".into(),
                        recommended: None,
                        preview: None,
                    },
                ],
            }]),
            context: None,
        };

        // 1. Declining/skipping must NOT record an install grant:
        let coord_clone = coordinator.clone();
        let grants_clone = Arc::clone(&grants);
        let req_clone = req.clone();
        let waiter = tokio::spawn(async move {
            coord_clone
                .suspend_for_question("run-1", "tc-1", &req_clone, grants_clone)
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        coordinator
            .respond_question(&RespondCustomActionRequest {
                request_id: "resp-1".into(),
                run_id: "run-1".into(),
                call_id: "tc-1".into(),
                response: MessageUserResponse {
                    cancelled: false,
                    answers: vec![MessageUserAnswer {
                        question_index: 0,
                        question_id: "q-1".into(),
                        header: "Install gh".into(),
                        selected_option_ids: vec!["skip".into()],
                        selected_labels: vec!["Skip".into()],
                        custom_text: None,
                    }],
                    unanswered: vec![],
                    global_note: None,
                    dont_ask_again: None,
                },
                locale: LanguageKind::En,
            })
            .unwrap();
        let resp = waiter.await.unwrap().unwrap();
        assert!(!resp.cancelled);
        assert!(!grants.lock().unwrap().authorized_installs.contains("gh"));

        // 2. Approving `install-gh` records the grant in Rust:
        let coord_clone = coordinator.clone();
        let grants_clone = Arc::clone(&grants);
        let waiter2 = tokio::spawn(async move {
            coord_clone
                .suspend_for_question("run-1", "tc-2", &req, grants_clone)
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        coordinator
            .respond_question(&RespondCustomActionRequest {
                request_id: "resp-2".into(),
                run_id: "run-1".into(),
                call_id: "tc-2".into(),
                response: MessageUserResponse {
                    cancelled: false,
                    answers: vec![MessageUserAnswer {
                        question_index: 0,
                        question_id: "q-1".into(),
                        header: "Install gh".into(),
                        selected_option_ids: vec!["install-gh".into()],
                        selected_labels: vec!["Install gh via Homebrew".into()],
                        custom_text: None,
                    }],
                    unanswered: vec![],
                    global_note: None,
                    dont_ask_again: None,
                },
                locale: LanguageKind::En,
            })
            .unwrap();
        let resp2 = waiter2.await.unwrap().unwrap();
        assert!(!resp2.cancelled);
        assert!(grants.lock().unwrap().authorized_installs.contains("gh"));
        assert!(!grants.lock().unwrap().authorized_installs.contains("glab"));
    }

    #[test]
    fn message_user_card_validation_filters_unsafe_links_and_paths_and_restores_after_restart() {
        use crate::actions::message_user::{
            MessageBlock, MessageFindingItem, MessageFindingSeverity,
        };
        use crate::actions::session_store::{PersistedActionSession, now_millis, upsert_session};

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
