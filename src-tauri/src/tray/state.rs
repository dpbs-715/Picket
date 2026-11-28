//! Pure Rust tray state snapshot types, urgency ranking, tooltip calculation, and
//! authoritative snapshot construction directly from `RepositoryStore`,
//! `CustomActionCoordinator`, and `UpdateManager`.

use crate::{
    i18n::{self, LanguageKind},
    repository::types::{
        Remote, RepositorySnapshot, ThemeKind, WorktreeSnapshot, deserialize_theme_or_default,
    },
    runner::RunProcessStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_TRAY_REPOSITORIES: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum TraySessionStatus {
    #[default]
    Idle,
    Running,
    NeedsDecision,
    Conflict,
    WaitingManual,
    PushFailed,
    Done,
}

impl TraySessionStatus {
    pub fn urgency_rank(self) -> u8 {
        match self {
            Self::Conflict => 0,
            Self::NeedsDecision => 1,
            Self::PushFailed => 2,
            Self::WaitingManual => 3,
            Self::Running => 4,
            Self::Idle | Self::Done => 5,
        }
    }

    pub fn is_urgent(self) -> bool {
        self.urgency_rank() < 5
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrayRepositorySummary {
    pub repository_id: String,
    pub worktree_id: String,
    pub name: String,
    pub branch: String,
    pub detached: bool,
    pub ahead: u32,
    pub behind: u32,
    pub has_upstream: bool,
    pub change_count: u32,
    pub local_keep_count: u32,
    pub worktree_count: u32,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_error: Option<String>,
    #[serde(default)]
    pub session_status: TraySessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrayRunningServiceSummary {
    pub process_id: String,
    pub repository_id: String,
    pub repository_name: String,
    pub worktree_id: String,
    pub command_id: String,
    pub label: String,
    pub status: RunProcessStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrayStateSnapshot {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[serde(default)]
    pub locale: LanguageKind,
    #[serde(default, deserialize_with = "deserialize_theme_or_default")]
    pub theme: ThemeKind,
    pub has_update: bool,
    pub refreshing: bool,
    pub repositories: Vec<TrayRepositorySummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<TrayRunningServiceSummary>>", optional))]
    pub running_services: Vec<TrayRunningServiceSummary>,
}

impl Default for TrayStateSnapshot {
    fn default() -> Self {
        Self {
            protocol_version: 1,
            locale: LanguageKind::En,
            theme: ThemeKind::System,
            has_update: false,
            refreshing: false,
            repositories: Vec::new(),
            running_services: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrayUiContextUpdate {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub locale: Option<LanguageKind>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub theme: Option<ThemeKind>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub active_worktree_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TrayMainWindowAction {
    Show,
    OpenSettings,
    #[serde(rename_all = "camelCase")]
    FocusWorktree {
        repository_id: String,
        worktree_id: String,
        open_dock: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayWorktreeSessionInfo {
    pub status: TraySessionStatus,
    pub detail: Option<String>,
}

#[cfg(test)]
pub fn validate_tray_snapshot(snapshot: &TrayStateSnapshot) -> Result<(), String> {
    if snapshot.protocol_version != 1 {
        return Err("Unsupported tray state protocol version".into());
    }
    if snapshot.repositories.len() > MAX_TRAY_REPOSITORIES {
        return Err("Tray repository summary list exceeds limit".into());
    }
    Ok(())
}

pub fn tray_needs_attention(snapshot: &TrayStateSnapshot) -> bool {
    snapshot.has_update
        || snapshot.repositories.iter().any(|repo| {
            matches!(
                repo.session_status,
                TraySessionStatus::NeedsDecision
                    | TraySessionStatus::Conflict
                    | TraySessionStatus::PushFailed
            )
        })
}

pub fn build_tray_tooltip(snapshot: &TrayStateSnapshot) -> String {
    let locale = snapshot.locale;
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::Conflict)
    {
        return i18n::desktop::tray_tooltip_conflict(locale).to_string();
    }
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::NeedsDecision)
    {
        return i18n::desktop::tray_tooltip_needs_decision(locale).to_string();
    }
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::PushFailed)
    {
        return i18n::desktop::tray_tooltip_push_failed(locale).to_string();
    }
    if snapshot
        .repositories
        .iter()
        .any(|r| r.session_status == TraySessionStatus::Running)
    {
        return i18n::desktop::tray_tooltip_running(locale).to_string();
    }
    let changed_repos = snapshot
        .repositories
        .iter()
        .filter(|r| r.change_count > 0 || r.ahead > 0 || r.behind > 0 || r.read_error.is_some())
        .count();
    if changed_repos > 0 {
        return i18n::desktop::tray_tooltip_changed_repos(locale, changed_repos);
    }
    if snapshot.has_update {
        return i18n::desktop::tray_tooltip_update_available(locale).to_string();
    }
    "Picket".to_string()
}

fn worktree_priority_score(
    tree: &WorktreeSnapshot,
    session: Option<&TrayWorktreeSessionInfo>,
    is_active: bool,
) -> u8 {
    let status = session.map_or(TraySessionStatus::Idle, |s| s.status);
    if status.is_urgent() {
        return status.urgency_rank();
    }
    if tree.read_error.is_some() {
        return 10;
    }
    let normal_changes = tree.files.iter().filter(|f| !f.local_keep).count();
    let sync_count = match &tree.remote {
        Remote::Tracked { ahead, behind, .. } => ahead.saturating_add(*behind),
        _ => 0,
    };
    if normal_changes > 0 || sync_count > 0 || tree.detached {
        return if is_active { 15 } else { 20 };
    }
    if is_active { 25 } else { 30 }
}

fn summarize_worktree(
    repo_name: &str,
    worktree_count: usize,
    tree: &WorktreeSnapshot,
    session: Option<&TrayWorktreeSessionInfo>,
    active: bool,
) -> TrayRepositorySummary {
    let session_status = session.map_or(TraySessionStatus::Idle, |s| s.status);
    let normal_changes = tree.files.iter().filter(|f| !f.local_keep).count();
    let local_keep_count = tree.files.len().saturating_sub(normal_changes);
    let (has_upstream, ahead, behind) = match &tree.remote {
        Remote::Tracked { ahead, behind, .. } => (true, *ahead, *behind),
        _ => (false, 0, 0),
    };
    let status_detail = if session_status.is_urgent() {
        session
            .and_then(|s| s.detail.clone())
            .filter(|d| !d.trim().is_empty())
    } else {
        None
    };

    TrayRepositorySummary {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        name: repo_name.to_owned(),
        branch: tree.branch.clone(),
        detached: tree.detached,
        ahead: u32::try_from(ahead).unwrap_or(u32::MAX),
        behind: u32::try_from(behind).unwrap_or(u32::MAX),
        has_upstream,
        change_count: normal_changes as u32,
        local_keep_count: local_keep_count as u32,
        worktree_count: worktree_count as u32,
        active,
        read_error: tree.read_error.clone(),
        session_status,
        status_detail,
    }
}

/// Builds the authoritative `TrayStateSnapshot` directly in Rust from cached repository snapshots,
/// live C-Push / Custom Action sessions, update status, and UI locale/theme/active-worktree context.
#[allow(clippy::too_many_arguments)]
pub fn build_tray_state_snapshot(
    snapshots: &[RepositorySnapshot],
    sessions: &HashMap<String, TrayWorktreeSessionInfo>,
    running_services: Vec<TrayRunningServiceSummary>,
    active_worktree_id: Option<&str>,
    locale: LanguageKind,
    theme: ThemeKind,
    has_update: bool,
    refreshing: bool,
) -> TrayStateSnapshot {
    let mut repositories = Vec::with_capacity(snapshots.len().min(MAX_TRAY_REPOSITORIES));
    for snap in snapshots.iter().take(MAX_TRAY_REPOSITORIES) {
        if snap.worktrees.is_empty() {
            continue;
        }
        let active_tree = active_worktree_id
            .and_then(|active_id| snap.worktrees.iter().find(|t| t.worktree_id == active_id));
        let mut representative = active_tree
            .or_else(|| {
                snap.worktrees
                    .iter()
                    .find(|t| t.worktree_id == snap.active_worktree_id)
            })
            .unwrap_or(&snap.worktrees[0]);
        let mut best_score = worktree_priority_score(
            representative,
            sessions.get(&representative.worktree_id),
            active_tree.is_some_and(|a| a.worktree_id == representative.worktree_id),
        );
        for candidate in &snap.worktrees {
            let is_active = active_tree.is_some_and(|a| a.worktree_id == candidate.worktree_id);
            let score =
                worktree_priority_score(candidate, sessions.get(&candidate.worktree_id), is_active);
            if score < best_score {
                best_score = score;
                representative = candidate;
            }
        }
        let is_active = active_worktree_id == Some(representative.worktree_id.as_str());
        repositories.push(summarize_worktree(
            &snap.repository.name,
            snap.worktrees.len(),
            representative,
            sessions.get(&representative.worktree_id),
            is_active,
        ));
    }
    TrayStateSnapshot {
        protocol_version: 1,
        locale,
        theme,
        has_update,
        refreshing,
        repositories,
        running_services,
    }
}
