#[cfg(test)]
mod tests;
pub mod types;

use crate::{
    actions::CustomActionCoordinator,
    i18n::{self, LanguageKind},
    lifecycle::cleanup_for_restart,
    repo_lock::RepoWriteCoordinator,
    repository::RepositoryStore,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};
use tokio::sync::watch;
use types::*;

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(120);
const PROGRESS_EMIT_MIN_BYTES: u64 = 64 * 1024;
const MAX_RELEASE_NOTES_CHARS: usize = 4_000;

#[derive(Clone, Default)]
pub struct UpdateManager {
    inner: Arc<Mutex<UpdateManagerInner>>,
}

#[derive(Default)]
struct UpdateManagerInner {
    snapshot: Option<UpdateStateSnapshot>,
    pending_update: Option<Update>,
    cancel_tx: Option<watch::Sender<bool>>,
    in_flight: bool,
}

use crate::time::{now_utc_iso as now_iso8601, unix_secs_to_iso8601};

fn configured_pubkey(app: &AppHandle) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|value| value.get("pubkey"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_owned()
}

pub(crate) fn app_bundle_root_from_exe(exe_path: &Path) -> Option<PathBuf> {
    let macos_dir = exe_path.parent()?;
    if macos_dir.file_name().and_then(|s| s.to_str()) != Some("MacOS") {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name().and_then(|s| s.to_str()) != Some("Contents") {
        return None;
    }
    let bundle_dir = contents_dir.parent()?;
    if bundle_dir.extension().and_then(|s| s.to_str()) != Some("app") {
        return None;
    }
    Some(bundle_dir.to_path_buf())
}

fn is_mounted_read_only_volume(bundle_path: &Path) -> bool {
    let path_str = bundle_path.to_string_lossy();
    if !path_str.starts_with("/Volumes/") {
        return false;
    }
    let Some(parent) = bundle_path.parent() else {
        return true;
    };
    tempfile::NamedTempFile::new_in(parent).is_err()
}

pub(crate) fn detect_install_blocker(
    exe_path: &Path,
    is_debug_build: bool,
    pubkey: &str,
) -> UpdateInstallBlocker {
    if is_debug_build {
        return UpdateInstallBlocker::DevBuild;
    }
    let Some(bundle_root) = app_bundle_root_from_exe(exe_path) else {
        return UpdateInstallBlocker::DevBuild;
    };
    let bundle_str = bundle_root.to_string_lossy();
    if bundle_str.contains("/AppTranslocation/") {
        return UpdateInstallBlocker::AppTranslocation;
    }
    if is_mounted_read_only_volume(&bundle_root) {
        return UpdateInstallBlocker::ReadOnlyVolume;
    }
    if pubkey.trim().is_empty() {
        return UpdateInstallBlocker::SigningKeyUnconfigured;
    }
    UpdateInstallBlocker::None
}

pub(crate) fn blocker_message(
    blocker: UpdateInstallBlocker,
    locale: LanguageKind,
) -> Option<&'static str> {
    match blocker {
        UpdateInstallBlocker::None => None,
        UpdateInstallBlocker::DevBuild => Some(i18n::updater::updater_blocker_dev_build(locale)),
        UpdateInstallBlocker::ReadOnlyVolume => {
            Some(i18n::updater::updater_blocker_read_only_volume(locale))
        }
        UpdateInstallBlocker::AppTranslocation => {
            Some(i18n::updater::updater_blocker_app_translocation(locale))
        }
        UpdateInstallBlocker::SigningKeyUnconfigured => Some(
            i18n::updater::updater_blocker_signing_key_unconfigured(locale),
        ),
    }
}

pub(crate) fn sanitize_release_notes(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_owned();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() <= MAX_RELEASE_NOTES_CHARS {
        return Some(trimmed);
    }
    let truncated: String = trimmed.chars().take(MAX_RELEASE_NOTES_CHARS).collect();
    Some(format!("{truncated}…"))
}

pub(crate) fn classify_updater_error(
    error: &UpdaterError,
    locale: LanguageKind,
    installing: bool,
) -> String {
    match error {
        UpdaterError::Minisign(_)
        | UpdaterError::Base64(_)
        | UpdaterError::SignatureUtf8(_)
        | UpdaterError::MissingSignedVersion
        | UpdaterError::SignedVersionMismatch { .. } => {
            i18n::updater::updater_signature_error(locale).to_owned()
        }
        other => {
            let detail = other.to_string();
            if installing {
                i18n::updater::updater_install_failed_error(locale, &detail)
            } else {
                i18n::updater::updater_check_failed_error(locale, &detail)
            }
        }
    }
}

pub(crate) fn validate_install_preconditions(
    blocker: UpdateInstallBlocker,
    git_busy: bool,
    requested_version: &str,
    pending_version: Option<&str>,
    locale: LanguageKind,
) -> Result<(), String> {
    if git_busy {
        return Err(i18n::updater::updater_git_busy_error(locale).to_owned());
    }
    if let Some(msg) = blocker_message(blocker, locale) {
        return Err(msg.to_owned());
    }
    let trimmed = requested_version.trim();
    if trimmed.is_empty() || pending_version != Some(trimmed) {
        return Err(i18n::updater::updater_no_pending_update_error(locale).to_owned());
    }
    Ok(())
}

impl UpdateManager {
    fn current_blocker(app: &AppHandle) -> UpdateInstallBlocker {
        let exe_path = std::env::current_exe().unwrap_or_default();
        let pubkey = configured_pubkey(app);
        detect_install_blocker(&exe_path, cfg!(debug_assertions), &pubkey)
    }

    fn ensure_snapshot(inner: &mut UpdateManagerInner, app: &AppHandle) -> UpdateStateSnapshot {
        let blocker = Self::current_blocker(app);
        let current_version = app.package_info().version.to_string();
        let snapshot = inner
            .snapshot
            .get_or_insert_with(|| UpdateStateSnapshot::initial(current_version.clone(), blocker));
        snapshot.current_version = current_version;
        snapshot.install_blocker = blocker;
        snapshot.can_install = snapshot.phase == UpdatePhase::Available
            && blocker == UpdateInstallBlocker::None
            && inner.pending_update.is_some();
        snapshot.clone()
    }

    fn emit_snapshot(app: &AppHandle, snapshot: &UpdateStateSnapshot) {
        let _ = app.emit(UPDATE_STATE_EVENT, snapshot);
        app.state::<crate::tray::TrayCoordinator>()
            .refresh_from_app(app);
    }

    pub fn has_update(&self) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.snapshot.as_ref().map(|s| s.phase))
            .is_some_and(|phase| {
                matches!(
                    phase,
                    UpdatePhase::Available | UpdatePhase::Downloading | UpdatePhase::Installing
                )
            })
    }

    pub fn get_status(&self, app: &AppHandle) -> Result<UpdateStateSnapshot, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "Updater state lock is unavailable".to_string())?;
        Ok(Self::ensure_snapshot(&mut guard, app))
    }

    pub async fn check(
        &self,
        app: &AppHandle,
        manual: bool,
        locale: LanguageKind,
    ) -> Result<UpdateStateSnapshot, String> {
        let previous_snapshot = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| "Updater state lock is unavailable".to_string())?;
            let current = Self::ensure_snapshot(&mut guard, app);
            if guard.in_flight {
                return Ok(current);
            }
            guard.in_flight = true;
            if manual {
                let mut checking = current.clone();
                checking.phase = UpdatePhase::Checking;
                checking.error = None;
                checking.downloaded_bytes = None;
                checking.total_bytes = None;
                guard.snapshot = Some(checking.clone());
                Self::emit_snapshot(app, &checking);
            }
            current
        };

        let updater_result = app
            .updater_builder()
            .timeout(CHECK_TIMEOUT)
            .build()
            .map_err(|err| classify_updater_error(&err, locale, false));

        let check_outcome = match updater_result {
            Ok(updater) => updater.check().await,
            Err(message) => {
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                if !manual {
                    guard.snapshot = Some(previous_snapshot.clone());
                    return Ok(previous_snapshot);
                }
                let mut next = Self::ensure_snapshot(&mut guard, app);
                next.phase = UpdatePhase::Error;
                next.error = Some(message.clone());
                next.can_install = false;
                next.checked_at = Some(now_iso8601());
                guard.snapshot = Some(next.clone());
                Self::emit_snapshot(app, &next);
                return Err(message);
            }
        };

        let blocker = Self::current_blocker(app);
        let checked_at = now_iso8601();

        match check_outcome {
            Ok(Some(update)) => {
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                let available_version = update.version.clone();
                let release_notes = sanitize_release_notes(update.body.clone());
                let published_at = update
                    .date
                    .map(|dt| unix_secs_to_iso8601(dt.unix_timestamp().max(0) as u64));
                guard.pending_update = Some(update);
                let mut next = Self::ensure_snapshot(&mut guard, app);
                next.phase = UpdatePhase::Available;
                next.available_version = Some(available_version);
                next.release_notes = release_notes;
                next.published_at = published_at;
                next.downloaded_bytes = None;
                next.total_bytes = None;
                next.install_blocker = blocker;
                next.can_install = blocker == UpdateInstallBlocker::None;
                next.error = None;
                next.checked_at = Some(checked_at);
                guard.snapshot = Some(next.clone());
                Self::emit_snapshot(app, &next);
                Ok(next)
            }
            Ok(None) | Err(UpdaterError::ReleaseNotFound) => {
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                guard.pending_update = None;
                let mut next = Self::ensure_snapshot(&mut guard, app);
                next.phase = if manual {
                    UpdatePhase::UpToDate
                } else {
                    UpdatePhase::Idle
                };
                next.available_version = None;
                next.release_notes = None;
                next.published_at = None;
                next.downloaded_bytes = None;
                next.total_bytes = None;
                next.can_install = false;
                next.error = None;
                next.checked_at = Some(checked_at);
                guard.snapshot = Some(next.clone());
                if manual {
                    Self::emit_snapshot(app, &next);
                }
                Ok(next)
            }
            Err(error) => {
                let message = classify_updater_error(&error, locale, false);
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                if !manual {
                    guard.snapshot = Some(previous_snapshot.clone());
                    return Ok(previous_snapshot);
                }
                let mut next = Self::ensure_snapshot(&mut guard, app);
                next.phase = UpdatePhase::Error;
                next.error = Some(message.clone());
                next.downloaded_bytes = None;
                next.total_bytes = None;
                next.can_install = false;
                next.checked_at = Some(checked_at);
                guard.snapshot = Some(next.clone());
                Self::emit_snapshot(app, &next);
                Err(message)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn install(
        &self,
        app: &AppHandle,
        version: &str,
        locale: LanguageKind,
        repo_writes: &RepoWriteCoordinator,
        repositories: &RepositoryStore,
        actions: &CustomActionCoordinator,
    ) -> Result<UpdateStateSnapshot, String> {
        let (update, mut cancel_rx, base_snapshot) = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| "Updater state lock is unavailable".to_string())?;
            if guard.in_flight {
                return Err(i18n::updater::updater_busy_error(locale).to_owned());
            }
            let current = Self::ensure_snapshot(&mut guard, app);
            let pending_version = guard.pending_update.as_ref().map(|u| u.version.as_str());
            let git_busy = repo_writes.has_active_leases()
                || repositories.has_active_write()
                || actions.has_active_operation();
            validate_install_preconditions(
                current.install_blocker,
                git_busy,
                version,
                pending_version,
                locale,
            )?;
            let update = guard
                .pending_update
                .clone()
                .ok_or_else(|| i18n::updater::updater_no_pending_update_error(locale).to_owned())?;
            let (tx, rx) = watch::channel(false);
            guard.in_flight = true;
            guard.cancel_tx = Some(tx);
            let mut downloading = current.clone();
            downloading.phase = UpdatePhase::Downloading;
            downloading.downloaded_bytes = Some(0);
            downloading.total_bytes = None;
            downloading.error = None;
            downloading.can_install = false;
            guard.snapshot = Some(downloading.clone());
            Self::emit_snapshot(app, &downloading);
            (update, rx, downloading)
        };

        let downloaded_counter = Arc::new(AtomicU64::new(0));
        let last_emitted_bytes = Arc::new(AtomicU64::new(0));
        let last_emitted_at = Arc::new(Mutex::new(Instant::now()));

        let app_for_progress = app.clone();
        let inner_for_progress = Arc::clone(&self.inner);
        let downloaded_for_cb = Arc::clone(&downloaded_counter);
        let last_bytes_for_cb = Arc::clone(&last_emitted_bytes);
        let last_at_for_cb = Arc::clone(&last_emitted_at);
        let progress_base = base_snapshot.clone();

        let app_for_finish = app.clone();
        let inner_for_finish = Arc::clone(&self.inner);
        let downloaded_for_finish = Arc::clone(&downloaded_counter);
        let finish_base = base_snapshot.clone();

        let download_future = update.download(
            move |chunk_len, total| {
                let next_total = downloaded_for_cb.fetch_add(chunk_len as u64, Ordering::SeqCst)
                    + chunk_len as u64;
                let prev_bytes = last_bytes_for_cb.load(Ordering::SeqCst);
                let should_emit = next_total.saturating_sub(prev_bytes) >= PROGRESS_EMIT_MIN_BYTES
                    || total.is_some_and(|t| next_total >= t)
                    || last_at_for_cb
                        .lock()
                        .map(|t| t.elapsed() >= PROGRESS_EMIT_INTERVAL)
                        .unwrap_or(false);
                if should_emit {
                    last_bytes_for_cb.store(next_total, Ordering::SeqCst);
                    if let Ok(mut at) = last_at_for_cb.lock() {
                        *at = Instant::now();
                    }
                    let mut snap = progress_base.clone();
                    snap.phase = UpdatePhase::Downloading;
                    snap.downloaded_bytes = Some(next_total);
                    snap.total_bytes = total;
                    if let Ok(mut guard) = inner_for_progress.lock() {
                        guard.snapshot = Some(snap.clone());
                    }
                    Self::emit_snapshot(&app_for_progress, &snap);
                }
            },
            move || {
                let final_bytes = downloaded_for_finish.load(Ordering::SeqCst);
                let mut snap = finish_base.clone();
                snap.phase = UpdatePhase::Downloading;
                snap.downloaded_bytes = Some(final_bytes);
                snap.total_bytes = Some(final_bytes);
                if let Ok(mut guard) = inner_for_finish.lock() {
                    guard.snapshot = Some(snap.clone());
                }
                Self::emit_snapshot(&app_for_finish, &snap);
            },
        );

        let wait_for_cancel = async {
            loop {
                if *cancel_rx.borrow() {
                    return;
                }
                if cancel_rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };

        let bytes = tokio::select! {
            biased;
            _ = wait_for_cancel => {
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                guard.cancel_tx = None;
                let mut restored = Self::ensure_snapshot(&mut guard, app);
                restored.phase = UpdatePhase::Available;
                restored.downloaded_bytes = None;
                restored.total_bytes = None;
                restored.error = None;
                restored.can_install = restored.install_blocker == UpdateInstallBlocker::None
                    && guard.pending_update.is_some();
                guard.snapshot = Some(restored.clone());
                Self::emit_snapshot(app, &restored);
                return Ok(restored);
            }
            result = download_future => {
                match result {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let message = classify_updater_error(&error, locale, true);
                        let mut guard = self
                            .inner
                            .lock()
                            .map_err(|_| "Updater state lock is unavailable".to_string())?;
                        guard.in_flight = false;
                        guard.cancel_tx = None;
                        let mut failed = Self::ensure_snapshot(&mut guard, app);
                        failed.phase = UpdatePhase::Error;
                        failed.downloaded_bytes = None;
                        failed.total_bytes = None;
                        failed.error = Some(message.clone());
                        failed.can_install = false;
                        guard.snapshot = Some(failed.clone());
                        Self::emit_snapshot(app, &failed);
                        return Err(message);
                    }
                }
            }
        };

        // Atomically enter the update-install drain gate right before touching Picket.app on disk
        // so no new Git write, C-Push, Pull & Rebase, branch switch, or Custom Action can start
        // while the bundle is replaced and the process restarts.
        let external_busy = repositories.has_active_write() || actions.has_active_operation();
        let _drain_guard = match repo_writes.try_begin_update_drain(external_busy, locale) {
            Ok(guard) => guard,
            Err(err) => {
                let message = err.message;
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                guard.cancel_tx = None;
                let mut restored = Self::ensure_snapshot(&mut guard, app);
                restored.phase = UpdatePhase::Available;
                restored.downloaded_bytes = None;
                restored.total_bytes = None;
                restored.error = Some(message.clone());
                restored.can_install = restored.install_blocker == UpdateInstallBlocker::None
                    && guard.pending_update.is_some();
                guard.snapshot = Some(restored.clone());
                Self::emit_snapshot(app, &restored);
                return Err(message);
            }
        };

        {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| "Updater state lock is unavailable".to_string())?;
            guard.cancel_tx = None;
            let mut installing = Self::ensure_snapshot(&mut guard, app);
            installing.phase = UpdatePhase::Installing;
            installing.can_install = false;
            installing.error = None;
            guard.snapshot = Some(installing.clone());
            Self::emit_snapshot(app, &installing);
        }

        let install_update = update.clone();
        let install_res = tokio::task::spawn_blocking(move || install_update.install(&bytes)).await;

        match install_res {
            Ok(Ok(())) => {
                cleanup_for_restart(app).await;
                app.restart();
            }
            Ok(Err(error)) => {
                let message = classify_updater_error(&error, locale, true);
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                let mut failed = Self::ensure_snapshot(&mut guard, app);
                failed.phase = UpdatePhase::Error;
                failed.error = Some(message.clone());
                failed.can_install = false;
                guard.snapshot = Some(failed.clone());
                Self::emit_snapshot(app, &failed);
                Err(message)
            }
            Err(join_err) => {
                let message =
                    i18n::updater::updater_install_failed_error(locale, &join_err.to_string());
                let mut guard = self
                    .inner
                    .lock()
                    .map_err(|_| "Updater state lock is unavailable".to_string())?;
                guard.in_flight = false;
                let mut failed = Self::ensure_snapshot(&mut guard, app);
                failed.phase = UpdatePhase::Error;
                failed.error = Some(message.clone());
                failed.can_install = false;
                guard.snapshot = Some(failed.clone());
                Self::emit_snapshot(app, &failed);
                Err(message)
            }
        }
    }

    pub fn cancel(
        &self,
        app: &AppHandle,
        locale: LanguageKind,
    ) -> Result<UpdateStateSnapshot, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| i18n::updater::updater_state_lock_error(locale).to_owned())?;
        if let Some(tx) = guard.cancel_tx.as_ref() {
            let _ = tx.send(true);
        }
        Ok(Self::ensure_snapshot(&mut guard, app))
    }

    pub fn open_release_page(locale: LanguageKind) -> Result<(), String> {
        // Hardcoded HTTPS GitHub Releases URL — never accepts arbitrary URLs from frontend.
        let status = std::process::Command::new("/usr/bin/open")
            .arg(RELEASE_PAGE_URL)
            .status()
            .map_err(|err| {
                i18n::updater::updater_open_release_page_error(locale, &err.to_string())
            })?;
        if !status.success() {
            return Err(i18n::updater::updater_open_release_page_error(
                locale,
                &format!("open exited with {status}"),
            ));
        }
        Ok(())
    }
}
