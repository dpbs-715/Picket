use super::{
    process::{
        CANCEL_SETTLE_TIMEOUT, EXIT_TIMEOUT, MAX_TASK_TOTAL_TIMEOUT, PROBE_TIMEOUT,
        PROTOCOL_VERSION, START_TIMEOUT, SpawnedPiProcess, next_event, write_command,
    },
    protocol::{PiCommand, PiEvent, PiTaskOutcome, PiToolExecutionResult, RequestContext},
};
use crate::i18n::{self, LanguageKind};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
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
struct SupervisorState {
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
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
        if let Ok(mut active) = self.state.active.lock() {
            active.remove(&self.id);
        }
        self.state.settled.notify_waiters();
    }
}

impl PiSupervisor {
    #[cfg(test)]
    pub(super) fn has_active_requests(&self) -> bool {
        !self.0.active.lock().unwrap().is_empty()
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
        let mut active = self.0.active.lock().map_err(|_| match localized {
            Some(locale) => i18n::pi::pi_registry_unavailable(locale).to_string(),
            None => "Request registry is unavailable".to_string(),
        })?;
        if self.0.closing.load(Ordering::SeqCst) {
            return Err(match localized {
                Some(locale) => i18n::pi::pi_shutting_down(locale).into(),
                None => "Application is shutting down".into(),
            });
        }
        if active.contains_key(request_id) {
            return Err(match localized {
                Some(locale) => i18n::pi::pi_duplicate_request(locale).into(),
                None => "Request is already running".into(),
            });
        }
        active.insert(request_id.to_owned(), cancel);
        Ok((
            ActiveRequest {
                id: request_id.to_owned(),
                state: self.0.clone(),
            },
            cancelled,
        ))
    }

    pub fn cancel(&self, request_id: &str) -> Result<(), String> {
        let active = self
            .0
            .active
            .lock()
            .map_err(|_| "Request registry is unavailable")?;
        let sender = active.get(request_id).ok_or("No matching active request")?;
        sender
            .send(true)
            .map_err(|_| "Request has already settled".to_string())
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        {
            let active = self
                .0
                .active
                .lock()
                .map_err(|_| "Request registry is unavailable")?;
            self.0.closing.store(true, Ordering::SeqCst);
            for sender in active.values() {
                let _ = sender.send(true);
            }
        }
        timeout(EXIT_TIMEOUT + EXIT_TIMEOUT, async {
            loop {
                let settled = self.0.settled.notified();
                if self
                    .0
                    .active
                    .lock()
                    .map_err(|_| "Request registry is unavailable")?
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
                || pi_core_version != "1.6.0"
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
    ) -> Result<PiTaskOutcome, String> {
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
    ) -> Result<PiTaskOutcome, String>
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
        let (_registration, mut cancelled) = self.register_request(&request_id, Some(locale))?;
        let mut proc = SpawnedPiProcess::spawn(paths, Some(locale), &scrub)?;
        let pid = proc.pid;
        let total_ceiling = if idle_timeout < Duration::from_secs(5) {
            idle_timeout.saturating_mul(8)
        } else {
            MAX_TASK_TOTAL_TIMEOUT.max(idle_timeout)
        };
        let mut timed_out = false;
        let exchange = match timeout(total_ceiling, async {
            let ready = match timeout(START_TIMEOUT.min(total_ceiling), next_event(&mut proc.lines))
                .await
            {
                Ok(result) => result?,
                Err(_) => return Err(i18n::pi::pi_handshake_timeout(locale).to_string()),
            };
            let PiEvent::Ready {
                protocol_version,
                pi_core_version,
                pid: reported_pid,
                ..
            } = &ready
            else {
                return Err(i18n::pi::pi_invalid_handshake(locale).into());
            };
            if *protocol_version != PROTOCOL_VERSION
                || *reported_pid != pid
                || pi_core_version != "1.6.0"
            {
                return Err(i18n::pi::pi_incompatible_version(locale).into());
            }
            on_event(&ready)?;
            write_command(&mut proc.stdin, command).await?;
            let mut cancel_sent = false;
            loop {
                let step_timeout = if cancel_sent {
                    CANCEL_SETTLE_TIMEOUT.min(idle_timeout)
                } else {
                    idle_timeout
                };
                let event = tokio::select! {
                    biased;
                    change = cancelled.changed(), if !cancel_sent => {
                        change.map_err(|_| i18n::pi::pi_cancel_channel_closed(locale).to_string())?;
                        write_command(&mut proc.stdin, &PiCommand::Cancel { request_id: request_id.clone() }).await?;
                        cancel_sent = true;
                        continue;
                    }
                    step = timeout(step_timeout, next_event(&mut proc.lines)) => {
                        match step {
                            Ok(result) => result?,
                            Err(_) => {
                                timed_out = true;
                                return Err(i18n::pi::pi_request_timed_out(locale).to_string());
                            }
                        }
                    }
                };
                if let PiEvent::ProtocolError { message } = event {
                    return Err(i18n::pi::pi_protocol_rejected(locale, &message));
                }
                let Some((id, event_context)) = event.envelope() else {
                    return Err(i18n::pi::pi_unexpected_control_event(locale).into());
                };
                if id != request_id || event_context != &expected_context {
                    return Err(i18n::pi::pi_mismatched_target(locale).into());
                }
                on_event(&event)?;
                match event {
                    PiEvent::ToolCall {
                        call_id,
                        tool_name,
                        args,
                        ..
                    } => {
                        let tool_future = on_tool_call(call_id.clone(), tool_name, args);
                        tokio::pin!(tool_future);
                        let outcome = tokio::select! {
                            biased;
                            change = cancelled.changed(), if !cancel_sent => {
                                change.map_err(|_| i18n::pi::pi_cancel_channel_closed(locale).to_string())?;
                                write_command(&mut proc.stdin, &PiCommand::Cancel { request_id: request_id.clone() }).await?;
                                cancel_sent = true;
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
                        .await?;
                    }
                    PiEvent::Completed { text, .. } => return Ok(PiTaskOutcome::Completed { text }),
                    PiEvent::ModelsListed { models, .. } => {
                        return Ok(PiTaskOutcome::ModelsListed { models });
                    }
                    PiEvent::ReviewCompleted { findings, .. } => {
                        return Ok(PiTaskOutcome::ReviewCompleted { findings });
                    }
                    PiEvent::CPushPlanCompleted { plan, .. } => {
                        return Ok(PiTaskOutcome::CPushPlanCompleted { plan });
                    }
                    PiEvent::ConflictResolved { resolution, .. } => {
                        return Ok(PiTaskOutcome::ConflictResolved { resolution });
                    }
                    PiEvent::CustomActionCompleted { summary, links, .. } => {
                        return Ok(PiTaskOutcome::CustomActionCompleted { summary, links });
                    }
                    PiEvent::CustomActionDrafted { draft, .. } => {
                        return Ok(PiTaskOutcome::CustomActionDrafted { draft });
                    }
                    PiEvent::Cancelled { .. } => return Ok(PiTaskOutcome::Cancelled),
                    PiEvent::Failed { message, .. } => return Err(message),
                    _ => {}
                }
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                timed_out = true;
                Err(i18n::pi::pi_request_timed_out(locale).to_string())
            }
        };

        let (exit, raw_diagnostic) = proc.shutdown_and_reap(Some(locale)).await?;
        let diagnostic = scrub(raw_diagnostic);
        if !timed_out {
            let status = exit.map_err(&scrub)?;
            if !status.success() {
                return Err(scrub(i18n::pi::pi_exited_abnormally(
                    locale,
                    &status,
                    &diagnostic,
                )));
            }
        }
        exchange.map_err(|e| {
            let sanitized = scrub(e);
            i18n::pi::pi_append_diagnostic(locale, &sanitized, &diagnostic)
        })
    }
}
