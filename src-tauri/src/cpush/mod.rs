//! M4 & M5 C-Push, Preview Plan, and Pull-Rebase coordinator:
//! freezes authorized file scope, coordinates Pi Core split planning & authorized
//! conflict resolution, executes user-confirmed Preview Plans verbatim, serializes
//! Git writes per repository, preserves completed commits on partial/push failure
//! without repeating finished steps, reconciles manual IDE merge & terminal push
//! states against live Git + remote, and persists session history in
//! `cpush-sessions.json` (`0600`).
#[cfg(test)]
mod m5_tests;
#[cfg(test)]
mod tests;
pub mod types;

use crate::{
    foundation::pi::{
        PiCPushCommitGroup, PiCPushPlan, PiCPushQaTurn, PiCommand, PiConflictFile,
        PiConflictResolution, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome,
        RequestContext,
    },
    i18n::{self, LanguageKind},
    models::store::ModelStore,
    repository::{
        self, RepositoryStore,
        ai_context::FrozenCPushFile,
        remote::{
            ConflictedFileSummary, ExtractedConflictFile, PushOutcome, RebaseContinueOutcome,
            RebaseSyncOutcome, RebasedCommitSummary,
        },
        types::{CommitRequest, ReadTarget, RepositorySnapshot},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::NamedTempFile;
use types::*;

const CPUSH_STORAGE_VERSION: u8 = 1;
const CPUSH_STORAGE_FILE: &str = "cpush-sessions.json";
const MAX_STORED_SESSIONS: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCPushFile {
    version: u8,
    sessions: Vec<StoredCPushSession>,
}

#[derive(Clone, Default)]
pub struct CPushCoordinator {
    inner: Arc<CoordinatorInner>,
}

#[derive(Default)]
struct CoordinatorInner {
    storage_gate: tokio::sync::Mutex<()>,
    worktree_gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    sessions: Mutex<HashMap<String, StoredCPushSession>>,
    processed_requests: Mutex<HashMap<String, CPushCommandReply>>,
    active_operations: AtomicUsize,
}

pub(crate) struct ActiveOperationGuard {
    inner: Arc<CoordinatorInner>,
}

impl Drop for ActiveOperationGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
    }
}

impl CPushCoordinator {
    pub fn has_active_operation(&self) -> bool {
        self.inner.active_operations.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn track_operation(&self) -> ActiveOperationGuard {
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        ActiveOperationGuard {
            inner: Arc::clone(&self.inner),
        }
    }
}

fn file_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_owned()
}

fn scope_badge(locale: LanguageKind, files: &[FrozenCPushFile]) -> String {
    let partial_count = files.iter().filter(|f| f.range_kind == "partial").count();
    i18n::cpush_scope_badge(locale, files.len(), partial_count)
}

fn is_active_status(status: CPushSessionStatus) -> bool {
    !matches!(status, CPushSessionStatus::Idle | CPushSessionStatus::Done)
}

fn summarize_commits(commits: &[CPushLandedCommit]) -> Vec<CPushMessageCommitSummary> {
    commits
        .iter()
        .map(|c| CPushMessageCommitSummary {
            hash: c.short_oid.clone(),
            title: c.title.clone(),
        })
        .collect()
}

fn conflict_chips(
    locale: LanguageKind,
    files: &[ConflictedFileSummary],
) -> Vec<CPushMessageFileChip> {
    files
        .iter()
        .map(|f| CPushMessageFileChip {
            id: f.file_id.clone(),
            label: f.path.clone(),
            badge: Some(i18n::cpush_conflict_badge(locale).into()),
        })
        .collect()
}

fn apply_rebased_commits(session: &mut StoredCPushSession, rebased: &[RebasedCommitSummary]) {
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

fn build_conflict_options(
    locale: LanguageKind,
    has_model: bool,
    has_commits: bool,
) -> Vec<CPushMessageOption> {
    let mut opts = Vec::new();
    if has_model {
        opts.push(CPushMessageOption {
            id: "agent-merge".into(),
            label: i18n::cpush_option_agent_merge(locale).into(),
            recommended: Some(true),
        });
    }
    opts.push(CPushMessageOption {
        id: "manual-merge".into(),
        label: i18n::cpush_option_manual_merge(locale).into(),
        recommended: (!has_model).then_some(true),
    });
    opts.push(CPushMessageOption {
        id: "abort-rebase".into(),
        label: i18n::cpush_option_abort_rebase(locale, has_commits).into(),
        recommended: None,
    });
    opts
}

fn enter_conflict_state(
    session: &mut StoredCPushSession,
    upstream: &str,
    conflicted_files: &[ConflictedFileSummary],
    has_model: bool,
    extra_reason: Option<&str>,
) {
    let locale = session.locale;
    session.session_status = CPushSessionStatus::Conflict;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let names: Vec<&str> = conflicted_files.iter().map(|f| f.path.as_str()).collect();
    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush_timeline_local_commit_titled(locale, &c.short_oid, &c.title),
        })
        .collect();
    timeline.push(CPushTimelineItem {
        state: "failed".into(),
        label: i18n::cpush_timeline_sync_conflict(locale, upstream, &names),
    });
    session.timeline = timeline;

    let description = i18n::cpush_conflict_description(locale, upstream, &names, extra_reason);
    let commits = summarize_commits(&session.commits);
    session.message = Some(CPushUiMessage {
        id: format!("conflict-{}-{}", session.run_id, session.qa_log.len() + 1),
        kind: "conflict".into(),
        title: i18n::cpush_conflict_title(locale).into(),
        description,
        scope: Some(i18n::cpush_conflict_scope(locale, upstream)),
        files: Some(conflict_chips(locale, conflicted_files)),
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(build_conflict_options(
            locale,
            has_model,
            !session.commits.is_empty(),
        )),
        progress: None,
    });
}

fn enter_push_failed_state(
    session: &mut StoredCPushSession,
    remote_target: &str,
    reason: &str,
    title_override: Option<&str>,
) {
    let locale = session.locale;
    session.session_status = CPushSessionStatus::PushFailed;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush_timeline_local_commit_written(locale, &c.short_oid),
        })
        .collect();
    timeline.push(CPushTimelineItem {
        state: "failed".into(),
        label: i18n::cpush_timeline_push_failed(locale, remote_target),
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
            .unwrap_or_else(|| i18n::cpush_push_failed_title(locale))
            .into(),
        description: i18n::cpush_push_failed_description(locale, remote_target, reason),
        scope: Some(i18n::cpush_push_stage_scope(locale, remote_target)),
        files: None,
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(vec![
            CPushMessageOption {
                id: "retry-push".into(),
                label: i18n::cpush_option_retry_push(locale).into(),
                recommended: Some(true),
            },
            CPushMessageOption {
                id: "dismiss-session".into(),
                label: i18n::cpush_option_keep_local_end(locale).into(),
                recommended: None,
            },
        ]),
        progress: None,
    });
}

fn read_sessions_file(
    storage_dir: &Path,
    locale: LanguageKind,
) -> (Vec<StoredCPushSession>, Option<String>) {
    let path = storage_dir.join(CPUSH_STORAGE_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(_) => {
            return (
                Vec::new(),
                Some(i18n::cpush_sessions_unreadable_warning(locale).into()),
            );
        }
    };
    match serde_json::from_slice::<StoredCPushFile>(&bytes) {
        Ok(parsed) if parsed.version == CPUSH_STORAGE_VERSION => (parsed.sessions, None),
        _ => (
            Vec::new(),
            Some(i18n::cpush_sessions_corrupted_warning(locale).into()),
        ),
    }
}

fn write_sessions_file_atomic(
    storage_dir: &Path,
    sessions: &[StoredCPushSession],
) -> Result<(), String> {
    fs::create_dir_all(storage_dir).map_err(|e| {
        format!("Cannot create application data directory for C-Push sessions: {e}")
    })?;
    let payload = serde_json::to_vec_pretty(&StoredCPushFile {
        version: CPUSH_STORAGE_VERSION,
        sessions: sessions.iter().take(MAX_STORED_SESSIONS).cloned().collect(),
    })
    .map_err(|e| format!("Cannot serialize C-Push session history: {e}"))?;

    let mut temp = NamedTempFile::new_in(storage_dir)
        .map_err(|e| format!("Cannot create temporary C-Push session file: {e}"))?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("Cannot set permissions on C-Push session file: {e}"))?;
    temp.write_all(&payload)
        .map_err(|e| format!("Failed to write C-Push session file: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync C-Push session file: {e}"))?;
    temp.persist(storage_dir.join(CPUSH_STORAGE_FILE))
        .map_err(|e| format!("Failed to atomically persist C-Push session file: {e}"))?;
    Ok(())
}

fn validate_plan_in_rust(
    plan: PiCPushPlan,
    frozen_files: &[FrozenCPushFile],
    allow_skips: bool,
    locale: LanguageKind,
) -> Result<PiCPushPlan, String> {
    let authorized_ids: HashSet<&str> = frozen_files.iter().map(|f| f.file_id.as_str()).collect();
    match plan {
        PiCPushPlan::Question {
            title,
            scope,
            description,
            affected_file_ids,
            options,
        } => {
            if title.trim().is_empty() || description.trim().is_empty() || options.is_empty() {
                return Err(i18n::cpush_plan_incomplete_question_error(locale).into());
            }
            let filtered_affected: Vec<String> = affected_file_ids
                .into_iter()
                .filter(|id| authorized_ids.contains(id.as_str()))
                .collect();
            Ok(PiCPushPlan::Question {
                title,
                scope,
                description,
                affected_file_ids: filtered_affected,
                options,
            })
        }
        PiCPushPlan::Ready {
            summary,
            groups,
            skipped_file_ids,
        } => {
            if !allow_skips && !skipped_file_ids.is_empty() {
                return Err(i18n::cpush_plan_unconfirmed_skip_error(locale).into());
            }
            let mut seen = HashSet::with_capacity(frozen_files.len());
            for skipped in &skipped_file_ids {
                if !authorized_ids.contains(skipped.as_str()) {
                    return Err(i18n::cpush_plan_unauthorized_skip_error(locale).into());
                }
                if !seen.insert(skipped.as_str()) {
                    return Err(i18n::cpush_plan_duplicate_file_error(locale).into());
                }
            }
            for group in &groups {
                if group.file_ids.is_empty() || group.message.trim().is_empty() {
                    return Err(i18n::cpush_plan_empty_group_error(locale).into());
                }
                for id in &group.file_ids {
                    if !authorized_ids.contains(id.as_str()) {
                        return Err(i18n::cpush_plan_unselected_file_error(locale).into());
                    }
                    if !seen.insert(id.as_str()) {
                        return Err(i18n::cpush_plan_duplicate_file_error(locale).into());
                    }
                }
            }
            if seen.len() != authorized_ids.len() {
                return Err(i18n::cpush_plan_incomplete_coverage_error(locale).into());
            }
            if !allow_skips {
                let sensitive: Vec<&FrozenCPushFile> = frozen_files
                    .iter()
                    .filter(|f| crate::repository::ai_context::is_sensitive_local_file(&f.path))
                    .collect();
                if !sensitive.is_empty() {
                    let affected_file_ids: Vec<String> =
                        sensitive.iter().map(|f| f.file_id.clone()).collect();
                    let names: Vec<String> =
                        sensitive.iter().map(|f| file_basename(&f.path)).collect();
                    let has_other_files = sensitive.len() < frozen_files.len();
                    let (title, scope, description, options) =
                        i18n::cpush_sensitive_question(locale, &names, has_other_files);
                    return Ok(PiCPushPlan::Question {
                        title,
                        scope,
                        description,
                        affected_file_ids,
                        options,
                    });
                }
            }
            Ok(PiCPushPlan::Ready {
                summary,
                groups,
                skipped_file_ids,
            })
        }
    }
}

fn validate_confirmed_plan_in_rust(
    confirmed: &CPushConfirmedPlan,
    frozen_files: &[FrozenCPushFile],
    locale: LanguageKind,
) -> Result<(Vec<PiCPushCommitGroup>, Vec<String>), String> {
    let authorized_ids: HashSet<&str> = frozen_files.iter().map(|f| f.file_id.as_str()).collect();
    if confirmed.groups.is_empty() && confirmed.skipped_file_ids.is_empty() {
        return Err(i18n::cpush_plan_empty_group_error(locale).into());
    }
    let mut seen = HashSet::with_capacity(frozen_files.len());
    for skipped in &confirmed.skipped_file_ids {
        if !authorized_ids.contains(skipped.as_str()) {
            return Err(i18n::cpush_plan_unauthorized_skip_error(locale).into());
        }
        if !seen.insert(skipped.as_str()) {
            return Err(i18n::cpush_plan_duplicate_file_error(locale).into());
        }
    }
    let mut validated_groups = Vec::with_capacity(confirmed.groups.len());
    for (index, group) in confirmed.groups.iter().enumerate() {
        if group.file_ids.is_empty() {
            return Err(i18n::cpush_plan_empty_group_error(locale).into());
        }
        if group.message.trim().is_empty()
            || group.message.contains('\0')
            || group.message.len() > 1024 * 1024
        {
            return Err(i18n::cpush_plan_invalid_message_error(locale).into());
        }
        for id in &group.file_ids {
            if !authorized_ids.contains(id.as_str()) {
                return Err(i18n::cpush_plan_unselected_file_error(locale).into());
            }
            if !seen.insert(id.as_str()) {
                return Err(i18n::cpush_plan_duplicate_file_error(locale).into());
            }
        }
        let id = if group.id.trim().is_empty() {
            format!("group-{}", index + 1)
        } else {
            group.id.clone()
        };
        // Preserve the user's edited commit message verbatim without re-normalizing or rewriting:
        validated_groups.push(PiCPushCommitGroup {
            id,
            message: group.message.clone(),
            file_ids: group.file_ids.clone(),
        });
    }
    if seen.len() != authorized_ids.len() {
        return Err(i18n::cpush_plan_incomplete_coverage_error(locale).into());
    }
    Ok((validated_groups, confirmed.skipped_file_ids.clone()))
}

const CPUSH_AI_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

impl CPushCoordinator {
    fn worktree_gate(&self, worktree_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
        Ok(self
            .inner
            .worktree_gates
            .lock()
            .map_err(|_| "C-Push coordinator lock is unavailable")?
            .entry(worktree_id.to_owned())
            .or_default()
            .clone())
    }

    async fn persist_all(&self, storage_dir: &Path) -> Result<(), String> {
        let _guard = self.inner.storage_gate.lock().await;
        let all: Vec<StoredCPushSession> = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .values()
            .cloned()
            .collect();
        write_sessions_file_atomic(storage_dir, &all)
    }

    fn save_in_memory(&self, session: &StoredCPushSession) -> Result<(), String> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .insert(session.worktree_id.clone(), session.clone());
        Ok(())
    }

    fn remove_in_memory(&self, worktree_id: &str) -> Result<(), String> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .remove(worktree_id);
        Ok(())
    }

    pub async fn restore(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
    ) -> Result<(Vec<CPushSessionSnapshot>, Option<String>), String> {
        self.restore_with_locale(storage_dir, snapshots, LanguageKind::En)
            .await
    }

    pub async fn restore_with_locale(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
        locale: LanguageKind,
    ) -> Result<(Vec<CPushSessionSnapshot>, Option<String>), String> {
        let _guard = self.inner.storage_gate.lock().await;
        let (stored_list, warning) = read_sessions_file(storage_dir, locale);
        let live_trees: HashMap<&str, &repository::types::WorktreeSnapshot> = snapshots
            .iter()
            .flat_map(|snap| {
                snap.worktrees
                    .iter()
                    .map(|wt| (wt.worktree_id.as_str(), wt))
            })
            .collect();

        let mut memory = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?;
        let mut public_sessions = Vec::new();
        let mut dirty = false;

        for mut session in stored_list {
            session.auto_collapse = false;
            if session.plan_only && session.commits.is_empty() {
                dirty = true;
                continue;
            }
            // Interrupted `Running` sessions must NEVER auto-rerun commits, rebases, or pushes (`W04`):
            if session.session_status == CPushSessionStatus::Running {
                dirty = true;
                session.locale = locale;
                if session.commits.is_empty() && session.planned_groups.is_empty() {
                    let live_has_conflict = live_trees
                        .get(session.worktree_id.as_str())
                        .is_some_and(|wt| wt.files.iter().any(|f| f.status == "U"));
                    if !live_has_conflict {
                        continue;
                    }
                }

                if let Some(live_tree) = live_trees.get(session.worktree_id.as_str()) {
                    // 1. Check if an in-progress local commit step landed right before exit:
                    if session.next_step_index < session.planned_groups.len()
                        && let Some(live_head) = &live_tree.head
                    {
                        let expected_prev = if session.next_step_index == 0 {
                            session.initial_head.as_deref()
                        } else {
                            session
                                .commits
                                .get(session.next_step_index - 1)
                                .map(|c| c.oid.as_str())
                        };
                        if Some(live_head.as_str()) != expected_prev
                            && !session.commits.iter().any(|c| &c.oid == live_head)
                        {
                            let group = &session.planned_groups[session.next_step_index];
                            let first_line = group
                                .message
                                .lines()
                                .next()
                                .unwrap_or_else(|| i18n::cpush_commit_completed_fallback(locale))
                                .to_owned();
                            session.commits.push(CPushLandedCommit {
                                oid: live_head.clone(),
                                short_oid: live_head.chars().take(7).collect(),
                                title: first_line,
                                branch: live_tree.branch.clone(),
                                file_ids: group.file_ids.clone(),
                            });
                            session.committed_file_ids.extend(group.file_ids.clone());
                            session.next_step_index += 1;
                        }
                    }

                    // 2. If live Git currently has conflicted files, surface `Conflict` directly:
                    let live_conflicts: Vec<ConflictedFileSummary> = live_tree
                        .files
                        .iter()
                        .filter(|f| f.status == "U")
                        .map(|f| ConflictedFileSummary {
                            file_id: f.id.clone(),
                            path: f.path.clone(),
                        })
                        .collect();
                    if !live_conflicts.is_empty() {
                        let upstream = match &live_tree.remote {
                            repository::types::Remote::Tracked { branch, .. } => branch.as_str(),
                            _ => i18n::remote_branch_fallback(locale),
                        };
                        let has_model = !session.model_id.is_empty();
                        enter_conflict_state(
                            &mut session,
                            upstream,
                            &live_conflicts,
                            has_model,
                            Some(i18n::cpush_restart_conflict_reason(locale)),
                        );
                        public_sessions.push(session.to_public());
                        memory.insert(session.worktree_id.clone(), session);
                        continue;
                    }
                }

                let has_remaining_commits = session.next_step_index < session.planned_groups.len();
                let commit_summaries = summarize_commits(&session.commits);

                if !has_remaining_commits && !session.commits.is_empty() && session.push_authorized
                {
                    // All local commits landed, interrupted during remote sync / push (`W04`):
                    session.session_status = CPushSessionStatus::PushFailed;
                    session.can_retry_remaining = false;
                    session.message = Some(CPushUiMessage {
                        id: format!("interrupted-push-{}", session.run_id),
                        kind: "pushFailed".into(),
                        title: i18n::cpush_interrupted_push_title(locale).into(),
                        description: i18n::cpush_interrupted_push_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            CPushMessageOption {
                                id: "retry-push".into(),
                                label: i18n::cpush_option_retry_push(locale).into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
                                id: "dismiss-session".into(),
                                label: i18n::cpush_option_keep_local_end(locale).into(),
                                recommended: None,
                            },
                        ]),
                        progress: None,
                    });
                } else {
                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = has_remaining_commits;
                    let mut options = Vec::new();
                    if has_remaining_commits {
                        options.push(CPushMessageOption {
                            id: "retry-remaining".into(),
                            label: i18n::cpush_option_confirm_continue_remaining(locale).into(),
                            recommended: Some(true),
                        });
                    }
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush_option_dismiss_interrupted(
                            locale,
                            !session.commits.is_empty(),
                        )
                        .into(),
                        recommended: (!has_remaining_commits).then_some(true),
                    });
                    session.message = Some(CPushUiMessage {
                        id: format!("interrupted-{}", session.run_id),
                        kind: "error".into(),
                        title: i18n::cpush_interrupted_commits_title(locale, session.commits.len()),
                        description: i18n::cpush_interrupted_commits_desc(locale).into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(options),
                        progress: None,
                    });
                }
            }
            public_sessions.push(session.to_public());
            memory.insert(session.worktree_id.clone(), session);
        }
        let updated_all: Vec<StoredCPushSession> = memory.values().cloned().collect();
        drop(memory);
        if dirty {
            let _ = write_sessions_file_atomic(storage_dir, &updated_all);
        }
        Ok((public_sessions, warning))
    }

    /// Reconcile `WaitingManual` / `Conflict` / `PushFailed` sessions when returning from IDE or Terminal (`R04`).
    pub async fn reconcile_worktree(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &ReconcileCPushRequest,
    ) -> Result<Option<CPushProgressEvent>, String> {
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)?;
        let candidate_status = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .map(|s| s.session_status);
        if !matches!(
            candidate_status,
            Some(
                CPushSessionStatus::WaitingManual
                    | CPushSessionStatus::Conflict
                    | CPushSessionStatus::PushFailed
            )
        ) {
            return Ok(None);
        }

        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let Ok(_wt_guard) = wt_gate.try_lock_owned() else {
            return Ok(None);
        };

        let mut session = match self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned()
        {
            Some(s)
                if matches!(
                    s.session_status,
                    CPushSessionStatus::WaitingManual
                        | CPushSessionStatus::Conflict
                        | CPushSessionStatus::PushFailed
                ) =>
            {
                s
            }
            _ => return Ok(None),
        };
        if let Some(loc) = request.locale {
            session.locale = loc;
        }
        let locale = session.locale;

        let (local_live, local_snapshot) = repo_store
            .inspect_live_remote_status(&request.target, false, locale)
            .await?;

        // 1. If conflicts or rebase are still unfinished -> keep waiting without auto-writing (`R04`):
        if local_live.rebase_in_progress
            || local_live.merge_or_other_in_progress
            || !local_live.conflicted_files.is_empty()
        {
            if session.session_status == CPushSessionStatus::WaitingManual {
                let chips = conflict_chips(locale, &local_live.conflicted_files);
                let desc = if !local_live.conflicted_files.is_empty() {
                    let names: Vec<&str> = local_live
                        .conflicted_files
                        .iter()
                        .map(|f| f.path.as_str())
                        .collect();
                    i18n::cpush_manual_still_conflicted_desc(locale, &names)
                } else {
                    i18n::cpush_manual_rebase_incomplete_desc(locale).into()
                };
                if let Some(msg) = session.message.as_mut() {
                    msg.description = desc;
                    msg.files = (!chips.is_empty()).then_some(chips);
                }
                session.auto_collapse = false;
                self.save_in_memory(&session)?;
                self.persist_all(storage_dir).await?;
                return Ok(Some(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: session.to_public(),
                    repository_snapshot: Some(local_snapshot),
                }));
            }
            return Ok(None);
        }

        // 2. Rebase / conflict is no longer active! Verify live remote reachability (`development.md:131`):
        let (verified_live, verified_snapshot) = repo_store
            .inspect_live_remote_status(&request.target, true, locale)
            .await?;

        if session.session_status == CPushSessionStatus::PushFailed {
            // Only auto-archive `PushFailed` if live verified remote confirms `ahead == 0`:
            if let Some(spec) = &verified_live.upstream
                && verified_live.ahead == 0
                && verified_snapshot
                    .worktrees
                    .iter()
                    .find(|w| w.worktree_id == request.target.worktree_id)
                    .is_some_and(|w| {
                        matches!(
                            w.remote,
                            repository::types::Remote::Tracked {
                                local_only: false,
                                ..
                            }
                        )
                    })
            {
                session.session_status = CPushSessionStatus::Done;
                session.can_retry_remaining = false;
                session.auto_collapse = true;
                session.timeline.push(CPushTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush_timeline_terminal_pushed(locale, &spec.display),
                });
                let commits = summarize_commits(&session.commits);
                session.message = Some(CPushUiMessage {
                    id: format!("done-ext-push-{}", session.run_id),
                    kind: "done".into(),
                    title: i18n::cpush_terminal_pushed_title(locale, &spec.display),
                    description: i18n::cpush_terminal_pushed_from_failed_desc(locale).into(),
                    scope: Some(i18n::cpush_synced_scope(locale, &session.branch)),
                    files: None,
                    commits: (!commits.is_empty()).then_some(commits),
                    options: None,
                    progress: None,
                });
                self.save_in_memory(&session)?;
                self.persist_all(storage_dir).await?;
                return Ok(Some(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: session.to_public(),
                    repository_snapshot: Some(verified_snapshot),
                }));
            }
            return Ok(None);
        }

        // `WaitingManual` or `Conflict` where conflicts & rebase are now resolved!
        apply_rebased_commits(&mut session, &verified_live.unpushed_commits);
        let remote_verified = verified_snapshot
            .worktrees
            .iter()
            .find(|w| w.worktree_id == request.target.worktree_id)
            .is_some_and(|w| {
                matches!(
                    w.remote,
                    repository::types::Remote::Tracked {
                        local_only: false,
                        ..
                    }
                )
            });

        if let Some(spec) = &verified_live.upstream
            && verified_live.ahead == 0
            && remote_verified
        {
            // Terminal already pushed (`R04`)!
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            session.auto_collapse = true;
            session.qa_log.push(CPushQaItem {
                question_id: format!("manual-ext-{}", session.run_id),
                question: i18n::cpush_qa_manual_ext_question(locale).into(),
                answer: i18n::cpush_qa_manual_ext_answer(locale).into(),
                option_id: None,
            });
            session.timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_manual_resolved_and_pushed(locale, &spec.display),
            });
            let commits = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("done-manual-pushed-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush_terminal_pushed_title(locale, &spec.display),
                description: i18n::cpush_terminal_pushed_from_manual_desc(locale).into(),
                scope: Some(i18n::cpush_synced_scope(locale, &session.branch)),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: None,
                progress: None,
            });
        } else if !session.push_authorized {
            // Standalone Pull & Rebase never auto-pushes (`R06`):
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            session.auto_collapse = false;
            session.timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_manual_rebase_done(locale).into(),
            });
            let commits = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("done-manual-rebase-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush_manual_rebase_done_title(locale).into(),
                description: i18n::cpush_manual_rebase_done_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: None,
                progress: None,
            });
        } else {
            // Resolved locally, still unpushed (`ahead > 0` or unverified remote) -> ask confirmation (`R04`)!
            let target_label = verified_live
                .upstream
                .as_ref()
                .map(|u| u.display.as_str())
                .unwrap_or_else(|| i18n::remote_branch_fallback(locale));
            session.session_status = CPushSessionStatus::NeedsDecision;
            session.can_retry_remaining = false;
            session.auto_collapse = false;
            session.timeline = vec![
                CPushTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush_timeline_manual_rebase_done(locale).into(),
                },
                CPushTimelineItem {
                    state: "active".into(),
                    label: i18n::cpush_timeline_awaiting_continue_push(locale, target_label),
                },
            ];
            let commits = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("resolved-continue-push-{}", session.run_id),
                kind: "resolved".into(),
                title: i18n::cpush_resolved_continue_push_title(locale).into(),
                description: i18n::cpush_resolved_continue_push_desc(locale, target_label),
                scope: Some(i18n::cpush_push_stage_scope(locale, target_label)),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: Some(vec![
                    CPushMessageOption {
                        id: "continue-push".into(),
                        label: i18n::cpush_option_continue_push(locale).into(),
                        recommended: Some(true),
                    },
                    CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush_option_skip_push_keep_local(locale).into(),
                        recommended: None,
                    },
                ]),
                progress: None,
            });
        }

        self.save_in_memory(&session)?;
        self.persist_all(storage_dir).await?;
        Ok(Some(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: Some(verified_snapshot),
        }))
    }

    /// Standalone Pull & Rebase (`push_authorized = false`, `R06`).
    pub async fn pull_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &PullRebaseRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        let existing_active = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned();
        if let Some(active) = existing_active
            && is_active_status(active.session_status)
        {
            if active.plan_only && active.commits.is_empty() {
                let _ = self.remove_in_memory(&request.target.worktree_id);
                let _ = self.persist_all(storage_dir).await;
            } else {
                return Err(i18n::cpush_active_session_busy_error(locale, false).into());
            }
        }

        let outcome = repo_store
            .sync_rebase_onto_upstream_with_locale(&request.target, locale)
            .await?;
        match outcome {
            RebaseSyncOutcome::MissingUpstream { remotes, .. } => {
                Err(i18n::pull_rebase_missing_upstream_error(locale, &remotes))
            }
            RebaseSyncOutcome::UnsafeDirtyOverlap {
                upstream,
                overlapping_paths,
                ..
            } => Err(i18n::pull_rebase_dirty_overlap_error(
                locale,
                &upstream,
                &overlapping_paths,
            )),
            RebaseSyncOutcome::UnsafePublishedHistory { reason, .. } => Err(reason),
            RebaseSyncOutcome::AlreadyUpToDate { snapshot, .. }
            | RebaseSyncOutcome::Completed { snapshot, .. } => {
                let idle_session = CPushSessionSnapshot {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: request.model_id.clone().unwrap_or_default(),
                    frozen_file_ids: Vec::new(),
                    committed_file_ids: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    session_status: CPushSessionStatus::Idle,
                    message: None,
                    timeline: Vec::new(),
                    qa_log: Vec::new(),
                    commits: Vec::new(),
                    can_retry_remaining: false,
                    auto_collapse: None,
                };
                let reply = CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request.request_id.clone(),
                    session: idle_session,
                    prepared_plan: None,
                    repository_snapshot: Some(snapshot),
                };
                self.inner
                    .processed_requests
                    .lock()
                    .map_err(|_| "C-Push request registry lock is unavailable")?
                    .insert(request.request_id.clone(), reply.clone());
                Ok(reply)
            }
            RebaseSyncOutcome::Conflict {
                upstream,
                conflicted_files,
                snapshot,
            } => {
                let active_tree = snapshot
                    .worktrees
                    .iter()
                    .find(|w| w.worktree_id == request.target.worktree_id);
                let branch = active_tree
                    .map(|w| w.branch.clone())
                    .unwrap_or_else(|| "HEAD".into());
                let model_id = request.model_id.clone().unwrap_or_default();
                let mut session = StoredCPushSession {
                    run_id: request.request_id.clone(),
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    frozen_scope_version: request.target.scope_version.clone(),
                    model_id: model_id.clone(),
                    branch,
                    initial_head: active_tree.and_then(|w| w.head.clone()),
                    prompt: String::new(),
                    recent_commits: Vec::new(),
                    unselected_paths: Vec::new(),
                    frozen_files: Vec::new(),
                    planned_groups: Vec::new(),
                    skipped_file_ids: Vec::new(),
                    next_step_index: 0,
                    committed_file_ids: Vec::new(),
                    commits: Vec::new(),
                    qa_log: Vec::new(),
                    timeline: Vec::new(),
                    session_status: CPushSessionStatus::Conflict,
                    message: None,
                    can_retry_remaining: false,
                    push_authorized: false,
                    plan_only: false,
                    auto_collapse: false,
                    locale,
                    commit_language: locale,
                };
                enter_conflict_state(
                    &mut session,
                    &upstream,
                    &conflicted_files,
                    !model_id.is_empty(),
                    None,
                );
                self.save_in_memory(&session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: Some(snapshot.clone()),
                });
                let reply = CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request.request_id.clone(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: Some(snapshot),
                };
                self.inner
                    .processed_requests
                    .lock()
                    .map_err(|_| "C-Push request registry lock is unavailable")?
                    .insert(request.request_id.clone(), reply.clone());
                Ok(reply)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn plan_cpush(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &PlanCPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        let existing_active = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned();
        if let Some(active) = existing_active
            && is_active_status(active.session_status)
        {
            if active.plan_only && active.commits.is_empty() {
                let _ = self.remove_in_memory(&request.target.worktree_id);
                let _ = self.persist_all(storage_dir).await;
            } else {
                return Err(i18n::cpush_active_session_busy_error(locale, true).into());
            }
        }

        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let extracted = repo_store
            .extract_cpush_scope(&request.target, &request.selections, locale)
            .await?;

        let run_id = request.request_id.clone();
        let mut session = StoredCPushSession {
            run_id: run_id.clone(),
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            frozen_scope_version: request.target.scope_version.clone(),
            model_id: request.model_id.clone(),
            branch: extracted.branch,
            initial_head: extracted.initial_head,
            prompt: request.prompt.clone(),
            recent_commits: extracted.recent_commits,
            unselected_paths: extracted.unselected_paths,
            frozen_files: extracted.files,
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: Vec::new(),
            commits: Vec::new(),
            qa_log: Vec::new(),
            timeline: Vec::new(),
            session_status: CPushSessionStatus::Idle,
            message: None,
            can_retry_remaining: false,
            push_authorized: false,
            plan_only: true,
            auto_collapse: false,
            locale,
            commit_language: request.commit_language,
        };

        let plan = self
            .invoke_pi_planner(paths, supervisor, &spec, &run_id, &session)
            .await?;

        let reply = self
            .apply_plan_or_execute(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                plan,
                &mut on_progress,
            )
            .await?;

        self.inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .insert(request.request_id.clone(), reply.clone());
        Ok(reply)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_cpush(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &StartCPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        let existing_active = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned();
        if let Some(active) = existing_active
            && is_active_status(active.session_status)
        {
            if active.plan_only && active.commits.is_empty() {
                let _ = self.remove_in_memory(&request.target.worktree_id);
                let _ = self.persist_all(storage_dir).await;
            } else {
                return Err(i18n::cpush_active_session_busy_error(locale, true).into());
            }
        }

        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let extracted = repo_store
            .extract_cpush_scope(&request.target, &request.selections, locale)
            .await?;

        let run_id = request.request_id.clone();
        if let Some(confirmed) = &request.confirmed_plan {
            let (validated_groups, validated_skipped) =
                validate_confirmed_plan_in_rust(confirmed, &extracted.files, locale)?;
            let mut session = StoredCPushSession {
                run_id: run_id.clone(),
                repository_id: request.target.repository_id.clone(),
                worktree_id: request.target.worktree_id.clone(),
                session_id: request.target.session_id.clone(),
                frozen_scope_version: request.target.scope_version.clone(),
                model_id: request.model_id.clone(),
                branch: extracted.branch,
                initial_head: extracted.initial_head,
                prompt: request.prompt.clone(),
                recent_commits: extracted.recent_commits,
                unselected_paths: extracted.unselected_paths,
                frozen_files: extracted.files,
                planned_groups: validated_groups,
                skipped_file_ids: validated_skipped,
                next_step_index: 0,
                committed_file_ids: Vec::new(),
                commits: Vec::new(),
                qa_log: Vec::new(),
                timeline: Vec::new(),
                session_status: CPushSessionStatus::Running,
                message: None,
                can_retry_remaining: false,
                push_authorized: true,
                plan_only: false,
                auto_collapse: false,
                locale,
                commit_language: request.commit_language,
            };
            let reply = self
                .execute_planned_groups(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &mut on_progress,
                )
                .await?;
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        let badge = scope_badge(locale, &extracted.files);
        let initial_timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_scope_frozen(locale, &badge),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush_timeline_planning(locale).into(),
            },
        ];
        let progress_msg = CPushUiMessage {
            id: format!("progress-{run_id}"),
            kind: "progress".into(),
            title: i18n::cpush_progress_planning_title(locale).into(),
            description: i18n::cpush_progress_planning_desc(locale).into(),
            scope: Some(badge.clone()),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 1,
                total: 4,
                label: i18n::cpush_progress_planning_steps(locale).into(),
            }),
        };

        let mut session = StoredCPushSession {
            run_id: run_id.clone(),
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            frozen_scope_version: request.target.scope_version.clone(),
            model_id: request.model_id.clone(),
            branch: extracted.branch,
            initial_head: extracted.initial_head,
            prompt: request.prompt.clone(),
            recent_commits: extracted.recent_commits,
            unselected_paths: extracted.unselected_paths,
            frozen_files: extracted.files,
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: Vec::new(),
            commits: Vec::new(),
            qa_log: Vec::new(),
            timeline: initial_timeline,
            session_status: CPushSessionStatus::Running,
            message: Some(progress_msg),
            can_retry_remaining: false,
            push_authorized: true,
            plan_only: false,
            auto_collapse: false,
            locale,
            commit_language: request.commit_language,
        };
        self.save_in_memory(&session)?;
        let _ = self.persist_all(storage_dir).await;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let plan_result = self
            .invoke_pi_planner(paths, supervisor, &spec, &run_id, &session)
            .await;

        let plan = match plan_result {
            Ok(plan) => plan,
            Err(err) => {
                let _ = self.remove_in_memory(&session.worktree_id);
                let _ = self.persist_all(storage_dir).await;
                let idle_snapshot = CPushSessionSnapshot {
                    session_status: CPushSessionStatus::Idle,
                    message: None,
                    timeline: Vec::new(),
                    ..session.to_public()
                };
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: idle_snapshot,
                    repository_snapshot: None,
                });
                return Err(err);
            }
        };

        let reply = self
            .apply_plan_or_execute(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                plan,
                &mut on_progress,
            )
            .await?;

        self.inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .insert(request.request_id.clone(), reply.clone());
        Ok(reply)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn respond_cpush(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request: &RespondCPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        let req_locale = request.locale.unwrap_or_default();
        repository::validate_request_id_with_locale(&request.request_id, req_locale)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;
        let _op_guard = self.track_operation();

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        let mut session = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push session cache lock is unavailable")?
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or_else(|| i18n::cpush_no_pending_session_error(req_locale))?;
        if let Some(loc) = request.locale {
            session.locale = loc;
        }
        let locale = session.locale;

        if session.run_id != request.run_id
            || session.repository_id != request.target.repository_id
            || session.worktree_id != request.target.worktree_id
        {
            return Err(i18n::cpush_mismatched_reply_target_error(locale).into());
        }
        if !matches!(
            session.session_status,
            CPushSessionStatus::NeedsDecision
                | CPushSessionStatus::Conflict
                | CPushSessionStatus::WaitingManual
                | CPushSessionStatus::PushFailed
        ) {
            return Err(i18n::cpush_not_interactive_error(locale).into());
        }
        let current_msg = session
            .message
            .clone()
            .ok_or_else(|| i18n::cpush_missing_active_message_error(locale))?;
        if current_msg.id != request.message_id {
            return Err(i18n::cpush_stale_message_id_error(locale).into());
        }
        session.auto_collapse = false;
        if let Some(req_model) = &request.model_id
            && !req_model.trim().is_empty()
            && session.model_id.is_empty()
        {
            session.model_id = req_model.trim().to_owned();
        }

        // 1. Dismissing session (keeping any landed local commits intact):
        if request.option_id.as_deref() == Some("dismiss-session") {
            if session.commits.is_empty() {
                session.session_status = CPushSessionStatus::Idle;
                session.can_retry_remaining = false;
                session.message = None;
                self.remove_in_memory(&session.worktree_id)?;
            } else {
                session.session_status = CPushSessionStatus::Done;
                session.can_retry_remaining = false;
                let commit_summaries = summarize_commits(&session.commits);
                session.message = Some(CPushUiMessage {
                    id: format!("done-local-{}", session.run_id),
                    kind: "done".into(),
                    title: i18n::cpush_dismiss_kept_local_title(locale, session.commits.len()),
                    description: i18n::cpush_dismiss_kept_local_desc(locale).into(),
                    scope: Some(session.branch.clone()),
                    files: None,
                    commits: Some(commit_summaries),
                    options: None,
                    progress: None,
                });
                self.save_in_memory(&session)?;
            }
            self.persist_all(storage_dir).await?;
            let public = session.to_public();
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public.clone(),
                repository_snapshot: None,
            });
            let reply = CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request.request_id.clone(),
                session: public,
                prepared_plan: None,
                repository_snapshot: None,
            };
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 2. Retrying remaining local commits (`C04`):
        if request.option_id.as_deref() == Some("retry-remaining") {
            if !session.can_retry_remaining
                || session.next_step_index >= session.planned_groups.len()
            {
                return Err(i18n::cpush_no_retry_remaining_error(locale).into());
            }
            let reply = self
                .execute_planned_groups(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &mut on_progress,
                )
                .await?;
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 3. Retrying ONLY push (`retry-push` / `continue-push`, `R02`, `R04`) or setting upstream (`push-set-upstream:<remote>`, `R05`):
        if matches!(
            request.option_id.as_deref(),
            Some("retry-push" | "continue-push")
        ) {
            let reply = self
                .sync_and_push_commits(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    None,
                    None,
                    &mut on_progress,
                )
                .await?;
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        if let Some(chosen_remote) = request
            .option_id
            .as_deref()
            .and_then(|opt| opt.strip_prefix("push-set-upstream:"))
        {
            let chosen = chosen_remote.to_owned();
            session.qa_log.push(CPushQaItem {
                question_id: current_msg.id.clone(),
                question: current_msg.title.clone(),
                answer: i18n::cpush_push_track_label(locale, &chosen, &session.branch),
                option_id: request.option_id.clone(),
            });
            let reply = self
                .sync_and_push_commits(
                    storage_dir,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    Some(&chosen),
                    None,
                    &mut on_progress,
                )
                .await?;
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // Free-text reply to missing-upstream confirmation (`R05`):
        if current_msg.id.starts_with("missing-upstream-")
            && let Some(free_text) = &request.text
        {
            let trimmed = free_text.trim();
            if trimmed.is_empty() {
                return Err(i18n::cpush_enter_remote_or_select_error(locale).into());
            }
            let available: Vec<String> = current_msg
                .options
                .as_ref()
                .into_iter()
                .flatten()
                .filter_map(|o| o.id.strip_prefix("push-set-upstream:").map(str::to_owned))
                .collect();
            let matched: Vec<&String> = available
                .iter()
                .filter(|r| trimmed.contains(r.as_str()))
                .collect();
            if matched.len() == 1 {
                let chosen = matched[0].clone();
                session.qa_log.push(CPushQaItem {
                    question_id: current_msg.id.clone(),
                    question: current_msg.title.clone(),
                    answer: trimmed.to_owned(),
                    option_id: None,
                });
                let reply = self
                    .sync_and_push_commits(
                        storage_dir,
                        repo_store,
                        &request.request_id,
                        &mut session,
                        Some(&chosen),
                        None,
                        &mut on_progress,
                    )
                    .await?;
                self.inner
                    .processed_requests
                    .lock()
                    .map_err(|_| "C-Push request registry lock is unavailable")?
                    .insert(request.request_id.clone(), reply.clone());
                return Ok(reply);
            }
            return Err(i18n::cpush_ambiguous_remote_input_error(locale, &available));
        }

        // 4. Aborting active rebase (`abort-rebase`):
        if request.option_id.as_deref() == Some("abort-rebase") {
            let read_target = self.session_read_target(&session);
            let snapshot = repo_store.abort_active_rebase(&read_target, locale).await?;
            if session.commits.is_empty() {
                session.session_status = CPushSessionStatus::Idle;
                session.message = None;
                self.remove_in_memory(&session.worktree_id)?;
            } else {
                let branch_name = session.branch.clone();
                enter_push_failed_state(
                    &mut session,
                    &branch_name,
                    i18n::cpush_aborted_rebase_reason(locale),
                    Some(i18n::cpush_aborted_rebase_title(locale)),
                );
                self.save_in_memory(&session)?;
            }
            self.persist_all(storage_dir).await?;
            let public = session.to_public();
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public.clone(),
                repository_snapshot: Some(snapshot.clone()),
            });
            let reply = CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request.request_id.clone(),
                session: public,
                prepared_plan: None,
                repository_snapshot: Some(snapshot),
            };
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 5. Manual IDE merge flow (`manual-merge` & `recheck-manual`, `R03`, `R04`):
        if request.option_id.as_deref() == Some("manual-merge") {
            let read_target = self.session_read_target(&session);
            let (live, snapshot) = repo_store
                .inspect_live_remote_status(&read_target, false, locale)
                .await?;
            session.qa_log.push(CPushQaItem {
                question_id: current_msg.id.clone(),
                question: current_msg.title.clone(),
                answer: i18n::cpush_option_manual_merge(locale).into(),
                option_id: Some("manual-merge".into()),
            });
            session.session_status = CPushSessionStatus::WaitingManual;
            session.can_retry_remaining = false;
            session.timeline = vec![
                CPushTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush_timeline_chose_manual_merge(locale).into(),
                },
                CPushTimelineItem {
                    state: "active".into(),
                    label: i18n::cpush_timeline_waiting_return_reconcile(locale).into(),
                },
            ];
            let chips = if !live.conflicted_files.is_empty() {
                Some(conflict_chips(locale, &live.conflicted_files))
            } else {
                current_msg.files.clone()
            };
            session.message = Some(CPushUiMessage {
                id: format!("waiting-manual-{}", session.run_id),
                kind: "waitingManual".into(),
                title: i18n::cpush_waiting_manual_title(locale).into(),
                description: i18n::cpush_waiting_manual_desc(locale).into(),
                scope: Some(i18n::cpush_waiting_manual_scope(locale).into()),
                files: chips,
                commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
                options: Some(vec![CPushMessageOption {
                    id: "abort-rebase".into(),
                    label: i18n::cpush_option_abort_rebase(locale, !session.commits.is_empty())
                        .into(),
                    recommended: None,
                }]),
                progress: None,
            });
            self.save_in_memory(&session)?;
            self.persist_all(storage_dir).await?;
            let public = session.to_public();
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public.clone(),
                repository_snapshot: Some(snapshot.clone()),
            });
            let reply = CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request.request_id.clone(),
                session: public,
                prepared_plan: None,
                repository_snapshot: Some(snapshot),
            };
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 6. Authorized Agent Conflict Merge (`agent-merge` or free-text on `conflict`, `R03`):
        if session.session_status == CPushSessionStatus::Conflict
            && (request.option_id.as_deref() == Some("agent-merge") || request.text.is_some())
        {
            let instruction = request
                .text
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_owned();
            if request.option_id.is_none() && instruction.is_empty() {
                return Err(i18n::cpush_enter_merge_instruction_error(locale).into());
            }
            let reply = self
                .execute_agent_conflict_merge(
                    storage_dir,
                    paths,
                    supervisor,
                    models,
                    repo_store,
                    &request.request_id,
                    &mut session,
                    &current_msg,
                    &instruction,
                    &mut on_progress,
                )
                .await?;
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push request registry lock is unavailable")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 7. Answering a disambiguation question (`C02`):
        if session.session_status != CPushSessionStatus::NeedsDecision {
            return Err(i18n::cpush_unsupported_reply_state_error(locale).into());
        }
        let answer_label = if let Some(opt_id) = &request.option_id {
            let matched = current_msg
                .options
                .as_ref()
                .and_then(|opts| opts.iter().find(|o| &o.id == opt_id))
                .ok_or_else(|| i18n::cpush_invalid_option_error(locale))?;
            matched.label.clone()
        } else if let Some(free_text) = &request.text {
            let trimmed = free_text.trim();
            if trimmed.is_empty() {
                return Err(i18n::cpush_empty_reply_text_error(locale).into());
            }
            trimmed.to_owned()
        } else {
            return Err(i18n::cpush_choose_option_or_text_error(locale).into());
        };

        let previous_question_msg = current_msg.clone();
        let previous_timeline = session.timeline.clone();

        session.qa_log.push(CPushQaItem {
            question_id: current_msg.id.clone(),
            question: current_msg.title.clone(),
            answer: answer_label.clone(),
            option_id: request.option_id.clone(),
        });
        session.session_status = CPushSessionStatus::Running;
        session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_confirmed_decision(
                    locale,
                    &current_msg.title,
                    &answer_label,
                ),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush_timeline_replanning(locale).into(),
            },
        ];
        session.message = Some(CPushUiMessage {
            id: format!("progress-replan-{}", request.request_id),
            kind: "progress".into(),
            title: i18n::cpush_progress_replanning_title(locale).into(),
            description: i18n::cpush_progress_replanning_desc(locale, &answer_label),
            scope: Some(session.branch.clone()),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 2,
                total: 4,
                label: i18n::cpush_progress_replanning_steps(locale).into(),
            }),
        });
        self.save_in_memory(&session)?;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let spec = match models
            .resolve_runtime_spec_with_locale(storage_dir, &session.model_id, locale)
            .await
        {
            Ok(spec) => spec,
            Err(err) => {
                session.qa_log.pop();
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.timeline = previous_timeline;
                session.message = Some(previous_question_msg);
                self.save_in_memory(&session)?;
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: session.to_public(),
                    repository_snapshot: None,
                });
                return Err(err);
            }
        };

        let plan = match self
            .invoke_pi_planner(paths, supervisor, &spec, &request.request_id, &session)
            .await
        {
            Ok(plan) => plan,
            Err(err) => {
                session.qa_log.pop();
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.timeline = previous_timeline;
                session.message = Some(previous_question_msg);
                self.save_in_memory(&session)?;
                let _ = self.persist_all(storage_dir).await;
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: session.to_public(),
                    repository_snapshot: None,
                });
                return Err(err);
            }
        };

        let reply = self
            .apply_plan_or_execute(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                plan,
                &mut on_progress,
            )
            .await?;
        self.inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push request registry lock is unavailable")?
            .insert(request.request_id.clone(), reply.clone());
        Ok(reply)
    }

    fn session_read_target(&self, session: &StoredCPushSession) -> ReadTarget {
        ReadTarget {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_agent_conflict_merge(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        previous_msg: &CPushUiMessage,
        instruction: &str,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        if session.model_id.trim().is_empty() {
            return Err(i18n::cpush_agent_merge_no_model_error(locale).into());
        }
        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &session.model_id, locale)
            .await?;
        let read_target = self.session_read_target(session);
        let batch = repo_store
            .extract_conflict_batch(&read_target, locale)
            .await?;

        let answer_text = if instruction.is_empty() {
            i18n::cpush_option_agent_merge(locale).to_owned()
        } else {
            instruction.to_owned()
        };
        let prev_timeline = session.timeline.clone();
        let prev_msg = previous_msg.clone();

        session.qa_log.push(CPushQaItem {
            question_id: previous_msg.id.clone(),
            question: previous_msg.title.clone(),
            answer: answer_text.clone(),
            option_id: instruction.is_empty().then(|| "agent-merge".into()),
        });
        session.session_status = CPushSessionStatus::Running;
        session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_authorized_agent_merge(locale, &answer_text),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush_timeline_merging_files(locale, batch.files.len()),
            },
        ];
        session.message = Some(CPushUiMessage {
            id: format!("progress-merge-{request_id}"),
            kind: "progress".into(),
            title: i18n::cpush_progress_merging_title(locale).into(),
            description: i18n::cpush_progress_merging_desc(locale, instruction),
            scope: Some(i18n::cpush_merging_scope(locale, &batch.upstream)),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush_progress_merging_steps(locale).into(),
            }),
        });
        self.save_in_memory(session)?;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let resolution = match self
            .invoke_pi_conflict_resolver(
                paths,
                supervisor,
                &spec,
                request_id,
                session,
                &batch.upstream,
                instruction,
                &batch.files,
            )
            .await
        {
            Ok(res) => res,
            Err(err) => {
                session.qa_log.pop();
                session.session_status = CPushSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description = i18n::cpush_merge_resolver_failed_desc(locale, &err);
                session.message = Some(restored_msg);
                self.save_in_memory(session)?;
                let _ = self.persist_all(storage_dir).await;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: None,
                });
                return Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: None,
                });
            }
        };

        let continue_outcome = repo_store
            .apply_conflict_resolution_and_continue(
                &read_target,
                &batch.files,
                &resolution.resolved_files,
                locale,
            )
            .await;

        match continue_outcome {
            Err(err) => {
                let refreshed = repo_store
                    .refresh_with_locale(&read_target, locale)
                    .await
                    .ok();
                session.session_status = CPushSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description = i18n::cpush_merge_apply_failed_desc(locale, &err);
                session.message = Some(restored_msg);
                self.save_in_memory(session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: refreshed.clone(),
                });
                Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: refreshed,
                })
            }
            Ok(RebaseContinueOutcome::NextConflict {
                upstream,
                conflicted_files,
                snapshot,
            }) => {
                enter_conflict_state(
                    session,
                    &upstream,
                    &conflicted_files,
                    true,
                    Some(i18n::cpush_next_commit_conflict_reason(locale)),
                );
                self.save_in_memory(session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: Some(snapshot.clone()),
                });
                Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: Some(snapshot),
                })
            }
            Ok(RebaseContinueOutcome::Completed {
                upstream,
                rebased_commits,
                snapshot,
                ..
            }) => {
                apply_rebased_commits(session, &rebased_commits);
                if session.push_authorized {
                    self.sync_and_push_commits(
                        storage_dir,
                        repo_store,
                        request_id,
                        session,
                        None,
                        Some(snapshot),
                        on_progress,
                    )
                    .await
                } else {
                    // Standalone Pull & Rebase never auto-pushes (`R06`):
                    session.session_status = CPushSessionStatus::Done;
                    session.can_retry_remaining = false;
                    session.timeline = vec![
                        CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush_timeline_agent_merge_done(
                                locale,
                                &resolution.summary,
                            ),
                        },
                        CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush_timeline_rebase_done_no_push(locale, &upstream),
                        },
                    ];
                    let commits = summarize_commits(&session.commits);
                    session.message = Some(CPushUiMessage {
                        id: format!("done-merge-pull-{}", session.run_id),
                        kind: "done".into(),
                        title: i18n::cpush_done_merge_pull_title(locale, &upstream),
                        description: i18n::cpush_done_merge_pull_desc(locale, &resolution.summary),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commits.is_empty()).then_some(commits),
                        options: None,
                        progress: None,
                    });
                    self.save_in_memory(session)?;
                    self.persist_all(storage_dir).await?;
                    let public = session.to_public();
                    let _ = on_progress(CPushProgressEvent {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        session: public.clone(),
                        repository_snapshot: Some(snapshot.clone()),
                    });
                    Ok(CPushCommandReply {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        kind: "cPush",
                        request_id: request_id.to_owned(),
                        session: public,
                        prepared_plan: None,
                        repository_snapshot: Some(snapshot),
                    })
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn invoke_pi_conflict_resolver(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        spec: &crate::models::store::ResolvedRuntimeSpec,
        request_id: &str,
        session: &StoredCPushSession,
        upstream: &str,
        instruction: &str,
        files: &[ExtractedConflictFile],
    ) -> Result<PiConflictResolution, String> {
        let context = RequestContext {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        };
        let pi_request_id = format!("{request_id}-conflict");
        let files_wire: Vec<PiConflictFile> = files
            .iter()
            .map(|f| PiConflictFile {
                file_id: f.file_id.clone(),
                path: f.path.clone(),
                base_content: f.base_content.clone(),
                upstream_content: f.upstream_content.clone(),
                local_content: f.local_content.clone(),
                conflicted_content: f.conflicted_content.clone(),
            })
            .collect();
        let commits_wire: Vec<String> = session
            .commits
            .iter()
            .map(|c| format!("{} {}", c.short_oid, c.title))
            .collect();
        let command = PiCommand::ResolveConflict {
            request_id: pi_request_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: session.branch.clone(),
            upstream: upstream.to_owned(),
            user_instruction: instruction.to_owned(),
            commits: commits_wire,
            files: files_wire,
            locale: session.locale,
        };

        let outcome = supervisor
            .run_task(
                paths,
                pi_request_id,
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
            )
            .await?;

        match outcome {
            PiTaskOutcome::ConflictResolved { resolution } => Ok(resolution),
            PiTaskOutcome::Cancelled => {
                Err(i18n::cpush_conflict_cancelled_error(session.locale).into())
            }
            _ => Err("Conflict merge returned an unexpected event type".into()),
        }
    }

    async fn invoke_pi_planner(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        spec: &crate::models::store::ResolvedRuntimeSpec,
        request_id: &str,
        session: &StoredCPushSession,
    ) -> Result<PiCPushPlan, String> {
        let context = RequestContext {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        };
        let files_wire: Vec<PiPlanFileEntry> = session
            .frozen_files
            .iter()
            .map(|f| PiPlanFileEntry {
                file_id: f.file_id.clone(),
                path: f.path.clone(),
                previous_path: f.previous_path.clone(),
                status: f.status.clone(),
                range_kind: f.range_kind.clone(),
                added: f.added,
                removed: f.removed,
                ignored_for_review: f.ignored_for_review,
                binary: f.binary,
                patch: f.patch.clone(),
                skip_reason: f.skip_reason.clone(),
            })
            .collect();
        let qa_wire: Vec<PiCPushQaTurn> = session
            .qa_log
            .iter()
            .map(|qa| PiCPushQaTurn {
                question_id: qa.question_id.clone(),
                question: qa.question.clone(),
                answer: qa.answer.clone(),
                option_id: qa.option_id.clone(),
            })
            .collect();
        let command = PiCommand::PlanCPush {
            request_id: request_id.to_owned(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: session.branch.clone(),
            recent_commits: session.recent_commits.clone(),
            prompt: session.prompt.clone(),
            files: files_wire,
            unselected_paths: session.unselected_paths.clone(),
            qa_history: qa_wire,
            locale: session.locale,
            commit_language: session.commit_language,
        };

        let outcome = supervisor
            .run_task(
                paths,
                request_id.to_owned(),
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
            )
            .await?;

        match outcome {
            PiTaskOutcome::CPushPlanCompleted { plan } => validate_plan_in_rust(
                plan,
                &session.frozen_files,
                !session.qa_log.is_empty(),
                session.locale,
            ),
            PiTaskOutcome::Cancelled => {
                Err(i18n::cpush_plan_cancelled_error(session.locale).into())
            }
            _ => Err(
                "C-Push planning returned an unexpected event type; prompt and selection were preserved."
                    .into(),
            ),
        }
    }

    async fn apply_plan_or_execute(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        plan: PiCPushPlan,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        match plan {
            PiCPushPlan::Question {
                title,
                scope,
                description,
                affected_file_ids,
                options,
            } => {
                let by_id: HashMap<&str, &FrozenCPushFile> = session
                    .frozen_files
                    .iter()
                    .map(|f| (f.file_id.as_str(), f))
                    .collect();
                let chips: Vec<CPushMessageFileChip> = affected_file_ids
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()).copied())
                    .map(|file| CPushMessageFileChip {
                        id: file.file_id.clone(),
                        label: file_basename(&file.path),
                        badge: Some(if file.range_kind == "partial" {
                            i18n::cpush_partial_badge(locale).into()
                        } else {
                            i18n::cpush_needs_confirm_badge(locale).into()
                        }),
                    })
                    .collect();
                let question_id = format!("q-{}-{}", session.run_id, session.qa_log.len() + 1);
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.can_retry_remaining = false;
                session.timeline = vec![
                    CPushTimelineItem {
                        state: "done".into(),
                        label: i18n::cpush_timeline_frozen_count(
                            locale,
                            session.frozen_files.len(),
                        ),
                    },
                    CPushTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush_timeline_waiting_decision(locale, &title),
                    },
                ];
                session.message = Some(CPushUiMessage {
                    id: question_id,
                    kind: "question".into(),
                    title,
                    description,
                    scope: (!scope.trim().is_empty()).then_some(scope),
                    files: (!chips.is_empty()).then_some(chips),
                    commits: None,
                    options: Some(
                        options
                            .into_iter()
                            .map(|o| CPushMessageOption {
                                id: o.id,
                                label: o.label,
                                recommended: o.recommended,
                            })
                            .collect(),
                    ),
                    progress: None,
                });
                self.save_in_memory(session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: None,
                });
                Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: None,
                })
            }
            PiCPushPlan::Ready {
                summary,
                groups,
                skipped_file_ids,
            } => {
                if session.plan_only {
                    self.remove_in_memory(&session.worktree_id)?;
                    self.persist_all(storage_dir).await?;
                    let idle_public = CPushSessionSnapshot {
                        session_status: CPushSessionStatus::Idle,
                        message: None,
                        timeline: Vec::new(),
                        skipped_file_ids: skipped_file_ids.clone(),
                        ..session.to_public()
                    };
                    let _ = on_progress(CPushProgressEvent {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        session: idle_public.clone(),
                        repository_snapshot: None,
                    });
                    return Ok(CPushCommandReply {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        kind: "cPush",
                        request_id: request_id.to_owned(),
                        session: idle_public,
                        prepared_plan: Some(CPushPreparedPlan {
                            summary,
                            groups,
                            skipped_file_ids,
                        }),
                        repository_snapshot: None,
                    });
                }
                session.planned_groups = groups;
                session.skipped_file_ids = skipped_file_ids;
                session.next_step_index = 0;
                self.execute_planned_groups(
                    storage_dir,
                    repo_store,
                    request_id,
                    session,
                    on_progress,
                )
                .await
            }
        }
    }

    async fn execute_planned_groups(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let total = session.planned_groups.len();
        if total == 0 {
            let read_target = self.session_read_target(session);
            if session.push_authorized
                && let Ok((live_remote, snap)) = repo_store
                    .inspect_live_remote_status(&read_target, false, locale)
                    .await
                && live_remote.ahead > 0
            {
                return self
                    .sync_and_push_commits(
                        storage_dir,
                        repo_store,
                        request_id,
                        session,
                        None,
                        Some(snap),
                        on_progress,
                    )
                    .await;
            }
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            session.timeline = vec![CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_all_skipped(locale).into(),
            }];
            session.message = Some(CPushUiMessage {
                id: format!("done-skipped-{}", session.run_id),
                kind: "done".into(),
                title: i18n::cpush_all_skipped_title(locale).into(),
                description: i18n::cpush_all_skipped_desc(locale).into(),
                scope: Some(session.branch.clone()),
                files: None,
                commits: None,
                options: None,
                progress: None,
            });
            self.save_in_memory(session)?;
            self.persist_all(storage_dir).await?;
            let public = session.to_public();
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: public.clone(),
                repository_snapshot: None,
            });
            return Ok(CPushCommandReply {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                kind: "cPush",
                request_id: request_id.to_owned(),
                session: public,
                prepared_plan: None,
                repository_snapshot: None,
            });
        }

        let by_id: HashMap<String, FrozenCPushFile> = session
            .frozen_files
            .iter()
            .map(|f| (f.file_id.clone(), f.clone()))
            .collect();
        let read_target = self.session_read_target(session);
        let mut latest_snapshot: Option<RepositorySnapshot> = None;

        while session.next_step_index < total {
            let step_idx = session.next_step_index;
            let group = session.planned_groups[step_idx].clone();
            let step_title = group
                .message
                .lines()
                .next()
                .unwrap_or_else(|| i18n::cpush_commit_changes_fallback(locale))
                .to_owned();

            session.session_status = CPushSessionStatus::Running;
            session.can_retry_remaining = false;
            let mut timeline: Vec<CPushTimelineItem> = session
                .commits
                .iter()
                .enumerate()
                .map(|(i, c)| CPushTimelineItem {
                    state: "done".into(),
                    label: i18n::cpush_timeline_step_done(
                        locale,
                        i + 1,
                        total,
                        &c.short_oid,
                        &c.title,
                    ),
                })
                .collect();
            timeline.push(CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush_timeline_step_writing(locale, step_idx + 1, total, &step_title),
            });
            session.timeline = timeline;
            session.message = Some(CPushUiMessage {
                id: format!("progress-{}-step-{}", session.run_id, step_idx + 1),
                kind: "progress".into(),
                title: i18n::cpush_progress_step_title(locale, step_idx + 1, total),
                description: step_title.clone(),
                scope: Some(i18n::cpush_progress_step_scope(
                    locale,
                    &session.branch,
                    group.file_ids.len(),
                )),
                files: None,
                commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
                options: None,
                progress: Some(CPushMessageProgress {
                    completed: (step_idx + 1) as u32,
                    total: (total + 2) as u32,
                    label: i18n::cpush_progress_step_label(locale, step_idx + 1, total),
                }),
            });
            self.save_in_memory(session)?;
            let _ = on_progress(CPushProgressEvent {
                protocol_version: CPUSH_PROTOCOL_VERSION,
                session: session.to_public(),
                repository_snapshot: latest_snapshot.clone(),
            });

            let expected_head = if step_idx == 0 {
                session.initial_head.clone()
            } else {
                Some(session.commits[step_idx - 1].oid.clone())
            };
            let step_frozen_files: Vec<FrozenCPushFile> = group
                .file_ids
                .iter()
                .filter_map(|id| by_id.get(id).cloned())
                .collect();

            let step_outcome = async {
                let (_, selections) = repo_store
                    .resolve_cpush_step_selections(
                        &read_target,
                        &session.branch,
                        expected_head.as_deref(),
                        &step_frozen_files,
                        locale,
                    )
                    .await?;
                let step_req = CommitRequest {
                    request_id: format!("{request_id}-s{}", step_idx + 1),
                    target: read_target.clone(),
                    selections,
                    message: group.message.clone(),
                    push: false,
                    locale,
                };
                repo_store.commit(&step_req).await
            }
            .await;

            match step_outcome {
                Ok((snapshot, commit_info)) => {
                    latest_snapshot = Some(snapshot.clone());
                    session.commits.push(CPushLandedCommit {
                        oid: commit_info.oid,
                        short_oid: commit_info.short_oid,
                        title: commit_info.title,
                        branch: commit_info.branch,
                        file_ids: group.file_ids.clone(),
                    });
                    session.committed_file_ids.extend(group.file_ids.clone());
                    session.next_step_index = step_idx + 1;
                    self.save_in_memory(session)?;
                    self.persist_all(storage_dir).await?;
                }
                Err(err) => {
                    if latest_snapshot.is_none()
                        && let Ok(refreshed) =
                            repo_store.refresh_with_locale(&read_target, locale).await
                    {
                        latest_snapshot = Some(refreshed);
                    }
                    let failed_chips: Vec<CPushMessageFileChip> = step_frozen_files
                        .iter()
                        .map(|f| CPushMessageFileChip {
                            id: f.file_id.clone(),
                            label: file_basename(&f.path),
                            badge: (f.range_kind == "partial")
                                .then(|| i18n::cpush_partial_badge(locale).into()),
                        })
                        .collect();
                    let commit_summaries = summarize_commits(&session.commits);
                    let mut failed_timeline: Vec<CPushTimelineItem> = session
                        .commits
                        .iter()
                        .enumerate()
                        .map(|(i, c)| CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush_timeline_step_done(
                                locale,
                                i + 1,
                                total,
                                &c.short_oid,
                                &c.title,
                            ),
                        })
                        .collect();
                    failed_timeline.push(CPushTimelineItem {
                        state: "failed".into(),
                        label: i18n::cpush_timeline_step_paused(
                            locale,
                            step_idx + 1,
                            total,
                            &step_title,
                        ),
                    });

                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = true;
                    session.timeline = failed_timeline;
                    session.message = Some(CPushUiMessage {
                        id: format!("err-{}-step-{}", session.run_id, step_idx + 1),
                        kind: "error".into(),
                        title: i18n::cpush_step_paused_title(locale, step_idx, total),
                        description: i18n::cpush_step_paused_desc(locale, &err, step_idx > 0),
                        scope: Some(i18n::cpush_step_scope(locale, step_idx + 1, total)),
                        files: (!failed_chips.is_empty()).then_some(failed_chips),
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            CPushMessageOption {
                                id: "retry-remaining".into(),
                                label: i18n::cpush_option_retry_remaining(locale).into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
                                id: "dismiss-session".into(),
                                label: i18n::cpush_option_close_or_keep_completed(
                                    locale,
                                    step_idx > 0,
                                )
                                .into(),
                                recommended: None,
                            },
                        ]),
                        progress: None,
                    });
                    self.save_in_memory(session)?;
                    self.persist_all(storage_dir).await?;
                    let public = session.to_public();
                    let _ = on_progress(CPushProgressEvent {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        session: public.clone(),
                        repository_snapshot: latest_snapshot.clone(),
                    });
                    return Ok(CPushCommandReply {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        kind: "cPush",
                        request_id: request_id.to_owned(),
                        session: public,
                        prepared_plan: None,
                        repository_snapshot: latest_snapshot,
                    });
                }
            }
        }

        // All planned local commits landed — proceed directly to remote sync & push (`M5`)!
        self.sync_and_push_commits(
            storage_dir,
            repo_store,
            request_id,
            session,
            None,
            latest_snapshot,
            on_progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn sync_and_push_commits(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        set_upstream_remote: Option<&str>,
        initial_snapshot: Option<RepositorySnapshot>,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        let read_target = self.session_read_target(session);
        let commit_summaries = summarize_commits(&session.commits);

        // Emit pushing progress:
        session.session_status = CPushSessionStatus::Running;
        session.can_retry_remaining = false;
        session.auto_collapse = false;
        let mut push_timeline: Vec<CPushTimelineItem> = session
            .commits
            .iter()
            .map(|c| CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_local_commit_titled(locale, &c.short_oid, &c.title),
            })
            .collect();
        push_timeline.push(CPushTimelineItem {
            state: "active".into(),
            label: i18n::cpush_timeline_pushing(locale).into(),
        });
        session.timeline = push_timeline;
        session.message = Some(CPushUiMessage {
            id: format!("progress-push-{request_id}"),
            kind: "progress".into(),
            title: i18n::cpush_progress_pushing_title(locale).into(),
            description: i18n::cpush_progress_pushing_desc(locale).into(),
            scope: Some(session.branch.clone()),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries.clone()),
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush_progress_pushing_steps(locale).into(),
            }),
        });
        self.save_in_memory(session)?;
        let _ = self.persist_all(storage_dir).await;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: initial_snapshot.clone(),
        });

        let push_result = repo_store
            .push_branch(&read_target, set_upstream_remote, locale)
            .await;

        let final_snapshot = match push_result {
            Ok(PushOutcome::Pushed {
                remote_branch,
                snapshot,
            }) => {
                self.mark_pushed_done(session, &remote_branch, false);
                Some(snapshot)
            }
            Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                if remotes.is_empty() {
                    let branch_name = session.branch.clone();
                    enter_push_failed_state(
                        session,
                        &branch_name,
                        i18n::cpush_no_remote_configured_reason(locale),
                        Some(i18n::cpush_no_remote_configured_title(locale)),
                    );
                } else {
                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = false;
                    let mut timeline: Vec<CPushTimelineItem> = session
                        .commits
                        .iter()
                        .map(|c| CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush_timeline_local_commit_written(locale, &c.short_oid),
                        })
                        .collect();
                    timeline.push(CPushTimelineItem {
                        state: "active".into(),
                        label: i18n::cpush_timeline_waiting_upstream_choice(locale).into(),
                    });
                    session.timeline = timeline;

                    let mut options: Vec<CPushMessageOption> = remotes
                        .iter()
                        .enumerate()
                        .map(|(idx, r)| CPushMessageOption {
                            id: format!("push-set-upstream:{r}"),
                            label: i18n::cpush_push_track_label(locale, r, &session.branch),
                            recommended: (remotes.len() == 1 && idx == 0).then_some(true),
                        })
                        .collect();
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: i18n::cpush_option_keep_local_only(locale).into(),
                        recommended: None,
                    });
                    session.message = Some(CPushUiMessage {
                        id: format!(
                            "missing-upstream-{}-{}",
                            session.run_id,
                            session.qa_log.len() + 1
                        ),
                        kind: "question".into(),
                        title: i18n::cpush_missing_upstream_question_title(locale).into(),
                        description: i18n::cpush_missing_upstream_question_desc(
                            locale,
                            &session.branch,
                        ),
                        scope: Some(i18n::cpush_branch_scope(locale, &session.branch)),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(options),
                        progress: None,
                    });
                }
                Some(snapshot)
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                non_fast_forward: true,
                snapshot: pre_sync_snapshot,
                ..
            }) => {
                // Remote advanced (`behind > 0`): attempt safe sync rebase via the shared engine!
                match repo_store
                    .sync_rebase_onto_upstream_with_locale(&read_target, locale)
                    .await
                {
                    outcome @ (Ok(RebaseSyncOutcome::Completed { .. })
                    | Ok(RebaseSyncOutcome::AlreadyUpToDate { .. })) => {
                        let reb_snap = match outcome {
                            Ok(RebaseSyncOutcome::Completed {
                                rebased_commits,
                                snapshot,
                                ..
                            }) => {
                                apply_rebased_commits(session, &rebased_commits);
                                snapshot
                            }
                            Ok(RebaseSyncOutcome::AlreadyUpToDate { snapshot, .. }) => snapshot,
                            _ => unreachable!(),
                        };
                        match repo_store.push_branch(&read_target, None, locale).await {
                            Ok(PushOutcome::Pushed {
                                remote_branch: pushed_target,
                                snapshot: pushed_snap,
                            }) => {
                                self.mark_pushed_done(session, &pushed_target, true);
                                Some(pushed_snap)
                            }
                            Ok(PushOutcome::Failed {
                                remote_branch: failed_target,
                                reason,
                                snapshot: failed_snap,
                                ..
                            }) => {
                                enter_push_failed_state(
                                    session,
                                    &failed_target,
                                    &reason,
                                    Some(i18n::cpush_rebased_push_failed_title(locale)),
                                );
                                Some(failed_snap)
                            }
                            Ok(PushOutcome::MissingUpstream { snapshot: s, .. }) => {
                                enter_push_failed_state(
                                    session,
                                    &remote_branch,
                                    i18n::cpush_missing_upstream_short_reason(locale),
                                    None,
                                );
                                Some(s)
                            }
                            Err(err) => {
                                enter_push_failed_state(session, &remote_branch, &err, None);
                                Some(reb_snap)
                            }
                        }
                    }
                    Ok(RebaseSyncOutcome::Conflict {
                        upstream,
                        conflicted_files,
                        snapshot: conf_snap,
                    }) => {
                        let has_model = !session.model_id.is_empty();
                        enter_conflict_state(
                            session,
                            &upstream,
                            &conflicted_files,
                            has_model,
                            None,
                        );
                        Some(conf_snap)
                    }
                    Ok(RebaseSyncOutcome::UnsafeDirtyOverlap {
                        upstream,
                        overlapping_paths,
                        snapshot: ov_snap,
                    }) => {
                        enter_push_failed_state(
                            session,
                            &upstream,
                            &i18n::cpush_dirty_overlap_push_failed_reason(
                                locale,
                                &overlapping_paths,
                            ),
                            Some(i18n::cpush_dirty_overlap_push_failed_title(locale)),
                        );
                        Some(ov_snap)
                    }
                    Ok(RebaseSyncOutcome::UnsafePublishedHistory {
                        upstream,
                        reason: pub_reason,
                        snapshot: pub_snap,
                    }) => {
                        enter_push_failed_state(
                            session,
                            &upstream,
                            &pub_reason,
                            Some(i18n::cpush_published_history_risk_title(locale)),
                        );
                        Some(pub_snap)
                    }
                    Ok(RebaseSyncOutcome::MissingUpstream { snapshot: ms, .. }) => {
                        enter_push_failed_state(
                            session,
                            &remote_branch,
                            i18n::cpush_missing_upstream_short_reason(locale),
                            None,
                        );
                        Some(ms)
                    }
                    Err(sync_err) => {
                        enter_push_failed_state(session, &remote_branch, &sync_err, None);
                        Some(pre_sync_snapshot)
                    }
                }
            }
            Ok(PushOutcome::Failed {
                remote_branch,
                non_fast_forward: false,
                reason,
                snapshot,
            }) => {
                enter_push_failed_state(session, &remote_branch, &reason, None);
                Some(snapshot)
            }
            Err(err) => {
                let fallback_snap = match repo_store.refresh_with_locale(&read_target, locale).await
                {
                    Ok(s) => Some(s),
                    Err(_) => initial_snapshot,
                };
                let branch_name = session.branch.clone();
                enter_push_failed_state(session, &branch_name, &err, None);
                fallback_snap
            }
        };

        self.save_in_memory(session)?;
        self.persist_all(storage_dir).await?;
        let public = session.to_public();
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: public.clone(),
            repository_snapshot: final_snapshot.clone(),
        });
        Ok(CPushCommandReply {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            kind: "cPush",
            request_id: request_id.to_owned(),
            session: public,
            prepared_plan: None,
            repository_snapshot: final_snapshot,
        })
    }

    fn mark_pushed_done(
        &self,
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
                label: i18n::cpush_timeline_commit_item(locale, &c.short_oid, &c.title),
            })
            .collect();
        if rebased {
            timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush_timeline_rebased_synced(locale, remote_branch),
            });
        }
        timeline.push(CPushTimelineItem {
            state: "done".into(),
            label: i18n::cpush_pushed_done_title(locale, count, remote_branch),
        });

        session.timeline = timeline;
        session.session_status = CPushSessionStatus::Done;
        session.can_retry_remaining = false;
        session.auto_collapse = false;

        session.message = Some(CPushUiMessage {
            id: format!("done-{}", session.run_id),
            kind: "done".into(),
            title: i18n::cpush_pushed_done_title(locale, count, remote_branch),
            description: i18n::cpush_pushed_done_description(
                locale,
                session.skipped_file_ids.len(),
            ),
            scope: Some(i18n::cpush_synced_scope(locale, &session.branch)),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
            options: None,
            progress: None,
        });
    }
}
