//! Interactive `message_user` oneshot Q&A registry and Rust-verified CLI install grants.

use super::{CoordinatorInner, CustomActionCoordinator};
use crate::{
    actions::types::*,
    error::{AppError, AppErrorCode},
    i18n, repository,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

#[derive(Default)]
pub(in crate::actions) struct SessionAuthGrants {
    pub(in crate::actions) authorized_installs: HashSet<String>,
}

pub(super) struct PendingQuestion {
    sender: oneshot::Sender<MessageUserResponse>,
    grants: Arc<Mutex<SessionAuthGrants>>,
    install_options: HashMap<String, String>,
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
    pub fn respond_question(&self, request: &RespondCustomActionRequest) -> Result<bool, AppError> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)
            .map_err(|msg| AppError::action(AppErrorCode::ActionInvalidInput, msg))?;
        let key = (request.run_id.clone(), request.call_id.clone());
        let entry = {
            let mut map = self
                .inner
                .pending_questions
                .lock()
                .map_err(|_| AppError::internal("Question registry is unavailable"))?;
            map.remove(&key)
        };
        let Some(pending) = entry else {
            return Err(AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::actions::action_question_inactive_error(request.locale),
            ));
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

        pending.sender.send(request.response.clone()).map_err(|_| {
            AppError::action(
                AppErrorCode::ActionInvalidInput,
                i18n::actions::action_question_settled_error(request.locale),
            )
        })?;
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
    use crate::i18n::LanguageKind;
    use std::time::Duration;

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
}
