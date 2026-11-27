//! Shared C-Push / Pull-Rebase session formatting and state-transition helpers.

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

pub(crate) fn is_active_status(status: CPushSessionStatus) -> bool {
    !matches!(status, CPushSessionStatus::Idle | CPushSessionStatus::Done)
}

pub(crate) fn summarize_commits(commits: &[CPushLandedCommit]) -> Vec<CPushMessageCommitSummary> {
    commits
        .iter()
        .map(|c| CPushMessageCommitSummary {
            hash: c.short_oid.clone(),
            title: c.title.clone(),
        })
        .collect()
}

pub(crate) fn conflict_chips(
    locale: LanguageKind,
    files: &[ConflictedFileSummary],
) -> Vec<CPushMessageFileChip> {
    files
        .iter()
        .map(|f| CPushMessageFileChip {
            id: f.file_id.clone(),
            label: f.path.clone(),
            badge: Some(i18n::cpush::cpush_conflict_badge(locale).into()),
        })
        .collect()
}

pub(crate) fn apply_rebased_commits(
    session: &mut StoredCPushSession,
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
) -> Vec<CPushMessageOption> {
    let mut opts = Vec::new();
    if has_model {
        opts.push(CPushMessageOption {
            id: "agent-merge".into(),
            label: i18n::cpush::cpush_option_agent_merge(locale).into(),
            recommended: Some(true),
        });
    }
    opts.push(CPushMessageOption {
        id: "manual-merge".into(),
        label: i18n::cpush::cpush_option_manual_merge(locale).into(),
        recommended: (!has_model).then_some(true),
    });
    opts.push(CPushMessageOption {
        id: "abort-rebase".into(),
        label: i18n::cpush::cpush_option_abort_rebase(locale, has_commits).into(),
        recommended: None,
    });
    opts
}

pub(crate) fn enter_conflict_state(
    session: &mut StoredCPushSession,
    upstream: &str,
    conflicted_files: &[ConflictedFileSummary],
    has_model: bool,
    extra_reason: Option<&str>,
) {
    let locale = session.locale;
    session.session_status = CPushSessionStatus::Conflict;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let names: Vec<&str> = conflicted_files.iter().map(|f| f.path.as_str()).collect();
    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_local_commit_titled(locale, &c.short_oid, &c.title),
        })
        .collect();
    timeline.push(CPushTimelineItem {
        state: "failed".into(),
        label: i18n::cpush::cpush_timeline_sync_conflict(locale, upstream, &names),
    });
    session.timeline = timeline;

    let description =
        i18n::cpush::cpush_conflict_description(locale, upstream, &names, extra_reason);
    let commits = summarize_commits(&session.commits);
    session.message = Some(CPushUiMessage {
        id: format!("conflict-{}-{}", session.run_id, session.qa_log.len() + 1),
        kind: "conflict".into(),
        title: i18n::cpush::cpush_conflict_title(locale).into(),
        description,
        scope: Some(i18n::cpush::cpush_conflict_scope(locale, upstream)),
        files: Some(conflict_chips(locale, conflicted_files)),
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(build_conflict_options(
            locale,
            has_model,
            !session.commits.is_empty(),
        )),
        links: None,
        card: None,
        questionnaire: None,
        progress: None,
    });
}

pub(crate) fn enter_push_failed_state(
    session: &mut StoredCPushSession,
    remote_target: &str,
    reason: &str,
    title_override: Option<&str>,
) {
    let locale = session.locale;
    session.session_status = CPushSessionStatus::PushFailed;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_local_commit_written(locale, &c.short_oid),
        })
        .collect();
    timeline.push(CPushTimelineItem {
        state: "failed".into(),
        label: i18n::cpush::cpush_timeline_push_failed(locale, remote_target),
    });
    session.timeline = timeline;

    let commits = summarize_commits(&session.commits);
    session.message = Some(CPushUiMessage {
        id: format!(
            "push-failed-{}-{}",
            session.run_id,
            session.qa_log.len() + 1
        ),
        kind: "pushFailed".into(),
        title: title_override
            .unwrap_or_else(|| i18n::cpush::cpush_push_failed_title(locale))
            .into(),
        description: i18n::cpush::cpush_push_failed_description(locale, remote_target, reason),
        scope: Some(i18n::cpush::cpush_push_stage_scope(locale, remote_target)),
        files: None,
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(vec![
            CPushMessageOption {
                id: "retry-push".into(),
                label: i18n::cpush::cpush_option_retry_push(locale).into(),
                recommended: Some(true),
            },
            CPushMessageOption {
                id: "dismiss-session".into(),
                label: i18n::cpush::cpush_option_keep_local_end(locale).into(),
                recommended: None,
            },
        ]),
        links: None,
        card: None,
        questionnaire: None,
        progress: None,
    });
}

pub(crate) fn mark_pushed_done(
    session: &mut StoredCPushSession,
    remote_branch: &str,
    rebased: bool,
) {
    let locale = session.locale;
    let count = session.commits.len().max(1);
    let commit_summaries = summarize_commits(&session.commits);
    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_commit_item(locale, &c.short_oid, &c.title),
        })
        .collect();
    if rebased {
        timeline.push(CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush::cpush_timeline_rebased_synced(locale, remote_branch),
        });
    }
    timeline.push(CPushTimelineItem {
        state: "done".into(),
        label: i18n::cpush::cpush_pushed_done_title(locale, count, remote_branch),
    });

    session.timeline = timeline;
    session.session_status = CPushSessionStatus::Done;
    session.pending_call_id = None;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    session.message = Some(CPushUiMessage {
        id: format!("done-{}", session.run_id),
        kind: "done".into(),
        title: i18n::cpush::cpush_pushed_done_title(locale, count, remote_branch),
        description: i18n::cpush::cpush_pushed_done_description(
            locale,
            session.skipped_file_ids.len(),
        ),
        scope: Some(i18n::cpush::cpush_synced_scope(locale, &session.branch)),
        files: None,
        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
        options: None,
        links: None,
        card: None,
        questionnaire: None,
        progress: None,
    });
}
