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
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) type ProgressEmitFn = Arc<
    dyn Fn(&'static str, Option<String>, Option<CustomActionUiMessage>, Option<RepositorySnapshot>)
        + Send
        + Sync,
>;
pub(super) type PersistStateFn = Arc<dyn Fn(&str, String) + Send + Sync>;

#[derive(Clone, Copy)]
pub(super) enum Tier2ConfirmKind {
    Command,
    FileEdit,
    FileCreate,
}

#[derive(Default)]
pub(super) struct ActionRunState {
    pub(super) timeline: Vec<CustomActionTimelineItem>,
    pub(super) commits: Vec<CPushLandedCommit>,
    pub(super) links: Vec<CustomActionLink>,
    pub(super) published_card: Option<MessageUserCard>,
    pub(super) committed_file_ids: HashSet<String>,
    pub(super) action_edited_hashes: HashMap<String, String>,
    pub(super) committed_edited_paths: HashSet<String>,
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
    pub(super) bundled_node_path: PathBuf,
    pub(super) remotes: Vec<RemoteRepoIdentity>,
    pub(super) frozen_selections: Vec<CommitSelection>,
    pub(super) live_changes: Vec<Change>,
    pub(super) local_keep_paths: HashSet<String>,
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
        self.confirm_tier2_operation(
            Tier2ConfirmKind::Command,
            call_id,
            reason,
            preview,
            allow_dont_ask_again,
            declined_error,
        )
        .await
    }

    pub(super) async fn confirm_tier2_edit(
        &self,
        call_id: &str,
        reason: &str,
        preview: &str,
        allow_dont_ask_again: bool,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        self.confirm_tier2_operation(
            Tier2ConfirmKind::FileEdit,
            call_id,
            reason,
            preview,
            allow_dont_ask_again,
            declined_error,
        )
        .await
    }

    pub(super) async fn confirm_tier2_create(
        &self,
        call_id: &str,
        reason: &str,
        preview: &str,
        allow_dont_ask_again: bool,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        self.confirm_tier2_operation(
            Tier2ConfirmKind::FileCreate,
            call_id,
            reason,
            preview,
            allow_dont_ask_again,
            declined_error,
        )
        .await
    }

    async fn confirm_tier2_operation(
        &self,
        kind: Tier2ConfirmKind,
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
            kind,
            reason,
            preview,
            allow_dont_ask_again,
            &self.action_label,
        );
        let first_q = &q.questions_slice()[0];
        let card = q.to_card(&first_q.header);
        (self.emit)(
            "needsDecision",
            Some(confirm_call_id.clone()),
            Some(CustomActionUiMessage {
                id: format!("ask-{}-{confirm_call_id}", self.run_id),
                kind: "question".into(),
                title: first_q.header.clone(),
                description: first_q.question.clone(),
                scope: Some(self.action_label.clone()),
                files: None,
                commits: None,
                links: None,
                card: Some(card),
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
    kind: Tier2ConfirmKind,
    reason: &str,
    preview: &str,
    allow_dont_ask_again: bool,
    action_label: &str,
) -> MessageUserRequest {
    let (header, question, approve_label, approve_desc) = match kind {
        Tier2ConfirmKind::Command => (
            i18n::actions::action_tier2_header(locale),
            i18n::actions::action_tier2_question(locale, reason),
            i18n::actions::action_tier2_approve_label(locale),
            i18n::actions::action_tier2_approve_desc(locale),
        ),
        Tier2ConfirmKind::FileEdit => (
            i18n::actions::action_tier2_edit_header(locale),
            i18n::actions::action_tier2_edit_question(locale, reason),
            i18n::actions::action_tier2_edit_approve_label(locale),
            i18n::actions::action_tier2_edit_approve_desc(locale),
        ),
        Tier2ConfirmKind::FileCreate => (
            i18n::actions::action_tier2_create_header(locale),
            i18n::actions::action_tier2_create_question(locale, reason),
            i18n::actions::action_tier2_create_approve_label(locale),
            i18n::actions::action_tier2_create_approve_desc(locale),
        ),
    };
    MessageUserRequest {
        title: Some(header.into()),
        subtitle: Some(action_label.to_owned()),
        tone: Some(MessageTone::Warning),
        blocks: None,
        links: None,
        questions: Some(vec![MessageQuestionItem {
            id: "confirm-write".into(),
            header: header.into(),
            question,
            multi_select: None,
            options: vec![
                MessageQuestionOption {
                    id: "approve-write".into(),
                    label: approve_label.into(),
                    description: approve_desc.into(),
                    recommended: Some(true),
                    preview: Some(preview.to_owned()),
                },
                MessageQuestionOption {
                    id: "cancel-write".into(),
                    label: i18n::actions::action_tier2_cancel_label(locale).into(),
                    description: i18n::actions::action_tier2_cancel_desc(locale).into(),
                    recommended: None,
                    preview: None,
                },
            ],
        }]),
        context: Some(MessageQuestionContext {
            scope: Some(action_label.to_owned()),
            affected_file_ids: None,
            links: None,
            allow_dont_ask_again: allow_dont_ask_again.then_some(true),
            dont_ask_again_label: allow_dont_ask_again
                .then(|| i18n::actions::action_tier2_dont_ask_label(locale).into()),
        }),
    }
}

const TIMELINE_MAX_ITEMS: usize = 60;
const TIMELINE_DETAIL_MAX_CHARS: usize = 48;

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
    if timeline.len() >= TIMELINE_MAX_ITEMS {
        let excess = timeline.len() + 1 - TIMELINE_MAX_ITEMS;
        timeline.drain(..excess);
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

fn truncate_timeline_text(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{collected}…")
    } else {
        collected
    }
}

fn is_short_identifier_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 24
        && !token.starts_with('-')
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '#' | '-' | '_' | '.' | '/' | ':' | '*')
        })
}

fn format_git_detail(raw_args: &[&str]) -> String {
    let mut idx = 0;
    while idx < raw_args.len() {
        let tok = raw_args[idx];
        if matches!(tok, "-C" | "-c" | "--git-dir" | "--work-tree") {
            idx += 2;
        } else if tok.starts_with('-') {
            idx += 1;
        } else {
            break;
        }
    }
    let Some(&sub) = raw_args.get(idx) else {
        return "command".into();
    };
    let mut parts: Vec<String> = vec![sub.to_owned()];
    let mut skip_next = false;
    let mut target_tokens: Vec<String> = Vec::new();
    let mut hint_flag: Option<String> = None;

    for &arg in raw_args.iter().skip(idx + 1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(
            arg,
            "-m" | "--message"
                | "-F"
                | "--file"
                | "-c"
                | "--format"
                | "--pretty"
                | "-S"
                | "-G"
                | "--grep"
        ) {
            skip_next = true;
            continue;
        }
        if arg == "--" {
            continue;
        }
        if arg == "--help" || arg == "-h" {
            hint_flag = Some("--help".into());
            break;
        }
        if arg == "-n" {
            skip_next = false;
            hint_flag = hint_flag.or_else(|| Some("-n".into()));
            continue;
        }
        if arg.starts_with('-') {
            if hint_flag.is_none()
                && matches!(
                    arg,
                    "--cached" | "--staged" | "--stat" | "--name-only" | "--oneline"
                )
            {
                hint_flag = Some(arg.to_owned());
            }
            continue;
        }
        if hint_flag.as_deref() == Some("-n")
            && target_tokens.is_empty()
            && arg.chars().all(|c| c.is_ascii_digit())
        {
            hint_flag = Some(format!("-n {arg}"));
            continue;
        }
        let compact = if arg.contains('/')
            && arg
                .rsplit('/')
                .next()
                .is_some_and(|leaf| leaf.contains('.'))
        {
            arg.rsplit('/').next().unwrap_or(arg)
        } else {
            arg
        };
        if !compact.is_empty() && target_tokens.len() < 2 {
            target_tokens.push(truncate_timeline_text(compact, 24));
        }
    }

    if let Some(flag) = hint_flag
        && (target_tokens.is_empty()
            || flag == "--help"
            || matches!(flag.as_str(), "--cached" | "--staged"))
    {
        parts.push(flag);
    }
    parts.extend(target_tokens);
    truncate_timeline_text(&parts.join(" "), TIMELINE_DETAIL_MAX_CHARS)
}

fn format_platform_cli_detail(cli: &str, raw_args: &[&str]) -> String {
    let mut parts: Vec<String> = vec![cli.to_owned()];
    if let Some(&first) = raw_args.first() {
        parts.push(truncate_timeline_text(first, 20));
        if let Some(&second) = raw_args.get(1) {
            if second == "--help" || second == "-h" {
                parts.push("--help".into());
            } else if !second.starts_with('-') {
                parts.push(truncate_timeline_text(second, 24));
                if let Some(&third) = raw_args.get(2)
                    && is_short_identifier_token(third)
                {
                    parts.push(truncate_timeline_text(third, 18));
                }
            }
        }
    }
    truncate_timeline_text(&parts.join(" "), TIMELINE_DETAIL_MAX_CHARS)
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
            let arg_list: Vec<&str> = args
                .get("args")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let detail = format_git_detail(&arg_list);
            i18n::actions::action_tool_status_label(locale, tool_name, &detail)
        }
        "run_platform_cli" => {
            let cli = args.get("cli").and_then(Value::as_str).unwrap_or("cli");
            let arg_list: Vec<&str> = args
                .get("args")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let detail = format_platform_cli_detail(cli, &arg_list);
            i18n::actions::action_tool_status_label(locale, tool_name, detail.trim())
        }
        "read_repo_file" | "edit_repo_file" | "write_repo_file" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("file");
            i18n::actions::action_tool_status_label(locale, tool_name, path)
        }
        "run_repo_command" => {
            let cmd = args
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("command");
            let first_arg = args
                .get("args")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            let detail = format!("{cmd} {first_arg}");
            i18n::actions::action_tool_status_label(locale, tool_name, detail.trim())
        }
        "manage_submit_worktree" => {
            let op = args.get("op").and_then(Value::as_str).unwrap_or("worktree");
            i18n::actions::action_tool_status_label(locale, tool_name, op)
        }
        _ => i18n::actions::action_tool_status_label(locale, tool_name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_second_level_platform_cli_and_git_subcommands() {
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_platform_cli",
                &json!({"cli": "glab", "args": ["issue", "create", "--title", "fix"]})
            ),
            "执行 glab issue create"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_platform_cli",
                &json!({"cli": "glab", "args": ["issue", "view", "220"]})
            ),
            "执行 glab issue view 220"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_platform_cli",
                &json!({"cli": "glab", "args": ["mr", "merge", "42", "--sha", "abcdef"]})
            ),
            "执行 glab mr merge 42"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_platform_cli",
                &json!({"cli": "glab", "args": ["issue", "--help"]})
            ),
            "执行 glab issue --help"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_git",
                &json!({"args": ["log", "-n", "5", "--oneline"]})
            ),
            "执行 git log -n 5"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_git",
                &json!({"args": ["push", "origin", "220-fix-report"]})
            ),
            "执行 git push origin 220-fix-report"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_git",
                &json!({"args": ["diff", "--cached", "--", "src/main/java/FooService.java"]})
            ),
            "执行 git diff --cached FooService.java"
        );
        assert_eq!(
            format_tool_status_label(
                LanguageKind::Zh,
                "run_git",
                &json!({"args": ["tag", "-a", "v0.2.0", "-m", "secret message body"]})
            ),
            "执行 git tag v0.2.0"
        );
    }

    #[test]
    fn retains_up_to_sixty_timeline_steps_and_updates_active_in_place() {
        let mut timeline = Vec::new();
        for idx in 1..=65 {
            push_timeline_step(&mut timeline, "active", format!("step {idx}"));
            push_timeline_step(&mut timeline, "done", format!("step {idx}"));
        }
        assert_eq!(timeline.len(), TIMELINE_MAX_ITEMS);
        assert_eq!(timeline.first().unwrap().label, "step 6");
        assert_eq!(timeline.last().unwrap().label, "step 65");
        assert_eq!(timeline.last().unwrap().state, "done");
    }
}
