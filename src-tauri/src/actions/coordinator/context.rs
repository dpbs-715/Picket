//! Unified runtime state, host-verified recovery handles, tool execution context,
//! Tier-2 confirmation helper, and timeline formatters shared across all action profiles.

use super::{CustomActionCoordinator, SessionAuthGrants};
use crate::{
    actions::{profile::ExecutionProfile, session_store::SubmitSessionLedger, types::*},
    i18n::{self, LanguageKind},
    pi_runtime::PiToolExecutionResult,
    repository::{
        RepositoryStore,
        ai_context::FrozenCPushFile,
        remote::ConflictedFileSummary,
        types::{Change, CommitSelection, ReadTarget, RepositorySnapshot},
    },
    skills::RuntimeSkillSnapshot,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) type ProgressEmitFn = Arc<
    dyn Fn(&'static str, Option<String>, Option<WorkflowUiMessage>, Option<RepositorySnapshot>)
        + Send
        + Sync,
>;

pub(super) type RawEventEmitFn = Arc<dyn Fn(ActionProgressEvent) + Send + Sync>;

#[derive(Clone, Copy)]
pub(super) enum Tier2ConfirmKind {
    Command,
    FileEdit,
    FileCreate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tier2RiskLevel {
    /// Routine in-scope repository edits, local commands, and standard Git/platform CLI writes.
    /// Prompted in `Ask`, auto-allowed in `Auto` and `AllowAll`.
    RoutineWrite,
    /// High-risk or unconventional Git/platform CLI writes (`RequireConfirmTier2`).
    /// Prompted in `Ask` and `Auto`, auto-allowed in `AllowAll`.
    HighRiskWrite,
}

/// Host-verified recovery snapshot issued ONLY when a real Git sync/push/rebase boundary occurs
/// under `RecoveryHandoffPolicy::AgentFirst`. Validated on `handoff_to_recovery({ handleId })`
/// so models can never fabricate fake recovery states.
#[derive(Clone, Debug)]
pub(super) enum RecoveryHandlePayload {
    PushFailed {
        remote_target: String,
        reason: String,
        title_override: Option<String>,
    },
    MissingUpstream {
        remotes: Vec<String>,
    },
    Conflict {
        upstream: String,
        conflicted_files: Vec<ConflictedFileSummary>,
    },
}

#[derive(Default)]
pub(super) struct ActionRunState {
    pub(super) status: ActionSessionStatus,
    pub(super) pending_call_id: Option<String>,
    pub(super) message: Option<WorkflowUiMessage>,
    pub(super) summary: String,
    pub(super) timeline: Vec<WorkflowTimelineItem>,
    pub(super) qa_log: Vec<CPushQaItem>,
    pub(super) commits: Vec<CPushLandedCommit>,
    pub(super) links: Vec<WorkflowLink>,
    pub(super) published_card: Option<MessageUserCard>,
    pub(super) frozen_files: Vec<FrozenCPushFile>,
    pub(super) planned_groups: Vec<CPushCommitGroup>,
    pub(super) skipped_file_ids: Vec<String>,
    pub(super) next_step_index: usize,
    pub(super) committed_file_ids: HashSet<String>,
    pub(super) committed_file_order: Vec<String>,
    pub(super) action_edited_hashes: HashMap<String, String>,
    pub(super) committed_edited_paths: HashSet<String>,
    pub(super) trusted_action_update: Option<TrustedActionUpdate>,
    pub(super) wrote_repo: bool,
    pub(super) submit_ledger: SubmitSessionLedger,
    pub(super) branch: String,
    pub(super) initial_head: Option<String>,
    pub(super) prompt: String,
    pub(super) recent_commits: Vec<String>,
    pub(super) unselected_paths: Vec<String>,
    pub(super) model_id: String,
    pub(super) push_authorized: bool,
    pub(super) can_retry_remaining: bool,
    pub(super) auto_collapse: bool,
    pub(super) qa_confirmed: bool,
    pub(super) user_cancelled: bool,
    pub(super) settled_reply: Option<ActionCommandReply>,
    pub(super) recovery_handles: HashMap<String, RecoveryHandlePayload>,
    pub(super) next_recovery_seq: u32,
}

impl ActionRunState {
    pub(super) fn to_stored_session(&self, meta: &SessionPersistMeta<'_>) -> StoredActionSession {
        StoredActionSession {
            run_id: meta.run_id.to_owned(),
            repository_id: meta.repository_id.to_owned(),
            worktree_id: meta.worktree_id.to_owned(),
            session_id: meta.session_id.to_owned(),
            action_id: meta.action_id.to_owned(),
            action_label: meta.action_label.to_owned(),
            profile: meta.profile,
            action_prompt_hash: meta.action_prompt_hash.to_owned(),
            frozen_scope_version: meta.frozen_scope_version.to_owned(),
            model_id: self.model_id.clone(),
            branch: self.branch.clone(),
            initial_head: self.initial_head.clone(),
            prompt: self.prompt.clone(),
            recent_commits: self.recent_commits.clone(),
            unselected_paths: self.unselected_paths.clone(),
            frozen_files: self.frozen_files.clone(),
            planned_groups: self.planned_groups.clone(),
            skipped_file_ids: self.skipped_file_ids.clone(),
            next_step_index: self.next_step_index,
            committed_file_ids: self.committed_file_order.clone(),
            status: self.status,
            pending_call_id: self.pending_call_id.clone(),
            updated_at_ms: crate::actions::session_store::now_millis(),
            summary: self.summary.clone(),
            card: self.published_card.clone(),
            message: self.message.clone(),
            timeline: self.timeline.clone(),
            qa_log: self.qa_log.clone(),
            commits: self.commits.clone(),
            links: self.links.clone(),
            can_retry_remaining: self.can_retry_remaining,
            push_authorized: self.push_authorized,
            auto_collapse: self.auto_collapse,
            notice_dismissed: false,
            dismissed_message_id: None,
            edited_file_hashes: self
                .action_edited_hashes
                .iter()
                .filter(|(path, _)| !self.committed_edited_paths.contains(*path))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            submit_ledger: self.submit_ledger.clone(),
            locale: meta.locale,
            commit_language: meta.commit_language,
        }
    }

    pub(super) fn sync_from_stored(&mut self, session: &StoredActionSession) {
        self.status = session.status;
        self.pending_call_id = session.pending_call_id.clone();
        self.message = session.message.clone();
        self.summary = session.summary.clone();
        self.timeline = session.timeline.clone();
        self.qa_log = session.qa_log.clone();
        self.commits = session.commits.clone();
        self.links = session.links.clone();
        self.planned_groups = session.planned_groups.clone();
        self.skipped_file_ids = session.skipped_file_ids.clone();
        self.next_step_index = session.next_step_index;
        self.committed_file_order = session.committed_file_ids.clone();
        self.committed_file_ids = session.committed_file_ids.iter().cloned().collect();
        self.can_retry_remaining = session.can_retry_remaining;
        self.push_authorized = session.push_authorized;
        self.auto_collapse = session.auto_collapse;
    }

    pub(super) fn record_committed_file_id(&mut self, file_id: String) {
        if self.committed_file_ids.insert(file_id.clone()) {
            self.committed_file_order.push(file_id);
        }
    }
}

pub(super) struct ActionToolContext {
    pub(super) coordinator: CustomActionCoordinator,
    pub(super) store: RepositoryStore,
    pub(super) profile: ExecutionProfile,
    pub(super) emit: ProgressEmitFn,
    pub(super) raw_emit: RawEventEmitFn,
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
    pub(super) request_id: String,
    pub(super) run_id: String,
    pub(super) action_id: String,
    pub(super) action_label: String,
    pub(super) expected_hash: String,
    pub(super) locale: LanguageKind,
    pub(super) commit_language: LanguageKind,
    pub(super) approval_mode: ActionApprovalMode,
}

pub(super) struct SessionPersistMeta<'a> {
    pub(super) run_id: &'a str,
    pub(super) repository_id: &'a str,
    pub(super) worktree_id: &'a str,
    pub(super) session_id: &'a str,
    pub(super) action_id: &'a str,
    pub(super) action_label: &'a str,
    pub(super) profile: ActionProfileKind,
    pub(super) action_prompt_hash: &'a str,
    pub(super) frozen_scope_version: &'a str,
    pub(super) locale: LanguageKind,
    pub(super) commit_language: LanguageKind,
}

pub(super) fn parse_status_str(status: &str) -> ActionSessionStatus {
    match status {
        "running" => ActionSessionStatus::Running,
        "needsDecision" => ActionSessionStatus::NeedsDecision,
        "conflict" => ActionSessionStatus::Conflict,
        "waitingManual" => ActionSessionStatus::WaitingManual,
        "pushFailed" => ActionSessionStatus::PushFailed,
        "interrupted" => ActionSessionStatus::Interrupted,
        "done" => ActionSessionStatus::Done,
        "cancelled" => ActionSessionStatus::Cancelled,
        "failed" => ActionSessionStatus::Failed,
        _ => ActionSessionStatus::Idle,
    }
}

impl ActionToolContext {
    pub(super) fn persist_meta(&self) -> SessionPersistMeta<'_> {
        SessionPersistMeta {
            run_id: &self.run_id,
            repository_id: &self.target.repository_id,
            worktree_id: &self.target.worktree_id,
            session_id: &self.target.session_id,
            action_id: &self.action_id,
            action_label: &self.action_label,
            profile: self.profile.kind,
            action_prompt_hash: &self.expected_hash,
            frozen_scope_version: &self.target.scope_version,
            locale: self.locale,
            commit_language: self.commit_language,
        }
    }

    pub(super) async fn persist(&self, status: &str, summary: String) {
        let parsed = parse_status_str(status);
        let record = {
            let meta = self.persist_meta();
            let Ok(mut st) = self.run_state.lock() else {
                return;
            };
            st.status = parsed;
            if !summary.is_empty() {
                st.summary = summary;
            }
            st.to_stored_session(&meta)
        };
        let _ = self.coordinator.save_session_in_memory(&record);
        let _ =
            crate::actions::session_store::upsert_session_async(&self.storage_dir, record).await;
    }

    pub(super) fn register_recovery_handle(&self, payload: RecoveryHandlePayload) -> Value {
        let (handle_id, kind, reason, actions): (String, &str, String, Vec<&str>) = {
            let Ok(mut st) = self.run_state.lock() else {
                return json!(null);
            };
            st.next_recovery_seq = st.next_recovery_seq.saturating_add(1);
            let id = format!("rh-{}", st.next_recovery_seq);
            let (kind, reason, actions) = match &payload {
                RecoveryHandlePayload::PushFailed { reason, .. } => (
                    "pushFailed",
                    reason.clone(),
                    vec!["retryPush", "dismissSession"],
                ),
                RecoveryHandlePayload::MissingUpstream { remotes, .. } => (
                    "missingUpstream",
                    format!("Branch has no upstream configured (remotes: {remotes:?})."),
                    vec!["retryPush", "setUpstreamFreeText", "dismissSession"],
                ),
                RecoveryHandlePayload::Conflict {
                    upstream,
                    conflicted_files,
                    ..
                } => (
                    "conflict",
                    format!(
                        "Rebase onto {upstream} hit conflicts in {} file(s).",
                        conflicted_files.len()
                    ),
                    vec!["agentConflictMerge", "manualMerge", "abortRebase"],
                ),
            };
            st.recovery_handles.insert(id.clone(), payload);
            (id, kind, reason, actions)
        };
        json!({
            "handleId": handle_id,
            "kind": kind,
            "reason": reason,
            "availableRecoveryActions": actions,
            "hint": "You may handle this yourself per the button instructions OR call handoff_to_recovery({ handleId }) to hand control to Picket's zero-token recovery UI."
        })
    }

    pub(super) async fn confirm_tier2_write(
        &self,
        call_id: &str,
        risk: Tier2RiskLevel,
        reason: &str,
        preview: &str,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        self.confirm_tier2_operation(
            Tier2ConfirmKind::Command,
            risk,
            call_id,
            reason,
            preview,
            declined_error,
        )
        .await
    }

    pub(super) async fn confirm_tier2_edit(
        &self,
        call_id: &str,
        risk: Tier2RiskLevel,
        reason: &str,
        preview: &str,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        self.confirm_tier2_operation(
            Tier2ConfirmKind::FileEdit,
            risk,
            call_id,
            reason,
            preview,
            declined_error,
        )
        .await
    }

    pub(super) async fn confirm_tier2_create(
        &self,
        call_id: &str,
        risk: Tier2RiskLevel,
        reason: &str,
        preview: &str,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        self.confirm_tier2_operation(
            Tier2ConfirmKind::FileCreate,
            risk,
            call_id,
            reason,
            preview,
            declined_error,
        )
        .await
    }

    async fn confirm_tier2_operation(
        &self,
        kind: Tier2ConfirmKind,
        risk: Tier2RiskLevel,
        call_id: &str,
        reason: &str,
        preview: &str,
        declined_error: &'static str,
    ) -> Result<(), PiToolExecutionResult> {
        match (self.approval_mode, risk) {
            (ActionApprovalMode::AllowAll, _) => return Ok(()),
            (ActionApprovalMode::Auto, Tier2RiskLevel::RoutineWrite) => return Ok(()),
            (ActionApprovalMode::Auto, Tier2RiskLevel::HighRiskWrite)
            | (ActionApprovalMode::Ask, _) => {}
        }
        let confirm_call_id = format!("confirm-{call_id}");
        let q = build_tier2_confirm_questionnaire(
            self.locale,
            kind,
            reason,
            preview,
            &self.action_label,
        );
        let first_q = &q.questions_slice()[0];
        let card = q.to_card(&first_q.header);
        let waiter = self
            .coordinator
            .register_pending_question(&self.run_id, &confirm_call_id, &q, Arc::clone(&self.grants))
            .map_err(PiToolExecutionResult::error)?;
        (self.emit)(
            "needsDecision",
            Some(confirm_call_id.clone()),
            Some(WorkflowUiMessage {
                id: format!("ask-{}-{confirm_call_id}", self.run_id),
                kind: "question".into(),
                title: first_q.header.clone(),
                description: first_q.question.clone(),
                scope: Some(self.action_label.clone()),
                files: None,
                commits: None,
                options: None,
                links: None,
                card: Some(card),
                questionnaire: Some(q.clone()),
                progress: None,
            }),
            None,
        );
        match waiter.wait().await {
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
            allow_dont_ask_again: None,
            dont_ask_again_label: None,
        }),
    }
}

const TIMELINE_MAX_ITEMS: usize = 60;
const TIMELINE_DETAIL_MAX_CHARS: usize = 48;

pub(super) fn push_timeline_step(
    timeline: &mut Vec<WorkflowTimelineItem>,
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
    timeline.push(WorkflowTimelineItem {
        state: state.to_owned(),
        label,
    });
}

pub(super) fn append_unique_links(target: &mut Vec<WorkflowLink>, incoming: Vec<WorkflowLink>) {
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
        "read_repo_file" | "read_scope_diff" | "edit_repo_file" | "write_repo_file" => {
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
