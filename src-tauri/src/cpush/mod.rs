//! M4 & M5 C-Push, manual Commit-and-Push, and Pull-Rebase coordinator:
//! freezes authorized file scope, coordinates Pi Core split planning & authorized
//! conflict resolution, serializes Git writes per repository, preserves completed
//! commits on partial/push failure without repeating finished steps, reconciles
//! manual IDE merge & terminal push states against live Git + remote, and persists
//! session history in `cpush-sessions.json` (`0600`).
#[cfg(test)]
mod m5_tests;
#[cfg(test)]
mod tests;
pub mod types;

use crate::{
    foundation::pi::{
        PiCPushPlan, PiConflictResolution, PiPaths, PiSupervisor, PiTaskOutcome, RequestContext,
    },
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
    sync::{Arc, Mutex},
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
}

fn file_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_owned()
}

fn scope_badge(files: &[FrozenCPushFile]) -> String {
    let partial_count = files.iter().filter(|f| f.range_kind == "partial").count();
    if partial_count > 0 {
        format!(
            "{} 个已选文件 · 含 {} 项部分暂存",
            files.len(),
            partial_count
        )
    } else {
        format!("{} 个已选文件", files.len())
    }
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

fn conflict_chips(files: &[ConflictedFileSummary]) -> Vec<CPushMessageFileChip> {
    files
        .iter()
        .map(|f| CPushMessageFileChip {
            id: f.file_id.clone(),
            label: f.path.clone(),
            badge: Some("冲突".into()),
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

fn build_conflict_options(has_model: bool, has_commits: bool) -> Vec<CPushMessageOption> {
    let mut opts = Vec::new();
    if has_model {
        opts.push(CPushMessageOption {
            id: "agent-merge".into(),
            label: "让 Agent 合并".into(),
            recommended: Some(true),
        });
    }
    opts.push(CPushMessageOption {
        id: "manual-merge".into(),
        label: "我手动合并".into(),
        recommended: (!has_model).then_some(true),
    });
    opts.push(CPushMessageOption {
        id: "abort-rebase".into(),
        label: if has_commits {
            "中止变基（保留本地提交）".into()
        } else {
            "中止变基并恢复原状".into()
        },
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
    session.session_status = CPushSessionStatus::Conflict;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let names: Vec<&str> = conflicted_files.iter().map(|f| f.path.as_str()).collect();
    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: format!("本地提交 {}：{}", c.short_oid, c.title),
        })
        .collect();
    timeline.push(CPushTimelineItem {
        state: "failed".into(),
        label: format!("同步 {upstream} 时遇到冲突：{}", names.join("、")),
    });
    session.timeline = timeline;

    let base_desc = format!(
        "变基至 {upstream} 时 {} 发生代码冲突。可点击文件在编辑器中查看冲突，选择处理方式或输入合并要求：",
        names.join("、")
    );
    let description = match extra_reason {
        Some(reason) if !reason.trim().is_empty() => format!("{reason}。{base_desc}"),
        _ => base_desc,
    };
    let commits = summarize_commits(&session.commits);
    session.message = Some(CPushUiMessage {
        id: format!("conflict-{}-{}", session.run_id, session.qa_log.len() + 1),
        kind: "conflict".into(),
        title: "同步远端改动遇到代码冲突".into(),
        description,
        scope: Some(format!("阶段：rebase {upstream}")),
        files: Some(conflict_chips(conflicted_files)),
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(build_conflict_options(
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
    session.session_status = CPushSessionStatus::PushFailed;
    session.can_retry_remaining = false;
    session.auto_collapse = false;

    let mut timeline: Vec<CPushTimelineItem> = session
        .commits
        .iter()
        .map(|c| CPushTimelineItem {
            state: "done".into(),
            label: format!("本地提交 {} 写入成功", c.short_oid),
        })
        .collect();
    timeline.push(CPushTimelineItem {
        state: "failed".into(),
        label: format!("推送至 {remote_target} 失败"),
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
        title: title_override.unwrap_or("已在本地提交，推送失败").into(),
        description: format!(
            "本地提交已成功写入历史，推送至 {remote_target} 未成功（{reason}）。点击「重试推送」仅推送已有提交，不会重复提交。"
        ),
        scope: Some(format!("阶段：推送至 {remote_target}")),
        files: None,
        commits: (!commits.is_empty()).then_some(commits),
        options: Some(vec![
            CPushMessageOption {
                id: "retry-push".into(),
                label: "重试推送".into(),
                recommended: Some(true),
            },
            CPushMessageOption {
                id: "dismiss-session".into(),
                label: "保留本地提交并结束".into(),
                recommended: None,
            },
        ]),
        progress: None,
    });
}

fn read_sessions_file(storage_dir: &Path) -> (Vec<StoredCPushSession>, Option<String>) {
    let path = storage_dir.join(CPUSH_STORAGE_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(_) => {
            return (
                Vec::new(),
                Some("无法读取本地 C-Push 会话记录，已从空白会话记录启动。".into()),
            );
        }
    };
    match serde_json::from_slice::<StoredCPushFile>(&bytes) {
        Ok(parsed) if parsed.version == CPUSH_STORAGE_VERSION => (parsed.sessions, None),
        _ => (
            Vec::new(),
            Some("本地 C-Push 会话记录损坏或版本不兼容，已忽略旧记录；原文件未被覆盖。".into()),
        ),
    }
}

fn write_sessions_file_atomic(
    storage_dir: &Path,
    sessions: &[StoredCPushSession],
) -> Result<(), String> {
    fs::create_dir_all(storage_dir)
        .map_err(|e| format!("无法创建应用数据目录保存会话记录：{e}"))?;
    let payload = serde_json::to_vec_pretty(&StoredCPushFile {
        version: CPUSH_STORAGE_VERSION,
        sessions: sessions.iter().take(MAX_STORED_SESSIONS).cloned().collect(),
    })
    .map_err(|e| format!("无法序列化 C-Push 会话记录：{e}"))?;

    let mut temp =
        NamedTempFile::new_in(storage_dir).map_err(|e| format!("无法创建会话记录临时文件：{e}"))?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("无法设置会话记录权限：{e}"))?;
    temp.write_all(&payload)
        .map_err(|e| format!("写入会话记录失败：{e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("同步会话记录失败：{e}"))?;
    temp.persist(storage_dir.join(CPUSH_STORAGE_FILE))
        .map_err(|e| format!("原子更新会话记录失败：{e}"))?;
    Ok(())
}

fn validate_plan_in_rust(
    plan: PiCPushPlan,
    frozen_files: &[FrozenCPushFile],
    allow_skips: bool,
    raw_draft: &str,
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
                return Err("模型返回的 C-Push 提问内容不完整".into());
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
                return Err("未经确认，模型不能跳过已勾选的变更文件".into());
            }
            let mut seen = HashSet::with_capacity(frozen_files.len());
            for skipped in &skipped_file_ids {
                if !authorized_ids.contains(skipped.as_str()) {
                    return Err("C-Push 规划包含未经授权的跳过文件".into());
                }
                if !seen.insert(skipped.as_str()) {
                    return Err("C-Push 规划存在重复的文件归属".into());
                }
            }
            for group in &groups {
                if group.file_ids.is_empty() || group.message.trim().is_empty() {
                    return Err("C-Push 规划存在空文件组或空提交说明".into());
                }
                for id in &group.file_ids {
                    if !authorized_ids.contains(id.as_str()) {
                        return Err("C-Push 规划尝试提交未勾选文件，已被桌面层拦截".into());
                    }
                    if !seen.insert(id.as_str()) {
                        return Err("同一文件被重复分配到多个提交分组".into());
                    }
                }
            }
            if seen.len() != authorized_ids.len() {
                return Err("C-Push 规划未完整覆盖全部已勾选文件".into());
            }
            let trimmed_draft = raw_draft.trim();
            if groups.len() > 1
                && !trimmed_draft.is_empty()
                && groups.iter().all(|g| g.message.trim() == trimmed_draft)
            {
                return Err(
                    "多提交拆分不能将同一份人工草稿原样复制到所有提交；请重试或补充说明归属".into(),
                );
            }
            Ok(PiCPushPlan::Ready {
                summary,
                groups,
                skipped_file_ids,
            })
        }
    }
}

const CPUSH_AI_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

impl CPushCoordinator {
    fn worktree_gate(&self, worktree_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
        Ok(self
            .inner
            .worktree_gates
            .lock()
            .map_err(|_| "C-Push 协调器锁不可用")?
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
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .values()
            .cloned()
            .collect();
        write_sessions_file_atomic(storage_dir, &all)
    }

    fn save_in_memory(&self, session: &StoredCPushSession) -> Result<(), String> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .insert(session.worktree_id.clone(), session.clone());
        Ok(())
    }

    fn remove_in_memory(&self, worktree_id: &str) -> Result<(), String> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .remove(worktree_id);
        Ok(())
    }

    pub async fn restore(
        &self,
        storage_dir: &Path,
        snapshots: &[RepositorySnapshot],
    ) -> Result<(Vec<CPushSessionSnapshot>, Option<String>), String> {
        let _guard = self.inner.storage_gate.lock().await;
        let (stored_list, warning) = read_sessions_file(storage_dir);
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
            .map_err(|_| "C-Push 会话缓存锁不可用")?;
        let mut public_sessions = Vec::new();
        let mut dirty = false;

        for mut session in stored_list {
            session.auto_collapse = false;
            // Interrupted `Running` sessions must NEVER auto-rerun commits, rebases, or pushes (`W04`):
            if session.session_status == CPushSessionStatus::Running {
                dirty = true;
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
                                .unwrap_or("提交已完成")
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
                            _ => "远端分支",
                        };
                        let has_model = !session.model_id.is_empty();
                        enter_conflict_state(
                            &mut session,
                            upstream,
                            &live_conflicts,
                            has_model,
                            Some("应用重启后检测到未完成的合并冲突，未自动继续写入"),
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
                        title: "上次会话已完成本地提交后中断，尚未确认推送".into(),
                        description: "已核对真实 Git 历史并保留已完成提交，未自动重跑推送。请确认后点击「重试推送」或保留本地提交。".into(),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            CPushMessageOption {
                                id: "retry-push".into(),
                                label: "重试推送".into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
                                id: "dismiss-session".into(),
                                label: "保留本地提交并结束".into(),
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
                            label: "确认继续剩余提交".into(),
                            recommended: Some(true),
                        });
                    }
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: if session.commits.is_empty() {
                            "结束中断会话".into()
                        } else {
                            "结束会话（保留已完成提交）".into()
                        },
                        recommended: (!has_remaining_commits).then_some(true),
                    });
                    session.message = Some(CPushUiMessage {
                        id: format!("interrupted-{}", session.run_id),
                        kind: "error".into(),
                        title: if session.commits.is_empty() {
                            "上次 C-Push 会话在写入前中断".into()
                        } else {
                            format!("上次会话已完成 {} 个提交后中断", session.commits.len())
                        },
                        description:
                            "已核对真实 Git 状态，未自动重跑写入。请确认继续剩余步骤或结束本次会话。"
                                .into(),
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
        repository::validate_request_id(&request.request_id)?;
        let candidate_status = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
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
            .map_err(|_| "C-Push 会话缓存锁不可用")?
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

        let (local_live, local_snapshot) = repo_store
            .inspect_live_remote_status(&request.target, false)
            .await?;

        // 1. If conflicts or rebase are still unfinished -> keep waiting without auto-writing (`R04`):
        if local_live.rebase_in_progress
            || local_live.merge_or_other_in_progress
            || !local_live.conflicted_files.is_empty()
        {
            if session.session_status == CPushSessionStatus::WaitingManual {
                let chips = conflict_chips(&local_live.conflicted_files);
                let desc = if !local_live.conflicted_files.is_empty() {
                    let names: Vec<&str> = local_live
                        .conflicted_files
                        .iter()
                        .map(|f| f.path.as_str())
                        .collect();
                    format!(
                        "检测到 {} 仍存在未解决冲突，请在外部编辑器继续处理并完成变基后切回。",
                        names.join("、")
                    )
                } else {
                    "冲突标记已清除，但变基尚未结束；请在 IDE 或终端完成变基（git rebase --continue）后切回。".into()
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
            .inspect_live_remote_status(&request.target, true)
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
                    label: format!("检测到终端已完成推送至 {}", spec.display),
                });
                let commits = summarize_commits(&session.commits);
                session.message = Some(CPushUiMessage {
                    id: format!("done-ext-push-{}", session.run_id),
                    kind: "done".into(),
                    title: format!("已推送至 {}（终端已完成）", spec.display),
                    description: "经联网核对远端分支已包含最新提交，已自动收起旧提示。".into(),
                    scope: Some(format!("{} · 已同步", session.branch)),
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
                question: "合并冲突处理".into(),
                answer: "在外部完成合并并在终端推送".into(),
                option_id: None,
            });
            session.timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: format!("检测到冲突已解决且终端已推送至 {}", spec.display),
            });
            let commits = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("done-manual-pushed-{}", session.run_id),
                kind: "done".into(),
                title: format!("已推送至 {}（终端已完成）", spec.display),
                description: "经核对远端分支已包含合并后的最新提交，已自动归档会话并收起旧提问。"
                    .into(),
                scope: Some(format!("{} · 已同步", session.branch)),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: None,
                progress: None,
            });
        } else if !session.push_authorized {
            // Standalone `拉取并变基` never auto-pushes (`R06`):
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            session.auto_collapse = false;
            session.timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: "检测到手动冲突合并与变基已完成".into(),
            });
            let commits = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("done-manual-rebase-{}", session.run_id),
                kind: "done".into(),
                title: "冲突已解决，变基已完成".into(),
                description:
                    "已在外部完成冲突合并与变基；单独发起的拉取不包含自动推送权限，如需推送请明确触发。"
                        .into(),
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
                .unwrap_or("远端分支");
            session.session_status = CPushSessionStatus::NeedsDecision;
            session.can_retry_remaining = false;
            session.auto_collapse = false;
            session.timeline = vec![
                CPushTimelineItem {
                    state: "done".into(),
                    label: "检测到手动冲突合并与变基已完成".into(),
                },
                CPushTimelineItem {
                    state: "active".into(),
                    label: format!("本地提交尚未推送至 {target_label}，等待确认继续推送"),
                },
            ];
            let commits = summarize_commits(&session.commits);
            session.message = Some(CPushUiMessage {
                id: format!("resolved-continue-push-{}", session.run_id),
                kind: "resolved".into(),
                title: "冲突已解决，是否继续推送？".into(),
                description: format!(
                    "检测到冲突已解决但尚未推送至 {target_label}。确认后点击「继续推送」："
                ),
                scope: Some(format!("阶段：推送至 {target_label}")),
                files: None,
                commits: (!commits.is_empty()).then_some(commits),
                options: Some(vec![
                    CPushMessageOption {
                        id: "continue-push".into(),
                        label: "继续推送".into(),
                        recommended: Some(true),
                    },
                    CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: "暂不推送（保留本地提交）".into(),
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

    /// Standalone `拉取并变基` (`push_authorized = false`, `R06`).
    pub async fn pull_rebase(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &PullRebaseRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        repository::validate_request_id(&request.request_id)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push 请求记录锁不可用")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        if let Some(active) = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .get(&request.target.worktree_id)
            .cloned()
            && is_active_status(active.session_status)
        {
            return Err("当前工作树已有未结束的会话事项，请先在下方消息栏处理。".into());
        }

        let outcome = repo_store
            .sync_rebase_onto_upstream(&request.target)
            .await?;
        match outcome {
            RebaseSyncOutcome::MissingUpstream { remotes, .. } => {
                if remotes.is_empty() {
                    Err("当前仓库尚未配置任何 Git 远端（remote），无法执行拉取并变基；草稿与选择已保留。".into())
                } else {
                    Err(format!(
                        "当前分支尚未设置跟踪远端分支（可用远端：{}），无法确定拉取目标；草稿与选择已保留。",
                        remotes.join("、")
                    ))
                }
            }
            RebaseSyncOutcome::UnsafeDirtyOverlap {
                upstream,
                overlapping_paths,
                ..
            } => Err(format!(
                "远端 {upstream} 的更新涉及当前存在未提交改动的 {}，无法在不损伤暂存/草稿边界的前提下自动变基；已暂停操作，草稿与选择完整保留。",
                overlapping_paths.join("、")
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
                    repository_snapshot: Some(snapshot),
                };
                self.inner
                    .processed_requests
                    .lock()
                    .map_err(|_| "C-Push 请求记录锁不可用")?
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
                    draft: String::new(),
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
                    auto_collapse: false,
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
                    repository_snapshot: Some(snapshot),
                };
                self.inner
                    .processed_requests
                    .lock()
                    .map_err(|_| "C-Push 请求记录锁不可用")?
                    .insert(request.request_id.clone(), reply.clone());
                Ok(reply)
            }
        }
    }

    /// Manual `提交并推送` (`A01`, `R02`, `R05`): works with or without an AI model.
    pub async fn commit_and_push(
        &self,
        storage_dir: &Path,
        repo_store: &RepositoryStore,
        request: &CommitAndPushRequest,
        mut on_progress: impl FnMut(CPushProgressEvent) -> Result<(), String> + Send,
    ) -> Result<CPushCommandReply, String> {
        repository::validate_request_id(&request.request_id)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push 请求记录锁不可用")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        if let Some(active) = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .get(&request.target.worktree_id)
            .cloned()
            && is_active_status(active.session_status)
        {
            return Err("当前工作树已有未结束的会话事项，请先在下方消息栏处理。".into());
        }

        let commit_req = CommitRequest {
            request_id: format!("{}-local", request.request_id),
            target: request.target.clone(),
            selections: request.selections.clone(),
            message: request.message.clone(),
            push: false,
        };
        let (post_commit_snapshot, commit_info) = repo_store.commit(&commit_req).await?;
        let committed_ids: Vec<String> = request
            .selections
            .iter()
            .map(|s| s.file_id.clone())
            .collect();
        let model_id = request.model_id.clone().unwrap_or_default();

        let mut session = StoredCPushSession {
            run_id: request.request_id.clone(),
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            frozen_scope_version: request.target.scope_version.clone(),
            model_id,
            branch: commit_info.branch.clone(),
            initial_head: commit_info.parent.clone(),
            draft: request.message.clone(),
            recent_commits: Vec::new(),
            unselected_paths: Vec::new(),
            frozen_files: Vec::new(),
            planned_groups: Vec::new(),
            skipped_file_ids: Vec::new(),
            next_step_index: 0,
            committed_file_ids: committed_ids.clone(),
            commits: vec![CPushLandedCommit {
                oid: commit_info.oid,
                short_oid: commit_info.short_oid,
                title: commit_info.title,
                branch: commit_info.branch,
                file_ids: committed_ids,
            }],
            qa_log: Vec::new(),
            timeline: Vec::new(),
            session_status: CPushSessionStatus::Running,
            message: None,
            can_retry_remaining: false,
            push_authorized: true,
            auto_collapse: false,
        };

        let reply = self
            .sync_and_push_commits(
                storage_dir,
                repo_store,
                &request.request_id,
                &mut session,
                None,
                Some(post_commit_snapshot),
                &mut on_progress,
            )
            .await?;

        self.inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push 请求记录锁不可用")?
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
        repository::validate_request_id(&request.request_id)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push 请求记录锁不可用")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        if let Some(active) = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .get(&request.target.worktree_id)
            .cloned()
            && is_active_status(active.session_status)
        {
            return Err("当前工作树已有未结束的 C-Push 会话，请在下方消息栏继续处理。".into());
        }

        let spec = models
            .resolve_runtime_spec(storage_dir, &request.model_id)
            .await?;
        let extracted = repo_store
            .extract_cpush_scope(&request.target, &request.selections)
            .await?;

        let run_id = request.request_id.clone();
        let badge = scope_badge(&extracted.files);
        let initial_timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: format!("已固定范围：{badge}"),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: "正在分析改动归属并规划拆分提交…".into(),
            },
        ];
        let progress_msg = CPushUiMessage {
            id: format!("progress-{run_id}"),
            kind: "progress".into(),
            title: "正在分析并拆分已选改动…".into(),
            description: "已固定本次授权范围；未选文件与部分暂存文件的未暂存草稿将保留在原处。"
                .into(),
            scope: Some(badge.clone()),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 1,
                total: 4,
                label: "读取范围 → 规划拆分 → 本地提交 → 远端推送".into(),
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
            draft: request.draft.clone(),
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
            auto_collapse: false,
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
            .map_err(|_| "C-Push 请求记录锁不可用")?
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
        repository::validate_request_id(&request.request_id)?;
        let wt_gate = self.worktree_gate(&request.target.worktree_id)?;
        let _wt_guard = wt_gate.lock_owned().await;

        if let Some(existing_reply) = self
            .inner
            .processed_requests
            .lock()
            .map_err(|_| "C-Push 请求记录锁不可用")?
            .get(&request.request_id)
            .cloned()
        {
            return Ok(existing_reply);
        }

        let mut session = self
            .inner
            .sessions
            .lock()
            .map_err(|_| "C-Push 会话缓存锁不可用")?
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or("未找到当前工作树的待处理会话")?;

        if session.run_id != request.run_id
            || session.repository_id != request.target.repository_id
            || session.worktree_id != request.target.worktree_id
        {
            return Err("会话回复的目标工作树或任务标识不匹配，已忽略该请求".into());
        }
        if !matches!(
            session.session_status,
            CPushSessionStatus::NeedsDecision
                | CPushSessionStatus::Conflict
                | CPushSessionStatus::WaitingManual
                | CPushSessionStatus::PushFailed
        ) {
            return Err("当前会话不在可交互处理状态".into());
        }
        let current_msg = session.message.clone().ok_or("当前会话缺少待处理事项")?;
        if current_msg.id != request.message_id {
            return Err("当前事项状态已更新，请基于最新提示重新确认".into());
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
                    title: format!("已保留 {} 个本地提交并结束本次会话", session.commits.len()),
                    description:
                        "本地提交已保留在当前分支，未继续推送远端；其余工作区草稿保持不变。".into(),
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
                repository_snapshot: None,
            };
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push 请求记录锁不可用")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 2. Retrying remaining local commits (`C04`):
        if request.option_id.as_deref() == Some("retry-remaining") {
            if !session.can_retry_remaining
                || session.next_step_index >= session.planned_groups.len()
            {
                return Err("当前会话没有可重试的剩余本地提交步骤".into());
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
                .map_err(|_| "C-Push 请求记录锁不可用")?
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
                .map_err(|_| "C-Push 请求记录锁不可用")?
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
                answer: format!("推送并跟踪 {chosen}/{}", session.branch),
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
                .map_err(|_| "C-Push 请求记录锁不可用")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // Free-text reply to missing-upstream confirmation (`R05`):
        if current_msg.id.starts_with("missing-upstream-")
            && let Some(free_text) = &request.text
        {
            let trimmed = free_text.trim();
            if trimmed.is_empty() {
                return Err("请输入远端名称或点击上方推送目标选项".into());
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
                    .map_err(|_| "C-Push 请求记录锁不可用")?
                    .insert(request.request_id.clone(), reply.clone());
                return Ok(reply);
            }
            return Err(format!(
                "未能从输入中唯一确认远端目标（当前可用远端：{}），请直接点击对应远端按钮或明确写出远端名。",
                available.join("、")
            ));
        }

        // 4. Aborting active rebase (`abort-rebase`):
        if request.option_id.as_deref() == Some("abort-rebase") {
            let read_target = self.session_read_target(&session);
            let snapshot = repo_store.abort_active_rebase(&read_target).await?;
            if session.commits.is_empty() {
                session.session_status = CPushSessionStatus::Idle;
                session.message = None;
                self.remove_in_memory(&session.worktree_id)?;
            } else {
                let branch_name = session.branch.clone();
                enter_push_failed_state(
                    &mut session,
                    &branch_name,
                    "已中止变基，本地提交保持原状",
                    Some("已中止变基，保留本地提交"),
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
                repository_snapshot: Some(snapshot),
            };
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push 请求记录锁不可用")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 5. Manual IDE merge flow (`manual-merge` & `recheck-manual`, `R03`, `R04`):
        if request.option_id.as_deref() == Some("manual-merge") {
            let read_target = self.session_read_target(&session);
            let (live, snapshot) = repo_store
                .inspect_live_remote_status(&read_target, false)
                .await?;
            session.qa_log.push(CPushQaItem {
                question_id: current_msg.id.clone(),
                question: current_msg.title.clone(),
                answer: "我手动合并".into(),
                option_id: Some("manual-merge".into()),
            });
            session.session_status = CPushSessionStatus::WaitingManual;
            session.can_retry_remaining = false;
            session.timeline = vec![
                CPushTimelineItem {
                    state: "done".into(),
                    label: "已选择：前往外部编辑器手动合并冲突".into(),
                },
                CPushTimelineItem {
                    state: "active".into(),
                    label: "已暂停写入操作，等待切回 Picket 后自动核对 Git 与远端状态".into(),
                },
            ];
            let chips = if !live.conflicted_files.is_empty() {
                Some(conflict_chips(&live.conflicted_files))
            } else {
                current_msg.files.clone()
            };
            session.message = Some(CPushUiMessage {
                id: format!("waiting-manual-{}", session.run_id),
                kind: "waitingManual".into(),
                title: "等待你在 IDE 完成手动合并".into(),
                description:
                    "请在外部编辑器解决冲突并完成变基后切回 Picket，桌面层将自动核对状态。".into(),
                scope: Some("阶段：合并冲突".into()),
                files: chips,
                commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
                options: Some(vec![CPushMessageOption {
                    id: "abort-rebase".into(),
                    label: if session.commits.is_empty() {
                        "中止变基并恢复原状".into()
                    } else {
                        "中止变基（保留本地提交）".into()
                    },
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
                repository_snapshot: Some(snapshot),
            };
            self.inner
                .processed_requests
                .lock()
                .map_err(|_| "C-Push 请求记录锁不可用")?
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
                return Err("请输入具体的冲突合并要求或选择上方处理方式".into());
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
                .map_err(|_| "C-Push 请求记录锁不可用")?
                .insert(request.request_id.clone(), reply.clone());
            return Ok(reply);
        }

        // 7. Answering a disambiguation question (`C02`):
        if session.session_status != CPushSessionStatus::NeedsDecision {
            return Err("当前会话状态不支持该回复操作".into());
        }
        let answer_label = if let Some(opt_id) = &request.option_id {
            let matched = current_msg
                .options
                .as_ref()
                .and_then(|opts| opts.iter().find(|o| &o.id == opt_id))
                .ok_or("所选回复项不在当前问题选项中")?;
            matched.label.clone()
        } else if let Some(free_text) = &request.text {
            let trimmed = free_text.trim();
            if trimmed.is_empty() {
                return Err("请输入具体的回复内容".into());
            }
            trimmed.to_owned()
        } else {
            return Err("请选择一个处理选项或输入具体要求".into());
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
                label: format!("已确认：{} → {}", current_msg.title, answer_label),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: "正在按你的决定生成拆分提交方案…".into(),
            },
        ];
        session.message = Some(CPushUiMessage {
            id: format!("progress-replan-{}", request.request_id),
            kind: "progress".into(),
            title: "正在按你的确认更新提交计划…".into(),
            description: format!("已记录选择：{}", answer_label),
            scope: Some(session.branch.clone()),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 2,
                total: 4,
                label: "读取范围 → 重新规划 → 本地提交 → 远端推送".into(),
            }),
        });
        self.save_in_memory(&session)?;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let spec = match models
            .resolve_runtime_spec(storage_dir, &session.model_id)
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
            .map_err(|_| "C-Push 请求记录锁不可用")?
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
        if session.model_id.trim().is_empty() {
            return Err(
                "当前尚未配置或选择模型，无法执行 Agent 合并；请先添加模型或选择「我手动合并」。"
                    .into(),
            );
        }
        let spec = models
            .resolve_runtime_spec(storage_dir, &session.model_id)
            .await?;
        let read_target = self.session_read_target(session);
        let batch = repo_store.extract_conflict_batch(&read_target).await?;

        let answer_text = if instruction.is_empty() {
            "让 Agent 合并".to_owned()
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
                label: format!("已授权 Agent 处理冲突：{}", answer_text),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: format!("正在合并 {} 个冲突文件并校验结果…", batch.files.len()),
            },
        ];
        session.message = Some(CPushUiMessage {
            id: format!("progress-merge-{request_id}"),
            kind: "progress".into(),
            title: "Agent 正在合并冲突文件…".into(),
            description: if instruction.is_empty() {
                "仅修改当前冲突文件，合并后将校验冲突标记并继续变基。".into()
            } else {
                format!("正在按要求「{instruction}」合并冲突文件…")
            },
            scope: Some(format!("阶段：合并 {}", batch.upstream)),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: "冲突合并 → 校验标记 → 完成变基".into(),
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
                restored_msg.description =
                    format!("{err}。冲突文件保持原状，可调整要求重试或选择「我手动合并」。");
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
                    repository_snapshot: None,
                });
            }
        };

        let continue_outcome = repo_store
            .apply_conflict_resolution_and_continue(
                &read_target,
                &batch.files,
                &resolution.resolved_files,
            )
            .await;

        match continue_outcome {
            Err(err) => {
                let refreshed = repo_store.refresh(&read_target).await.ok();
                session.session_status = CPushSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description =
                    format!("{err}。未自动继续后续步骤，请在编辑器核对冲突状态或手动合并。");
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
                    Some("当前提交冲突已合并，下一笔提交变基时遇到新的冲突"),
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
                    // Standalone `拉取并变基` never auto-pushes (`R06`):
                    session.session_status = CPushSessionStatus::Done;
                    session.can_retry_remaining = false;
                    session.timeline = vec![
                        CPushTimelineItem {
                            state: "done".into(),
                            label: format!("Agent 已完成冲突合并：{}", resolution.summary),
                        },
                        CPushTimelineItem {
                            state: "done".into(),
                            label: format!("已完成变基至 {upstream}（未自动推送）"),
                        },
                    ];
                    let commits = summarize_commits(&session.commits);
                    session.message = Some(CPushUiMessage {
                        id: format!("done-merge-pull-{}", session.run_id),
                        kind: "done".into(),
                        title: format!("已完成冲突合并并变基至 {upstream}"),
                        description: format!(
                            "{}。单独发起的拉取不自动推送远端，工作区其余草稿均完整保留。",
                            resolution.summary
                        ),
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
        let files_wire: Vec<serde_json::Value> = files
            .iter()
            .map(|f| {
                let mut obj = serde_json::json!({
                    "fileId": f.file_id,
                    "path": f.path,
                    "upstreamContent": f.upstream_content,
                    "localContent": f.local_content,
                    "conflictedContent": f.conflicted_content,
                });
                if let Some(base) = &f.base_content {
                    obj["baseContent"] = serde_json::Value::String(base.clone());
                }
                obj
            })
            .collect();
        let commits_wire: Vec<String> = session
            .commits
            .iter()
            .map(|c| format!("{} {}", c.short_oid, c.title))
            .collect();
        let command = serde_json::json!({
            "type": "resolveConflict",
            "requestId": pi_request_id,
            "context": context,
            "model": {
                "provider": spec.provider.as_id(),
                "apiFormat": spec.api_format.as_id(),
                "modelId": spec.model_id,
                "baseUrl": spec.base_url,
                "apiKey": spec.api_key,
                "contextWindow": spec.context_window,
                "maxOutputTokens": spec.max_output_tokens,
            },
            "branch": session.branch,
            "upstream": upstream,
            "userInstruction": instruction,
            "commits": commits_wire,
            "files": files_wire,
        });

        let outcome = supervisor
            .run_task(
                paths,
                pi_request_id,
                context,
                command,
                CPUSH_AI_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
            )
            .await?;

        match outcome {
            PiTaskOutcome::ConflictResolved { resolution } => Ok(resolution),
            PiTaskOutcome::Cancelled => Err("已取消 Agent 冲突合并".into()),
            _ => Err("冲突合并返回了未预期的事件类型".into()),
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
        let files_wire: Vec<serde_json::Value> = session
            .frozen_files
            .iter()
            .map(|f| {
                serde_json::json!({
                    "fileId": f.file_id,
                    "path": f.path,
                    "previousPath": f.previous_path,
                    "status": f.status,
                    "rangeKind": f.range_kind,
                    "added": f.added,
                    "removed": f.removed,
                    "ignoredForReview": f.ignored_for_review,
                    "binary": f.binary,
                    "patch": f.patch,
                    "skipReason": f.skip_reason,
                })
            })
            .collect();
        let command = serde_json::json!({
            "type": "planCPush",
            "requestId": request_id,
            "context": context,
            "model": {
                "provider": spec.provider.as_id(),
                "apiFormat": spec.api_format.as_id(),
                "modelId": spec.model_id,
                "baseUrl": spec.base_url,
                "apiKey": spec.api_key,
                "contextWindow": spec.context_window,
                "maxOutputTokens": spec.max_output_tokens,
            },
            "branch": session.branch,
            "recentCommits": session.recent_commits,
            "draft": session.draft,
            "files": files_wire,
            "unselectedPaths": session.unselected_paths,
            "qaHistory": session.qa_log,
        });

        let outcome = supervisor
            .run_task(
                paths,
                request_id.to_owned(),
                context,
                command,
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
                &session.draft,
            ),
            PiTaskOutcome::Cancelled => Err("已取消 C-Push 规划；草稿与选择已保留。".into()),
            _ => Err("C-Push 规划返回了未预期的事件类型；草稿与选择已保留。".into()),
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
                            "仅已暂存".into()
                        } else {
                            "待确认".into()
                        }),
                    })
                    .collect();
                let question_id = format!("q-{}-{}", session.run_id, session.qa_log.len() + 1);
                session.session_status = CPushSessionStatus::NeedsDecision;
                session.can_retry_remaining = false;
                session.timeline = vec![
                    CPushTimelineItem {
                        state: "done".into(),
                        label: format!("已固定 {} 个已选文件范围", session.frozen_files.len()),
                    },
                    CPushTimelineItem {
                        state: "active".into(),
                        label: format!("等待决定：{title}"),
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
                    repository_snapshot: None,
                })
            }
            PiCPushPlan::Ready {
                groups,
                skipped_file_ids,
                ..
            } => {
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
        let total = session.planned_groups.len();
        if total == 0 {
            session.session_status = CPushSessionStatus::Done;
            session.can_retry_remaining = false;
            session.timeline = vec![CPushTimelineItem {
                state: "done".into(),
                label: "已按你的选择跳过本次全部已选文件".into(),
            }];
            session.message = Some(CPushUiMessage {
                id: format!("done-skipped-{}", session.run_id),
                kind: "done".into(),
                title: "已按确认跳过提交".into(),
                description: "所选文件与工作区草稿均保留在原处，未写入新提交。".into(),
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
                .unwrap_or("提交改动")
                .to_owned();

            session.session_status = CPushSessionStatus::Running;
            session.can_retry_remaining = false;
            let mut timeline: Vec<CPushTimelineItem> = session
                .commits
                .iter()
                .enumerate()
                .map(|(i, c)| CPushTimelineItem {
                    state: "done".into(),
                    label: format!("提交 {}/{}：{} {}", i + 1, total, c.short_oid, c.title),
                })
                .collect();
            timeline.push(CPushTimelineItem {
                state: "active".into(),
                label: format!("正在写入提交 {}/{}：{step_title}", step_idx + 1, total),
            });
            session.timeline = timeline;
            session.message = Some(CPushUiMessage {
                id: format!("progress-{}-step-{}", session.run_id, step_idx + 1),
                kind: "progress".into(),
                title: format!("正在写入本地提交 {}/{}…", step_idx + 1, total),
                description: step_title.clone(),
                scope: Some(format!(
                    "{} · {} 个文件",
                    session.branch,
                    group.file_ids.len()
                )),
                files: None,
                commits: (!session.commits.is_empty()).then(|| summarize_commits(&session.commits)),
                options: None,
                progress: Some(CPushMessageProgress {
                    completed: (step_idx + 1) as u32,
                    total: (total + 2) as u32,
                    label: format!("写入提交 {}/{} · 核对范围与 Git 钩子", step_idx + 1, total),
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
                    )
                    .await?;
                let step_req = CommitRequest {
                    request_id: format!("{request_id}-s{}", step_idx + 1),
                    target: read_target.clone(),
                    selections,
                    message: group.message.clone(),
                    push: false,
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
                        && let Ok(refreshed) = repo_store.refresh(&read_target).await
                    {
                        latest_snapshot = Some(refreshed);
                    }
                    let failed_chips: Vec<CPushMessageFileChip> = step_frozen_files
                        .iter()
                        .map(|f| CPushMessageFileChip {
                            id: f.file_id.clone(),
                            label: file_basename(&f.path),
                            badge: (f.range_kind == "partial").then(|| "仅已暂存".into()),
                        })
                        .collect();
                    let commit_summaries = summarize_commits(&session.commits);
                    let mut failed_timeline: Vec<CPushTimelineItem> = session
                        .commits
                        .iter()
                        .enumerate()
                        .map(|(i, c)| CPushTimelineItem {
                            state: "done".into(),
                            label: format!("提交 {}/{}：{} {}", i + 1, total, c.short_oid, c.title),
                        })
                        .collect();
                    failed_timeline.push(CPushTimelineItem {
                        state: "failed".into(),
                        label: format!("提交 {}/{} 暂停：{step_title}", step_idx + 1, total),
                    });

                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = true;
                    session.timeline = failed_timeline;
                    session.message = Some(CPushUiMessage {
                        id: format!("err-{}-step-{}", session.run_id, step_idx + 1),
                        kind: "error".into(),
                        title: if step_idx > 0 {
                            format!(
                                "已完成 {step_idx}/{total} 个提交，提交 {} 暂停",
                                step_idx + 1
                            )
                        } else {
                            format!("提交 1/{total} 暂停")
                        },
                        description: format!(
                            "{err}。{}修复后可重试剩余步骤，不会重复创建已有提交。",
                            if step_idx > 0 {
                                "已完成的本地提交已安全保留；"
                            } else {
                                "未创建新提交，草稿与选择已保留；"
                            }
                        ),
                        scope: Some(format!("提交 {}/{}", step_idx + 1, total)),
                        files: (!failed_chips.is_empty()).then_some(failed_chips),
                        commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                        options: Some(vec![
                            CPushMessageOption {
                                id: "retry-remaining".into(),
                                label: "重试剩余提交".into(),
                                recommended: Some(true),
                            },
                            CPushMessageOption {
                                id: "dismiss-session".into(),
                                label: if step_idx > 0 {
                                    "结束会话（保留已完成提交）".into()
                                } else {
                                    "关闭本次会话".into()
                                },
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
                label: format!("本地提交 {}：{}", c.short_oid, c.title),
            })
            .collect();
        push_timeline.push(CPushTimelineItem {
            state: "active".into(),
            label: "正在核对远端状态并推送本地提交…".into(),
        });
        session.timeline = push_timeline;
        session.message = Some(CPushUiMessage {
            id: format!("progress-push-{request_id}"),
            kind: "progress".into(),
            title: "正在推送本地提交至远端…".into(),
            description: "仅推送当前分支上的本地提交，不会重复创建提交。".into(),
            scope: Some(session.branch.clone()),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries.clone()),
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: "核对远端 → 安全同步 → 推送分支".into(),
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
            .push_branch(&read_target, set_upstream_remote)
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
                        "当前仓库尚未配置任何 Git 远端（remote）",
                        Some("已在本地提交，尚未配置 Git 远端"),
                    );
                } else {
                    session.session_status = CPushSessionStatus::NeedsDecision;
                    session.can_retry_remaining = false;
                    let mut timeline: Vec<CPushTimelineItem> = session
                        .commits
                        .iter()
                        .map(|c| CPushTimelineItem {
                            state: "done".into(),
                            label: format!("本地提交 {} 写入成功", c.short_oid),
                        })
                        .collect();
                    timeline.push(CPushTimelineItem {
                        state: "active".into(),
                        label: "等待确认新推送远端目标".into(),
                    });
                    session.timeline = timeline;

                    let mut options: Vec<CPushMessageOption> = remotes
                        .iter()
                        .enumerate()
                        .map(|(idx, r)| CPushMessageOption {
                            id: format!("push-set-upstream:{r}"),
                            label: format!("推送并跟踪 {r}/{}", session.branch),
                            recommended: (remotes.len() == 1 && idx == 0).then_some(true),
                        })
                        .collect();
                    options.push(CPushMessageOption {
                        id: "dismiss-session".into(),
                        label: "仅保留本地提交（暂不推送）".into(),
                        recommended: None,
                    });
                    session.message = Some(CPushUiMessage {
                        id: format!(
                            "missing-upstream-{}-{}",
                            session.run_id,
                            session.qa_log.len() + 1
                        ),
                        kind: "question".into(),
                        title: "尚未配置跟踪远端分支，确认推送目标？".into(),
                        description: format!(
                            "本地提交已写入 {}，但尚未配置 upstream。请明确确认要建立跟踪并推送的远端目标：",
                            session.branch
                        ),
                        scope: Some(format!("分支：{}", session.branch)),
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
                match repo_store.sync_rebase_onto_upstream(&read_target).await {
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
                        match repo_store.push_branch(&read_target, None).await {
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
                                    Some("已完成同步变基，但推送失败"),
                                );
                                Some(failed_snap)
                            }
                            Ok(PushOutcome::MissingUpstream { snapshot: s, .. }) => {
                                enter_push_failed_state(
                                    session,
                                    &remote_branch,
                                    "缺少跟踪远端分支",
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
                            &format!(
                                "远端有新提交且涉及当前未提交文件（{}），为保全暂存/未暂存草稿已暂停自动变基；请处理该文件后重试推送",
                                overlapping_paths.join("、")
                            ),
                            Some("已在本地提交，同步变基需先处理重叠草稿"),
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
                            Some("检测到已发布历史风险，已暂停自动变基与推送"),
                        );
                        Some(pub_snap)
                    }
                    Ok(RebaseSyncOutcome::MissingUpstream { snapshot: ms, .. }) => {
                        enter_push_failed_state(
                            session,
                            &remote_branch,
                            "尚未配置跟踪远端分支",
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
                let fallback_snap = match repo_store.refresh(&read_target).await {
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
            repository_snapshot: final_snapshot,
        })
    }

    fn mark_pushed_done(
        &self,
        session: &mut StoredCPushSession,
        remote_branch: &str,
        rebased: bool,
    ) {
        let count = session.commits.len().max(1);
        let commit_summaries = summarize_commits(&session.commits);
        let mut timeline: Vec<CPushTimelineItem> = session
            .commits
            .iter()
            .map(|c| CPushTimelineItem {
                state: "done".into(),
                label: format!("提交 {}：{}", c.short_oid, c.title),
            })
            .collect();
        if rebased {
            timeline.push(CPushTimelineItem {
                state: "done".into(),
                label: format!("已安全变基并同步至 {remote_branch}"),
            });
        }
        timeline.push(CPushTimelineItem {
            state: "done".into(),
            label: format!("{count} 个提交已推送至 {remote_branch}"),
        });

        session.timeline = timeline;
        session.session_status = CPushSessionStatus::Done;
        session.can_retry_remaining = false;
        session.auto_collapse = false;

        let skipped_note = if session.skipped_file_ids.is_empty() {
            "未选文件与部分暂存文件的未暂存草稿均安全保留在工作区。".to_owned()
        } else {
            format!(
                "已按你的选择跳过 {} 个文件；其余未选改动与未暂存草稿均安全保留在工作区。",
                session.skipped_file_ids.len()
            )
        };
        session.message = Some(CPushUiMessage {
            id: format!("done-{}", session.run_id),
            kind: "done".into(),
            title: format!("{count} 个提交已推送至 {remote_branch}"),
            description: skipped_note,
            scope: Some(format!("{} · 已同步", session.branch)),
            files: None,
            commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
            options: None,
            progress: None,
        });
    }
}
