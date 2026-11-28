//! Protocol types, limits, sanitization, and URL/ANSI helpers for local Run commands and
//! long-running user project processes.

use crate::{
    i18n::{LanguageKind, deserialize_language_or_default},
    repository::types::ReadTarget,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const RUN_PROTOCOL_VERSION: u8 = 1;
pub const RUN_STATE_EVENT: &str = "picket:run-state";
pub const RUN_LOG_BATCH_EVENT: &str = "picket:run-log-batch";
pub const RUN_LOGS_SELECT_EVENT: &str = "picket:run-logs-select";
pub const RUN_DISCOVER_PROGRESS_EVENT: &str = "picket:run-discover-progress";
pub const RUN_LOGS_WINDOW_LABEL: &str = "run-logs";

pub const MAX_RUN_COMMANDS_PER_REPO: usize = 64;
pub const MAX_RUN_COMMAND_ID_CHARS: usize = 64;
pub const MAX_RUN_COMMAND_LABEL_CHARS: usize = 48;
pub const MAX_RUN_COMMAND_GROUP_CHARS: usize = 48;
pub const MAX_RUN_COMMAND_TEXT_CHARS: usize = 1_000;
pub const MAX_RUN_COMMAND_CWD_CHARS: usize = 256;
pub const MAX_RUN_ENV_ENTRIES: usize = 32;
pub const MAX_RUN_ENV_KEY_CHARS: usize = 128;
pub const MAX_RUN_ENV_VALUE_CHARS: usize = 2_000;
pub const MAX_LOG_LINES_PER_PROCESS: usize = 2_000;
pub const MAX_LOG_BYTES_PER_PROCESS: usize = 512 * 1024;
pub const MAX_LOG_LINE_BYTES: usize = 16 * 1024;
pub const MAX_DETECTED_URLS: usize = 8;

pub const ALLOWED_RUN_COMMAND_ICONS: &[&str] = &[
    "node", "pnpm", "vite", "java", "spring", "rust", "python", "go", "docker", "desktop",
    "database", "terminal",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunEnvEntry {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PersistedRunCommand {
    pub id: String,
    pub label: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<RunEnvEntry>>", optional))]
    pub env: Vec<RunEnvEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum RunProcessStatus {
    Starting,
    Running,
    Stopping,
    Exited,
    Failed,
}

impl RunProcessStatus {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum RunLogStream {
    Stdout,
    Stderr,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RunLogLine {
    #[cfg_attr(test, ts(type = "number"))]
    pub seq: u64,
    pub stream: RunLogStream,
    pub text: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct RunProcessSnapshot {
    pub process_id: String,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub generation: u64,
    pub repository_id: String,
    pub repository_name: String,
    pub worktree_id: String,
    pub worktree_label: String,
    pub command_id: String,
    pub label: String,
    pub command: String,
    pub cwd: String,
    pub status: RunProcessStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    #[cfg_attr(test, ts(type = "number"))]
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "number | undefined"))]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub detected_urls: Vec<String>,
    pub log_line_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_log_line: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RunLogBatchEvent {
    pub process_id: String,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub generation: u64,
    pub lines: Vec<RunLogLine>,
    pub detected_urls: Vec<String>,
    pub log_line_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct UserEnvSummary {
    pub shell: String,
    pub path_count: u32,
    pub loaded_from_login_shell: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRunCommandRequest {
    pub target: ReadTarget,
    pub command: PersistedRunCommand,
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<RunEnvEntry>>", optional))]
    pub shared_env: Vec<RunEnvEntry>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopRunCommandRequest {
    pub process_id: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestartRunCommandRequest {
    pub target: ReadTarget,
    pub command: PersistedRunCommand,
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<RunEnvEntry>>", optional))]
    pub shared_env: Vec<RunEnvEntry>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetRunLogsRequest {
    pub process_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct RunProcessLogsReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<RunProcessSnapshot>,
    pub lines: Vec<RunLogLine>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClearRunLogsRequest {
    pub process_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRunLogsWindowRequest {
    #[serde(default)]
    pub process_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRunLocalUrlRequest {
    pub url: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredRunCommand {
    pub label: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub description: String,
    pub category: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoverRunCommandsRequest {
    pub request_id: String,
    pub target: ReadTarget,
    pub model_id: String,
    #[serde(default)]
    pub existing_commands: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DiscoverRunCommandsProgressEvent {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    pub request_id: String,
    #[cfg_attr(test, ts(type = "'running' | 'done' | 'cancelled' | 'failed'"))]
    pub status: &'static str,
    pub status_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DiscoverRunCommandsReply {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'discoverRunCommands'"))]
    pub kind: &'static str,
    pub request_id: String,
    #[cfg_attr(test, ts(type = "'done' | 'cancelled'"))]
    pub status: &'static str,
    pub summary: String,
    pub commands: Vec<DiscoveredRunCommand>,
}

pub fn make_process_id(worktree_id: &str, command_id: &str) -> String {
    format!("{worktree_id}::{command_id}")
}

/// Validates and normalizes an optional worktree-relative working directory.
/// Returns `Ok(None)` for empty / `"."` (worktree root), or `Ok(Some(clean))` for a valid
/// relative subdirectory that never escapes the worktree or enters `.git`.
pub fn normalize_run_cwd(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let raw_trimmed = value.trim();
    if raw_trimmed.is_empty() || raw_trimmed == "." {
        return Ok(None);
    }
    if raw_trimmed.starts_with('/')
        || raw_trimmed.starts_with('~')
        || raw_trimmed.contains('\0')
        || raw_trimmed.contains('\\')
    {
        return Err(format!("Invalid relative working directory: `{value}`"));
    }
    let trimmed = raw_trimmed
        .strip_prefix("./")
        .unwrap_or(raw_trimmed)
        .trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_RUN_COMMAND_CWD_CHARS {
        return Err(format!("Invalid relative working directory: `{value}`"));
    }
    let mut clean_segments = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.eq_ignore_ascii_case(".git")
        {
            return Err(format!("Invalid relative working directory: `{value}`"));
        }
        clean_segments.push(segment);
    }
    Ok(Some(clean_segments.join("/")))
}

pub fn is_valid_run_env_key(key: &str) -> bool {
    let trimmed = key.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_RUN_ENV_KEY_CHARS {
        return false;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn normalize_run_icon(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed == "auto" {
        return None;
    }
    ALLOWED_RUN_COMMAND_ICONS
        .contains(&trimmed.as_str())
        .then_some(trimmed)
}

pub fn normalize_run_group(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.contains('\0')
        || trimmed.contains('\r')
        || trimmed.contains('\n')
        || trimmed.chars().count() > MAX_RUN_COMMAND_GROUP_CHARS
    {
        return Err(format!("Invalid run command group: `{value}`"));
    }
    Ok(Some(trimmed.to_owned()))
}

pub fn sanitize_run_env_entries(raw: Vec<RunEnvEntry>) -> Vec<RunEnvEntry> {
    let mut seen_keys = HashSet::new();
    let mut env = Vec::new();
    for entry in raw {
        let key = entry.key.trim().to_owned();
        if !is_valid_run_env_key(&key)
            || entry.value.contains('\0')
            || entry.value.chars().count() > MAX_RUN_ENV_VALUE_CHARS
        {
            continue;
        }
        if seen_keys.insert(key.clone()) {
            env.push(RunEnvEntry {
                key,
                value: entry.value,
            });
            if env.len() >= MAX_RUN_ENV_ENTRIES {
                break;
            }
        }
    }
    env
}

pub fn validate_run_env_entries(entries: &[RunEnvEntry]) -> bool {
    if entries.len() > MAX_RUN_ENV_ENTRIES {
        return false;
    }
    let mut seen_keys = HashSet::with_capacity(entries.len());
    for env in entries {
        let key = env.key.trim();
        if !is_valid_run_env_key(key)
            || !seen_keys.insert(key.to_owned())
            || env.value.contains('\0')
            || env.value.chars().count() > MAX_RUN_ENV_VALUE_CHARS
        {
            return false;
        }
    }
    true
}

pub fn sanitize_run_command(raw: PersistedRunCommand) -> Option<PersistedRunCommand> {
    let id = raw.id.trim().to_owned();
    let label = raw.label.trim().to_owned();
    let command = raw.command.replace("\r\n", "\n").trim().to_owned();
    if id.is_empty()
        || id.chars().count() > MAX_RUN_COMMAND_ID_CHARS
        || id.contains('\0')
        || label.is_empty()
        || label.chars().count() > MAX_RUN_COMMAND_LABEL_CHARS
        || label.contains('\0')
        || command.is_empty()
        || command.chars().count() > MAX_RUN_COMMAND_TEXT_CHARS
        || command.contains('\0')
    {
        return None;
    }
    let cwd = normalize_run_cwd(raw.cwd.as_deref()).ok()?;
    let group = normalize_run_group(raw.group.as_deref()).ok()?;
    let mut seen_keys = HashSet::new();
    let mut env = Vec::new();
    for entry in raw.env {
        let key = entry.key.trim().to_owned();
        if !is_valid_run_env_key(&key)
            || entry.value.contains('\0')
            || entry.value.chars().count() > MAX_RUN_ENV_VALUE_CHARS
        {
            return None;
        }
        if seen_keys.insert(key.clone()) {
            env.push(RunEnvEntry {
                key,
                value: entry.value,
            });
            if env.len() >= MAX_RUN_ENV_ENTRIES {
                break;
            }
        }
    }
    let icon = normalize_run_icon(raw.icon.as_deref());
    Some(PersistedRunCommand {
        id,
        label,
        command,
        cwd,
        group,
        icon,
        env,
    })
}

pub fn validate_run_commands(commands: &[PersistedRunCommand]) -> bool {
    if commands.len() > MAX_RUN_COMMANDS_PER_REPO {
        return false;
    }
    let mut seen_ids = HashSet::with_capacity(commands.len());
    for item in commands {
        let id = item.id.trim();
        let label = item.label.trim();
        let command = item.command.replace("\r\n", "\n");
        let cmd_trimmed = command.trim();
        let invalid_icon = item.icon.as_deref().is_some_and(|raw| {
            let trimmed = raw.trim();
            !trimmed.is_empty()
                && trimmed != "auto"
                && !ALLOWED_RUN_COMMAND_ICONS.contains(&trimmed.to_ascii_lowercase().as_str())
        });
        if id.is_empty()
            || id.chars().count() > MAX_RUN_COMMAND_ID_CHARS
            || id.contains('\0')
            || !seen_ids.insert(id.to_owned())
            || label.is_empty()
            || label.chars().count() > MAX_RUN_COMMAND_LABEL_CHARS
            || label.contains('\0')
            || cmd_trimmed.is_empty()
            || cmd_trimmed.chars().count() > MAX_RUN_COMMAND_TEXT_CHARS
            || cmd_trimmed.contains('\0')
            || normalize_run_cwd(item.cwd.as_deref()).is_err()
            || normalize_run_group(item.group.as_deref()).is_err()
            || invalid_icon
            || item.env.len() > MAX_RUN_ENV_ENTRIES
        {
            return false;
        }
        for env in &item.env {
            if !is_valid_run_env_key(&env.key)
                || env.value.contains('\0')
                || env.value.chars().count() > MAX_RUN_ENV_VALUE_CHARS
            {
                return false;
            }
        }
    }
    true
}

/// Strips ANSI CSI/OSC escape sequences so streamed dev-server logs render cleanly.
pub fn strip_ansi_codes(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                // CSI sequence: ESC [ ... <final byte 0x40..=0x7e>
                i += 2;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                // OSC sequence: ESC ] ... (BEL or ESC \)
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            // Any other 2-byte ESC sequence (consume only ASCII second bytes to stay on UTF-8 char boundaries):
            if i + 1 < bytes.len() && bytes[i + 1].is_ascii() {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'\r' {
            i += 1;
            continue;
        }
        // Copy valid UTF-8 char from current index:
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Validates that a URL is strictly a local loopback HTTP(S) URL (`localhost`, `127.0.0.1`, `[::1]`).
pub fn validate_local_http_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed
            .bytes()
            .any(|b| b <= 0x20 || b == 0x7f || b == b'\\' || b == b'"' || b == b'\'')
    {
        return Err("Invalid URL syntax".into());
    }
    let rest = if let Some(after) = trimmed.strip_prefix("http://") {
        after
    } else if let Some(after) = trimmed.strip_prefix("https://") {
        after
    } else {
        return Err("URL must start with http:// or https://".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err("URL authority is invalid".into());
    }
    let (host, port_opt) = if let Some(after_bracket) = authority.strip_prefix("[::1]") {
        if after_bracket.is_empty() {
            ("[::1]", None)
        } else if let Some(port) = after_bracket.strip_prefix(':') {
            ("[::1]", Some(port))
        } else {
            return Err("Invalid IPv6 loopback host".into());
        }
    } else if let Some((h, p)) = authority.split_once(':') {
        (h, Some(p))
    } else {
        (authority, None)
    };

    let host_lower = host.to_ascii_lowercase();
    if !matches!(host_lower.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
        return Err("Only localhost, 127.0.0.1, or [::1] URLs are allowed".into());
    }
    if let Some(port_str) = port_opt {
        if port_str.is_empty() || !port_str.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Invalid port number".into());
        }
        let port: u16 = port_str
            .parse()
            .map_err(|_| "Invalid port number".to_string())?;
        if port == 0 {
            return Err("Port cannot be 0".into());
        }
    }
    Ok(trimmed.to_owned())
}

/// Extracts unique valid local loopback URLs from a log line.
pub fn extract_local_urls(line: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for token in line.split_whitespace() {
        let mut candidate = token;
        if candidate.starts_with('[') && candidate.ends_with(']') && candidate.len() > 2 {
            candidate = &candidate[1..candidate.len() - 1];
        }
        candidate = candidate
            .trim_start_matches(['(', '[', '<', '"', '\'', ',', ';'])
            .trim_end_matches(['.', ',', ';', ')', '>', '"', '\'']);
        while candidate.ends_with(']') && !candidate.ends_with("[::1]") {
            candidate = &candidate[..candidate.len() - 1];
        }
        if !(candidate.starts_with("http://") || candidate.starts_with("https://")) {
            continue;
        }
        if let Ok(valid) = validate_local_http_url(candidate)
            && !urls.contains(&valid)
        {
            urls.push(valid);
            if urls.len() >= MAX_DETECTED_URLS {
                break;
            }
        }
    }
    urls
}
