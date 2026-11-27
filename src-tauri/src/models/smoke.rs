//! Local loopback HTTP SSE fixture server used only in `#[cfg(test)]` and explicit
//! `--foundation-ui-smoke` runs. Speaks real OpenAI Completions, OpenAI Responses,
//! and Anthropic Messages SSE frames (both text and tool calls) so the bundled Pi Core
//! sidecar (`@anthropic-ai/sdk` and `openai` SDK) executes full HTTP + SSE round-trips
//! without external network keys.
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
    #[cfg_attr(not(feature = "smoke-harness"), allow(dead_code))]
    pub x_api_key: Option<String>,
    pub body: String,
}

enum MockSseReply {
    Text(String),
    ToolCalls(Vec<(String, String, serde_json::Value)>),
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

    let reply = if body.contains("authorized Git conflict merge resolver") {
        build_conflict_smoke_reply(&body)
    } else if body.contains("Picket's C-Push Agent") {
        build_cpush_smoke_reply(&body)
    } else {
        MockSseReply::Text("OK".to_owned())
    };

    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;

    match reply {
        MockSseReply::Text(response_text) => {
            write_text_sse(&mut stream, &path, &response_text, slow_delay_ms, &stop)?;
        }
        MockSseReply::ToolCalls(calls) => {
            write_tool_calls_sse(&mut stream, &path, &calls, slow_delay_ms, &stop)?;
        }
    }
    stream.flush()?;
    Ok(())
}

fn write_text_sse(
    stream: &mut TcpStream,
    path: &str,
    response_text: &str,
    slow_delay_ms: u64,
    stop: &AtomicBool,
) -> std::io::Result<()> {
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
        wait_slow(slow_delay_ms, stop);
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
            stream,
            serde_json::json!({"type":"response.created","response":{"id":"resp_1","status":"in_progress","output":[]}}),
        )?;
        send(
            stream,
            serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"id":"m1","type":"message","role":"assistant","status":"in_progress","content":[]}}),
        )?;
        send(
            stream,
            serde_json::json!({"type":"response.content_part.added","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        )?;
        send(
            stream,
            serde_json::json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":response_text}),
        )?;
        stream.flush()?;
        wait_slow(slow_delay_ms, stop);
        send(
            stream,
            serde_json::json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":response_text}),
        )?;
        send(
            stream,
            serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"id":"m1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":response_text,"annotations":[]}]}}),
        )?;
        send(
            stream,
            serde_json::json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[{"id":"m1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":response_text,"annotations":[]}]}],"usage":{"input_tokens":8,"output_tokens":8,"total_tokens":16}}}),
        )?;
    } else {
        let chunk1 = serde_json::json!({
            "id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1700000000, "model": "test",
            "choices": [{ "index": 0, "delta": { "content": response_text }, "finish_reason": null }]
        });
        write!(stream, "data: {chunk1}\n\n")?;
        stream.flush()?;
        wait_slow(slow_delay_ms, stop);
        let chunk2 = serde_json::json!({
            "id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1700000000, "model": "test",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20 }
        });
        write!(stream, "data: {chunk2}\n\ndata: [DONE]\n\n")?;
    }
    Ok(())
}

fn write_tool_calls_sse(
    stream: &mut TcpStream,
    path: &str,
    calls: &[(String, String, serde_json::Value)],
    slow_delay_ms: u64,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    if path.ends_with("/messages") {
        let start = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_tc", "type": "message", "role": "assistant", "model": "claude-sonnet-4-20250514",
                "content": [], "stop_reason": null, "stop_sequence": null,
                "usage": { "input_tokens": 10, "output_tokens": 1 }
            }
        });
        write!(stream, "event: message_start\ndata: {start}\n\n")?;
        for (idx, (id, name, args)) in calls.iter().enumerate() {
            write!(
                stream,
                "event: content_block_start\ndata: {}\n\n",
                serde_json::json!({"type":"content_block_start","index":idx,"content_block":{"type":"tool_use","id":id,"name":name,"input":{}}})
            )?;
            write!(
                stream,
                "event: content_block_delta\ndata: {}\n\n",
                serde_json::json!({"type":"content_block_delta","index":idx,"delta":{"type":"input_json_delta","partial_json":args.to_string()}})
            )?;
            write!(
                stream,
                "event: content_block_stop\ndata: {}\n\n",
                serde_json::json!({"type":"content_block_stop","index":idx})
            )?;
        }
        stream.flush()?;
        wait_slow(slow_delay_ms, stop);
        write!(
            stream,
            "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
            serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":24}}),
            serde_json::json!({"type":"message_stop"})
        )?;
    } else if path.ends_with("/responses") {
        let send = |s: &mut TcpStream, v: serde_json::Value| -> std::io::Result<()> {
            write!(s, "data: {v}\n\n")
        };
        send(
            stream,
            serde_json::json!({"type":"response.created","response":{"id":"resp_tc","status":"in_progress","output":[]}}),
        )?;
        let mut completed_output = Vec::new();
        for (idx, (id, name, args)) in calls.iter().enumerate() {
            let args_str = args.to_string();
            let item_id = format!("fc_{idx}");
            send(
                stream,
                serde_json::json!({"type":"response.output_item.added","output_index":idx,"item":{"id":item_id,"type":"function_call","call_id":id,"name":name,"arguments":"","status":"in_progress"}}),
            )?;
            send(
                stream,
                serde_json::json!({"type":"response.function_call_arguments.delta","output_index":idx,"delta":args_str}),
            )?;
            send(
                stream,
                serde_json::json!({"type":"response.function_call_arguments.done","output_index":idx,"arguments":args_str}),
            )?;
            let done_item = serde_json::json!({"id":item_id,"type":"function_call","call_id":id,"name":name,"arguments":args_str,"status":"completed"});
            send(
                stream,
                serde_json::json!({"type":"response.output_item.done","output_index":idx,"item":done_item}),
            )?;
            completed_output.push(done_item);
        }
        stream.flush()?;
        wait_slow(slow_delay_ms, stop);
        send(
            stream,
            serde_json::json!({"type":"response.completed","response":{"id":"resp_tc","status":"completed","output":completed_output,"usage":{"input_tokens":10,"output_tokens":16,"total_tokens":26}}}),
        )?;
    } else {
        let tool_calls: Vec<serde_json::Value> = calls
            .iter()
            .enumerate()
            .map(|(idx, (id, name, args))| {
                serde_json::json!({
                    "index": idx,
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": args.to_string() }
                })
            })
            .collect();
        let chunk1 = serde_json::json!({
            "id": "chatcmpl-tc", "object": "chat.completion.chunk", "created": 1700000000, "model": "test",
            "choices": [{ "index": 0, "delta": { "role": "assistant", "tool_calls": tool_calls }, "finish_reason": null }]
        });
        write!(stream, "data: {chunk1}\n\n")?;
        stream.flush()?;
        wait_slow(slow_delay_ms, stop);
        let chunk2 = serde_json::json!({
            "id": "chatcmpl-tc", "object": "chat.completion.chunk", "created": 1700000000, "model": "test",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 18, "total_tokens": 30 }
        });
        write!(stream, "data: {chunk2}\n\ndata: [DONE]\n\n")?;
    }
    Ok(())
}

fn wait_slow(ms: u64, stop: &AtomicBool) {
    let mut waited = 0u64;
    while waited < ms && !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(20));
        waited += 20;
    }
}

fn extract_authorized_plan_files(body: &str) -> Vec<(String, String)> {
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

fn extract_conflict_summary_paths(decoded: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(section) = decoded.split("Authorized conflicted files (").nth(1) else {
        return out;
    };
    for line in section.lines().skip(1) {
        let trimmed = line.trim();
        if !trimmed.starts_with("- ") {
            if !out.is_empty() {
                break;
            }
            continue;
        }
        if let Some(after_tick) = trimmed.split('`').nth(1)
            && !after_tick.is_empty()
        {
            out.push(after_tick.to_owned());
        }
    }
    out
}

fn extract_read_conflict_section(decoded: &str, path: &str, section: &str) -> Option<String> {
    let header = format!("Conflicted file `{path}` [{section}]");
    let pos = decoded.rfind(&header)?;
    let after = &decoded[pos + header.len()..];
    let first_nl = after.find('\n')?;
    let body = &after[first_nl + 1..];
    let mut lines = Vec::new();
    for line in body.lines() {
        if line.starts_with("Conflicted file `") || line.starts_with("... (") {
            break;
        }
        if let Some((num, rest)) = line.split_once(": ")
            && num.chars().all(|c| c.is_ascii_digit())
            && !num.is_empty()
        {
            lines.push(rest.to_owned());
        } else if !lines.is_empty() {
            break;
        }
    }
    Some(lines.join("\n"))
}

fn build_conflict_smoke_reply(raw_body: &str) -> MockSseReply {
    let decoded = decode_prompt_text(raw_body);
    let instruction = decoded
        .split("User merge instruction (MUST follow):\n")
        .nth(1)
        .and_then(|s| s.split("\n\nAuthorized conflicted files").next())
        .unwrap_or("")
        .trim()
        .to_owned();
    let paths = extract_conflict_summary_paths(&decoded);

    // If tool already rejected leftover markers once and instruction specifically asked to simulate leftover markers,
    // stop retrying so Rust cleanly surfaces the failure instead of looping forever.
    if instruction.contains("simulate leftover conflict markers")
        && decoded.contains("still contains conflict markers")
    {
        return MockSseReply::Text("Unable to resolve conflict cleanly.".into());
    }

    // Check if we already read `upstream` and `local` sections for all files:
    let all_read = !paths.is_empty()
        && paths.iter().all(|p| {
            extract_read_conflict_section(&decoded, p, "upstream").is_some()
                && extract_read_conflict_section(&decoded, p, "local").is_some()
        });

    if !all_read {
        let mut calls = Vec::new();
        for (idx, p) in paths.iter().enumerate() {
            calls.push((
                format!("rc-up-{idx}"),
                "read_conflict_file".into(),
                serde_json::json!({ "path": p, "section": "upstream" }),
            ));
            calls.push((
                format!("rc-loc-{idx}"),
                "read_conflict_file".into(),
                serde_json::json!({ "path": p, "section": "local" }),
            ));
        }
        return MockSseReply::ToolCalls(calls);
    }

    let summary = if instruction.is_empty() {
        "Merged remote upstream changes while preserving local edits"
    } else {
        "Resolved merge conflicts following your instructions"
    };
    let mut write_calls = Vec::new();
    for (idx, p) in paths.iter().enumerate() {
        let upstream = extract_read_conflict_section(&decoded, p, "upstream").unwrap_or_default();
        let local = extract_read_conflict_section(&decoded, p, "local").unwrap_or_default();
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
        write_calls.push((
            format!("wc-{idx}"),
            "write_conflict_resolution".into(),
            serde_json::json!({
                "path": p,
                "content": content,
                "summary": summary,
            }),
        ));
    }
    MockSseReply::ToolCalls(write_calls)
}

fn extract_latest_message_user_answer(decoded: &str) -> Option<String> {
    let marker = "User responses:\n";
    let pos = decoded.rfind(marker)?;
    let after = &decoded[pos + marker.len()..];
    let answer = after
        .lines()
        .take_while(|line| line.starts_with("- "))
        .collect::<Vec<_>>()
        .join("\n");
    (!answer.is_empty()).then_some(answer)
}

fn build_cpush_smoke_reply(raw_body: &str) -> MockSseReply {
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
    let latest_answer = extract_latest_message_user_answer(&decoded);
    let has_qa = latest_answer.is_some();

    let extra_instruction = decoded
        .split("User extra instructions (supplemental grouping/message/push guidance ONLY")
        .nth(1)
        .and_then(|s| s.split_once(":\n").map(|(_, rest)| rest))
        .map(|s| s.split("\n\nUnselected worktree files").next().unwrap_or(s))
        .map(|s| s.split("\n\nLocal Keep paths").next().unwrap_or(s))
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
        return MockSseReply::ToolCalls(vec![(
            "tc-q-boundary".into(),
            "message_user".into(),
            serde_json::json!({
                "title": "Extra instruction exceeds authorized C-Push permissions",
                "subtitle": "Authorization boundary check",
                "tone": "warning",
                "questions": [{
                    "id": "boundary",
                    "header": "Scope boundary",
                    "question": "Extra instructions cannot include unselected files, force-push, edit code, or rewrite published history. Choose how to proceed:",
                    "options": [
                        { "id": "proceed-authorized", "label": "Proceed strictly within selected files", "description": "Commit only currently selected files", "recommended": true },
                        { "id": "dismiss-session", "label": "Stop and adjust selection", "description": "Cancel so you can update selection" }
                    ]
                }],
                "context": {
                    "scope": "Authorization boundary check",
                    "affectedFileIds": affected
                }
            }),
        )]);
    }

    // If ambiguous doc + code files are selected and user has not answered yet -> ask via message_user!
    if let Some((doc_id, _)) = doc_file
        && !code_files.is_empty()
        && !has_qa
    {
        return MockSseReply::ToolCalls(vec![(
            "tc-q-doc".into(),
            "message_user".into(),
            serde_json::json!({
                "title": "Include release notes in this commit?",
                "subtitle": "Affects commit 2/2 (docs)",
                "tone": "info",
                "questions": [{
                    "id": "doc-grouping",
                    "header": "Release notes",
                    "question": "Code changes and documentation are both selected. Confirm how to group the release notes:",
                    "options": [
                        { "id": "merge", "label": "Fold into commit 1", "description": "Include RELEASE_NOTES.md inside the primary code commit" },
                        { "id": "split", "label": "Commit separately and push together", "description": "Create 2 clean commits (code + docs) and push together", "recommended": true },
                        { "id": "skip", "label": "Skip for now", "description": "Leave RELEASE_NOTES.md uncommitted in the working tree" }
                    ]
                }],
                "context": {
                    "scope": "Affects commit 2/2 (docs)",
                    "affectedFileIds": [doc_id]
                }
            }),
        )]);
    }

    if let Some(ref answer) = latest_answer {
        let lower_answer = answer.to_ascii_lowercase();

        // Out-of-scope or ambiguous free-text keeps asking:
        if lower_answer.contains("unselected")
            || lower_answer.contains("out-of-scope")
            || lower_answer.contains("not sure maybe")
        {
            let affected: Vec<String> = doc_file
                .map(|(id, _)| vec![id.clone()])
                .unwrap_or_else(|| files.iter().take(1).map(|(id, _)| id.clone()).collect());
            return MockSseReply::ToolCalls(vec![(
                "tc-q-reconfirm".into(),
                "message_user".into(),
                serde_json::json!({
                    "title": "Instruction exceeds selected scope or is ambiguous",
                    "subtitle": "Reconfirm file attribution",
                    "tone": "warning",
                    "questions": [{
                        "id": "reconfirm",
                        "header": "Reconfirm scope",
                        "question": "C-Push only processes files selected in this run and cannot automatically include unselected files. Choose how to proceed within the authorized scope:",
                        "options": [
                            { "id": "merge", "label": "Fold into commit 1", "description": "Include all selected files in one commit" },
                            { "id": "split", "label": "Commit separately and push together", "description": "Split code and documentation into two commits", "recommended": true },
                            { "id": "skip", "label": "Skip for now", "description": "Exclude release notes and commit code only" }
                        ]
                    }],
                    "context": {
                        "scope": "Reconfirm file attribution",
                        "affectedFileIds": affected
                    }
                }),
            )]);
        }

        if let Some((doc_id, _)) = doc_file
            && !code_files.is_empty()
        {
            if lower_answer.contains("skip") {
                return MockSseReply::ToolCalls(vec![(
                    "tc-commit-skip-doc".into(),
                    "commit_exact_scope".into(),
                    serde_json::json!({
                        "summary": "Skipped documentation as confirmed and committed code changes",
                        "groups": [{
                            "id": "group-1",
                            "fileIds": code_files,
                            "message": "feat(scope): update staged config and core logic\n\n- commit staged configuration changes\n- keep release notes in the working tree as confirmed"
                        }],
                        "skippedFileIds": [doc_id],
                        "push": true
                    }),
                )]);
            }
            if lower_answer.contains("fold into commit 1")
                || lower_answer.contains("(id=merge)")
                || lower_answer.contains("\"merge\"")
            {
                let all_ids: Vec<String> = files.iter().map(|(id, _)| id.clone()).collect();
                return MockSseReply::ToolCalls(vec![(
                    "tc-commit-merge".into(),
                    "commit_exact_scope".into(),
                    serde_json::json!({
                        "summary": "Merged into a single commit as confirmed",
                        "groups": [{
                            "id": "group-1",
                            "fileIds": all_ids,
                            "message": "feat(scope): update staged config and release notes\n\n- commit staged changes and related code\n- update release notes documentation"
                        }],
                        "skippedFileIds": [],
                        "push": true
                    }),
                )]);
            }
            return MockSseReply::ToolCalls(vec![(
                "tc-commit-split".into(),
                "commit_exact_scope".into(),
                serde_json::json!({
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
                    "skippedFileIds": [],
                    "push": true
                }),
            )]);
        }
    }

    // Clear scope fast path (1 turn):
    let all_ids: Vec<String> = files.iter().map(|(id, _)| id.clone()).collect();
    let skip_push = extra_instruction.contains("without pushing")
        || extra_instruction.contains("skip push")
        || extra_instruction.contains("local only")
        || extra_instruction.contains("只提交不推送");
    MockSseReply::ToolCalls(vec![(
        "tc-commit-fast".into(),
        "commit_exact_scope".into(),
        serde_json::json!({
            "summary": if skip_push {
                "Scope is unambiguous; committed locally without pushing"
            } else {
                "Scope is unambiguous; committed and pushed directly"
            },
            "groups": [{
                "id": "group-1",
                "fileIds": all_ids,
                "message": "feat(scope): commit selected scope changes\n\n- write local commit for the exact selected scope\n- preserve unselected files and unstaged drafts"
            }],
            "skippedFileIds": [],
            "push": !skip_push
        }),
    )])
}

#[cfg(feature = "smoke-harness")]
#[derive(Default)]
pub struct ModelSmokeState(pub Mutex<Option<LocalModelSseServer>>);

#[cfg(feature = "smoke-harness")]
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
