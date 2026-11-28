//! Managed local project process execution (`RunProcessManager`) with user login-shell
//! environment, PGID process-group termination, bounded streaming log buffers, and local URL
//! extraction. Independent of `RepoWriteCoordinator` so running dev servers never block Git.

#[cfg(unix)]
use super::process_io::{SIGKILL, SIGTERM};
use super::{
    process_io::{
        cap_log_text, is_process_group_alive, read_bounded_lossy_lines, signal_pid_direct,
        signal_process_group, terminate_process_group_hard,
    },
    types::{
        MAX_DETECTED_URLS, MAX_LOG_BYTES_PER_PROCESS, MAX_LOG_LINE_BYTES,
        MAX_LOG_LINES_PER_PROCESS, PersistedRunCommand, RUN_LOG_BATCH_EVENT, RUN_STATE_EVENT,
        RunEnvEntry, RunLogBatchEvent, RunLogLine, RunLogStream, RunProcessLogsReply,
        RunProcessSnapshot, RunProcessStatus, UserEnvSummary, extract_local_urls, make_process_id,
        normalize_run_cwd, sanitize_run_command, sanitize_run_env_entries, strip_ansi_codes,
        validate_run_env_entries,
    },
    user_env::UserEnvCache,
};
use crate::{
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
    repository::{RepositoryStore, types::ReadTarget},
    tray::state::TrayRunningServiceSummary,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    process::Command,
    sync::{Notify, mpsc, watch},
    time::{Instant, sleep, timeout},
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

const STOP_GRACE_PERIOD: Duration = Duration::from_secs(4);
const STARTUP_CANCEL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(60);
const LOG_FLUSH_BATCH_SIZE: usize = 32;
const LAST_LINE_STATE_EMIT_INTERVAL: Duration = Duration::from_millis(250);
const MAX_LAST_LOG_LINE_CHARS: usize = 160;

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct RunTargetContext {
    pub repository_id: String,
    pub repository_name: String,
    pub worktree_id: String,
    pub worktree_label: String,
    pub worktree_root: PathBuf,
}

struct ManagedProcess {
    generation: u64,
    snapshot: RunProcessSnapshot,
    pgid: Option<u32>,
    logs: VecDeque<RunLogLine>,
    log_bytes: usize,
    next_seq: u64,
    last_preview_emit: Option<Instant>,
    cancel_requested: bool,
    cancel_tx: watch::Sender<bool>,
    stop_in_progress: bool,
    child_reaped: bool,
    reaped_exit_code: Option<i32>,
    reaped_signal: Option<i32>,
    reaped_error: Option<String>,
    exit_notify: Arc<Notify>,
    done_notify: Arc<Notify>,
}

impl ManagedProcess {
    fn push_log(&mut self, stream: RunLogStream, raw_text: &str) -> RunLogLine {
        let text = cap_log_text(strip_ansi_codes(raw_text), MAX_LOG_LINE_BYTES);
        for url in extract_local_urls(&text) {
            if self.snapshot.detected_urls.len() < MAX_DETECTED_URLS
                && !self.snapshot.detected_urls.contains(&url)
            {
                self.snapshot.detected_urls.push(url);
            }
        }
        let preview = text.trim();
        if !preview.is_empty() && !matches!(stream, RunLogStream::System) && !self.cancel_requested
        {
            self.snapshot.last_log_line =
                Some(preview.chars().take(MAX_LAST_LOG_LINE_CHARS).collect());
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let line = RunLogLine {
            seq,
            stream,
            text,
            timestamp_ms: now_epoch_ms(),
        };
        let line_len = line.text.len() + 32;
        self.logs.push_back(line.clone());
        self.log_bytes = self.log_bytes.saturating_add(line_len);
        while self.logs.len() > MAX_LOG_LINES_PER_PROCESS
            || (self.log_bytes > MAX_LOG_BYTES_PER_PROCESS && self.logs.len() > 1)
        {
            if let Some(evicted) = self.logs.pop_front() {
                self.log_bytes = self.log_bytes.saturating_sub(evicted.text.len() + 32);
            }
        }
        self.snapshot.log_line_count = self.logs.len() as u32;
        line
    }
}

struct ManagerInner {
    processes: Mutex<HashMap<String, ManagedProcess>>,
    selected_logs_process_id: Mutex<Option<String>>,
    cancelled_discover_ids: Mutex<HashSet<String>>,
    closing: AtomicBool,
    generation_seq: AtomicU64,
    user_env: UserEnvCache,
    #[cfg(test)]
    stop_grace_override: Mutex<Option<Duration>>,
}

impl Default for ManagerInner {
    fn default() -> Self {
        Self {
            processes: Mutex::default(),
            selected_logs_process_id: Mutex::default(),
            cancelled_discover_ids: Mutex::default(),
            closing: AtomicBool::new(false),
            generation_seq: AtomicU64::new(1),
            user_env: UserEnvCache::default(),
            #[cfg(test)]
            stop_grace_override: Mutex::default(),
        }
    }
}

#[derive(Clone, Default)]
pub struct RunProcessManager {
    inner: Arc<ManagerInner>,
}

fn resolve_worktree_cwd(
    worktree_root: &Path,
    cwd: Option<&str>,
    locale: LanguageKind,
) -> Result<(PathBuf, String), AppError> {
    let canon_root = fs::canonicalize(worktree_root).map_err(|_| {
        AppError::runner(
            AppErrorCode::RunnerInvalidInput,
            i18n::runner::cwd_not_found(locale, "."),
        )
    })?;
    let clean_rel = normalize_run_cwd(cwd).map_err(|_| {
        AppError::runner(
            AppErrorCode::RunnerInvalidInput,
            i18n::runner::invalid_cwd(locale, cwd.unwrap_or("")),
        )
    })?;
    let Some(rel) = clean_rel else {
        return Ok((canon_root, ".".into()));
    };
    let candidate = canon_root.join(&rel);
    let resolved = fs::canonicalize(&candidate).map_err(|_| {
        AppError::runner(
            AppErrorCode::RunnerInvalidInput,
            i18n::runner::cwd_not_found(locale, &rel),
        )
    })?;
    if !resolved.starts_with(&canon_root) || !resolved.is_dir() {
        return Err(AppError::runner(
            AppErrorCode::RunnerInvalidInput,
            i18n::runner::invalid_cwd(locale, &rel),
        ));
    }
    let enters_git = resolved
        .strip_prefix(&canon_root)
        .map(|rel_after| {
            rel_after
                .components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
        })
        .unwrap_or(true);
    if enters_git {
        return Err(AppError::runner(
            AppErrorCode::RunnerInvalidInput,
            i18n::runner::invalid_cwd(locale, &rel),
        ));
    }
    Ok((resolved, rel))
}

fn emit_state_only(app: Option<&AppHandle>, snapshots: &[RunProcessSnapshot]) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(RUN_STATE_EVENT, snapshots);
}

fn emit_state_to_app(app: Option<&AppHandle>, snapshots: &[RunProcessSnapshot]) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(RUN_STATE_EVENT, snapshots);
    if let Some(tray) = app.try_state::<crate::tray::TrayCoordinator>() {
        tray.refresh_from_app(app);
    }
}

fn emit_log_batch_to_app(app: Option<&AppHandle>, batch: &RunLogBatchEvent) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(RUN_LOG_BATCH_EVENT, batch);
}

/// Merges user login-shell environment, repository-level shared env, and per-command env in
/// strict precedence (`user_vars` < `shared_env` < `command_env`).
/// When `JAVA_HOME` is explicitly provided by `shared_env` or `command_env`, `<JAVA_HOME>/bin`
/// is prepended to `PATH` (deduplicated) so `mvn`, `./mvnw`, `gradle`, and `java` use that JDK.
pub fn merge_run_process_env(
    user_vars: &BTreeMap<String, String>,
    shared_env: &[RunEnvEntry],
    command_env: &[RunEnvEntry],
) -> BTreeMap<String, String> {
    let mut merged = user_vars.clone();
    let mut explicit_java_home: Option<String> = None;

    for entry in shared_env.iter().chain(command_env.iter()) {
        let key = entry.key.trim();
        if key.is_empty() {
            continue;
        }
        merged.insert(key.to_owned(), entry.value.clone());
        if key == "JAVA_HOME" {
            let trimmed = entry.value.trim();
            explicit_java_home = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        }
    }

    if let Some(java_home) = explicit_java_home {
        let java_bin = format!("{}/bin", java_home.trim_end_matches('/'));
        let mut path_parts: Vec<String> = vec![java_bin.clone()];
        if let Some(existing_path) = merged.get("PATH") {
            for part in existing_path.split(':') {
                let trimmed = part.trim();
                if !trimmed.is_empty() && trimmed != java_bin {
                    path_parts.push(trimmed.to_owned());
                }
            }
        }
        merged.insert("PATH".into(), path_parts.join(":"));
    }

    merged
}

impl RunProcessManager {
    #[cfg(test)]
    pub(crate) fn user_env_cache(&self) -> &UserEnvCache {
        &self.inner.user_env
    }

    #[cfg(test)]
    pub(crate) fn set_stop_grace_for_test(&self, duration: Duration) {
        if let Ok(mut guard) = self.inner.stop_grace_override.lock() {
            *guard = Some(duration);
        }
    }

    fn stop_grace_period(&self) -> Duration {
        #[cfg(test)]
        if let Some(custom) = self.inner.stop_grace_override.lock().ok().and_then(|g| *g) {
            return custom;
        }
        STOP_GRACE_PERIOD
    }

    pub async fn refresh_user_env(&self, locale: LanguageKind) -> UserEnvSummary {
        self.inner.user_env.refresh().await.summary(locale)
    }

    pub fn set_selected_logs_process(&self, process_id: Option<String>) {
        if let Ok(mut guard) = self.inner.selected_logs_process_id.lock() {
            *guard = process_id.filter(|id| !id.trim().is_empty());
        }
    }

    pub fn selected_logs_process(&self) -> Option<String> {
        self.inner
            .selected_logs_process_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub fn mark_discover_cancelled(&self, request_id: &str) {
        if let Ok(mut guard) = self.inner.cancelled_discover_ids.lock() {
            if guard.len() >= 128 {
                guard.clear();
            }
            guard.insert(request_id.to_owned());
        }
    }

    pub fn is_discover_cancelled(&self, request_id: &str) -> bool {
        self.inner
            .cancelled_discover_ids
            .lock()
            .map(|guard| guard.contains(request_id))
            .unwrap_or(false)
    }

    pub fn clear_discover_cancelled(&self, request_id: &str) {
        if let Ok(mut guard) = self.inner.cancelled_discover_ids.lock() {
            guard.remove(request_id);
        }
    }

    pub fn list_snapshots(&self) -> Vec<RunProcessSnapshot> {
        let Ok(guard) = self.inner.processes.lock() else {
            return Vec::new();
        };
        let mut items: Vec<RunProcessSnapshot> =
            guard.values().map(|entry| entry.snapshot.clone()).collect();
        items.sort_by(|a, b| {
            b.status
                .is_active()
                .cmp(&a.status.is_active())
                .then_with(|| b.started_at_ms.cmp(&a.started_at_ms))
                .then_with(|| a.process_id.cmp(&b.process_id))
        });
        items
    }

    pub fn active_snapshots(&self) -> Vec<RunProcessSnapshot> {
        self.list_snapshots()
            .into_iter()
            .filter(|snap| snap.status.is_active())
            .collect()
    }

    #[cfg(test)]
    pub fn has_active_processes(&self) -> bool {
        self.inner
            .processes
            .lock()
            .map(|guard| guard.values().any(|p| p.snapshot.status.is_active()))
            .unwrap_or(false)
    }

    pub fn tray_running_services(&self) -> Vec<TrayRunningServiceSummary> {
        self.active_snapshots()
            .into_iter()
            .map(|snap| TrayRunningServiceSummary {
                process_id: snap.process_id,
                repository_id: snap.repository_id,
                repository_name: snap.repository_name,
                worktree_id: snap.worktree_id,
                command_id: snap.command_id,
                label: snap.label,
                status: snap.status,
            })
            .collect()
    }

    pub fn get_logs(&self, process_id: &str) -> RunProcessLogsReply {
        let Ok(guard) = self.inner.processes.lock() else {
            return RunProcessLogsReply {
                process: None,
                lines: Vec::new(),
            };
        };
        match guard.get(process_id) {
            Some(entry) => RunProcessLogsReply {
                process: Some(entry.snapshot.clone()),
                lines: entry.logs.iter().cloned().collect(),
            },
            None => RunProcessLogsReply {
                process: None,
                lines: Vec::new(),
            },
        }
    }

    pub fn clear_logs(&self, process_id: &str, app: Option<&AppHandle>) -> bool {
        let cleared = {
            let Ok(mut guard) = self.inner.processes.lock() else {
                return false;
            };
            let Some(entry) = guard.get_mut(process_id) else {
                return false;
            };
            entry.logs.clear();
            entry.log_bytes = 0;
            entry.snapshot.log_line_count = 0;
            entry.snapshot.last_log_line = None;
            true
        };
        if cleared {
            emit_state_to_app(app, &self.list_snapshots());
        }
        cleared
    }

    pub async fn resolve_target_context(
        store: &RepositoryStore,
        target: &ReadTarget,
        locale: LanguageKind,
    ) -> Result<RunTargetContext, AppError> {
        let worktree_root = store
            .worktree_root_for_target(target, locale)
            .await
            .map_err(|msg| AppError::repository(AppErrorCode::RepoInvalidTarget, msg))?;
        let snapshots = store.cached_snapshots_in_order();
        let (repository_name, worktree_label) = snapshots
            .iter()
            .find(|snap| snap.repository.id == target.repository_id)
            .map(|snap| {
                let wt_label = snap
                    .worktrees
                    .iter()
                    .find(|wt| wt.worktree_id == target.worktree_id)
                    .map(|wt| wt.label.clone())
                    .unwrap_or_else(|| snap.repository.name.clone());
                (snap.repository.name.clone(), wt_label)
            })
            .unwrap_or_else(|| {
                let fallback = worktree_root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("repository")
                    .to_owned();
                (fallback.clone(), fallback)
            });
        Ok(RunTargetContext {
            repository_id: target.repository_id.clone(),
            repository_name,
            worktree_id: target.worktree_id.clone(),
            worktree_label,
            worktree_root,
        })
    }

    pub async fn start_command(
        &self,
        ctx: RunTargetContext,
        raw_command: PersistedRunCommand,
        raw_shared_env: Vec<RunEnvEntry>,
        locale: LanguageKind,
        app: Option<AppHandle>,
    ) -> Result<RunProcessSnapshot, AppError> {
        let clean = sanitize_run_command(raw_command).ok_or_else(|| {
            AppError::runner(
                AppErrorCode::RunnerInvalidInput,
                i18n::runner::invalid_command_config(locale),
            )
        })?;
        if !validate_run_env_entries(&raw_shared_env) {
            return Err(AppError::runner(
                AppErrorCode::RunnerInvalidInput,
                i18n::runner::invalid_command_config(locale),
            ));
        }
        let clean_shared_env = sanitize_run_env_entries(raw_shared_env);
        let (resolved_cwd, display_cwd) =
            resolve_worktree_cwd(&ctx.worktree_root, clean.cwd.as_deref(), locale)?;
        let process_id = make_process_id(&ctx.worktree_id, &clean.id);
        let generation = self.inner.generation_seq.fetch_add(1, Ordering::SeqCst);
        let exit_notify = Arc::new(Notify::new());
        let done_notify = Arc::new(Notify::new());
        let (cancel_tx, mut cancel_rx) = watch::channel(false);

        // Atomically reserve single-instance slot before loading env or spawning:
        {
            let mut guard = self
                .inner
                .processes
                .lock()
                .map_err(|_| AppError::internal("Run process registry lock is unavailable"))?;
            if self.inner.closing.load(Ordering::SeqCst) {
                return Err(AppError::runner(
                    AppErrorCode::RunnerFailed,
                    "Run process manager is shutting down.",
                ));
            }
            if let Some(existing) = guard.get(&process_id)
                && existing.snapshot.status.is_active()
            {
                return Err(AppError::runner(
                    AppErrorCode::RunnerAlreadyRunning,
                    i18n::runner::already_running(locale, &clean.label),
                ));
            }
            let snapshot = RunProcessSnapshot {
                process_id: process_id.clone(),
                generation,
                repository_id: ctx.repository_id.clone(),
                repository_name: ctx.repository_name.clone(),
                worktree_id: ctx.worktree_id.clone(),
                worktree_label: ctx.worktree_label.clone(),
                command_id: clean.id.clone(),
                label: clean.label.clone(),
                command: clean.command.clone(),
                cwd: display_cwd.clone(),
                status: RunProcessStatus::Starting,
                pid: None,
                exit_code: None,
                signal: None,
                started_at_ms: now_epoch_ms(),
                ended_at_ms: None,
                error_message: None,
                detected_urls: Vec::new(),
                log_line_count: 0,
                last_log_line: None,
            };
            guard.insert(
                process_id.clone(),
                ManagedProcess {
                    generation,
                    snapshot,
                    pgid: None,
                    logs: VecDeque::new(),
                    log_bytes: 0,
                    next_seq: 1,
                    last_preview_emit: None,
                    cancel_requested: false,
                    cancel_tx,
                    stop_in_progress: false,
                    child_reaped: false,
                    reaped_exit_code: None,
                    reaped_signal: None,
                    reaped_error: None,
                    exit_notify: Arc::clone(&exit_notify),
                    done_notify: Arc::clone(&done_notify),
                },
            );
        }
        emit_state_to_app(app.as_ref(), &self.list_snapshots());

        // Race login-shell env resolution against startup cancellation so stopping during
        // `Starting` aborts immediately instead of waiting up to 7s and spawning a ghost process.
        let user_env = tokio::select! {
            biased;
            _ = cancel_rx.wait_for(|&cancelled| cancelled) => {
                return self.finish_cancelled_startup(&process_id, generation, locale, app.as_ref());
            }
            env = self.inner.user_env.get_or_load() => env,
        };
        if *cancel_rx.borrow() {
            return self.finish_cancelled_startup(&process_id, generation, locale, app.as_ref());
        }

        let shell_display = user_env.shell.to_string_lossy().into_owned();
        let merged_env = merge_run_process_env(&user_env.vars, &clean_shared_env, &clean.env);

        let mut cmd = Command::new(&user_env.shell);
        cmd.arg("-c")
            .arg(&clean.command)
            .current_dir(&resolved_cwd)
            .env_clear()
            .envs(&merged_env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                let err_msg = i18n::runner::spawn_failed(locale, &clean.label, &err.to_string());
                if let Ok(mut guard) = self.inner.processes.lock()
                    && let Some(entry) = guard.get_mut(&process_id)
                    && entry.generation == generation
                {
                    entry.stop_in_progress = false;
                    entry.snapshot.status = RunProcessStatus::Failed;
                    entry.snapshot.ended_at_ms = Some(now_epoch_ms());
                    entry.snapshot.error_message = Some(err_msg.clone());
                    entry.push_log(RunLogStream::System, &err_msg);
                    entry.exit_notify.notify_waiters();
                    entry.done_notify.notify_waiters();
                }
                emit_state_to_app(app.as_ref(), &self.list_snapshots());
                return Err(AppError::runner(AppErrorCode::RunnerFailed, err_msg));
            }
        };

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let startup_result = {
            let mut guard = self
                .inner
                .processes
                .lock()
                .map_err(|_| AppError::internal("Run process registry lock is unavailable"))?;
            match guard.get_mut(&process_id) {
                Some(entry)
                    if !self.inner.closing.load(Ordering::SeqCst)
                        && entry.generation == generation
                        && !entry.cancel_requested
                        && entry.snapshot.status == RunProcessStatus::Starting =>
                {
                    entry.pgid = pid;
                    entry.snapshot.pid = pid;
                    entry.snapshot.status = RunProcessStatus::Running;
                    let start_line = entry.push_log(
                        RunLogStream::System,
                        &i18n::runner::system_started_line(
                            locale,
                            &clean.command,
                            &display_cwd,
                            &shell_display,
                        ),
                    );
                    let mut initial_lines = vec![start_line];
                    if !user_env.loaded_from_login_shell {
                        let warn_line = entry.push_log(
                            RunLogStream::System,
                            &i18n::runner::user_env_fallback_warning(locale, &shell_display),
                        );
                        initial_lines.push(warn_line);
                    }
                    let batch = RunLogBatchEvent {
                        process_id: process_id.clone(),
                        generation,
                        lines: initial_lines,
                        detected_urls: entry.snapshot.detected_urls.clone(),
                        log_line_count: entry.snapshot.log_line_count,
                    };
                    Some((entry.snapshot.clone(), batch))
                }
                _ => None,
            }
        };

        let Some((started_snapshot, initial_batch)) = startup_result else {
            // Stop/cancel raced with `cmd.spawn()`: immediately kill and reap the newborn process group.
            drop(stdout);
            drop(stderr);
            if let Some(raw_pid) = pid {
                terminate_process_group_hard(raw_pid);
                #[cfg(unix)]
                signal_pid_direct(raw_pid, SIGKILL);
            }
            let _ = child.start_kill();
            let _ = timeout(Duration::from_secs(2), child.wait()).await;
            if let Some(raw_pid) = pid
                && is_process_group_alive(raw_pid)
            {
                terminate_process_group_hard(raw_pid);
            }
            return self.finish_cancelled_startup(&process_id, generation, locale, app.as_ref());
        };
        emit_log_batch_to_app(app.as_ref(), &initial_batch);
        emit_state_to_app(app.as_ref(), &self.list_snapshots());

        // Channel for streaming stdout/stderr lines into a single throttled flusher task:
        let (tx, mut rx) = mpsc::channel::<(RunLogStream, String)>(256);
        if let Some(out) = stdout {
            let tx_out = tx.clone();
            tokio::spawn(async move {
                read_bounded_lossy_lines(out, MAX_LOG_LINE_BYTES, |line| {
                    let tx_out = tx_out.clone();
                    async move { tx_out.send((RunLogStream::Stdout, line)).await.is_ok() }
                })
                .await;
            });
        }
        if let Some(err) = stderr {
            let tx_err = tx.clone();
            tokio::spawn(async move {
                read_bounded_lossy_lines(err, MAX_LOG_LINE_BYTES, |line| {
                    let tx_err = tx_err.clone();
                    async move { tx_err.send((RunLogStream::Stderr, line)).await.is_ok() }
                })
                .await;
            });
        }
        drop(tx);

        // Throttled log aggregator task:
        let log_manager = self.clone();
        let log_pid = process_id.clone();
        let log_app = app.clone();
        let log_task = tokio::spawn(async move {
            let mut pending: Vec<(RunLogStream, String)> = Vec::new();
            let mut next_flush: Option<Instant> = None;

            loop {
                let recv = if let Some(deadline) = next_flush {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(item) => Some(item),
                        Err(_) => {
                            if !log_manager.flush_log_lines(
                                &log_pid,
                                generation,
                                &mut pending,
                                log_app.as_ref(),
                            ) {
                                break;
                            }
                            next_flush = None;
                            continue;
                        }
                    }
                } else {
                    Some(rx.recv().await)
                };

                match recv {
                    Some(Some(item)) => {
                        if pending.is_empty() {
                            next_flush = Some(Instant::now() + LOG_FLUSH_INTERVAL);
                        }
                        pending.push(item);
                        if pending.len() >= LOG_FLUSH_BATCH_SIZE {
                            if !log_manager.flush_log_lines(
                                &log_pid,
                                generation,
                                &mut pending,
                                log_app.as_ref(),
                            ) {
                                break;
                            }
                            next_flush = None;
                        }
                    }
                    Some(None) => {
                        if !pending.is_empty() {
                            let _ = log_manager.flush_log_lines(
                                &log_pid,
                                generation,
                                &mut pending,
                                log_app.as_ref(),
                            );
                        }
                        break;
                    }
                    None => {}
                }
            }
        });

        // Process exit reaper task:
        let wait_manager = self.clone();
        let wait_pid = process_id.clone();
        let wait_app = app.clone();
        tokio::spawn(async move {
            let wait_result = child.wait().await;
            let (exit_code, signal, failed_msg) = match wait_result {
                Ok(status) => {
                    let code = status.code();
                    #[cfg(unix)]
                    let sig = status.signal();
                    #[cfg(not(unix))]
                    let sig = None;
                    (code, sig, None)
                }
                Err(err) => (None, None, Some(err.to_string())),
            };

            // Mark `child_reaped` and wake `stop_process` IMMEDIATELY before any log drain
            // so lingering grandchildren holding pipes can never delay SIGKILL escalation.
            let stop_owned = {
                let Ok(mut guard) = wait_manager.inner.processes.lock() else {
                    return;
                };
                let Some(entry) = guard.get_mut(&wait_pid) else {
                    return;
                };
                if entry.generation != generation {
                    return;
                }
                entry.child_reaped = true;
                entry.reaped_exit_code = exit_code;
                entry.reaped_signal = signal;
                entry.reaped_error = failed_msg.clone();
                entry.snapshot.pid = None;
                entry.exit_notify.notify_waiters();
                entry.stop_in_progress || !entry.snapshot.status.is_active()
            };
            if stop_owned {
                return;
            }

            // Let buffered output finish draining briefly before marking spontaneous exit.
            let _ = timeout(Duration::from_millis(250), log_task).await;

            let batch = {
                let Ok(mut guard) = wait_manager.inner.processes.lock() else {
                    return;
                };
                let Some(entry) = guard.get_mut(&wait_pid) else {
                    return;
                };
                if entry.generation != generation
                    || entry.stop_in_progress
                    || !entry.snapshot.status.is_active()
                {
                    return;
                }

                let was_stopping =
                    entry.cancel_requested || entry.snapshot.status == RunProcessStatus::Stopping;
                entry.pgid = None;
                entry.snapshot.exit_code = if was_stopping && failed_msg.is_none() {
                    None
                } else {
                    exit_code
                };
                entry.snapshot.signal = signal;
                entry.snapshot.ended_at_ms = Some(now_epoch_ms());
                entry.snapshot.status = if failed_msg.is_some()
                    || (!was_stopping && exit_code.is_some_and(|c| c != 0))
                {
                    RunProcessStatus::Failed
                } else {
                    RunProcessStatus::Exited
                };
                if let Some(msg) = failed_msg {
                    entry.snapshot.error_message = Some(msg);
                }
                let exit_line = entry.push_log(
                    RunLogStream::System,
                    &i18n::runner::system_exited_line(locale, exit_code, signal),
                );
                entry.done_notify.notify_waiters();
                RunLogBatchEvent {
                    process_id: wait_pid.clone(),
                    generation,
                    lines: vec![exit_line],
                    detected_urls: entry.snapshot.detected_urls.clone(),
                    log_line_count: entry.snapshot.log_line_count,
                }
            };
            emit_log_batch_to_app(wait_app.as_ref(), &batch);
            emit_state_to_app(wait_app.as_ref(), &wait_manager.list_snapshots());
        });

        Ok(started_snapshot)
    }

    fn finish_cancelled_startup(
        &self,
        process_id: &str,
        generation: u64,
        locale: LanguageKind,
        app: Option<&AppHandle>,
    ) -> Result<RunProcessSnapshot, AppError> {
        let (snapshot, batch) = {
            let mut guard = self
                .inner
                .processes
                .lock()
                .map_err(|_| AppError::internal("Run process registry lock is unavailable"))?;
            let Some(entry) = guard.get_mut(process_id) else {
                return Err(AppError::runner(
                    AppErrorCode::RunnerNotFound,
                    i18n::runner::process_not_found(locale),
                ));
            };
            if entry.generation != generation || !entry.snapshot.status.is_active() {
                return Ok(entry.snapshot.clone());
            }
            entry.stop_in_progress = false;
            entry.pgid = None;
            entry.snapshot.pid = None;
            entry.snapshot.status = RunProcessStatus::Exited;
            entry.snapshot.ended_at_ms = Some(now_epoch_ms());
            let cancel_line = entry.push_log(
                RunLogStream::System,
                i18n::runner::system_startup_cancelled_line(locale),
            );
            entry.exit_notify.notify_waiters();
            entry.done_notify.notify_waiters();
            let batch = RunLogBatchEvent {
                process_id: process_id.to_owned(),
                generation,
                lines: vec![cancel_line],
                detected_urls: entry.snapshot.detected_urls.clone(),
                log_line_count: entry.snapshot.log_line_count,
            };
            (entry.snapshot.clone(), batch)
        };
        emit_log_batch_to_app(app, &batch);
        emit_state_to_app(app, &self.list_snapshots());
        Ok(snapshot)
    }

    fn is_generation_active(&self, process_id: &str, generation: u64) -> bool {
        self.inner
            .processes
            .lock()
            .ok()
            .and_then(|g| g.get(process_id).map(|e| (e.generation, e.snapshot.status)))
            .is_some_and(|(gen_id, status)| gen_id == generation && status.is_active())
    }

    fn read_stop_progress(&self, process_id: &str, generation: u64) -> (bool, bool) {
        self.inner
            .processes
            .lock()
            .ok()
            .and_then(|g| {
                g.get(process_id)
                    .filter(|e| e.generation == generation)
                    .map(|e| (e.child_reaped, e.snapshot.status.is_active()))
            })
            .unwrap_or((true, false))
    }

    fn emit_force_killed_log(
        &self,
        process_id: &str,
        generation: u64,
        locale: LanguageKind,
        app: Option<&AppHandle>,
    ) {
        let kill_batch = self.inner.processes.lock().ok().and_then(|mut guard| {
            guard
                .get_mut(process_id)
                .filter(|e| e.generation == generation)
                .map(|entry| {
                    let line = entry.push_log(
                        RunLogStream::System,
                        i18n::runner::system_force_killed_line(locale),
                    );
                    RunLogBatchEvent {
                        process_id: process_id.to_owned(),
                        generation,
                        lines: vec![line],
                        detected_urls: entry.snapshot.detected_urls.clone(),
                        log_line_count: entry.snapshot.log_line_count,
                    }
                })
        });
        if let Some(batch) = kill_batch {
            emit_log_batch_to_app(app, &batch);
        }
    }

    fn finalize_stopped_process(
        &self,
        process_id: &str,
        generation: u64,
        locale: LanguageKind,
        app: Option<&AppHandle>,
    ) {
        let exit_batch = {
            let Ok(mut guard) = self.inner.processes.lock() else {
                return;
            };
            let Some(entry) = guard.get_mut(process_id) else {
                return;
            };
            if entry.generation != generation || !entry.snapshot.status.is_active() {
                return;
            }
            entry.stop_in_progress = false;
            entry.pgid = None;
            entry.snapshot.pid = None;
            entry.snapshot.exit_code = if entry.reaped_error.is_some() {
                entry.reaped_exit_code
            } else {
                None
            };
            entry.snapshot.signal = entry.reaped_signal;
            entry.snapshot.ended_at_ms = Some(now_epoch_ms());
            entry.snapshot.status = if entry.reaped_error.is_some() {
                RunProcessStatus::Failed
            } else {
                RunProcessStatus::Exited
            };
            if let Some(msg) = entry.reaped_error.clone() {
                entry.snapshot.error_message = Some(msg);
            }
            let exit_line = entry.push_log(
                RunLogStream::System,
                &i18n::runner::system_exited_line(
                    locale,
                    entry.reaped_exit_code,
                    entry.reaped_signal,
                ),
            );
            entry.exit_notify.notify_waiters();
            entry.done_notify.notify_waiters();
            RunLogBatchEvent {
                process_id: process_id.to_owned(),
                generation,
                lines: vec![exit_line],
                detected_urls: entry.snapshot.detected_urls.clone(),
                log_line_count: entry.snapshot.log_line_count,
            }
        };
        emit_log_batch_to_app(app, &exit_batch);
        emit_state_to_app(app, &self.list_snapshots());
    }

    fn flush_log_lines(
        &self,
        process_id: &str,
        generation: u64,
        pending: &mut Vec<(RunLogStream, String)>,
        app: Option<&AppHandle>,
    ) -> bool {
        if pending.is_empty() {
            return true;
        }
        let batch_and_urls_changed = {
            let Ok(mut guard) = self.inner.processes.lock() else {
                pending.clear();
                return false;
            };
            let Some(entry) = guard.get_mut(process_id) else {
                pending.clear();
                return false;
            };
            if entry.generation != generation {
                pending.clear();
                return false;
            }
            let prev_url_count = entry.snapshot.detected_urls.len();
            let prev_last_line = entry.snapshot.last_log_line.clone();
            let mut lines = Vec::with_capacity(pending.len());
            for (stream, text) in pending.drain(..) {
                lines.push(entry.push_log(stream, &text));
            }
            let urls_changed = entry.snapshot.detected_urls.len() != prev_url_count;
            let preview_changed = entry.snapshot.last_log_line != prev_last_line
                && entry
                    .last_preview_emit
                    .is_none_or(|last| last.elapsed() >= LAST_LINE_STATE_EMIT_INTERVAL);
            if urls_changed || preview_changed {
                entry.last_preview_emit = Some(Instant::now());
            }
            (
                RunLogBatchEvent {
                    process_id: process_id.to_owned(),
                    generation,
                    lines,
                    detected_urls: entry.snapshot.detected_urls.clone(),
                    log_line_count: entry.snapshot.log_line_count,
                },
                urls_changed,
                preview_changed,
            )
        };
        emit_log_batch_to_app(app, &batch_and_urls_changed.0);
        if batch_and_urls_changed.1 {
            emit_state_to_app(app, &self.list_snapshots());
        } else if batch_and_urls_changed.2 {
            emit_state_only(app, &self.list_snapshots());
        }
        true
    }

    pub async fn stop_process(
        &self,
        process_id: &str,
        locale: LanguageKind,
        app: Option<AppHandle>,
    ) -> Result<RunProcessSnapshot, AppError> {
        enum StopAction {
            AlreadyDone(Box<RunProcessSnapshot>),
            AwaitExisting {
                generation: u64,
                done_notify: Arc<Notify>,
            },
            Execute {
                pgid: Option<u32>,
                exit_notify: Arc<Notify>,
                done_notify: Arc<Notify>,
                generation: u64,
                stopping_batch: RunLogBatchEvent,
            },
        }

        let action = {
            let mut guard = self
                .inner
                .processes
                .lock()
                .map_err(|_| AppError::internal("Run process registry lock is unavailable"))?;
            let Some(entry) = guard.get_mut(process_id) else {
                return Err(AppError::runner(
                    AppErrorCode::RunnerNotFound,
                    i18n::runner::process_not_found(locale),
                ));
            };
            if !entry.snapshot.status.is_active() {
                StopAction::AlreadyDone(Box::new(entry.snapshot.clone()))
            } else if entry.stop_in_progress {
                StopAction::AwaitExisting {
                    generation: entry.generation,
                    done_notify: Arc::clone(&entry.done_notify),
                }
            } else {
                let was_starting = entry.snapshot.status == RunProcessStatus::Starting;
                entry.cancel_requested = true;
                entry.stop_in_progress = true;
                let _ = entry.cancel_tx.send(true);
                entry.snapshot.status = RunProcessStatus::Stopping;
                let stop_line = entry.push_log(
                    RunLogStream::System,
                    if was_starting {
                        i18n::runner::system_cancelling_startup_line(locale)
                    } else {
                        i18n::runner::system_stopping_line(locale)
                    },
                );
                let stopping_batch = RunLogBatchEvent {
                    process_id: process_id.to_owned(),
                    generation: entry.generation,
                    lines: vec![stop_line],
                    detected_urls: entry.snapshot.detected_urls.clone(),
                    log_line_count: entry.snapshot.log_line_count,
                };
                StopAction::Execute {
                    pgid: entry.pgid,
                    exit_notify: Arc::clone(&entry.exit_notify),
                    done_notify: Arc::clone(&entry.done_notify),
                    generation: entry.generation,
                    stopping_batch,
                }
            }
        };

        match action {
            StopAction::AlreadyDone(snapshot) => return Ok(*snapshot),
            StopAction::AwaitExisting {
                generation,
                done_notify,
            } => {
                let deadline = Instant::now() + self.stop_grace_period() + Duration::from_secs(3);
                loop {
                    let notified = done_notify.notified();
                    tokio::pin!(notified);
                    if !self.is_generation_active(process_id, generation) {
                        break;
                    }
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let _ = timeout(deadline.saturating_duration_since(now), notified).await;
                }
            }
            StopAction::Execute {
                pgid,
                exit_notify,
                done_notify,
                generation,
                stopping_batch,
            } => {
                emit_log_batch_to_app(app.as_ref(), &stopping_batch);
                emit_state_to_app(app.as_ref(), &self.list_snapshots());

                // Run stop escalation in a spawned task so termination still completes cleanly
                // even if an external caller drops the `stop_process` future mid-flight.
                let manager = self.clone();
                let pid_key = process_id.to_owned();
                let stop_app = app.clone();
                let handle = tokio::spawn(async move {
                    let Some(pgid) = pgid else {
                        // Cancelled during `Starting` (before a child PGID was registered):
                        // wait for `start_command` to unwind and finalize immediately.
                        let deadline = Instant::now() + STARTUP_CANCEL_WAIT_TIMEOUT;
                        loop {
                            let notified = done_notify.notified();
                            tokio::pin!(notified);
                            if !manager.is_generation_active(&pid_key, generation) {
                                break;
                            }
                            let now = Instant::now();
                            if now >= deadline {
                                let _ = manager.finish_cancelled_startup(
                                    &pid_key,
                                    generation,
                                    locale,
                                    stop_app.as_ref(),
                                );
                                break;
                            }
                            let _ =
                                timeout(deadline.saturating_duration_since(now), notified).await;
                        }
                        return;
                    };

                    #[cfg(unix)]
                    signal_process_group(pgid, SIGTERM);
                    #[cfg(not(unix))]
                    signal_process_group(pgid, 15);

                    let grace_deadline = Instant::now() + manager.stop_grace_period();
                    loop {
                        let notified = exit_notify.notified();
                        tokio::pin!(notified);
                        let (reaped, active) = manager.read_stop_progress(&pid_key, generation);
                        if !active || (reaped && !is_process_group_alive(pgid)) {
                            break;
                        }
                        let now = Instant::now();
                        if now >= grace_deadline {
                            break;
                        }
                        let remaining = grace_deadline.saturating_duration_since(now);
                        if reaped {
                            // Direct shell already exited, but grandchildren in `-pgid` are still alive.
                            sleep(remaining.min(Duration::from_millis(50))).await;
                        } else {
                            let _ = timeout(remaining, notified).await;
                        }
                    }

                    let (reaped, active) = manager.read_stop_progress(&pid_key, generation);
                    if active && (!reaped || is_process_group_alive(pgid)) {
                        terminate_process_group_hard(pgid);
                        if !reaped {
                            #[cfg(unix)]
                            signal_pid_direct(pgid, SIGKILL);
                        }
                        manager.emit_force_killed_log(
                            &pid_key,
                            generation,
                            locale,
                            stop_app.as_ref(),
                        );
                        // Wait for both the direct child to be reaped AND every member of `-pgid`
                        // to exit, re-signaling `-pgid` briefly to defeat fork-races during SIGKILL.
                        for attempt in 0..40 {
                            let notified = exit_notify.notified();
                            tokio::pin!(notified);
                            let (r, a) = manager.read_stop_progress(&pid_key, generation);
                            if !a || (r && !is_process_group_alive(pgid)) {
                                break;
                            }
                            if matches!(attempt, 2 | 6 | 14) {
                                terminate_process_group_hard(pgid);
                                if !r {
                                    #[cfg(unix)]
                                    signal_pid_direct(pgid, SIGKILL);
                                }
                            }
                            if r {
                                sleep(Duration::from_millis(50)).await;
                            } else {
                                let _ = timeout(Duration::from_millis(50), notified).await;
                            }
                        }
                    }

                    manager.finalize_stopped_process(
                        &pid_key,
                        generation,
                        locale,
                        stop_app.as_ref(),
                    );
                });
                let _ = handle.await;
            }
        }

        let snapshot = self
            .inner
            .processes
            .lock()
            .ok()
            .and_then(|g| g.get(process_id).map(|e| e.snapshot.clone()))
            .ok_or_else(|| {
                AppError::runner(
                    AppErrorCode::RunnerNotFound,
                    i18n::runner::process_not_found(locale),
                )
            })?;
        Ok(snapshot)
    }

    pub async fn restart_command(
        &self,
        ctx: RunTargetContext,
        command: PersistedRunCommand,
        shared_env: Vec<RunEnvEntry>,
        locale: LanguageKind,
        app: Option<AppHandle>,
    ) -> Result<RunProcessSnapshot, AppError> {
        if self.inner.closing.load(Ordering::SeqCst) {
            return Err(AppError::runner(
                AppErrorCode::RunnerFailed,
                "Run process manager is shutting down.",
            ));
        }
        // Validate replacement config and cwd BEFORE stopping any healthy running instance.
        let clean = sanitize_run_command(command.clone()).ok_or_else(|| {
            AppError::runner(
                AppErrorCode::RunnerInvalidInput,
                i18n::runner::invalid_command_config(locale),
            )
        })?;
        if !validate_run_env_entries(&shared_env) {
            return Err(AppError::runner(
                AppErrorCode::RunnerInvalidInput,
                i18n::runner::invalid_command_config(locale),
            ));
        }
        let _ = resolve_worktree_cwd(&ctx.worktree_root, clean.cwd.as_deref(), locale)?;

        let process_id = make_process_id(&ctx.worktree_id, &clean.id);
        let is_active = self
            .inner
            .processes
            .lock()
            .ok()
            .and_then(|g| g.get(&process_id).map(|e| e.snapshot.status.is_active()))
            .unwrap_or(false);
        if is_active {
            let _ = self.stop_process(&process_id, locale, app.clone()).await;
        }
        self.start_command(ctx, clean, shared_env, locale, app)
            .await
    }

    /// Gracefully stops every active Run process group concurrently (SIGTERM -> grace -> SIGKILL),
    /// rejecting any new starts/restarts once shutdown begins.
    /// Used on full app quit and before in-app update restarts.
    pub async fn shutdown_all(&self) {
        if let Ok(_guard) = self.inner.processes.lock() {
            self.inner.closing.store(true, Ordering::SeqCst);
        } else {
            self.inner.closing.store(true, Ordering::SeqCst);
        }
        loop {
            let active_ids: Vec<String> = self
                .active_snapshots()
                .into_iter()
                .map(|snap| snap.process_id)
                .collect();
            if active_ids.is_empty() {
                break;
            }
            let mut join_set = tokio::task::JoinSet::new();
            for process_id in active_ids {
                let manager = self.clone();
                join_set.spawn(async move {
                    let _ = manager
                        .stop_process(&process_id, LanguageKind::En, None)
                        .await;
                });
            }
            while join_set.join_next().await.is_some() {}
        }
    }
}
