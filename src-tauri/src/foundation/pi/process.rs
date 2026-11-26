use super::{
    protocol::{PiCommand, PiEvent},
    supervisor::PiPaths,
};
use crate::i18n::{self, LanguageKind};
use serde::Serialize;
use std::{
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    task::JoinHandle,
    time::timeout,
};

pub(super) const PROTOCOL_VERSION: u32 = 1;
pub(super) const START_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const CANCEL_SETTLE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const MAX_TASK_TOTAL_TIMEOUT: Duration = Duration::from_secs(600);
pub(super) const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;

pub(super) struct SpawnedPiProcess {
    _working_directory: tempfile::TempDir,
    child: Child,
    pub(super) pid: u32,
    pub(super) stdin: ChildStdin,
    pub(super) lines: tokio::io::Lines<BufReader<ChildStdout>>,
    stderr_task: JoinHandle<String>,
}

impl SpawnedPiProcess {
    pub(super) fn spawn(
        paths: &PiPaths,
        localized: Option<LanguageKind>,
        scrub: &impl Fn(String) -> String,
    ) -> Result<Self, String> {
        if !paths.node.is_file() || !paths.script.is_file() {
            return Err(match localized {
                Some(locale) => i18n::pi::pi_runtime_missing(locale).into(),
                None => {
                    "Bundled Pi runtime is missing; run pnpm sidecar:prepare before building".into()
                }
            });
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
            .map_err(|e| match localized {
                Some(locale) => scrub(i18n::pi::pi_cannot_start_runtime(locale, &e)),
                None => format!("Cannot start bundled Pi runtime: {e}"),
            })?;
        let pid = child.id().ok_or_else(|| match localized {
            Some(locale) => i18n::pi::pi_missing_pid(locale).to_string(),
            None => "Pi process has no PID".to_string(),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| match localized {
            Some(locale) => i18n::pi::pi_missing_stdin(locale).to_string(),
            None => "Missing Pi input".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| match localized {
            Some(locale) => i18n::pi::pi_missing_stdout(locale).to_string(),
            None => "Missing Pi output".to_string(),
        })?;
        let mut stderr = child.stderr.take().ok_or_else(|| match localized {
            Some(locale) => i18n::pi::pi_missing_stderr(locale).to_string(),
            None => "Missing Pi error output".to_string(),
        })?;
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
        let lines = BufReader::new(stdout).lines();
        Ok(Self {
            _working_directory: working_directory,
            child,
            pid,
            stdin,
            lines,
            stderr_task,
        })
    }

    pub(super) async fn shutdown_and_reap(
        mut self,
        localized: Option<LanguageKind>,
    ) -> Result<(Result<ExitStatus, String>, String), String> {
        let _ = write_command(&mut self.stdin, &PiCommand::Shutdown).await;
        drop(self.stdin);
        let exit = match timeout(EXIT_TIMEOUT, self.child.wait()).await {
            Ok(status) => status.map_err(|e| e.to_string()),
            Err(_) => {
                let _ = self.child.kill().await;
                let _ = self.child.wait().await;
                Err(match localized {
                    Some(locale) => i18n::pi::pi_terminated_on_exit_timeout(locale).into(),
                    None => "Pi process did not exit gracefully; it was terminated".into(),
                })
            }
        };
        let diagnostic = self.stderr_task.await.map_err(|e| e.to_string())?;
        Ok((exit, diagnostic))
    }
}

pub(super) async fn write_command<T: Serialize>(
    stdin: &mut ChildStdin,
    command: &T,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(command).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

pub(super) async fn next_event(
    lines: &mut tokio::io::Lines<BufReader<ChildStdout>>,
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
