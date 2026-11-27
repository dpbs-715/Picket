//! Unified repository-level write exclusion and update-install drain gate.
//!
//! Shared by `RepositoryStore`, `CPushCoordinator`, `CustomActionCoordinator`, branch switching,
//! and `UpdateManager` so all repository mutations synchronize through one atomic mutex instead
//! of fragmented per-coordinator checks and locks.

use crate::i18n::{self, LanguageKind};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoWriteKind {
    CPush,
    PullRebase,
    CustomAction,
    BranchSwitch,
    Reconcile,
}

#[derive(Default)]
struct GateState {
    draining_for_update: bool,
    active_repos: HashMap<String, RepoWriteKind>,
}

#[derive(Clone, Default)]
pub struct RepoWriteCoordinator {
    inner: Arc<Mutex<GateState>>,
}

pub struct RepoWriteLease {
    inner: Arc<Mutex<GateState>>,
    repository_id: String,
}

impl Drop for RepoWriteLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.lock() {
            state.active_repos.remove(&self.repository_id);
        }
    }
}

pub struct UpdateDrainGuard {
    inner: Arc<Mutex<GateState>>,
}

impl Drop for UpdateDrainGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.lock() {
            state.draining_for_update = false;
        }
    }
}

fn busy_error_message(
    existing: RepoWriteKind,
    incoming: RepoWriteKind,
    locale: LanguageKind,
) -> String {
    match (existing, incoming) {
        (RepoWriteKind::CustomAction, RepoWriteKind::CustomAction) => {
            i18n::actions::action_already_running_error(locale).into()
        }
        (RepoWriteKind::CustomAction, RepoWriteKind::CPush) => {
            i18n::actions::action_cpush_busy_error(locale).into()
        }
        (RepoWriteKind::CustomAction, RepoWriteKind::PullRebase) => {
            i18n::actions::action_pull_busy_error(locale).into()
        }
        (RepoWriteKind::CPush | RepoWriteKind::PullRebase, RepoWriteKind::CustomAction) => {
            i18n::actions::action_cpush_active_error(locale).into()
        }
        (_, RepoWriteKind::CPush) => {
            i18n::cpush::cpush_active_session_busy_error(locale, true).into()
        }
        (_, RepoWriteKind::PullRebase) => {
            i18n::cpush::cpush_active_session_busy_error(locale, false).into()
        }
        (_, RepoWriteKind::CustomAction) => {
            i18n::actions::action_already_running_error(locale).into()
        }
        (_, RepoWriteKind::BranchSwitch) => match locale {
            LanguageKind::Zh => {
                "当前仓库正在执行写入、同步或动作任务，请等待完成后再切换分支。".into()
            }
            LanguageKind::En => {
                "Another write, sync, or action is active in this repository; wait for it to finish before switching branches."
                    .into()
            }
        },
        (_, RepoWriteKind::Reconcile) => String::new(),
    }
}

fn update_draining_error(locale: LanguageKind) -> String {
    match locale {
        LanguageKind::Zh => "Picket 正在安装更新并准备重启，已暂停新的仓库写入操作。".into(),
        LanguageKind::En => {
            "Picket is installing an update and preparing to restart; new repository writes are paused."
                .into()
        }
    }
}

impl RepoWriteCoordinator {
    /// Atomically checks update-drain status and acquires an exclusive repository-level write lease.
    pub fn try_acquire(
        &self,
        repository_id: &str,
        kind: RepoWriteKind,
        locale: LanguageKind,
    ) -> Result<RepoWriteLease, String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Repository write coordinator lock is unavailable".to_string())?;
        if state.draining_for_update {
            return Err(update_draining_error(locale));
        }
        if let Some(&existing) = state.active_repos.get(repository_id) {
            return Err(busy_error_message(existing, kind, locale));
        }
        state.active_repos.insert(repository_id.to_owned(), kind);
        Ok(RepoWriteLease {
            inner: Arc::clone(&self.inner),
            repository_id: repository_id.to_owned(),
        })
    }

    /// Best-effort non-blocking acquire for background focus reconciliation.
    pub fn try_acquire_silent(
        &self,
        repository_id: &str,
        kind: RepoWriteKind,
    ) -> Option<RepoWriteLease> {
        let mut state = self.inner.lock().ok()?;
        if state.draining_for_update || state.active_repos.contains_key(repository_id) {
            return None;
        }
        state.active_repos.insert(repository_id.to_owned(), kind);
        Some(RepoWriteLease {
            inner: Arc::clone(&self.inner),
            repository_id: repository_id.to_owned(),
        })
    }

    /// Rejects new low-level writes while an in-app update installation is draining.
    pub fn ensure_not_draining(&self, locale: LanguageKind) -> Result<(), String> {
        let state = self
            .inner
            .lock()
            .map_err(|_| "Repository write coordinator lock is unavailable".to_string())?;
        if state.draining_for_update {
            return Err(update_draining_error(locale));
        }
        Ok(())
    }

    pub fn has_active_leases(&self) -> bool {
        self.inner
            .lock()
            .map(|state| !state.active_repos.is_empty())
            .unwrap_or(false)
    }

    /// Atomically enters the update-install drain window if no repository write leases or
    /// external low-level writes are active. Dropping the guard releases the drain flag
    /// automatically if installation fails before restart.
    pub fn try_begin_update_drain(
        &self,
        external_git_busy: bool,
        locale: LanguageKind,
    ) -> Result<UpdateDrainGuard, String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Repository write coordinator lock is unavailable".to_string())?;
        if external_git_busy || !state.active_repos.is_empty() || state.draining_for_update {
            return Err(i18n::updater::updater_git_busy_error(locale).to_owned());
        }
        state.draining_for_update = true;
        Ok(UpdateDrainGuard {
            inner: Arc::clone(&self.inner),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_cross_coordinator_exclusion_per_repository_and_allows_distinct_repos() {
        let coord = RepoWriteCoordinator::default();

        // Pull & Rebase locks repo-a; Custom Action, C-Push, BranchSwitch, and Reconcile are all blocked:
        let pull_lease = coord
            .try_acquire("repo-a", RepoWriteKind::PullRebase, LanguageKind::En)
            .unwrap();
        assert!(
            coord
                .try_acquire("repo-a", RepoWriteKind::CustomAction, LanguageKind::En)
                .is_err()
        );
        assert!(
            coord
                .try_acquire("repo-a", RepoWriteKind::CPush, LanguageKind::En)
                .is_err()
        );
        assert!(
            coord
                .try_acquire("repo-a", RepoWriteKind::BranchSwitch, LanguageKind::En)
                .is_err()
        );
        assert!(
            coord
                .try_acquire_silent("repo-a", RepoWriteKind::Reconcile)
                .is_none()
        );

        // Distinct repo-b remains free to run concurrently:
        let other_lease = coord
            .try_acquire("repo-b", RepoWriteKind::CustomAction, LanguageKind::En)
            .unwrap();
        assert!(coord.has_active_leases());

        // Releasing repo-a allows branch switch immediately:
        drop(pull_lease);
        let branch_lease = coord
            .try_acquire("repo-a", RepoWriteKind::BranchSwitch, LanguageKind::En)
            .unwrap();
        drop(branch_lease);
        drop(other_lease);
        assert!(!coord.has_active_leases());
    }

    #[test]
    fn update_drain_gate_blocks_new_writes_and_recovers_on_drop() {
        let coord = RepoWriteCoordinator::default();
        let active = coord
            .try_acquire("repo-a", RepoWriteKind::CPush, LanguageKind::En)
            .unwrap();
        // Cannot enter drain while a repo lease is held:
        assert!(
            coord
                .try_begin_update_drain(false, LanguageKind::En)
                .is_err()
        );
        drop(active);

        // Cannot enter drain if an external low-level write is active:
        assert!(
            coord
                .try_begin_update_drain(true, LanguageKind::En)
                .is_err()
        );

        // Once idle, entering drain succeeds and blocks ALL new repo writes & low-level checks:
        let drain = coord
            .try_begin_update_drain(false, LanguageKind::En)
            .unwrap();
        assert!(coord.ensure_not_draining(LanguageKind::En).is_err());
        assert!(
            coord
                .try_acquire("repo-a", RepoWriteKind::CustomAction, LanguageKind::En)
                .is_err()
        );
        assert!(
            coord
                .try_acquire("repo-a", RepoWriteKind::BranchSwitch, LanguageKind::En)
                .is_err()
        );
        assert!(
            coord
                .try_acquire_silent("repo-a", RepoWriteKind::Reconcile)
                .is_none()
        );

        // Dropping drain guard (e.g. install failed) restores normal writes:
        drop(drain);
        assert!(coord.ensure_not_draining(LanguageKind::En).is_ok());
        let _ok = coord
            .try_acquire("repo-a", RepoWriteKind::CPush, LanguageKind::En)
            .unwrap();
    }

    #[tokio::test]
    async fn shared_coordinator_blocks_branch_switch_and_push_during_action_or_update_drain() {
        use crate::{
            actions::CustomActionCoordinator,
            repository::{
                RepositoryStore,
                smoke::Fixture,
                types::{BranchAction, BranchRequest, ReadTarget},
            },
        };

        let fixture = Fixture::new().unwrap();
        let shared = RepoWriteCoordinator::default();
        let store = RepositoryStore::with_write_coordinator(shared.clone());
        let _actions = CustomActionCoordinator::with_write_coordinator(shared.clone());

        let opened = store.open(&fixture.root).await.unwrap();
        let wt = &opened.worktrees[0];
        let target = ReadTarget {
            repository_id: opened.repository.id.clone(),
            worktree_id: wt.worktree_id.clone(),
            session_id: "s-lock".into(),
            scope_version: "sv-lock".into(),
        };

        // Holding a CustomAction lease blocks `change_branch` on the same repository:
        let action_lease = shared
            .try_acquire(
                &opened.repository.id,
                RepoWriteKind::CustomAction,
                LanguageKind::En,
            )
            .unwrap();
        let branch_err = store
            .change_branch(&BranchRequest {
                request_id: "req-branch-blocked".into(),
                target: target.clone(),
                action: BranchAction::SwitchBranch,
                branch_token: wt.branch_token.clone().unwrap(),
                branch: "main".into(),
                locale: LanguageKind::En,
            })
            .await
            .unwrap_err();
        assert!(
            branch_err.contains("active in this repository"),
            "unexpected branch error: {branch_err}"
        );
        drop(action_lease);

        // Entering update drain blocks low-level `push_branch` directly:
        let drain = shared
            .try_begin_update_drain(false, LanguageKind::En)
            .unwrap();
        let push_err = store
            .push_branch(&target, None, LanguageKind::En)
            .await
            .unwrap_err();
        assert!(
            push_err.contains("installing an update"),
            "unexpected push error: {push_err}"
        );
        drop(drain);
    }
}
