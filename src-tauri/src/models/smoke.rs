//! Local loopback HTTP SSE fixture server used only in `#[cfg(test)]` and explicit
//! `--foundation-ui-smoke` runs. Speaks real OpenAI Completions, OpenAI Responses,
//! and Anthropic Messages SSE frames so the bundled Pi Core sidecar (`@anthropic-ai/sdk`
//! and `openai` SDK) executes full HTTP + SSE round-trips without external network keys.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct RecordedModelHttpRequest {
    pub path: String,
    pub authorization: Option<String>,
    pub x_api_key: Option<String>,
    pub body: String,
}

pub struct LocalModelSseServer {
    pub base_url: String,
    stop: Arc<AtomicBool>,
    recorded: Arc<Mutex<Vec<RecordedModelHttpRequest>>>,
    #[cfg_attr(not(test), expect(dead_code))]
    fail_status: Arc<Mutex<Option<(u16, String)>>>,
    #[cfg_attr(not(test), expect(dead_code))]
    slow_delay_ms: Arc<Mutex<u64>>,
    handle: Option<JoinHandle<()>>,
}

impl LocalModelSseServer {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let fail_status = Arc::new(Mutex::new(None));
        let slow_delay_ms = Arc::new(Mutex::new(0));

        let stop_bg = stop.clone();
        let recorded_bg = recorded.clone();
        let fail_bg = fail_status.clone();
        let delay_bg = slow_delay_ms.clone();

        let handle = thread::spawn(move || {
            while !stop_bg.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let rec = recorded_bg.clone();
                        let fail = fail_bg.clone();
                        let delay = *delay_bg.lock().unwrap();
                        let stop_conn = stop_bg.clone();
                        thread::spawn(move || {
                            let _ = handle_connection(stream, rec, fail, delay, stop_conn);
                        });
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            stop,
            recorded,
            fail_status,
            slow_delay_ms,
            handle: Some(handle),
        })
    }

    #[cfg(test)]
    pub fn set_failure(&self, status: Option<(u16, String)>) {
        *self.fail_status.lock().unwrap() = status;
    }

    #[cfg(test)]
    pub fn set_slow_delay_ms(&self, ms: u64) {
        *self.slow_delay_ms.lock().unwrap() = ms;
    }

    pub fn requests(&self) -> Vec<RecordedModelHttpRequest> {
        self.recorded.lock().unwrap().clone()
    }
}

impl Drop for LocalModelSseServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    recorded: Arc<Mutex<Vec<RecordedModelHttpRequest>>>,
    fail_status: Arc<Mutex<Option<(u16, String)>>>,
    slow_delay_ms: u64,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let raw_target = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned();
    let path = raw_target
        .split_once('?')
        .map_or(raw_target.as_str(), |(p, _)| p)
        .to_owned();

    let mut content_length = 0usize;
    let mut authorization = None;
    let mut x_api_key = None;
    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line)? == 0 || header_line == "\r\n" || header_line == "\n"
        {
            break;
        }
        if let Some((name, value)) = header_line.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            let val = value.trim().to_owned();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            } else if key == "authorization" {
                authorization = Some(val);
            } else if key == "x-api-key" {
                x_api_key = Some(val);
            }
        }
    }
    let mut body_bytes = vec![0u8; content_length];
    reader.read_exact(&mut body_bytes)?;
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    recorded.lock().unwrap().push(RecordedModelHttpRequest {
        path: path.clone(),
        authorization,
        x_api_key,
        body: body.clone(),
    });

    if let Some((code, payload)) = fail_status.lock().unwrap().clone() {
        write!(
            stream,
            "HTTP/1.1 {code} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )?;
        stream.flush()?;
        return Ok(());
    }

    if path.ends_with("/models") {
        let payload = serde_json::json!({
            "data": [
                {
                    "id": "kimi-k3.1-pro",
                    "display_name": "Kimi K3.1 Pro",
                    "context_length": 1_048_576,
                    "max_completion_tokens": 131_072
                },
                {
                    "id": "kimi-k2.8"
                }
            ]
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )?;
        stream.flush()?;
        return Ok(());
    }

    let response_text = if body.contains("authorized Git conflict merge resolver") {
        build_conflict_smoke_resolution(&body)
    } else if body.contains("Picket's C-Push planner")
        || body.contains("C-Push commit splitting planner")
        || body.contains("<cpush_skill_spec>")
    {
        build_cpush_smoke_plan(&body)
    } else if body.contains("pre-commit code reviewer") {
        let file_id = extract_marker(&body, "fileId: ").unwrap_or_else(|| "unknown".into());
        let file_path = extract_marker(&body, "### File: ")
            .map(|raw| {
                raw.split(" (renamed from ")
                    .next()
                    .unwrap_or(&raw)
                    .to_owned()
            })
            .unwrap_or_else(|| "part.txt".into());
        serde_json::json!({
            "findings": [{
                "fileId": file_id,
                "path": file_path,
                "line": 1,
                "severity": "high",
                "title": "Verify staged configuration boundary",
                "reason": "Detected key-value update; verify upstream callers remain compatible before committing."
            }]
        })
        .to_string()
    } else {
        "OK".to_owned()
    };

    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;

    if path.ends_with("/messages") {
        let start = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-sonnet-4-20250514",
                "content": [], "stop_reason": null, "stop_sequence": null,
                "usage": { "input_tokens": 10, "output_tokens": 1 }
            }
        });
        write!(stream, "event: message_start\ndata: {start}\n\n")?;
        write!(
            stream,
            "event: content_block_start\ndata: {}\n\n",
            serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})
        )?;
        write!(
            stream,
            "event: content_block_delta\ndata: {}\n\n",
            serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":response_text}})
        )?;
        stream.flush()?;
        wait_slow(slow_delay_ms, &stop);
        write!(
            stream,
            "event: content_block_stop\ndata: {}\n\nevent: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
            serde_json::json!({"type":"content_block_stop","index":0}),
            serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":12}}),
            serde_json::json!({"type":"message_stop"})
        )?;
    } else if path.ends_with("/responses") {
        let send = |s: &mut TcpStream, v: serde_json::Value| -> std::io::Result<()> {
            write!(s, "data: {v}\n\n")
        };
        send(
            &mut stream,
            serde_json::json!({"type":"response.created","response":{"id":"resp_1","status":"in_progress","output":[]}}),
        )?;
        send(
            &mut stream,
            serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"id":"m1","type":"message","role":"assistant","status":"in_progress","content":[]}}),
        )?;
        send(
            &mut stream,
            serde_json::json!({"type":"response.content_part.added","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        )?;
        send(
            &mut stream,
            serde_json::json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":response_text}),
        )?;
        stream.flush()?;
        wait_slow(slow_delay_ms, &stop);
        send(
            &mut stream,
            serde_json::json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":response_text}),
        )?;
        send(
            &mut stream,
            serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"id":"m1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":response_text,"annotations":[]}]}}),
        )?;
        send(
            &mut stream,
            serde_json::json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[{"id":"m1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":response_text,"annotations":[]}]}],"usage":{"input_tokens":8,"output_tokens":8,"total_tokens":16}}}),
        )?;
    } else {
        let chunk1 = serde_json::json!({
            "id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1700000000, "model": "test",
            "choices": [{ "index": 0, "delta": { "content": response_text }, "finish_reason": null }]
        });
        write!(stream, "data: {chunk1}\n\n")?;
        stream.flush()?;
        wait_slow(slow_delay_ms, &stop);
        let chunk2 = serde_json::json!({
            "id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1700000000, "model": "test",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20 }
        });
        write!(stream, "data: {chunk2}\n\ndata: [DONE]\n\n")?;
    }
    stream.flush()?;
    Ok(())
}

fn wait_slow(ms: u64, stop: &AtomicBool) {
    let mut waited = 0u64;
    while waited < ms && !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(20));
        waited += 20;
    }
}

fn extract_marker(haystack: &str, prefix: &str) -> Option<String> {
    let idx = haystack.find(prefix)?;
    let rest = &haystack[idx + prefix.len()..];
    let end = rest.find(['\\', '\n', '\r', '"'])?;
    Some(rest[..end].trim().to_owned())
}

fn extract_authorized_plan_files(body: &str) -> Vec<(String, String)> {
    // Unescape JSON-encoded newlines if the prompt is inside JSON request body:
    let decoded = body.replace("\\n", "\n");
    let mut out = Vec::new();
    let mut rest = decoded.as_str();
    let marker = "### Authorized File: ";
    while let Some(pos) = rest.find(marker) {
        let after = &rest[pos + marker.len()..];
        let Some(line_end) = after.find('\n') else {
            break;
        };
        let raw_path = after[..line_end]
            .split(" (renamed from ")
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
        let next_block = &after[line_end + 1..];
        if let Some(id_pos) = next_block.find("- fileId: ") {
            let id_after = &next_block[id_pos + "- fileId: ".len()..];
            let id_end = id_after.find(['\n', '\r', '"']).unwrap_or(id_after.len());
            let file_id = id_after[..id_end].trim().to_owned();
            if !raw_path.is_empty() && !file_id.is_empty() {
                out.push((file_id, raw_path));
            }
        }
        rest = next_block;
    }
    out
}

fn extract_fenced_block(section: &str, heading: &str) -> String {
    let Some(pos) = section.find(heading) else {
        return String::new();
    };
    let after = &section[pos + heading.len()..];
    let Some(open) = after.find("```\n") else {
        return String::new();
    };
    let content_start = &after[open + 4..];
    let Some(close) = content_start.find("\n```") else {
        return content_start.to_owned();
    };
    content_start[..close].to_owned()
}

fn collect_json_strings(val: &serde_json::Value, out: &mut Vec<String>) {
    match val {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_json_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_json_strings(v, out);
            }
        }
        _ => {}
    }
}

fn decode_prompt_text(raw_body: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_body) {
        let mut parts = Vec::new();
        collect_json_strings(&val, &mut parts);
        parts.join("\n")
    } else {
        raw_body.replace("\\n", "\n")
    }
}

fn build_conflict_smoke_resolution(raw_body: &str) -> String {
    let decoded = decode_prompt_text(raw_body);
    let instruction = decoded
        .split("User merge instruction (MUST follow):\n")
        .nth(1)
        .and_then(|s| s.split("\n\nAuthorized conflicted files").next())
        .unwrap_or("")
        .trim()
        .to_owned();

    let mut resolved = Vec::new();
    let marker = "### Authorized Conflicted File: ";
    let mut rest = decoded.as_str();
    while let Some(pos) = rest.find(marker) {
        let after = &rest[pos + marker.len()..];
        let next_pos = after.find(marker);
        let section = match next_pos {
            Some(idx) => &after[..idx],
            None => after,
        };
        let path = section.lines().next().unwrap_or("").trim().to_owned();
        let file_id = section
            .lines()
            .find_map(|line| line.strip_prefix("- fileId: "))
            .unwrap_or("")
            .trim()
            .to_owned();
        let upstream = extract_fenced_block(section, "#### Remote Upstream Version (onto):");
        let local = extract_fenced_block(section, "#### Local Commit Version (being rebased):");

        let content = if instruction.contains("simulate leftover conflict markers") {
            format!("<<<<<<< HEAD\n{upstream}\n=======\n{local}\n>>>>>>> local\n")
        } else if instruction.contains("keep upstream only") {
            format!("{}\n", upstream.trim_end())
        } else if instruction.contains("keep local only") {
            format!("{}\n", local.trim_end())
        } else {
            let mut merged_lines: Vec<String> = Vec::new();
            for line in upstream.lines().chain(local.lines()) {
                let s = line.to_owned();
                if !merged_lines.contains(&s) {
                    merged_lines.push(s);
                }
            }
            if !instruction.is_empty() && instruction.contains("append merge note comment") {
                merged_lines.push(format!("// merged: {instruction}"));
            }
            format!("{}\n", merged_lines.join("\n"))
        };
        if !file_id.is_empty() && !path.is_empty() {
            resolved.push(serde_json::json!({
                "fileId": file_id,
                "path": path,
                "content": content,
            }));
        }
        rest = match next_pos {
            Some(idx) => &after[idx..],
            None => "",
        };
    }

    serde_json::json!({
        "summary": if instruction.is_empty() {
            "Merged remote upstream changes while preserving local edits"
        } else {
            "Resolved merge conflicts following your instructions"
        },
        "resolvedFiles": resolved,
    })
    .to_string()
}

fn build_cpush_smoke_plan(raw_body: &str) -> String {
    let decoded = decode_prompt_text(raw_body);
    let files = extract_authorized_plan_files(&decoded);
    let doc_file = files
        .iter()
        .find(|(_, path)| path.ends_with(".md") || path.contains("RELEASE_NOTES"));
    let code_files: Vec<String> = files
        .iter()
        .filter(|(id, _)| Some(id) != doc_file.map(|(d, _)| d))
        .map(|(id, _)| id.clone())
        .collect();
    let has_qa = decoded.contains("Previous Q&A decisions in this C-Push session:");
    let extra_instruction = decoded
        .split("User extra instructions (supplemental grouping/message guidance ONLY")
        .nth(1)
        .and_then(|s| s.split_once(":\n").map(|(_, rest)| rest))
        .map(|s| s.split("\n\nUnselected worktree files").next().unwrap_or(s))
        .map(|s| s.split("\n\nPrevious Q&A decisions").next().unwrap_or(s))
        .map(|s| s.split("\n\nAuthorized selected files").next().unwrap_or(s))
        .unwrap_or("")
        .to_ascii_lowercase();

    if !has_qa
        && (extra_instruction.contains("include unselected")
            || extra_instruction.contains("force-push")
            || extra_instruction.contains("force push")
            || extra_instruction.contains("edit code")
            || extra_instruction.contains("rewrite published"))
    {
        let affected: Vec<String> = files.iter().take(1).map(|(id, _)| id.clone()).collect();
        return serde_json::json!({
            "kind": "question",
            "title": "Extra instruction exceeds authorized C-Push permissions",
            "scope": "Authorization boundary check",
            "description": "Extra instructions cannot include unselected files, force-push, edit code, or rewrite published history. Choose how to proceed:",
            "affectedFileIds": affected,
            "options": [
                { "id": "proceed-authorized", "label": "Proceed strictly within selected files", "recommended": true },
                { "id": "dismiss-session", "label": "Stop and adjust selection" }
            ]
        })
        .to_string();
    }

    // If ambiguous doc + code files are selected and user has not answered yet -> ask!
    if let Some((doc_id, _)) = doc_file
        && !code_files.is_empty()
        && !has_qa
    {
        return serde_json::json!({
            "kind": "question",
            "title": "Include release notes in this commit?",
            "scope": "Affects commit 2/2 (docs)",
            "description": "Code changes and documentation are both selected. Confirm how to group the release notes:",
            "affectedFileIds": [doc_id],
            "options": [
                { "id": "merge", "label": "Fold into commit 1" },
                { "id": "split", "label": "Commit separately and push together", "recommended": true },
                { "id": "skip", "label": "Skip for now" }
            ]
        })
        .to_string();
    }

    if has_qa {
        let qa_section = decoded
            .split("Previous Q&A decisions in this C-Push session:")
            .nth(1)
            .and_then(|s| s.split("Authorized selected files").next())
            .unwrap_or("");
        let last_answer = qa_section
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with("A:"))
            .unwrap_or("");
        let lower_answer = last_answer.to_ascii_lowercase();

        // Out-of-scope or ambiguous free-text keeps asking (`C02`):
        if lower_answer.contains("unselected")
            || lower_answer.contains("out-of-scope")
            || lower_answer.contains("not sure maybe")
        {
            let affected: Vec<String> = doc_file
                .map(|(id, _)| vec![id.clone()])
                .unwrap_or_else(|| files.iter().take(1).map(|(id, _)| id.clone()).collect());
            return serde_json::json!({
                "kind": "question",
                "title": "Instruction exceeds selected scope or is ambiguous",
                "scope": "Reconfirm file attribution",
                "description": "C-Push only processes files selected in this run and cannot automatically include unselected files. Choose how to proceed within the authorized scope:",
                "affectedFileIds": affected,
                "options": [
                    { "id": "merge", "label": "Fold into commit 1" },
                    { "id": "split", "label": "Commit separately and push together", "recommended": true },
                    { "id": "skip", "label": "Skip for now" }
                ]
            })
            .to_string();
        }

        let sensitive_ids: Vec<String> = files
            .iter()
            .filter(|(_, path)| crate::repository::ai_context::is_sensitive_local_file(path))
            .map(|(id, _)| id.clone())
            .collect();
        if !sensitive_ids.is_empty()
            && (last_answer.contains("optionId=skip")
                || lower_answer.contains("exclude")
                || lower_answer.contains("skip"))
        {
            let keep_ids: Vec<String> = files
                .iter()
                .filter(|(id, _)| !sensitive_ids.contains(id))
                .map(|(id, _)| id.clone())
                .collect();
            let groups = if keep_ids.is_empty() {
                Vec::new()
            } else {
                vec![serde_json::json!({
                    "id": "group-1",
                    "fileIds": keep_ids,
                    "message": "feat(scope): commit confirmed application changes"
                })]
            };
            return serde_json::json!({
                "kind": "ready",
                "summary": "Excluded local sensitive config as confirmed and committed remaining changes",
                "groups": groups,
                "skippedFileIds": sensitive_ids
            })
            .to_string();
        }

        if let Some((doc_id, _)) = doc_file
            && !code_files.is_empty()
        {
            if last_answer.contains("optionId=skip") || lower_answer.contains("skip") {
                return serde_json::json!({
                    "kind": "ready",
                    "summary": "Skipped documentation as confirmed and committed code changes",
                    "groups": [{
                        "id": "group-1",
                        "fileIds": code_files,
                        "message": "feat(scope): update staged config and core logic\n\n- commit staged configuration changes\n- keep release notes in the working tree as confirmed"
                    }],
                    "skippedFileIds": [doc_id]
                })
                .to_string();
            }
            if last_answer.contains("optionId=merge") || last_answer.contains("Fold into commit 1")
            {
                let all_ids: Vec<String> = files.iter().map(|(id, _)| id.clone()).collect();
                return serde_json::json!({
                    "kind": "ready",
                    "summary": "Merged into a single commit as confirmed",
                    "groups": [{
                        "id": "group-1",
                        "fileIds": all_ids,
                        "message": "feat(scope): update staged config and release notes\n\n- commit staged changes and related code\n- update release notes documentation"
                    }],
                    "skippedFileIds": []
                })
                .to_string();
            }
            return serde_json::json!({
                "kind": "ready",
                "summary": "Split into two commits for code and documentation",
                "groups": [
                    {
                        "id": "group-1",
                        "fileIds": code_files,
                        "message": "feat(scope): update staged config and core logic\n\n- commit staged configuration changes\n- include related added and modified files"
                    },
                    {
                        "id": "group-2",
                        "fileIds": [doc_id],
                        "message": "docs: update release notes\n\n- document configuration and feature updates"
                    }
                ],
                "skippedFileIds": []
            })
            .to_string();
        }
    }

    // Clear scope (`C01`): no question needed, split >1 logical categories or single commit directly!
    let all_ids: Vec<String> = files.iter().map(|(id, _)| id.clone()).collect();
    serde_json::json!({
        "kind": "ready",
        "summary": "Scope is unambiguous; generated commit plan directly",
        "groups": [{
            "id": "group-1",
            "fileIds": all_ids,
            "message": "feat(scope): commit selected scope changes\n\n- write local commit for the exact selected scope\n- preserve unselected files and unstaged drafts"
        }],
        "skippedFileIds": []
    })
    .to_string()
}

#[derive(Default)]
pub struct ModelSmokeState(pub Mutex<Option<LocalModelSseServer>>);

impl ModelSmokeState {
    pub fn ensure_server(&self) -> Result<String, String> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| "Model smoke server lock is unavailable")?;
        if guard.is_none() {
            *guard = Some(LocalModelSseServer::start()?);
        }
        Ok(format!("{}/v1", guard.as_ref().unwrap().base_url))
    }

    pub fn requests(&self) -> Result<Vec<RecordedModelHttpRequest>, String> {
        let guard = self
            .0
            .lock()
            .map_err(|_| "Model smoke server lock is unavailable")?;
        Ok(guard
            .as_ref()
            .map_or_else(Vec::new, LocalModelSseServer::requests))
    }

    pub fn cleanup(&self) {
        if let Ok(mut guard) = self.0.lock() {
            let _ = guard.take();
        }
    }
}
