//! Shared live-run state, file-token resolution, user-response summarization,
//! and Rust-side sensitive-file safety questionnaire builders for C-Push.

use super::{CustomActionCoordinator, SessionAuthGrants, cpush_helpers::file_basename};
use crate::{
    actions::types::*,
    i18n::{self, LanguageKind},
    repository::{RepositoryStore, ai_context::FrozenCPushFile},
    skills::RuntimeSkillSnapshot,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) type SharedProgressCb =
    Arc<Mutex<Box<dyn FnMut(CPushProgressEvent) -> Result<(), String> + Send>>>;

pub(super) struct CPushLiveRunState {
    pub(super) session: StoredCPushSession,
    pub(super) qa_confirmed: bool,
    pub(super) user_cancelled: bool,
    pub(super) settled_reply: Option<CPushCommandReply>,
}

pub(super) struct CPushToolContext {
    pub(super) coordinator: CustomActionCoordinator,
    pub(super) store: RepositoryStore,
    pub(super) storage_dir: PathBuf,
    pub(super) skills: Arc<RuntimeSkillSnapshot>,
    pub(super) grants: Arc<Mutex<SessionAuthGrants>>,
    pub(super) run_state: Arc<Mutex<CPushLiveRunState>>,
    pub(super) progress_cb: SharedProgressCb,
    pub(super) request_id: String,
    pub(super) locale: LanguageKind,
}

pub(super) fn emit_progress_event(cb: &SharedProgressCb, event: CPushProgressEvent) {
    if let Ok(mut f) = cb.lock() {
        let _ = f(event);
    }
}

pub(super) fn resolve_cpush_file_token<'a>(
    token: &str,
    files: &'a [FrozenCPushFile],
) -> Option<&'a FrozenCPushFile> {
    let clean = token.trim();
    if clean.is_empty() {
        return None;
    }
    if let Some(found) = files.iter().find(|f| f.file_id == clean) {
        return Some(found);
    }
    if let Some(rest) = clean.strip_prefix('@')
        && let Ok(idx) = rest.parse::<usize>()
        && idx >= 1
        && idx <= files.len()
    {
        return files.get(idx - 1);
    }
    let normalized = clean.trim_start_matches("./");
    if let Some(found) = files.iter().find(|f| f.path == normalized) {
        return Some(found);
    }
    files
        .iter()
        .find(|f| f.previous_path.as_deref() == Some(normalized))
}

pub(super) fn summarize_user_response(
    req: &MessageUserRequest,
    resp: &MessageUserResponse,
) -> (String, Option<String>) {
    let mut parts = Vec::new();
    let mut first_opt_id: Option<String> = None;
    for ans in &resp.answers {
        if first_opt_id.is_none() {
            first_opt_id = ans.selected_option_ids.first().cloned();
        }
        let mut ans_parts = Vec::new();
        if !ans.selected_labels.is_empty() {
            ans_parts.push(ans.selected_labels.join(", "));
        } else if !ans.selected_option_ids.is_empty() {
            ans_parts.push(ans.selected_option_ids.join(", "));
        }
        if let Some(ref custom) = ans.custom_text {
            let trimmed = custom.trim();
            if !trimmed.is_empty() {
                ans_parts.push(trimmed.to_owned());
            }
        }
        if !ans_parts.is_empty() {
            parts.push(ans_parts.join(" — "));
        }
    }
    if let Some(ref note) = resp.global_note {
        let trimmed = note.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_owned());
        }
    }
    let label = if parts.is_empty() {
        req.questions_slice()
            .first()
            .map(|q| q.header.clone())
            .unwrap_or_else(|| "Confirmed".into())
    } else {
        parts.join("; ")
    };
    (label, first_opt_id)
}

pub(super) fn build_sensitive_questionnaire(
    locale: LanguageKind,
    sensitive_files: &[&FrozenCPushFile],
    has_other_files: bool,
) -> (
    MessageUserRequest,
    Vec<CPushMessageFileChip>,
    String,
    String,
    String,
) {
    let names: Vec<String> = sensitive_files
        .iter()
        .map(|f| file_basename(&f.path))
        .collect();
    let affected_ids: Vec<String> = sensitive_files.iter().map(|f| f.file_id.clone()).collect();
    let (title, scope, description, legacy_opts) =
        i18n::cpush::cpush_sensitive_question(locale, &names, has_other_files);
    let chips: Vec<CPushMessageFileChip> = sensitive_files
        .iter()
        .map(|file| CPushMessageFileChip {
            id: file.file_id.clone(),
            label: file_basename(&file.path),
            badge: Some(if file.range_kind == "partial" {
                i18n::cpush::cpush_partial_badge(locale).into()
            } else {
                i18n::cpush::cpush_needs_confirm_badge(locale).into()
            }),
        })
        .collect();
    let options: Vec<MessageQuestionOption> = legacy_opts
        .into_iter()
        .map(|o| {
            let desc = if o.id == "skip" {
                match locale {
                    LanguageKind::Zh => "将敏感文件保留在本地工作区，不纳入本次提交".into(),
                    LanguageKind::En => {
                        "Keep sensitive files in the local working tree and exclude from commits"
                            .into()
                    }
                }
            } else {
                match locale {
                    LanguageKind::Zh => "确认不含私密凭据后一并提交".into(),
                    LanguageKind::En => {
                        "Include in this commit after confirming no secrets are exposed".into()
                    }
                }
            };
            MessageQuestionOption {
                id: o.id,
                label: o.label,
                description: desc,
                recommended: o.recommended,
                preview: None,
            }
        })
        .collect();
    let header = match locale {
        LanguageKind::Zh => "敏感文件确认".into(),
        LanguageKind::En => "Sensitive files".into(),
    };
    let request = MessageUserRequest {
        title: Some(title.clone()),
        subtitle: Some(scope.clone()),
        tone: Some(MessageTone::Warning),
        blocks: None,
        links: None,
        questions: Some(vec![MessageQuestionItem {
            id: "sensitive-files".into(),
            header,
            question: description.clone(),
            multi_select: None,
            options,
        }]),
        context: Some(MessageQuestionContext {
            scope: Some(scope.clone()),
            affected_file_ids: Some(affected_ids),
            links: None,
            allow_dont_ask_again: None,
            dont_ask_again_label: None,
        }),
    };
    (request, chips, title, scope, description)
}
