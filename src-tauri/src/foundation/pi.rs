use crate::{
    i18n::{self, LanguageKind},
    repository::ai_context::ExtractedDiffFile,
    skills::RuntimeSkillOverrideCatalog,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
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
const CANCEL_SETTLE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TASK_TOTAL_TIMEOUT: Duration = Duration::from_secs(600);
const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_LINE_BYTES: usize = 256 * 1024;
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
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub scope_version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderSpec {
    pub provider: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<&'static str>,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiPlanFileEntry {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub ignored_for_review: bool,
    pub binary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCPushQaTurn {
    pub question_id: String,
    pub question: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiConflictFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_content: Option<String>,
    pub upstream_content: String,
    pub local_content: String,
    pub conflicted_content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiCommand {
    Probe {
        request_id: String,
        context: RequestContext,
    },
    TestConnection {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        locale: LanguageKind,
    },
    ListModels {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        locale: LanguageKind,
    },
    ReviewScope {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        prompt: String,
        files: Vec<ExtractedDiffFile>,
        skipped_paths: Vec<String>,
        locale: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        enable_tools: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    #[serde(rename = "planCPush")]
    PlanCPush {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        recent_commits: Vec<String>,
        prompt: String,
        files: Vec<PiPlanFileEntry>,
        unselected_paths: Vec<String>,
        qa_history: Vec<PiCPushQaTurn>,
        locale: LanguageKind,
        commit_language: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    ResolveConflict {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        upstream: String,
        user_instruction: String,
        commits: Vec<String>,
        files: Vec<PiConflictFile>,
        locale: LanguageKind,
    },
    RunCustomAction {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        detached: bool,
        remote_branches: Vec<String>,
        action_label: String,
        action_prompt: String,
        extra_prompt: String,
        selected_files: Vec<PiPlanFileEntry>,
        unselected_paths: Vec<String>,
        local_keep_paths: Vec<String>,
        locale: LanguageKind,
        commit_language: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_checkpoint: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    GenerateCustomAction {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        existing_labels: Vec<String>,
        user_request: String,
        locale: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    ToolResult {
        request_id: String,
        call_id: String,
        ok: bool,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    Cancel {
        request_id: String,
    },
    Shutdown,
}

impl PiCommand {
    pub const fn locale(&self) -> LanguageKind {
        match self {
            Self::TestConnection { locale, .. }
            | Self::ListModels { locale, .. }
            | Self::ReviewScope { locale, .. }
            | Self::PlanCPush { locale, .. }
            | Self::ResolveConflict { locale, .. }
            | Self::RunCustomAction { locale, .. }
            | Self::GenerateCustomAction { locale, .. } => *locale,
            Self::Probe { .. } | Self::ToolResult { .. } | Self::Cancel { .. } | Self::Shutdown => {
                LanguageKind::En
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PiToolExecutionResult {
    pub ok: bool,
    pub content: String,
    pub details: Option<Value>,
    pub is_error: bool,
}

impl PiToolExecutionResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            ok: true,
            content: content.into(),
            details: None,
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            ok: false,
            content: content.into(),
            details: None,
            is_error: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiReviewFinding {
    pub file_id: String,
    pub path: String,
    pub line: u32,
    pub severity: String,
    pub title: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiCPushCommitGroup {
    pub id: String,
    pub message: String,
    pub file_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiCPushQuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub recommended: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiCPushPlan {
    Ready {
        summary: String,
        groups: Vec<PiCPushCommitGroup>,
        skipped_file_ids: Vec<String>,
    },
    Question {
        title: String,
        scope: String,
        description: String,
        affected_file_ids: Vec<String>,
        options: Vec<PiCPushQuestionOption>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiResolvedConflictFile {
    pub file_id: String,
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiConflictResolution {
    pub summary: String,
    pub resolved_files: Vec<PiResolvedConflictFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiRemoteModelOption {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiActionLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiCustomActionDraft {
    pub label: String,
    pub prompt: String,
    pub summary: String,
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
    Activity {
        request_id: String,
        context: RequestContext,
    },
    TextDelta {
        request_id: String,
        context: RequestContext,
        delta: String,
    },
    ToolCall {
        request_id: String,
        context: RequestContext,
        call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolProgress {
        request_id: String,
        context: RequestContext,
        call_id: String,
        tool_name: String,
        status: String,
    },
    Completed {
        request_id: String,
        context: RequestContext,
        text: String,
    },
    ModelsListed {
        request_id: String,
        context: RequestContext,
        models: Vec<PiRemoteModelOption>,
    },
    ReviewCompleted {
        request_id: String,
        context: RequestContext,
        findings: Vec<PiReviewFinding>,
    },
    #[serde(rename = "cPushPlanCompleted")]
    CPushPlanCompleted {
        request_id: String,
        context: RequestContext,
        plan: PiCPushPlan,
    },
    ConflictResolved {
        request_id: String,
        context: RequestContext,
        resolution: PiConflictResolution,
    },
    CustomActionCompleted {
        request_id: String,
        context: RequestContext,
        summary: String,
        #[serde(default)]
        links: Vec<PiActionLink>,
    },
    CustomActionDrafted {
        request_id: String,
        context: RequestContext,
        draft: PiCustomActionDraft,
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
            | Self::Activity {
                request_id,
                context,
            }
            | Self::TextDelta {
                request_id,
                context,
                ..
            }
            | Self::ToolCall {
                request_id,
                context,
                ..
            }
            | Self::ToolProgress {
                request_id,
                context,
                ..
            }
            | Self::Completed {
                request_id,
                context,
                ..
            }
            | Self::ModelsListed {
                request_id,
                context,
                ..
            }
            | Self::ReviewCompleted {
                request_id,
                context,
                ..
            }
            | Self::CPushPlanCompleted {
                request_id,
                context,
                ..
            }
            | Self::ConflictResolved {
                request_id,
                context,
                ..
            }
            | Self::CustomActionCompleted {
                request_id,
                context,
                ..
            }
            | Self::CustomActionDrafted {
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

#[derive(Debug)]
pub enum PiTaskOutcome {
    Completed {
        text: String,
    },
    ModelsListed {
        models: Vec<PiRemoteModelOption>,
    },
    ReviewCompleted {
        findings: Vec<PiReviewFinding>,
    },
    CPushPlanCompleted {
        plan: PiCPushPlan,
    },
    ConflictResolved {
        resolution: PiConflictResolution,
    },
    CustomActionCompleted {
        summary: String,
        links: Vec<PiActionLink>,
    },
    CustomActionDrafted {
        draft: PiCustomActionDraft,
    },
    Cancelled,
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
            let ready = timeout(START_TIMEOUT, next_event(&mut lines))
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
                &mut stdin,
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
                        write_command(&mut stdin, &PiCommand::Cancel { request_id: request_id.clone() }).await?;
                        cancel_sent = true;
                        continue;
                    }
                    event = next_event(&mut lines) => event?,
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
        let _ = write_command(&mut stdin, &PiCommand::Shutdown).await;
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
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(i18n::pi_invalid_request_id(locale).into());
        }
        let (cancel, mut cancelled) = watch::channel(false);
        let _registration = {
            let mut active = self
                .0
                .active
                .lock()
                .map_err(|_| i18n::pi_registry_unavailable(locale))?;
            if self.0.closing.load(Ordering::SeqCst) {
                return Err(i18n::pi_shutting_down(locale).into());
            }
            if active.contains_key(&request_id) {
                return Err(i18n::pi_duplicate_request(locale).into());
            }
            active.insert(request_id.clone(), cancel);
            ActiveRequest {
                id: request_id.clone(),
                state: self.0.clone(),
            }
        };
        if !paths.node.is_file() || !paths.script.is_file() {
            return Err(i18n::pi_runtime_missing(locale).into());
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
            .map_err(|e| scrub(i18n::pi_cannot_start_runtime(locale, &e)))?;
        let pid = child.id().ok_or_else(|| i18n::pi_missing_pid(locale))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| i18n::pi_missing_stdin(locale))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| i18n::pi_missing_stdout(locale))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| i18n::pi_missing_stderr(locale))?;
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
        let total_ceiling = if idle_timeout < Duration::from_secs(5) {
            idle_timeout.saturating_mul(8)
        } else {
            MAX_TASK_TOTAL_TIMEOUT.max(idle_timeout)
        };
        let mut timed_out = false;
        let exchange = match timeout(total_ceiling, async {
            let ready = match timeout(START_TIMEOUT.min(total_ceiling), next_event(&mut lines)).await
            {
                Ok(result) => result?,
                Err(_) => return Err(i18n::pi_handshake_timeout(locale).to_string()),
            };
            let PiEvent::Ready {
                protocol_version,
                pi_core_version,
                pid: reported_pid,
                ..
            } = &ready
            else {
                return Err(i18n::pi_invalid_handshake(locale).into());
            };
            if *protocol_version != PROTOCOL_VERSION
                || *reported_pid != pid
                || pi_core_version != "1.6.0"
            {
                return Err(i18n::pi_incompatible_version(locale).into());
            }
            on_event(&ready)?;
            write_command(&mut stdin, command).await?;
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
                        change.map_err(|_| i18n::pi_cancel_channel_closed(locale).to_string())?;
                        write_command(&mut stdin, &PiCommand::Cancel { request_id: request_id.clone() }).await?;
                        cancel_sent = true;
                        continue;
                    }
                    step = timeout(step_timeout, next_event(&mut lines)) => {
                        match step {
                            Ok(result) => result?,
                            Err(_) => {
                                timed_out = true;
                                return Err(i18n::pi_request_timed_out(locale).to_string());
                            }
                        }
                    }
                };
                if let PiEvent::ProtocolError { message } = event {
                    return Err(i18n::pi_protocol_rejected(locale, &message));
                }
                let Some((id, event_context)) = event.envelope() else {
                    return Err(i18n::pi_unexpected_control_event(locale).into());
                };
                if id != request_id || event_context != &expected_context {
                    return Err(i18n::pi_mismatched_target(locale).into());
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
                                change.map_err(|_| i18n::pi_cancel_channel_closed(locale).to_string())?;
                                write_command(&mut stdin, &PiCommand::Cancel { request_id: request_id.clone() }).await?;
                                cancel_sent = true;
                                continue;
                            }
                            result = &mut tool_future => result,
                        };
                        write_command(
                            &mut stdin,
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
                Err(i18n::pi_request_timed_out(locale).to_string())
            }
        };

        let _ = write_command(&mut stdin, &PiCommand::Shutdown).await;
        drop(stdin);
        let exit = match timeout(EXIT_TIMEOUT, child.wait()).await {
            Ok(status) => status.map_err(|e| e.to_string()),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(i18n::pi_terminated_on_exit_timeout(locale).into())
            }
        };
        let diagnostic = scrub(stderr_task.await.map_err(|e| e.to_string())?);
        if !timed_out {
            let status = exit.map_err(&scrub)?;
            if !status.success() {
                return Err(scrub(i18n::pi_exited_abnormally(
                    locale,
                    &status,
                    &diagnostic,
                )));
            }
        }
        exchange.map_err(|e| {
            let sanitized = scrub(e);
            i18n::pi_append_diagnostic(locale, &sanitized, &diagnostic)
        })
    }
}

async fn write_command<T: Serialize>(
    stdin: &mut tokio::process::ChildStdin,
    command: &T,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(command).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
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

    fn sample_model() -> PiProviderSpec {
        PiProviderSpec {
            provider: "openai",
            api_format: Some("openai-responses"),
            model_id: "gpt-4.1".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            context_window: Some(200_000),
            max_output_tokens: Some(32_768),
        }
    }

    #[test]
    fn pi_command_serializes_locale_and_commit_language_in_camel_case() {
        let context = RequestContext {
            repository_id: "repo-1".into(),
            worktree_id: "wt-1".into(),
            session_id: "sess-1".into(),
            scope_version: "scope-1".into(),
        };
        let test_conn = serde_json::to_value(PiCommand::TestConnection {
            request_id: "req-conn".into(),
            context: context.clone(),
            model: sample_model(),
            locale: LanguageKind::Zh,
        })
        .unwrap();
        assert_eq!(test_conn["type"], "testConnection");
        assert_eq!(test_conn["locale"], "zh");

        let review = serde_json::to_value(PiCommand::ReviewScope {
            request_id: "req-rev".into(),
            context: context.clone(),
            model: sample_model(),
            branch: "feat/i18n".into(),
            prompt: "focus on auth".into(),
            files: Vec::new(),
            skipped_paths: Vec::new(),
            locale: LanguageKind::Zh,
            enable_tools: Some(true),
            skill_overrides: None,
        })
        .unwrap();
        assert_eq!(review["type"], "reviewScope");
        assert_eq!(review["prompt"], "focus on auth");
        assert_eq!(review["locale"], "zh");
        assert_eq!(review["enableTools"], true);

        let plan = serde_json::to_value(PiCommand::PlanCPush {
            request_id: "req-plan".into(),
            context: context.clone(),
            model: sample_model(),
            branch: "feat/i18n".into(),
            recent_commits: Vec::new(),
            prompt: "split lockfile".into(),
            files: Vec::new(),
            unselected_paths: Vec::new(),
            qa_history: Vec::new(),
            locale: LanguageKind::Zh,
            commit_language: LanguageKind::En,
            skill_overrides: None,
        })
        .unwrap();
        assert_eq!(plan["type"], "planCPush");
        assert_eq!(plan["prompt"], "split lockfile");
        assert_eq!(plan["locale"], "zh");
        assert_eq!(plan["commitLanguage"], "en");

        let resolve = serde_json::to_value(PiCommand::ResolveConflict {
            request_id: "req-conflict".into(),
            context,
            model: sample_model(),
            branch: "feat/i18n".into(),
            upstream: "origin/feat/i18n".into(),
            user_instruction: "keep both".into(),
            commits: vec!["abc1234 feat: local".into()],
            files: Vec::new(),
            locale: LanguageKind::En,
        })
        .unwrap();
        assert_eq!(resolve["type"], "resolveConflict");
        assert_eq!(resolve["locale"], "en");
    }

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

    #[tokio::test]
    async fn activity_heartbeats_extend_idle_timeout_while_silent_hang_times_out() {
        let directory = tempfile::tempdir().unwrap();
        let active_script = directory.path().join("thinking-stream.mjs");
        std::fs::write(
            &active_script,
            r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            rl.on('line', async (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(0); return; }
              const env = { requestId: cmd.requestId, context: cmd.context };
              emit({ ...env, type: 'started' });
              for (let i = 0; i < 4; i++) {
                await new Promise((r) => setTimeout(r, 80));
                emit({ ...env, type: 'activity' });
              }
              emit({ ...env, type: 'completed', text: 'feat: finished after long thinking\n\n- kept alive by activity heartbeats' });
            });
        "#,
        )
        .unwrap();

        let supervisor = PiSupervisor::default();
        let context = RequestContext {
            repository_id: "repo-1".into(),
            worktree_id: "wt-1".into(),
            session_id: "sess-1".into(),
            scope_version: "scope-1".into(),
        };
        let active_paths = PiPaths {
            node: PiPaths::development().node,
            script: active_script,
        };
        let active_cmd = PiCommand::TestConnection {
            request_id: "req-active".into(),
            context: context.clone(),
            model: sample_model(),
            locale: LanguageKind::En,
        };
        let outcome = supervisor
            .run_task(
                &active_paths,
                "req-active".into(),
                context.clone(),
                &active_cmd,
                Duration::from_millis(180),
                None,
                |_| Ok(()),
            )
            .await
            .unwrap();
        match outcome {
            PiTaskOutcome::Completed { text } => assert!(text.contains("kept alive")),
            other => panic!("expected Completed, got {other:?}"),
        }

        let silent_script = directory.path().join("silent-hang.mjs");
        std::fs::write(
            &silent_script,
            r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            rl.on('line', (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(1); return; }
              emit({ requestId: cmd.requestId, context: cmd.context, type: 'started' });
              setTimeout(() => {}, 10_000);
            });
        "#,
        )
        .unwrap();
        let silent_paths = PiPaths {
            node: PiPaths::development().node,
            script: silent_script,
        };
        let silent_cmd = PiCommand::TestConnection {
            request_id: "req-silent".into(),
            context: context.clone(),
            model: sample_model(),
            locale: LanguageKind::En,
        };
        let err = supervisor
            .run_task(
                &silent_paths,
                "req-silent".into(),
                context,
                &silent_cmd,
                Duration::from_millis(180),
                None,
                |_| Ok(()),
            )
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "Model request timed out; current draft and selection were preserved"
        );
    }

    #[tokio::test]
    async fn bidirectional_tool_call_roundtrip_and_cancellation_work() {
        let directory = tempfile::tempdir().unwrap();
        let tool_script = directory.path().join("tool-loop.mjs");
        std::fs::write(
            &tool_script,
            r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            let active = null;
            rl.on('line', (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(0); return; }
              if (cmd.type === 'cancel') {
                if (active) emit({ requestId: active.requestId, context: active.context, type: 'cancelled' });
                active = null;
                return;
              }
              if (cmd.type === 'toolResult') {
                if (active && cmd.callId === 'tc-1') {
                  emit({
                    requestId: active.requestId,
                    context: active.context,
                    type: 'completed',
                    text: `tool-reply:${cmd.content}`,
                  });
                  active = null;
                }
                return;
              }
              active = { requestId: cmd.requestId, context: cmd.context };
              emit({ requestId: cmd.requestId, context: cmd.context, type: 'started' });
              emit({
                requestId: cmd.requestId,
                context: cmd.context,
                type: 'toolCall',
                callId: 'tc-1',
                toolName: 'read_repo_file',
                args: { path: 'src/auth.ts' },
              });
            });
        "#,
        )
        .unwrap();

        let supervisor = PiSupervisor::default();
        let context = RequestContext {
            repository_id: "repo-1".into(),
            worktree_id: "wt-1".into(),
            session_id: "sess-1".into(),
            scope_version: "scope-1".into(),
        };
        let paths = PiPaths {
            node: PiPaths::development().node,
            script: tool_script,
        };
        let cmd = PiCommand::TestConnection {
            request_id: "req-tool-roundtrip".into(),
            context: context.clone(),
            model: sample_model(),
            locale: LanguageKind::En,
        };
        let outcome = supervisor
            .run_task_with_tools(
                &paths,
                "req-tool-roundtrip".into(),
                context.clone(),
                &cmd,
                Duration::from_secs(5),
                None,
                |_| Ok(()),
                |call_id, tool_name, args| async move {
                    assert_eq!(call_id, "tc-1");
                    assert_eq!(tool_name, "read_repo_file");
                    assert_eq!(args["path"], "src/auth.ts");
                    PiToolExecutionResult::success("export const ok = true;")
                },
            )
            .await
            .unwrap();
        match outcome {
            PiTaskOutcome::Completed { text } => {
                assert_eq!(text, "tool-reply:export const ok = true;");
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        // Verify cancellation while a tool handler is suspended waiting (e.g. ask_user_question).
        let cancel_sup = supervisor.clone();
        let cancel_cmd = PiCommand::TestConnection {
            request_id: "req-tool-cancel".into(),
            context: context.clone(),
            model: sample_model(),
            locale: LanguageKind::En,
        };
        let cancelled_outcome = supervisor
            .run_task_with_tools(
                &paths,
                "req-tool-cancel".into(),
                context,
                &cancel_cmd,
                Duration::from_secs(5),
                None,
                |_| Ok(()),
                |_call_id, _tool_name, _args| {
                    let sup = cancel_sup.clone();
                    async move {
                        let _ = sup.cancel("req-tool-cancel");
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        PiToolExecutionResult::success("unreachable")
                    }
                },
            )
            .await
            .unwrap();
        assert!(matches!(cancelled_outcome, PiTaskOutcome::Cancelled));
    }
}
