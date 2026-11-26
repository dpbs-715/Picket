//! Phase 2 Custom Action & `Create with Pi` coordinator:
//! manages same-repo write exclusion, real-time bidirectional `ask_user_question` oneshot
//! round-trips, Rust-verified `install_platform_cli` grants, 3-tier controlled CLI policy
//! enforcement with Tier-2 preview confirmation + "don't ask again" prompt-hash binding, and
//! direct reuse of `RepositoryStore::commit` / `push_branch` / `sync_rebase_onto_upstream`.

mod context;
mod generator;
mod runner;
mod tools_cli;
mod tools_git_ops;
mod tools_submit;

use super::{
    session_store::{self, ActionSessionSnapshot},
    types::*,
};
use crate::{
    i18n::{self, LanguageKind},
    repository,
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

#[derive(Default)]
pub(super) struct SessionAuthGrants {
    pub(super) authorized_installs: HashSet<String>,
}

struct PendingQuestion {
    sender: oneshot::Sender<AskUserQuestionResponse>,
    grants: Arc<Mutex<SessionAuthGrants>>,
    install_options: HashMap<String, String>,
}

#[derive(Default)]
pub(super) struct CoordinatorInner {
    active_operations: AtomicUsize,
    active_write_repos: Mutex<HashSet<String>>,
    pending_questions: Mutex<HashMap<(String, String), PendingQuestion>>,
    override_cli_dirs: Mutex<Option<Vec<PathBuf>>>,
    pub(super) seq: AtomicU64,
}

#[derive(Clone, Default)]
pub struct CustomActionCoordinator {
    pub(super) inner: Arc<CoordinatorInner>,
}

pub(super) struct ActiveWriteRepoGuard {
    inner: Arc<CoordinatorInner>,
    repository_id: String,
}

impl Drop for ActiveWriteRepoGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
        if let Ok(mut repos) = self.inner.active_write_repos.lock() {
            repos.remove(&self.repository_id);
        }
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

fn detect_install_option_tool(option: &AskUserQuestionOption) -> Option<&'static str> {
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
    pub fn has_active_operation(&self) -> bool {
        self.inner.active_operations.load(Ordering::SeqCst) > 0
    }

    pub fn has_active_operation_for_repo(&self, repository_id: &str) -> bool {
        self.inner
            .active_write_repos
            .lock()
            .map(|set| set.contains(repository_id))
            .unwrap_or(false)
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

    pub fn restore_sessions(
        &self,
        storage_dir: &Path,
        locale: LanguageKind,
    ) -> Vec<ActionSessionSnapshot> {
        session_store::reconcile_sessions_on_startup(storage_dir, locale)
    }

    pub fn dismiss_done_notice(
        &self,
        storage_dir: &Path,
        request: &DismissCustomActionDoneRequest,
    ) -> Result<bool, String> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
        session_store::dismiss_session_message(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            request.run_id.as_deref(),
            &request.message_id,
        )?;
        Ok(true)
    }

    pub(super) fn acquire_repo_write(
        &self,
        repository_id: &str,
        locale: LanguageKind,
    ) -> Result<ActiveWriteRepoGuard, String> {
        let mut repos = self
            .inner
            .active_write_repos
            .lock()
            .map_err(|_| "Custom action registry is unavailable")?;
        if repos.contains(repository_id) {
            return Err(i18n::actions::action_already_running_error(locale).into());
        }
        repos.insert(repository_id.to_owned());
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        Ok(ActiveWriteRepoGuard {
            inner: Arc::clone(&self.inner),
            repository_id: repository_id.to_owned(),
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

    pub(super) async fn suspend_for_question(
        &self,
        run_id: &str,
        call_id: &str,
        request: &AskUserQuestionRequest,
        grants: Arc<Mutex<SessionAuthGrants>>,
    ) -> Result<AskUserQuestionResponse, String> {
        let mut install_options = HashMap::new();
        for question in &request.questions {
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
        let _cleanup = PendingQuestionGuard {
            inner: Arc::clone(&self.inner),
            key,
        };
        rx.await
            .map_err(|_| "Interactive question channel closed before a response arrived.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ask_user_question_oneshot_records_install_grant_only_when_user_approves() {
        let coordinator = CustomActionCoordinator::default();
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let req = AskUserQuestionRequest {
            questions: vec![AskUserQuestionItem {
                id: "q-1".into(),
                header: "Install gh".into(),
                question: "GitHub CLI is missing. Install via Homebrew?".into(),
                multi_select: None,
                options: vec![
                    AskUserQuestionOption {
                        id: "install-gh".into(),
                        label: "Install gh via Homebrew".into(),
                        description: "Runs brew install gh".into(),
                        recommended: Some(true),
                        preview: Some("brew install gh".into()),
                    },
                    AskUserQuestionOption {
                        id: "skip".into(),
                        label: "Skip".into(),
                        description: "Cancel installation".into(),
                        recommended: None,
                        preview: None,
                    },
                ],
            }],
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
                response: AskUserQuestionResponse {
                    cancelled: false,
                    answers: vec![AskUserQuestionAnswer {
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
                response: AskUserQuestionResponse {
                    cancelled: false,
                    answers: vec![AskUserQuestionAnswer {
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
}
