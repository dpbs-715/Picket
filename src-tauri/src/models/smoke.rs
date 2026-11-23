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

    let response_text = if body.contains("authorized Git conflict merge resolver") {
        build_conflict_smoke_resolution(&body)
    } else if body.contains("C-Push commit splitting planner") {
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
                "title": "需确认已暂存配置变更边界",
                "reason": "检测到关键键值调整，提交前请确认上游调用方兼容。"
            }]
        })
        .to_string()
    } else if body.contains("commit message generator") {
        "feat(scope): 更新已暂存配置项并同步所选范围\n\n- 提交已暂存的配置键值调整\n- 同步记录所选锁文件范围变更".to_owned()
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

        let content = if instruction.contains("模拟残留冲突标记") {
            format!("<<<<<<< HEAD\n{upstream}\n=======\n{local}\n>>>>>>> local\n")
        } else if instruction.contains("仅保留远端") {
            format!("{}\n", upstream.trim_end())
        } else if instruction.contains("仅保留本地") {
            format!("{}\n", local.trim_end())
        } else {
            let mut merged_lines: Vec<String> = Vec::new();
            for line in upstream.lines().chain(local.lines()) {
                let s = line.to_owned();
                if !merged_lines.contains(&s) {
                    merged_lines.push(s);
                }
            }
            if !instruction.is_empty() && instruction.contains("附上合并说明注释") {
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
            "已合入远端改动并保留本地修改"
        } else {
            "已按你的合并要求解决冲突"
        },
        "resolvedFiles": resolved,
    })
    .to_string()
}

fn build_cpush_smoke_plan(raw_body: &str) -> String {
    let decoded = raw_body.replace("\\n", "\n");
    let files = extract_authorized_plan_files(raw_body);
    let doc_file = files
        .iter()
        .find(|(_, path)| path.ends_with(".md") || path.contains("RELEASE_NOTES"));
    let code_files: Vec<String> = files
        .iter()
        .filter(|(id, _)| Some(id) != doc_file.map(|(d, _)| d))
        .map(|(id, _)| id.clone())
        .collect();
    let has_qa = decoded.contains("Previous Q&A decisions in this C-Push session:");

    // If ambiguous doc + code files are selected and user has not answered yet -> ask!
    if let Some((doc_id, _)) = doc_file
        && !code_files.is_empty()
        && !has_qa
    {
        return serde_json::json!({
            "kind": "question",
            "title": "发布说明要一起提交吗？",
            "scope": "影响提交 2/2「docs」",
            "description": "检测到代码改动与文档同时勾选，请确认发布说明的归属方式：",
            "affectedFileIds": [doc_id],
            "options": [
                { "id": "merge", "label": "合并进提交 1" },
                { "id": "split", "label": "单独提交并一起推送", "recommended": true },
                { "id": "skip", "label": "暂不提交" }
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

        // Out-of-scope or ambiguous free-text keeps asking (`C02`):
        if last_answer.contains("未选")
            || last_answer.contains("越界")
            || last_answer.contains("随便看看不确定")
        {
            let affected: Vec<String> = doc_file
                .map(|(id, _)| vec![id.clone()])
                .unwrap_or_else(|| files.iter().take(1).map(|(id, _)| id.clone()).collect());
            return serde_json::json!({
                "kind": "question",
                "title": "该要求超出当前已勾选范围或含义不明",
                "scope": "需重新确认归属",
                "description": "C-Push 仅处理本次已勾选的变更文件，不能自动纳入未选文件。请在已授权范围内选择处理方式：",
                "affectedFileIds": affected,
                "options": [
                    { "id": "merge", "label": "合并进提交 1" },
                    { "id": "split", "label": "单独提交并一起推送", "recommended": true },
                    { "id": "skip", "label": "暂不提交" }
                ]
            })
            .to_string();
        }

        if let Some((doc_id, _)) = doc_file
            && !code_files.is_empty()
        {
            if last_answer.contains("optionId=skip") || last_answer.contains("暂不提交") {
                return serde_json::json!({
                    "kind": "ready",
                    "summary": "已按确认跳过文档，仅提交代码改动",
                    "groups": [{
                        "id": "group-1",
                        "fileIds": code_files,
                        "message": "feat(scope): 更新已暂存配置与核心实现\n\n- 沿用已暂存范围提交业务改动\n- 按确认保留发布说明在工作区"
                    }],
                    "skippedFileIds": [doc_id]
                })
                .to_string();
            }
            if last_answer.contains("optionId=merge") || last_answer.contains("合并进提交 1") {
                let all_ids: Vec<String> = files.iter().map(|(id, _)| id.clone()).collect();
                return serde_json::json!({
                    "kind": "ready",
                    "summary": "已按确认合并为单笔提交",
                    "groups": [{
                        "id": "group-1",
                        "fileIds": all_ids,
                        "message": "feat(scope): 更新已暂存配置并同步发布说明\n\n- 提交已暂存改动与关联代码\n- 同步更新发布说明文档"
                    }],
                    "skippedFileIds": []
                })
                .to_string();
            }
            return serde_json::json!({
                "kind": "ready",
                "summary": "已拆分为业务改动与文档两笔独立提交",
                "groups": [
                    {
                        "id": "group-1",
                        "fileIds": code_files,
                        "message": "feat(scope): 更新已暂存配置与核心实现\n\n- 沿用已暂存范围提交配置改动\n- 同步关联新增与修改文件"
                    },
                    {
                        "id": "group-2",
                        "fileIds": [doc_id],
                        "message": "docs: 更新发布说明文档\n\n- 记录本次配置与业务改动说明"
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
        "summary": "范围明确，直接生成提交计划",
        "groups": [{
            "id": "group-1",
            "fileIds": all_ids,
            "message": "feat(scope): 提交已选范围改动\n\n- 按已选精确范围写入本地提交\n- 保全未选文件与未暂存草稿"
        }],
        "skippedFileIds": []
    })
    .to_string()
}

#[derive(Default)]
pub struct ModelSmokeState(pub Mutex<Option<LocalModelSseServer>>);

impl ModelSmokeState {
    pub fn ensure_server(&self) -> Result<String, String> {
        let mut guard = self.0.lock().map_err(|_| "模型测试服务锁不可用")?;
        if guard.is_none() {
            *guard = Some(LocalModelSseServer::start()?);
        }
        Ok(format!("{}/v1", guard.as_ref().unwrap().base_url))
    }

    pub fn requests(&self) -> Result<Vec<RecordedModelHttpRequest>, String> {
        let guard = self.0.lock().map_err(|_| "模型测试服务锁不可用")?;
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
