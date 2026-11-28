//! Shared session formatting, file-token resolution, user-response summarization,
//! sensitive-file questionnaire builders, and recovery state-transition helpers.

use crate::{
    actions::types::*,
    i18n::{self, LanguageKind},
    repository::{
        ai_context::FrozenCPushFile,
        remote::{ConflictedFileSummary, RebasedCommitSummary},
    },
};
use std::path::Path;

pub(crate) fn file_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_owned()
}

pub(crate) fn scope_badge(locale: LanguageKind, files: &[FrozenCPushFile]) -> String {
    let partial_count = files.iter().filter(|f| f.range_kind == "partial").count();
    i18n::cpush::cpush_scope_badge(locale, files.len(), partial_count)
}

pub(crate) fn summarize_commits(
    commits: &[CPushLandedCommit],
) -> Vec<WorkflowMessageCommitSummary> {
    commits
        .iter()
        .map(|c| WorkflowMessageCommitSummary {
            hash: c.short_oid.clone(),
            title: c.title.clone(),
        })
        .collect()
}

pub(crate) fn conflict_chips(
    locale: LanguageKind,
    files: &[ConflictedFileSummary],
) -> Vec<WorkflowMessageFileChip> {
    files
        .iter()
        .map(|f| WorkflowMessageFileChip {
            id: f.file_id.clone(),
            label: f.path.clone(),
            badge: Some(i18n::cpush::cpush_conflict_badge(locale).into()),
        })
        .collect()
}

pub(crate) fn resolve_frozen_file_token<'a>(
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

pub(crate) fn summarize_user_response(
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

pub(crate) fn build_sensitive_questionnaire(
    locale: LanguageKind,
    sensitive_files: &[&FrozenCPushFile],
    has_other_files: bool,
) -> (
    MessageUserRequest,
    Vec<WorkflowMessageFileChip>,
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
    let chips: Vec<WorkflowMessageFileChip> = sensitive_files
        .iter()
        .map(|file| WorkflowMessageFileChip {
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
    let req = MessageUserRequest {
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
    (req, chips, title, scope, description)
}

pub(crate) fn apply_rebased_commits(
    session: &mut StoredActionSession,
    rebased: &[RebasedCommitSummary],
) {
    if rebased.is_empty() {
        return;
    }
    if rebased.len() == session.commits.len() {
        for (existing, updated) in session.commits.iter_mut().zip(rebased.iter()) {
            existing.oid = updated.oid.clone();
            existing.short_oid = updated.short_oid.clone();
            existing.title = updated.title.clone();
            existing.branch = updated.branch.clone();
        }
    } else {
        session.commits = rebased
            .iter()
            .map(|c| CPushLandedCommit {
                oid: c.oid.clone(),
                short_oid: c.short_oid.clone(),
                title: c.title.clone(),
                branch: c.branch.clone(),
                file_ids: Vec::new(),
            })
            .collect();
    }
}

pub(crate) fn build_conflict_options(
    locale: LanguageKind,
    has_model: bool,
    has_commits: bool,
) -> Vec<WorkflowMessageOption> {
    let mut opts = Vec::new();
    if has_model {
        opts.push(WorkflowMessageOption {
            id: "agent-merge".into(),
            label: i18n::cpush::cpush_option_agent_merge(locale).into(),
            recommended: Some(true),
        });
    }
    opts.push(WorkflowMessageOption {
        id: "manual-merge".into(),
        label: i18n::cpush::cpush_option_manual_merge(locale).into(),
        recommended: (!has_model).then_some(true),
    });
    opts.push(WorkflowMessageOption {
        id: "abort-rebase".into(),
        label: i18n::cpush::cpush_option_abort_rebase(locale, has_commits).into(),
        recommended: None,
    });
    opts
}

pub(crate) fn enter_conflict_state(
    session: &mut StoredActionSession,
    upstream: &str,
    conflicted_files: &[ConflictedFileSummary],
    has_model: bool,
    extra_reason: Option<&str>,
) {
    let locale = session.locale;
    session.status = ActionSessionStatus::Conflict;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let names: Vec<&str> = conflicted_files.iter().map(|f| f.path.as_str()).collect();
    let mut timeline: Vec<WorkflowTimelineItem> = session
        .commits
        .iter()
        .map(|c| WorkflowTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_local_commit_titled(locale, &c.short_oid, &c.title),
        })
        .collect();
    timeline.push(WorkflowTimelineItem {
        state: "failed".into(),
        label: i18n::cpush::cpush_timeline_sync_conflict(locale, upstream, &names),
    });
    session.timeline = timeline;

    let description =
        i18n::cpush::cpush_conflict_description(locale, upstream, &names, extra_reason);
    let commits = summarize_commits(&session.commits);
    let title: String = i18n::cpush::cpush_conflict_title(locale).into();
    session.summary = title.clone();
    session.message = Some(WorkflowUiMessage {
        id: format!("conflict-{}-{}", session.run_id, session.qa_log.len() + 1),
        kind: "conflict".into(),
        title,
        description,
        scope: Some(i18n::cpush::cpush_conflict_scope(locale, upstream)),
        files: Some(conflict_chips(locale, conflicted_files)),
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(build_conflict_options(
            locale,
            has_model,
            !session.commits.is_empty(),
        )),
        links: (!session.links.is_empty()).then_some(session.links.clone()),
        card: None,
        questionnaire: None,
        progress: None,
    });
}

pub(crate) fn enter_push_failed_state(
    session: &mut StoredActionSession,
    remote_target: &str,
    reason: &str,
    title_override: Option<&str>,
) {
    let locale = session.locale;
    session.status = ActionSessionStatus::PushFailed;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let mut timeline: Vec<WorkflowTimelineItem> = session
        .commits
        .iter()
        .map(|c| WorkflowTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_local_commit_written(locale, &c.short_oid),
        })
        .collect();
    timeline.push(WorkflowTimelineItem {
        state: "failed".into(),
        label: i18n::cpush::cpush_timeline_push_failed(locale, remote_target),
    });
    session.timeline = timeline;

    let commits = summarize_commits(&session.commits);
    let title: String = title_override
        .unwrap_or_else(|| i18n::cpush::cpush_push_failed_title(locale))
        .into();
    session.summary = title.clone();
    session.message = Some(WorkflowUiMessage {
        id: format!(
            "push-failed-{}-{}",
            session.run_id,
            session.qa_log.len() + 1
        ),
        kind: "pushFailed".into(),
        title,
        description: i18n::cpush::cpush_push_failed_description(locale, remote_target, reason),
        scope: Some(i18n::cpush::cpush_push_stage_scope(locale, remote_target)),
        files: None,
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(vec![
            WorkflowMessageOption {
                id: "retry-push".into(),
                label: i18n::cpush::cpush_option_retry_push(locale).into(),
                recommended: Some(true),
            },
            WorkflowMessageOption {
                id: "dismiss-session".into(),
                label: i18n::cpush::cpush_option_keep_local_end(locale).into(),
                recommended: None,
            },
        ]),
        links: (!session.links.is_empty()).then_some(session.links.clone()),
        card: None,
        questionnaire: None,
        progress: None,
    });
}

pub(crate) fn enter_missing_upstream_state(session: &mut StoredActionSession, remotes: &[String]) {
    let locale = session.locale;
    if remotes.is_empty() {
        let branch_name = session.branch.clone();
        enter_push_failed_state(
            session,
            &branch_name,
            i18n::cpush::cpush_no_remote_configured_reason(locale),
            Some(i18n::cpush::cpush_no_remote_configured_title(locale)),
        );
        return;
    }

    session.status = ActionSessionStatus::NeedsDecision;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let commit_summaries = summarize_commits(&session.commits);
    let mut timeline: Vec<WorkflowTimelineItem> = session
        .commits
        .iter()
        .map(|c| WorkflowTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_local_commit_written(locale, &c.short_oid),
        })
        .collect();
    timeline.push(WorkflowTimelineItem {
        state: "active".into(),
        label: i18n::cpush::cpush_timeline_waiting_upstream_choice(locale).into(),
    });
    session.timeline = timeline;

    let mut options: Vec<WorkflowMessageOption> = remotes
        .iter()
        .enumerate()
        .map(|(idx, r)| WorkflowMessageOption {
            id: format!("push-set-upstream:{r}"),
            label: i18n::cpush::cpush_push_track_label(locale, r, &session.branch),
            recommended: (remotes.len() == 1 && idx == 0).then_some(true),
        })
        .collect();
    options.push(WorkflowMessageOption {
        id: "dismiss-session".into(),
        label: i18n::cpush::cpush_option_keep_local_only(locale).into(),
        recommended: None,
    });
    let title: String = i18n::cpush::cpush_missing_upstream_question_title(locale).into();
    session.summary = title.clone();
    session.message = Some(WorkflowUiMessage {
        id: format!(
            "missing-upstream-{}-{}",
            session.run_id,
            session.qa_log.len() + 1
        ),
        kind: "question".into(),
        title,
        description: i18n::cpush::cpush_missing_upstream_question_desc(locale, &session.branch),
        scope: Some(i18n::cpush::cpush_branch_scope(locale, &session.branch)),
        files: None,
        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
        options: Some(options),
        links: (!session.links.is_empty()).then_some(session.links.clone()),
        card: None,
        questionnaire: None,
        progress: None,
    });
}

pub(crate) fn mark_pushed_done(
    session: &mut StoredActionSession,
    remote_branch: &str,
    rebased: bool,
) {
    let locale = session.locale;
    let count = session.commits.len().max(1);
    let commit_summaries = summarize_commits(&session.commits);
    let mut timeline: Vec<WorkflowTimelineItem> = session
        .commits
        .iter()
        .map(|c| WorkflowTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_commit_item(locale, &c.short_oid, &c.title),
        })
        .collect();
    if rebased {
        timeline.push(WorkflowTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_rebased_synced(locale, remote_branch),
        });
    }
    let title = i18n::cpush::cpush_pushed_done_title(locale, count, remote_branch);
    timeline.push(WorkflowTimelineItem {
        state: "done".into(),
        label: title.clone(),
    });

    session.timeline = timeline;
    session.status = ActionSessionStatus::Done;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;
    session.summary = title.clone();

    session.message = Some(WorkflowUiMessage {
        id: format!("done-{}", session.run_id),
        kind: "done".into(),
        title,
        description: i18n::cpush::cpush_pushed_done_description(
            locale,
            session.skipped_file_ids.len(),
        ),
        scope: Some(i18n::cpush::cpush_synced_scope(locale, &session.branch)),
        files: None,
        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
        options: None,
        links: (!session.links.is_empty()).then_some(session.links.clone()),
        card: None,
        questionnaire: None,
        progress: None,
    });
}
