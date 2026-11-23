use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Notify, watch},
    time::timeout,
};

const PROTOCOL_VERSION: u32 = 1;
const START_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestContext {
    repository_id: String,
    worktree_id: String,
    session_id: String,
    scope_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiEvent {
    Ready {
        protocol_version: u32,
        pi_core_version: String,
        node_version: String,
        pid: u32,
    },
    Started {
        request_id: String,
        context: RequestContext,
    },
    TextDelta {
        request_id: String,
        context: RequestContext,
        delta: String,
    },
    Completed {
        request_id: String,
        context: RequestContext,
        text: String,
    },
    Cancelled {
        request_id: String,
        context: RequestContext,
    },
    Failed {
        request_id: String,
        context: RequestContext,
        message: String,
    },
    ProtocolError {
        message: String,
    },
}

impl PiEvent {
    fn envelope(&self) -> Option<(&str, &RequestContext)> {
        match self {
            Self::Started {
                request_id,
                context,
            }
            | Self::TextDelta {
                request_id,
                context,
                ..
            }
            | Self::Completed {
                request_id,
                context,
                ..
            }
            | Self::Cancelled {
                request_id,
                context,
            }
            | Self::Failed {
                request_id,
                context,
                ..
            } => Some((request_id, context)),
            _ => None,
        }
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
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("Invalid request identity".into());
        }
        let (cancel, mut cancelled) = watch::channel(false);
        let _registration = {
            let mut active = self
                .0
                .active
                .lock()
                .map_err(|_| "Request registry is unavailable")?;
            if self.0.closing.load(Ordering::SeqCst) {
                return Err("Application is shutting down".into());
            }
            if active.contains_key(&request_id) {
                return Err("Request is already running".into());
            }
            active.insert(request_id.clone(), cancel);
            ActiveRequest {
                id: request_id.clone(),
                state: self.0.clone(),
            }
        };
        if !paths.node.is_file() || !paths.script.is_file() {
            return Err(
                "Bundled Pi runtime is missing; run pnpm sidecar:prepare before building".into(),
            );
        }
        let working_directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let mut child = Command::new(&paths.node)
            .arg(&paths.script)
            .current_dir(working_directory.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Cannot start bundled Pi runtime: {e}"))?;
        let pid = child.id().ok_or("Pi process has no PID")?;
        let mut stdin = child.stdin.take().ok_or("Missing Pi input")?;
        let stdout = child.stdout.take().ok_or("Missing Pi output")?;
        let mut stderr = child.stderr.take().ok_or("Missing Pi error output")?;
        let stderr_task = tokio::spawn(async move {
            let mut retained = Vec::new();
            let mut chunk = [0_u8; 1024];
            while let Ok(count) = stderr.read(&mut chunk).await {
                if count == 0 {
                    break;
                }
                let room = MAX_STDERR_BYTES.saturating_sub(retained.len());
                retained.extend_from_slice(&chunk[..count.min(room)]);
            }
            String::from_utf8_lossy(&retained).into_owned()
        });
        let mut lines = BufReader::new(stdout).lines();
        let exchange = timeout(PROBE_TIMEOUT, async {
            let ready = timeout(START_TIMEOUT, next_event(&mut lines)).await
                .map_err(|_| "Pi startup handshake timed out")??;
            let PiEvent::Ready { protocol_version, pi_core_version, node_version, pid: reported_pid } = &ready else {
                return Err("Expected Pi readiness handshake".into());
            };
            if *protocol_version != PROTOCOL_VERSION || *reported_pid != pid || pi_core_version != "1.6.0" {
                return Err("Incompatible Pi runtime handshake".into());
            }
            let mut report = ProbeReport { request_id: request_id.clone(), status: String::new(),
                child_pid: pid, node_version: node_version.clone(), pi_core_version: pi_core_version.clone(),
                delta_count: 0, child_exit_code: -1 };
            on_event(&ready)?;
            let context = RequestContext { repository_id: "foundation".into(), worktree_id: "temporary".into(),
                session_id: request_id.clone(), scope_version: "foundation-v1".into() };
            write_command(&mut stdin, &serde_json::json!({ "type": "probe", "requestId": request_id, "context": context })).await?;
            let mut cancel_sent = false;
            loop {
                let event = tokio::select! {
                    biased;
                    change = cancelled.changed(), if !cancel_sent => {
                        change.map_err(|_| "Cancellation channel closed")?;
                        write_command(&mut stdin, &serde_json::json!({ "type": "cancel", "requestId": request_id })).await?;
                        cancel_sent = true;
                        continue;
                    }
                    event = next_event(&mut lines) => event?,
                };
                let Some((id, event_context)) = event.envelope() else {
                    return Err(format!("Unexpected Pi control event: {event:?}"));
                };
                if id != request_id || event_context != &context { return Err("Pi response belongs to a different request or scope".into()); }
                on_event(&event)?;
                match event {
                    PiEvent::TextDelta { .. } => report.delta_count += 1,
                    PiEvent::Completed { .. } => { report.status = "completed".into(); return Ok(report); }
                    PiEvent::Cancelled { .. } => { report.status = "cancelled".into(); return Ok(report); }
                    PiEvent::Failed { message, .. } => return Err(message),
                    _ => {}
                }
            }
        }).await.map_err(|_| "Pi probe timed out".to_string()).and_then(|r| r);
        // Always close and reap the child, including timeout, UI disconnect and protocol errors.
        let _ = write_command(&mut stdin, &serde_json::json!({ "type": "shutdown" })).await;
        drop(stdin);
        let exit = match timeout(EXIT_TIMEOUT, child.wait()).await {
            Ok(status) => status.map_err(|e| e.to_string()),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err("Pi process did not exit gracefully; it was terminated".into())
            }
        };
        let diagnostic = stderr_task.await.map_err(|e| e.to_string())?;
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
}

async fn write_command(
    stdin: &mut tokio::process::ChildStdin,
    command: &serde_json::Value,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(command).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await.map_err(|e| e.to_string())
}

async fn next_event(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
) -> Result<PiEvent, String> {
    let line = lines
        .next_line()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Pi closed its output before completing the request")?;
    if line.len() > MAX_LINE_BYTES {
        return Err("Pi protocol frame exceeds the limit".into());
    }
    serde_json::from_str(&line).map_err(|e| format!("Invalid Pi protocol event: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_sdk_streams_and_child_is_reaped() {
        let mut deltas = String::new();
        let report = PiSupervisor::default()
            .run_probe(&PiPaths::development(), "complete-test".into(), |event| {
                if let PiEvent::TextDelta { delta, .. } = event {
                    deltas.push_str(delta);
                }
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(report.status, "completed");
        assert_eq!(report.child_exit_code, 0);
        assert!(report.delta_count > 1);
        assert!(deltas.contains("scripted SDK response"));
    }

    #[tokio::test]
    async fn cancellation_settles_and_allows_another_request() {
        let supervisor = PiSupervisor::default();
        let report = supervisor
            .run_probe(&PiPaths::development(), "cancel-test".into(), |event| {
                if matches!(event, PiEvent::TextDelta { .. }) {
                    supervisor.cancel("cancel-test")?;
                }
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(report.status, "cancelled");
        assert_eq!(report.child_exit_code, 0);
        let next = supervisor
            .run_probe(&PiPaths::development(), "after-cancel".into(), |_| Ok(()))
            .await
            .unwrap();
        assert_eq!(next.status, "completed");
        assert_eq!(next.child_exit_code, 0);
        supervisor.shutdown().await.unwrap();
        assert!(
            supervisor
                .run_probe(&PiPaths::development(), "after-close".into(), |_| Ok(()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn crashed_child_is_reaped_and_does_not_block_the_next_request() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("crash.mjs");
        std::fs::write(
            &script,
            r#"
            process.stdout.write(JSON.stringify({type:'ready', protocolVersion:1,
                piCoreVersion:'1.6.0', nodeVersion:process.versions.node, pid:process.pid}) + '\n');
            process.stdin.once('data', () => process.exit(23));
        "#,
        )
        .unwrap();
        let paths = PiPaths {
            node: PiPaths::development().node,
            script,
        };
        let supervisor = PiSupervisor::default();
        let mut crashed_pid = 0;
        let result = supervisor
            .run_probe(&paths, "crash-test".into(), |event| {
                if let PiEvent::Ready { pid, .. } = event {
                    crashed_pid = *pid;
                }
                Ok(())
            })
            .await;
        assert!(result.unwrap_err().contains("23"));
        assert_ne!(crashed_pid, 0);
        assert!(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &crashed_pid.to_string()])
                .output()
                .unwrap()
                .status
                .success()
        );
        let next = supervisor
            .run_probe(&PiPaths::development(), "after-crash".into(), |_| Ok(()))
            .await
            .unwrap();
        assert_eq!(next.status, "completed");
        supervisor.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn duplicate_request_is_rejected_while_running() {
        let supervisor = PiSupervisor::default();
        let other = supervisor.clone();
        let paths = PiPaths::development();
        let run =
            tokio::spawn(
                async move { other.run_probe(&paths, "same-id".into(), |_| Ok(())).await },
            );
        loop {
            if !supervisor.0.active.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        let duplicate = supervisor
            .run_probe(&PiPaths::development(), "same-id".into(), |_| Ok(()))
            .await;
        assert!(duplicate.unwrap_err().contains("already running"));
        supervisor.shutdown().await.unwrap();
        assert_eq!(run.await.unwrap().unwrap().status, "cancelled");
    }
}
