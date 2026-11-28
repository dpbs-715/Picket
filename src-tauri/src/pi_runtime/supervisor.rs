#[cfg(any(test, feature = "smoke-harness"))]
use super::process::PROBE_TIMEOUT;
use super::{
    process::{
        CANCEL_SETTLE_TIMEOUT, EXIT_TIMEOUT, MAX_TASK_TOTAL_TIMEOUT, PROTOCOL_VERSION,
        START_TIMEOUT, SpawnedPiProcess, next_event, write_command,
    },
    protocol::{PiCommand, PiEvent, PiTaskOutcome, PiToolExecutionResult, RequestContext},
};
use crate::{
    error::{AppError, AppErrorCode},
    i18n::{self, LanguageKind},
};
#[cfg(any(test, feature = "smoke-harness"))]
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Notify, watch},
    time::timeout,
};

#[derive(Clone)]
pub struct PiPaths {
    pub node: PathBuf,
    pub script: PathBuf,
}

impl PiPaths {
    pub fn development() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        Self {
            node: root.join(format!("binaries/picket-node-{}", env!("PICKET_TARGET"))),
            script: root.join("resources/generated/pi-sidecar.mjs"),
        }
    }

    pub fn bundled(
        executable: &std::path::Path,
        resources: &std::path::Path,
    ) -> Result<Self, String> {
        let directory = executable.parent().ok_or("Missing application directory")?;
        Ok(Self {
            node: directory.join("picket-node"),
            script: resources.join("sidecar/pi-sidecar.mjs"),
        })
    }
}

#[cfg(any(test, feature = "smoke-harness"))]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub request_id: String,
    pub status: String,
    pub child_pid: u32,
    pub node_version: String,
    pub pi_core_version: String,
    pub delta_count: usize,
    pub child_exit_code: i32,
}

#[derive(Default)]
struct RequestRegistry {
    active: HashMap<String, watch::Sender<bool>>,
    pre_cancelled: HashSet<String>,
}

#[derive(Default)]
struct SupervisorState {
    registry: Mutex<RequestRegistry>,
    settled: Notify,
    closing: AtomicBool,
}

#[derive(Clone, Default)]
pub struct PiSupervisor(Arc<SupervisorState>);

struct ActiveRequest {
    id: String,
    state: Arc<SupervisorState>,
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.state.registry.lock() {
            registry.active.remove(&self.id);
        }
        self.state.settled.notify_waiters();
    }
}

impl PiSupervisor {
    #[cfg(test)]
    pub(super) fn has_active_requests(&self) -> bool {
        !self.0.registry.lock().unwrap().active.is_empty()
    }

    fn register_request(
        &self,
        request_id: &str,
        localized: Option<LanguageKind>,
    ) -> Result<(ActiveRequest, watch::Receiver<bool>), String> {
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(match localized {
                Some(locale) => i18n::pi::pi_invalid_request_id(locale).into(),
                None => "Invalid request identity".into(),
            });
        }
        let (cancel, cancelled) = watch::channel(false);
        let mut registry = self.0.registry.lock().map_err(|_| match localized {
            Some(locale) => i18n::pi::pi_registry_unavailable(locale).to_string(),
            None => "Request registry is unavailable".to_string(),
        })?;
        if self.0.closing.load(Ordering::SeqCst) {
            return Err(match localized {
                Some(locale) => i18n::pi::pi_shutting_down(locale).into(),
                None => "Application is shutting down".into(),
            });
        }
        if registry.active.contains_key(request_id) {
            return Err(match localized {
                Some(locale) => i18n::pi::pi_duplicate_request(locale).into(),
                None => "Request is already running".into(),
            });
        }
        if registry.pre_cancelled.remove(request_id) {
            let _ = cancel.send(true);
        }
        registry.active.insert(request_id.to_owned(), cancel);
        Ok((
            ActiveRequest {
                id: request_id.to_owned(),
                state: self.0.clone(),
            },
            cancelled,
        ))
    }

    pub fn cancel(&self, request_id: &str) -> Result<(), String> {
        let registry = self
            .0
            .registry
            .lock()
            .map_err(|_| "Request registry is unavailable")?;
        let sender = registry
            .active
            .get(request_id)
            .ok_or("No matching active request")?;
        sender
            .send(true)
            .map_err(|_| "Request has already settled".to_string())
    }

    /// Cancels an active Pi request, or records a pre-cancellation tombstone under the same lock
    /// so a request that is still completing host setup immediately observes cancellation upon
    /// registration with zero race window.
    pub fn cancel_or_pre_cancel(&self, request_id: &str) {
        if let Ok(mut registry) = self.0.registry.lock() {
            if let Some(sender) = registry.active.get(request_id) {
                let _ = sender.send(true);
            } else {
                if registry.pre_cancelled.len() >= 128 {
                    registry.pre_cancelled.clear();
                }
                registry.pre_cancelled.insert(request_id.to_owned());
            }
        }
    }

    pub fn clear_pre_cancelled(&self, request_id: &str) {
        if let Ok(mut registry) = self.0.registry.lock() {
            registry.pre_cancelled.remove(request_id);
        }
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        {
            let registry = self
                .0
                .registry
                .lock()
                .map_err(|_| "Request registry is unavailable")?;
            self.0.closing.store(true, Ordering::SeqCst);
            for sender in registry.active.values() {
                let _ = sender.send(true);
            }
        }
        timeout(EXIT_TIMEOUT + EXIT_TIMEOUT, async {
            loop {
                let settled = self.0.settled.notified();
                if self
                    .0
                    .registry
                    .lock()
                    .map_err(|_| "Request registry is unavailable")?
                    .active
                    .is_empty()
                {
                    return Ok(());
                }
                settled.await;
            }
        })
        .await
        .map_err(|_| "Timed out waiting for Pi processes to exit".to_string())?
    }

    #[cfg(any(test, feature = "smoke-harness"))]
    pub async fn run_probe(
        &self,
        paths: &PiPaths,
        request_id: String,
        mut on_event: impl FnMut(&PiEvent) -> Result<(), String> + Send,
    ) -> Result<ProbeReport, String> {
        let (_registration, mut cancelled) = self.register_request(&request_id, None)?;
        let mut proc = SpawnedPiProcess::spawn(paths, None, &|raw| raw)?;
        let pid = proc.pid;
        let exchange = timeout(PROBE_TIMEOUT, async {
            let ready = timeout(START_TIMEOUT, next_event(&mut proc.lines))
                .await
                .map_err(|_| "Pi startup handshake timed out")??;
            let PiEvent::Ready {
                protocol_version,
                pi_core_version,
                node_version,
                pid: reported_pid,
            } = &ready
            else {
                return Err("Expected Pi readiness handshake".into());
            };
            if *protocol_version != PROTOCOL_VERSION
                || *reported_pid != pid
                || pi_core_version != env!("PICKET_PI_CORE_VERSION")
            {
                return Err("Incompatible Pi runtime handshake".into());
            }
            let mut report = ProbeReport {
                request_id: request_id.clone(),
                status: String::new(),
                child_pid: pid,
                node_version: node_version.clone(),
                pi_core_version: pi_core_version.clone(),
                delta_count: 0,
                child_exit_code: -1,
            };
            on_event(&ready)?;
            let context = RequestContext {
                repository_id: "foundation".into(),
                worktree_id: "temporary".into(),
                session_id: request_id.clone(),
                scope_version: "foundation-v1".into(),
            };
            write_command(
                &mut proc.stdin,
                &PiCommand::Probe {
                    request_id: request_id.clone(),
                    context: context.clone(),
                },
            )
            .await?;
            let mut cancel_sent = false;
            loop {
                let event = tokio::select! {
                    biased;
                    change = cancelled.changed(), if !cancel_sent => {
                        change.map_err(|_| "Cancellation channel closed")?;
                        write_command(&mut proc.stdin, &PiCommand::Cancel { request_id: request_id.clone() }).await?;
                        cancel_sent = true;
                        continue;
                    }
                    event = next_event(&mut proc.lines) => event?,
                };
                let Some((id, event_context)) = event.envelope() else {
                    return Err(format!("Unexpected Pi control event: {event:?}"));
                };
                if id != request_id || event_context != &context {
                    return Err("Pi response belongs to a different request or scope".into());
                }
                on_event(&event)?;
                match event {
                    PiEvent::TextDelta { .. } => report.delta_count += 1,
                    PiEvent::Completed { .. } => {
                        report.status = "completed".into();
                        return Ok(report);
                    }
                    PiEvent::Cancelled { .. } => {
                        report.status = "cancelled".into();
                        return Ok(report);
                    }
                    PiEvent::Failed { message, .. } => return Err(message),
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| "Pi probe timed out".to_string())
        .and_then(|r| r);
        // Always close and reap the child, including timeout, UI disconnect and protocol errors.
        let (exit, diagnostic) = proc.shutdown_and_reap(None).await?;
        let status = exit?;
        if !status.success() {
            return Err(format!("Pi exited with {status}: {diagnostic}"));
        }
        let mut report = exchange.map_err(|e| {
            if diagnostic.is_empty() {
                e
            } else {
                format!("{e}: {diagnostic}")
            }
        })?;
        report.child_exit_code = status.code().unwrap_or(-1);
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_task(
        &self,
        paths: &PiPaths,
        request_id: String,
        expected_context: RequestContext,
        command: &PiCommand,
        idle_timeout: Duration,
        secret_to_redact: Option<&str>,
        on_event: impl FnMut(&PiEvent) -> Result<(), String> + Send,
    ) -> Result<PiTaskOutcome, AppError> {
        self.run_task_with_tools(
            paths,
            request_id,
            expected_context,
            command,
            idle_timeout,
            secret_to_redact,
            on_event,
            |_call_id, tool_name, _args| async move {
                PiToolExecutionResult::error(format!(
                    "Tool `{tool_name}` is not enabled for this request"
                ))
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_task_with_tools<F, Fut>(
        &self,
        paths: &PiPaths,
        request_id: String,
        expected_context: RequestContext,
        command: &PiCommand,
        idle_timeout: Duration,
        secret_to_redact: Option<&str>,
        mut on_event: impl FnMut(&PiEvent) -> Result<(), String> + Send,
        mut on_tool_call: F,
    ) -> Result<PiTaskOutcome, AppError>
    where
        F: FnMut(String, String, Value) -> Fut + Send,
        Fut: Future<Output = PiToolExecutionResult> + Send,
    {
        let locale = command.locale();
        let scrub = |raw: String| -> String {
            let Some(secret) = secret_to_redact.map(str::trim).filter(|s| !s.is_empty()) else {
                return raw;
            };
            raw.replace(secret, "[REDACTED]")
        };
        let (_registration, mut cancelled) = self
            .register_request(&request_id, Some(locale))
            .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
        if *cancelled.borrow() {
            return Ok(PiTaskOutcome::Cancelled);
        }
        let mut proc = SpawnedPiProcess::spawn(paths, Some(locale), &scrub)
            .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
        let pid = proc.pid;
        let total_ceiling = if idle_timeout < Duration::from_secs(5) {
            idle_timeout.saturating_mul(8)
        } else {
            MAX_TASK_TOTAL_TIMEOUT.max(idle_timeout)
        };
        let mut remaining_budget = total_ceiling;
        let mut explicit_error = false;
        let exchange: Result<PiTaskOutcome, AppError> = async {
            let handshake_started = Instant::now();
            let ready = tokio::select! {
                biased;
                change = cancelled.changed() => {
                    change.map_err(|_| {
                        AppError::model(
                            AppErrorCode::PiRuntimeError,
                            i18n::pi::pi_cancel_channel_closed(locale),
                        )
                    })?;
                    return Ok(PiTaskOutcome::Cancelled);
                }
                step = timeout(START_TIMEOUT.min(remaining_budget), next_event(&mut proc.lines)) => {
                    match step {
                        Ok(result) => {
                            result.map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?
                        }
                        Err(_) => {
                            explicit_error = true;
                            return Err(AppError::model(
                                AppErrorCode::ModelTimeout,
                                i18n::pi::pi_handshake_timeout(locale),
                            ));
                        }
                    }
                }
            };
            remaining_budget = remaining_budget.saturating_sub(handshake_started.elapsed());
            if *cancelled.borrow() {
                return Ok(PiTaskOutcome::Cancelled);
            }
            let PiEvent::Ready {
                protocol_version,
                pi_core_version,
                pid: reported_pid,
                ..
            } = &ready
            else {
                explicit_error = true;
                return Err(AppError::model(
                    AppErrorCode::PiRuntimeError,
                    i18n::pi::pi_invalid_handshake(locale),
                ));
            };
            if *protocol_version != PROTOCOL_VERSION
                || *reported_pid != pid
                || pi_core_version != env!("PICKET_PI_CORE_VERSION")
            {
                explicit_error = true;
                return Err(AppError::model(
                    AppErrorCode::PiRuntimeError,
                    i18n::pi::pi_incompatible_version(locale),
                ));
            }
            on_event(&ready)
                .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
            write_command(&mut proc.stdin, command)
                .await
                .map_err(|msg| {
                    explicit_error = true;
                    AppError::model(AppErrorCode::PiRuntimeError, msg)
                })?;
            let mut cancel_sent = false;
            loop {
                if remaining_budget.is_zero() {
                    explicit_error = true;
                    return Err(AppError::model(
                        AppErrorCode::ModelTimeout,
                        i18n::pi::pi_request_timed_out(locale),
                    ));
                }
                let step_timeout = if cancel_sent {
                    CANCEL_SETTLE_TIMEOUT.min(idle_timeout)
                } else {
                    idle_timeout
                }
                .min(remaining_budget);
                let step_started = Instant::now();
                let event = tokio::select! {
                    biased;
                    change = cancelled.changed(), if !cancel_sent => {
                        change.map_err(|_| {
                            AppError::model(
                                AppErrorCode::PiRuntimeError,
                                i18n::pi::pi_cancel_channel_closed(locale),
                            )
                        })?;
                        write_command(&mut proc.stdin, &PiCommand::Cancel { request_id: request_id.clone() })
                            .await
                            .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
                        cancel_sent = true;
                        continue;
                    }
                    step = timeout(step_timeout, next_event(&mut proc.lines)) => {
                        remaining_budget = remaining_budget.saturating_sub(step_started.elapsed());
                        match step {
                            Ok(result) => {
                                result.map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?
                            }
                            Err(_) => {
                                explicit_error = true;
                                return Err(AppError::model(
                                    AppErrorCode::ModelTimeout,
                                    i18n::pi::pi_request_timed_out(locale),
                                ));
                            }
                        }
                    }
                };
                if let PiEvent::ProtocolError { message } = event {
                    explicit_error = true;
                    return Err(AppError::model(
                        AppErrorCode::PiRuntimeError,
                        i18n::pi::pi_protocol_rejected(locale, &message),
                    ));
                }
                let Some((id, event_context)) = event.envelope() else {
                    explicit_error = true;
                    return Err(AppError::model(
                        AppErrorCode::PiRuntimeError,
                        i18n::pi::pi_unexpected_control_event(locale),
                    ));
                };
                if id != request_id || event_context != &expected_context {
                    explicit_error = true;
                    return Err(AppError::model(
                        AppErrorCode::PiRuntimeError,
                        i18n::pi::pi_mismatched_target(locale),
                    ));
                }
                on_event(&event)
                    .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
                match event {
                    PiEvent::ToolCall {
                        call_id,
                        tool_name,
                        args,
                        ..
                    } => {
                        // Pause the active LLM execution budget while host tools (including
                        // blocking `message_user` questionnaires and Tier-2 human approvals) run.
                        // Git-mutating tools are non-interruptible once started so we never drop
                        // an in-flight commit, rebase, push, or worktree cleanup mid-write.
                        let non_interruptible = matches!(
                            tool_name.as_str(),
                            "commit_exact_scope"
                                | "sync_and_push"
                                | "manage_submit_worktree"
                                | "cleanup_submit_duplicates"
                        );
                        let tool_future = on_tool_call(call_id.clone(), tool_name, args);
                        tokio::pin!(tool_future);
                        let outcome = tokio::select! {
                            biased;
                            change = cancelled.changed(), if !cancel_sent => {
                                change.map_err(|_| {
                                    AppError::model(
                                        AppErrorCode::PiRuntimeError,
                                        i18n::pi::pi_cancel_channel_closed(locale),
                                    )
                                })?;
                                write_command(&mut proc.stdin, &PiCommand::Cancel { request_id: request_id.clone() })
                                    .await
                                    .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, msg))?;
                                cancel_sent = true;
                                if non_interruptible {
                                    let _ = (&mut tool_future).await;
                                }
                                continue;
                            }
                            result = &mut tool_future => result,
                        };
                        write_command(
                            &mut proc.stdin,
                            &PiCommand::ToolResult {
                                request_id: request_id.clone(),
                                call_id,
                                ok: outcome.ok,
                                content: outcome.content,
                                details: outcome.details,
                                is_error: Some(outcome.is_error),
                            },
                        )
                        .await
                        .map_err(|msg| {
                            explicit_error = true;
                            AppError::model(AppErrorCode::PiRuntimeError, msg)
                        })?;
                    }
                    PiEvent::Completed { text, .. } => return Ok(PiTaskOutcome::Completed { text }),
                    PiEvent::CustomActionCompleted { summary, links, .. } => {
                        return Ok(PiTaskOutcome::CustomActionCompleted { summary, links });
                    }
                    PiEvent::CustomActionDrafted { draft, .. } => {
                        return Ok(PiTaskOutcome::CustomActionDrafted { draft });
                    }
                    PiEvent::RunCommandsDiscovered {
                        summary, commands, ..
                    } => {
                        return Ok(PiTaskOutcome::RunCommandsDiscovered { summary, commands });
                    }
                    PiEvent::Cancelled { .. } => return Ok(PiTaskOutcome::Cancelled),
                    PiEvent::Failed {
                        message,
                        error_code,
                        ..
                    } => {
                        explicit_error = true;
                        return Err(AppError::from_pi_error_code(error_code.as_deref(), message));
                    }
                    _ => {}
                }
            }
        }
        .await;

        let (exit, raw_diagnostic) = proc
            .shutdown_and_reap(Some(locale))
            .await
            .map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, scrub(msg)))?;
        let diagnostic = scrub(raw_diagnostic);
        if explicit_error {
            return exchange.map_err(|err| {
                err.map_message(|m| {
                    let sanitized = scrub(m);
                    i18n::pi::pi_append_diagnostic(locale, &sanitized, &diagnostic)
                })
            });
        }
        if matches!(exchange, Ok(PiTaskOutcome::Cancelled)) {
            return Ok(PiTaskOutcome::Cancelled);
        }
        let status =
            exit.map_err(|msg| AppError::model(AppErrorCode::PiRuntimeError, scrub(msg)))?;
        if !status.success() {
            return Err(AppError::model(
                AppErrorCode::PiRuntimeError,
                scrub(i18n::pi::pi_exited_abnormally(locale, &status, &diagnostic)),
            ));
        }
        exchange.map_err(|err| {
            err.map_message(|m| {
                let sanitized = scrub(m);
                i18n::pi::pi_append_diagnostic(locale, &sanitized, &diagnostic)
            })
        })
    }
}
