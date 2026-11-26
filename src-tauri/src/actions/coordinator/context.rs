//! Shared runtime state, tool execution context, Tier-2 confirmation helper, and timeline formatters.

use super::{CustomActionCoordinator, SessionAuthGrants};
use crate::{
    actions::{session_store::SubmitSessionLedger, types::*},
    cpush::types::CPushLandedCommit,
    foundation::pi::PiToolExecutionResult,
    i18n::{self, LanguageKind},
    repository::{
        RepositoryStore,
        types::{Change, CommitSelection, ReadTarget, RepositorySnapshot},
    },
    skills::RuntimeSkillSnapshot,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) type ProgressEmitFn = Arc<
    dyn Fn(&'static str, Option<String>, Option<CustomActionUiMessage>, Option<RepositorySnapshot>)
        + Send
        + Sync,
>;
pub(super) type PersistStateFn = Arc<dyn Fn(&str, String) + Send + Sync>;

#[derive(Default)]
pub(super) struct ActionRunState {
    pub(super) timeline: Vec<CustomActionTimelineItem>,
    pub(super) commits: Vec<CPushLandedCommit>,
    pub(super) links: Vec<CustomActionLink>,
    pub(super) committed_file_ids: HashSet<String>,
    pub(super) skip_write_confirm: bool,
    pub(super) trusted_action_update: Option<TrustedActionUpdate>,
    pub(super) wrote_repo: bool,
    pub(super) submit_ledger: SubmitSessionLedger,
}

pub(super) struct ActionToolContext {
    pub(super) coordinator: CustomActionCoordinator,
    pub(super) store: RepositoryStore,
    pub(super) emit: ProgressEmitFn,
    pub(super) persist: PersistStateFn,
    pub(super) run_state: Arc<Mutex<ActionRunState>>,
    pub(super) grants: Arc<Mutex<SessionAuthGrants>>,
    pub(super) skills: Arc<RuntimeSkillSnapshot>,
    pub(super) root: PathBuf,
    pub(super) storage_dir: PathBuf,
    pub(super) remotes: Vec<RemoteRepoIdentity>,
    pub(super) frozen_selections: Vec<CommitSelection>,
    pub(super) live_changes: Vec<Change>,
    pub(super) target: ReadTarget,
    pub(super) run_id: String,
    pub(super) action_id: String,
    pub(super) action_label: String,
    pub(super) expected_hash: String,
    pub(super) locale: LanguageKind,
}

impl ActionToolContext {
    pub(super) async fn confirm_tier2_write(
        &self,
        call_id: &str,
        reason: &str,
        preview: &str,
        allow_dont_ask_again: bool,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        let already_trusted = allow_dont_ask_again
            && self
                .run_state
                .lock()
                .map(|st| st.skip_write_confirm)
                .unwrap_or(false);
        if already_trusted {
            return Ok(());
        }
        let confirm_call_id = format!("confirm-{call_id}");
        let q = build_tier2_confirm_questionnaire(
            self.locale,
            reason,
            preview,
            allow_dont_ask_again,
            &self.action_label,
        );
        (self.emit)(
            "needsDecision",
            Some(confirm_call_id.clone()),
            Some(CustomActionUiMessage {
                id: format!("ask-{}-{confirm_call_id}", self.run_id),
                kind: "question".into(),
                title: q.questions[0].header.clone(),
                description: q.questions[0].question.clone(),
                scope: Some(self.action_label.clone()),
                files: None,
                commits: None,
                links: None,
                questionnaire: Some(q.clone()),
                progress: None,
            }),
            None,
        );
        match self
            .coordinator
            .suspend_for_question(&self.run_id, &confirm_call_id, &q, Arc::clone(&self.grants))
            .await
        {
            Ok(resp) => {
                let approved = !resp.cancelled
                    && resp.answers.iter().any(|ans| {
                        ans.selected_option_ids
                            .iter()
                            .any(|id| id == "approve-write")
                    });
                if !approved {
                    return Err(PiToolExecutionResult::error(declined_error));
                }
                if allow_dont_ask_again
                    && resp.dont_ask_again == Some(true)
                    && let Ok(mut st) = self.run_state.lock()
                {
                    st.skip_write_confirm = true;
                    st.trusted_action_update = Some(TrustedActionUpdate {
                        repository_id: self.target.repository_id.clone(),
                        action_id: self.action_id.clone(),
                        trusted_prompt_hash: self.expected_hash.clone(),
                    });
                }
                Ok(())
            }
            Err(err) => Err(PiToolExecutionResult::error(err)),
        }
    }
}

fn build_tier2_confirm_questionnaire(
    locale: LanguageKind,
    reason: &str,
    preview: &str,
    allow_dont_ask_again: bool,
    action_label: &str,
) -> AskUserQuestionRequest {
    AskUserQuestionRequest {
        questions: vec![AskUserQuestionItem {
            id: "confirm-write".into(),
            header: i18n::actions::action_tier2_header(locale).into(),
            question: i18n::actions::action_tier2_question(locale, reason),
            multi_select: None,
            options: vec![
                AskUserQuestionOption {
                    id: "approve-write".into(),
                    label: i18n::actions::action_tier2_approve_label(locale).into(),
                    description: i18n::actions::action_tier2_approve_desc(locale).into(),
                    recommended: Some(true),
                    preview: Some(preview.to_owned()),
                },
                AskUserQuestionOption {
                    id: "cancel-write".into(),
                    label: i18n::actions::action_tier2_cancel_label(locale).into(),
                    description: i18n::actions::action_tier2_cancel_desc(locale).into(),
                    recommended: None,
                    preview: None,
                },
            ],
        }],
        context: Some(AskUserQuestionContext {
            scope: Some(action_label.to_owned()),
            affected_file_ids: None,
            links: None,
            allow_dont_ask_again: allow_dont_ask_again.then_some(true),
            dont_ask_again_label: allow_dont_ask_again
                .then(|| i18n::actions::action_tier2_dont_ask_label(locale).into()),
        }),
    }
}

pub(super) fn push_timeline_step(
    timeline: &mut Vec<CustomActionTimelineItem>,
    state: &str,
    label: String,
) {
    if let Some(last) = timeline.last_mut()
        && last.state == "active"
        && state != "active"
    {
        last.state = state.to_owned();
        last.label = label;
        return;
    }
    if state == "active" {
        for item in timeline.iter_mut() {
            if item.state == "active" {
                item.state = "done".into();
            }
        }
    }
    if timeline.len() >= 12 {
        timeline.remove(0);
    }
    timeline.push(CustomActionTimelineItem {
        state: state.to_owned(),
        label,
    });
}

pub(super) fn append_unique_links(
    target: &mut Vec<CustomActionLink>,
    incoming: Vec<CustomActionLink>,
) {
    for link in incoming {
        if !target.iter().any(|existing| existing.url == link.url) {
            target.push(link);
        }
    }
}

pub(super) fn format_tool_status_label(
    locale: LanguageKind,
    tool_name: &str,
    args: &Value,
) -> String {
    match tool_name {
        "install_platform_cli" => {
            let tool = args.get("tool").and_then(Value::as_str).unwrap_or("CLI");
            i18n::actions::action_tool_status_label(locale, tool_name, tool)
        }
        "run_git" => {
            let sub = args
                .get("args")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .unwrap_or("command");
            i18n::actions::action_tool_status_label(locale, tool_name, sub)
        }
        "run_platform_cli" => {
            let cli = args.get("cli").and_then(Value::as_str).unwrap_or("cli");
            let sub = args
                .get("args")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            let detail = format!("{cli} {sub}");
            i18n::actions::action_tool_status_label(locale, tool_name, detail.trim())
        }
        "read_repo_file" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("file");
            i18n::actions::action_tool_status_label(locale, tool_name, path)
        }
        "manage_submit_worktree" => {
            let op = args.get("op").and_then(Value::as_str).unwrap_or("worktree");
            i18n::actions::action_tool_status_label(locale, tool_name, op)
        }
        _ => i18n::actions::action_tool_status_label(locale, tool_name, ""),
    }
}
