//! Read-only Git scope verification and unified diff extraction for M3 AI tasks
//! (`generate_commit_message` and `review_selected_scope`).
//! Holds the repository gate only while reading a consistent Git snapshot; releases it
//! before any external model request starts.
use super::*;
use serde::Serialize;

const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const MAX_FILE_PATCH_BYTES: usize = 48 * 1024;
const MAX_TOTAL_PATCH_BYTES: usize = 192 * 1024;
const GENERATE_MAX_FILE_PATCH_BYTES: usize = 12 * 1024;
const GENERATE_MAX_TOTAL_PATCH_BYTES: usize = 48 * 1024;
const MIN_PARTIAL_PATCH_BUDGET: usize = 2 * 1024;
const CPUSH_MAX_FILE_PATCH_BYTES: usize = 8 * 1024;
const CPUSH_MAX_TOTAL_PATCH_BYTES: usize = 36 * 1024;
const CPUSH_MIN_PARTIAL_PATCH_BUDGET: usize = 1536;
const MAX_UNTRACKED_READ_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiScopeMode {
    Generate,
    Review,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedDiffFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub patch: String,
}

#[derive(Clone, Debug)]
pub struct ExtractedAiScope {
    pub branch: String,
    pub recent_commits: Vec<String>,
    pub files: Vec<ExtractedDiffFile>,
    pub skipped_paths: Vec<String>,
    pub reviewed_file_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenCPushFile {
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
    pub content_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExtractedCPushScope {
    pub branch: String,
    pub initial_head: Option<String>,
    pub recent_commits: Vec<String>,
    pub unselected_paths: Vec<String>,
    pub files: Vec<FrozenCPushFile>,
}

fn format_untracked_patch(path: &str, content: &str) -> String {
    let normalized = content.strip_suffix('\n').unwrap_or(content);
    if normalized.is_empty() {
        return format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +0,0 @@\n");
    }
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut out = format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub fn is_sensitive_local_file(path: &str) -> bool {
    let Some(raw_name) = Path::new(path).file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = raw_name.to_ascii_lowercase();
    if lower == ".env" {
        return true;
    }
    if let Some(suffix) = lower.strip_prefix(".env.") {
        return !matches!(
            suffix,
            "example" | "sample" | "template" | "dist" | "schema" | "defaults"
        ) && !suffix.ends_with(".example")
            && !suffix.ends_with(".sample")
            && !suffix.ends_with(".template");
    }
    if matches!(
        lower.as_str(),
        ".npmrc"
            | ".pypirc"
            | ".netrc"
            | "id_rsa"
            | "id_ed25519"
            | "id_ecdsa"
            | "id_dsa"
            | "credentials.json"
            | "client_secret.json"
    ) {
        return true;
    }
    matches!(
        Path::new(&lower).extension().and_then(|e| e.to_str()),
        Some("pem" | "key" | "p12" | "pfx" | "keystore" | "jks")
    )
}

fn redact_sensitive_patch(path: &str, raw_patch: &str) -> Option<String> {
    let lower = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !(lower == ".env" || lower.starts_with(".env.") || lower == ".npmrc") {
        return None;
    }
    let mut out = String::with_capacity(raw_patch.len());
    for line in raw_patch.lines() {
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("@@ ")
            || line.starts_with("new file mode")
            || line.starts_with("deleted file mode")
            || line.starts_with("\\ ")
        {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some(first) = line.chars().next()
            && matches!(first, '+' | '-' | ' ')
        {
            let body = &line[first.len_utf8()..];
            let trimmed = body.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                out.push_str(line);
            } else if let Some((key_part, val_part)) = body.split_once('=') {
                if val_part.trim().is_empty() {
                    out.push_str(line);
                } else {
                    out.push(first);
                    out.push_str(key_part);
                    out.push_str("=[REDACTED_LOCAL_VALUE]");
                }
            } else {
                out.push(first);
                out.push_str("[REDACTED_SENSITIVE_LINE]");
            }
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Some(out)
}

pub(crate) fn truncate_patch_head_tail(raw_patch: &str, max_bytes: usize) -> String {
    if raw_patch.len() <= max_bytes {
        return raw_patch.to_owned();
    }
    const MARKER_RESERVE: usize = 64;
    if max_bytes < MARKER_RESERVE + 128 {
        let cut = raw_patch.floor_char_boundary(max_bytes.saturating_sub(20));
        return format!("{}\n... [truncated]", &raw_patch[..cut]);
    }
    let usable = max_bytes - MARKER_RESERVE;
    let head_budget = (usable * 13) / 20;
    let tail_budget = usable - head_budget;

    let max_head = raw_patch.floor_char_boundary(head_budget);
    let head_end = raw_patch[..max_head]
        .rfind('\n')
        .filter(|&idx| idx > 0)
        .unwrap_or(max_head);

    let min_tail = raw_patch.ceil_char_boundary(raw_patch.len().saturating_sub(tail_budget));
    let tail_search_start = min_tail.max(head_end);
    let tail_start = raw_patch[tail_search_start..]
        .find('\n')
        .map(|offset| tail_search_start + offset + 1)
        .filter(|&idx| idx < raw_patch.len())
        .unwrap_or(tail_search_start);

    let omitted_lines = raw_patch[head_end..tail_start].lines().count().max(1);
    let head = raw_patch[..head_end].trim_end_matches('\n');
    let tail = raw_patch[tail_start..].trim_start_matches('\n');
    if tail.is_empty() {
        format!("{head}\n... [省略后续 {omitted_lines} 行改动] ...")
    } else {
        format!("{head}\n... [省略中间 {omitted_lines} 行改动] ...\n{tail}")
    }
}

impl RepositoryStore {
    pub async fn extract_ai_scope(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
        mode: AiScopeMode,
    ) -> Result<ExtractedAiScope> {
        if selections.is_empty() {
            return Err("请先选择至少一个变更文件；当前草稿已保留。".into());
        }
        let mut seen = HashSet::with_capacity(selections.len());
        for item in selections {
            if item.file_id.is_empty()
                || item.commit_token.is_empty()
                || !seen.insert(&item.file_id)
            {
                return Err("模型请求包含无效或重复的文件范围凭证，请刷新后重试。".into());
            }
        }

        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let (_, root) = self.registered_worktree_root(target).await?;
        let entry = WorktreeEntry {
            path: root.clone(),
            head: None,
            branch: String::new(),
            bare: false,
        };
        let (live_tree, live_commit_state) =
            self.read_worktree(&target.repository_id, &entry).await?;

        let change_by_id: HashMap<&str, &Change> = live_tree
            .files
            .iter()
            .map(|change| (change.id.as_str(), change))
            .collect();

        for item in selections {
            let Some(authorized) = live_commit_state.files.get(&item.file_id) else {
                return Err(
                    "所选文件已不在当前工作树变更列表中，请刷新确认后再执行模型任务；当前草稿与选择已保留。"
                        .into(),
                );
            };
            if authorized.conflicted {
                return Err(
                    "所选范围包含未解决的合并冲突文件，请先处理冲突后再生成说明或审查；草稿与选择已保留。"
                        .into(),
                );
            }
            if authorized.token != item.commit_token {
                return Err(
                    "所选文件范围、暂存内容或分支已变化，已暂停本次模型请求；请刷新核对最新变更，当前草稿与选择已保留。"
                        .into(),
                );
            }
        }

        let worktree_reader = self.inner.reader.for_worktree(&root).await?;
        let recent_commits = if mode == AiScopeMode::Generate && live_tree.head.is_some() {
            let output = worktree_reader
                .run(&root, &["log", "-n", "8", "--format=%s%x00", "HEAD"])
                .await?;
            if output.success {
                output
                    .bytes
                    .split(|byte| *byte == 0)
                    .filter_map(|raw| std::str::from_utf8(raw).ok())
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut files = Vec::new();
        let mut skipped_paths = Vec::new();
        let mut reviewed_file_ids = Vec::new();
        let mut total_patch_bytes = 0usize;

        for item in selections {
            let change = change_by_id[item.file_id.as_str()];
            if change.ignored_for_review {
                let note = match mode {
                    AiScopeMode::Review => format!("{}（锁文件默认忽略审查）", change.path),
                    AiScopeMode::Generate => format!(
                        "{}（锁文件，+{} -{}）",
                        change.path, change.range.added, change.range.removed
                    ),
                };
                skipped_paths.push(note);
                continue;
            }
            if change.binary {
                skipped_paths.push(format!("{}（二进制文件）", change.path));
                continue;
            }

            let raw_patch = if change.range.kind == "partial" || change.range.kind == "staged" {
                let base_tree = live_tree.head.as_deref().unwrap_or(EMPTY_TREE_OID);
                let mut args = vec![
                    "diff-index",
                    "--cached",
                    "-p",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=3",
                    base_tree,
                    "--",
                ];
                if let Some(previous) = change.previous_path.as_deref() {
                    args.push(previous);
                }
                args.push(&change.path);
                let bytes = worktree_reader.bytes(&root, &args).await?;
                if bytes.contains(&0) {
                    skipped_paths.push(format!("{}（二进制文件）", change.path));
                    continue;
                }
                match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        skipped_paths
                            .push(format!("{}（非 UTF-8 文本编码，未完整覆盖）", change.path));
                        continue;
                    }
                }
            } else if change.git_status == "??" {
                let full_path = root.join(&change.path);
                let metadata = fs::symlink_metadata(&full_path)
                    .map_err(|_| "无法读取未跟踪变更文件；当前草稿与选择已保留")?;
                if metadata.file_type().is_symlink() {
                    let link = fs::read_link(&full_path).map_err(|_| "无法读取符号链接目标")?;
                    format_untracked_patch(&change.path, &format!("symlink -> {}", link.display()))
                } else {
                    if metadata.len() as usize > MAX_FILE_PATCH_BYTES {
                        skipped_paths.push(format!("{}（大文件超出单文件审查上限）", change.path));
                        continue;
                    }
                    let bytes = fs::read(&full_path)
                        .map_err(|_| "无法读取未跟踪文件内容；当前草稿与选择已保留")?;
                    if bytes.contains(&0) {
                        skipped_paths.push(format!("{}（二进制文件）", change.path));
                        continue;
                    }
                    match String::from_utf8(bytes) {
                        Ok(text) => format_untracked_patch(&change.path, &text),
                        Err(_) => {
                            skipped_paths
                                .push(format!("{}（非 UTF-8 文本编码，未完整覆盖）", change.path));
                            continue;
                        }
                    }
                }
            } else {
                let bytes = worktree_reader
                    .bytes(
                        &root,
                        &[
                            "diff-files",
                            "-p",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--unified=3",
                            "--",
                            &change.path,
                        ],
                    )
                    .await?;
                if bytes.contains(&0) {
                    skipped_paths.push(format!("{}（二进制文件）", change.path));
                    continue;
                }
                match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        skipped_paths
                            .push(format!("{}（非 UTF-8 文本编码，未完整覆盖）", change.path));
                        continue;
                    }
                }
            };

            let raw_patch = if is_sensitive_local_file(&change.path) {
                skipped_paths.push(format!("{}（本地环境或敏感凭据值已本地脱敏）", change.path));
                match redact_sensitive_patch(&change.path, &raw_patch) {
                    Some(redacted) => redacted,
                    None => continue,
                }
            } else {
                raw_patch
            };

            let (file_patch_cap, total_patch_cap) = match mode {
                AiScopeMode::Generate => (
                    GENERATE_MAX_FILE_PATCH_BYTES,
                    GENERATE_MAX_TOTAL_PATCH_BYTES,
                ),
                AiScopeMode::Review => (MAX_FILE_PATCH_BYTES, MAX_TOTAL_PATCH_BYTES),
            };
            let remaining = total_patch_cap.saturating_sub(total_patch_bytes);
            if remaining < MIN_PARTIAL_PATCH_BUDGET {
                skipped_paths.push(format!(
                    "{}（所选总改动较大，超出单次上下文上限）",
                    change.path
                ));
                continue;
            }
            let allowed = file_patch_cap.min(remaining);
            let final_patch = if raw_patch.len() > allowed {
                skipped_paths.push(format!("{}（大文件改动较大，仅部分覆盖）", change.path));
                truncate_patch_head_tail(&raw_patch, allowed)
            } else {
                raw_patch
            };

            total_patch_bytes += final_patch.len();
            reviewed_file_ids.push(change.id.clone());
            files.push(ExtractedDiffFile {
                file_id: change.id.clone(),
                path: change.path.clone(),
                previous_path: change.previous_path.clone(),
                status: change.status.clone(),
                range_kind: change.range.kind.clone(),
                added: change.range.added,
                removed: change.range.removed,
                patch: final_patch,
            });
        }

        let branch = if live_tree.detached {
            "(detached)".into()
        } else {
            live_tree.branch
        };
        Ok(ExtractedAiScope {
            branch,
            recent_commits,
            files,
            skipped_paths,
            reviewed_file_ids,
        })
    }

    pub async fn extract_cpush_scope(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
    ) -> Result<ExtractedCPushScope> {
        if selections.is_empty() {
            return Err("请先选择至少一个变更文件后再发起 C-Push；当前草稿已保留。".into());
        }
        let mut seen = HashSet::with_capacity(selections.len());
        for item in selections {
            if item.file_id.is_empty()
                || item.commit_token.is_empty()
                || !seen.insert(item.file_id.as_str())
            {
                return Err("C-Push 请求包含无效或重复的文件范围凭证，请刷新后重试。".into());
            }
        }

        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let (_, root) = self.registered_worktree_root(target).await?;
        let entry = WorktreeEntry {
            path: root.clone(),
            head: None,
            branch: String::new(),
            bare: false,
        };
        let (live_tree, live_commit_state) =
            self.read_worktree(&target.repository_id, &entry).await?;
        if live_tree.detached {
            return Err(
                "当前处于分离 HEAD，请先在顶部新建或切换分支后再使用 C-Push；草稿与选择已保留。"
                    .into(),
            );
        }

        let change_by_id: HashMap<&str, &Change> = live_tree
            .files
            .iter()
            .map(|change| (change.id.as_str(), change))
            .collect();

        for item in selections {
            let Some(authorized) = live_commit_state.files.get(&item.file_id) else {
                return Err(
                    "所选文件已不在当前工作树变更列表中，请刷新确认后再发起 C-Push；草稿与选择已保留。"
                        .into(),
                );
            };
            if authorized.conflicted {
                return Err(
                    "所选范围包含未解决的合并冲突文件，请先处理冲突后再执行 C-Push；草稿与选择已保留。"
                        .into(),
                );
            }
            if authorized.token != item.commit_token {
                return Err(
                    "所选文件范围、暂存内容或分支已变化，已暂停本次 C-Push；请刷新核对最新变更，草稿与选择已保留。"
                        .into(),
                );
            }
        }

        let unselected_paths = live_tree
            .files
            .iter()
            .filter(|change| !seen.contains(change.id.as_str()))
            .map(|change| change.path.clone())
            .collect();

        let worktree_reader = self.inner.reader.for_worktree(&root).await?;
        let recent_commits = if live_tree.head.is_some() {
            let output = worktree_reader
                .run(&root, &["log", "-n", "8", "--format=%s%x00", "HEAD"])
                .await?;
            if output.success {
                output
                    .bytes
                    .split(|byte| *byte == 0)
                    .filter_map(|raw| std::str::from_utf8(raw).ok())
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut raw_entries: Vec<(Option<String>, Option<String>)> =
            Vec::with_capacity(selections.len());

        for item in selections {
            let change = change_by_id[item.file_id.as_str()];
            let mut raw_patch_opt = None;
            let mut skip_reason = None;

            if change.ignored_for_review {
                skip_reason = Some(format!(
                    "锁文件（+{} -{}，按关联依赖变更归组）",
                    change.range.added, change.range.removed
                ));
            } else if change.binary {
                skip_reason = Some("二进制文件（不展开文本补丁）".into());
            } else if change.range.kind == "partial" || change.range.kind == "staged" {
                let base_tree = live_tree.head.as_deref().unwrap_or(EMPTY_TREE_OID);
                let mut args = vec![
                    "diff-index",
                    "--cached",
                    "-p",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=1",
                    base_tree,
                    "--",
                ];
                if let Some(previous) = change.previous_path.as_deref() {
                    args.push(previous);
                }
                args.push(&change.path);
                let bytes = worktree_reader.bytes(&root, &args).await?;
                if bytes.contains(&0) {
                    skip_reason = Some("二进制文件".into());
                } else {
                    match String::from_utf8(bytes) {
                        Ok(text) => raw_patch_opt = Some(text),
                        Err(_) => skip_reason = Some("非 UTF-8 编码文本".into()),
                    }
                }
            } else if change.git_status == "??" {
                let full_path = root.join(&change.path);
                let metadata = fs::symlink_metadata(&full_path)
                    .map_err(|_| "无法读取未跟踪变更文件；当前草稿与选择已保留")?;
                if metadata.file_type().is_symlink() {
                    let link = fs::read_link(&full_path).map_err(|_| "无法读取符号链接目标")?;
                    raw_patch_opt = Some(format_untracked_patch(
                        &change.path,
                        &format!("symlink -> {}", link.display()),
                    ));
                } else if metadata.len() as usize > MAX_UNTRACKED_READ_BYTES {
                    skip_reason = Some("大文件（超出单文件内联上限）".into());
                } else {
                    let bytes = fs::read(&full_path)
                        .map_err(|_| "无法读取未跟踪文件内容；当前草稿与选择已保留")?;
                    if bytes.contains(&0) {
                        skip_reason = Some("二进制文件".into());
                    } else {
                        match String::from_utf8(bytes) {
                            Ok(text) => {
                                raw_patch_opt = Some(format_untracked_patch(&change.path, &text))
                            }
                            Err(_) => skip_reason = Some("非 UTF-8 编码文本".into()),
                        }
                    }
                }
            } else {
                let bytes = worktree_reader
                    .bytes(
                        &root,
                        &[
                            "diff-files",
                            "-p",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--unified=1",
                            "--",
                            &change.path,
                        ],
                    )
                    .await?;
                if bytes.contains(&0) {
                    skip_reason = Some("二进制文件".into());
                } else {
                    match String::from_utf8(bytes) {
                        Ok(text) => raw_patch_opt = Some(text),
                        Err(_) => skip_reason = Some("非 UTF-8 编码文本".into()),
                    }
                }
            }

            if is_sensitive_local_file(&change.path) {
                raw_patch_opt =
                    raw_patch_opt.and_then(|patch| redact_sensitive_patch(&change.path, &patch));
                skip_reason = Some(
                    "疑似本地环境或敏感凭据文件（值已本地脱敏，提交前需向用户确认是否排除）".into(),
                );
            }

            raw_entries.push((raw_patch_opt, skip_reason));
        }

        let mut patch_indices: Vec<usize> = raw_entries
            .iter()
            .enumerate()
            .filter_map(|(idx, (patch, _))| patch.as_ref().map(|p| (idx, p.len())))
            .map(|(idx, _)| idx)
            .collect();
        patch_indices.sort_by_key(|&idx| (raw_entries[idx].0.as_ref().map_or(0, String::len), idx));

        let mut remaining_budget = CPUSH_MAX_TOTAL_PATCH_BYTES;
        for (pos, &idx) in patch_indices.iter().enumerate() {
            let files_left = patch_indices.len() - pos;
            let fair_share = (remaining_budget / files_left).min(CPUSH_MAX_FILE_PATCH_BYTES);
            let Some(raw_patch) = raw_entries[idx].0.take() else {
                continue;
            };
            if raw_patch.len() <= fair_share {
                remaining_budget = remaining_budget.saturating_sub(raw_patch.len());
                raw_entries[idx].0 = Some(raw_patch);
            } else if fair_share >= CPUSH_MIN_PARTIAL_PATCH_BUDGET {
                let slimmed = truncate_patch_head_tail(&raw_patch, fair_share);
                remaining_budget = remaining_budget.saturating_sub(slimmed.len());
                raw_entries[idx].0 = Some(slimmed);
                raw_entries[idx]
                    .1
                    .get_or_insert_with(|| "大文件改动较大，补丁已精简".into());
            } else {
                raw_entries[idx]
                    .1
                    .get_or_insert_with(|| "所选总改动较大，省略内联补丁".into());
            }
        }

        let files = selections
            .iter()
            .zip(raw_entries)
            .map(|(item, (patch, skip_reason))| {
                let change = change_by_id[item.file_id.as_str()];
                FrozenCPushFile {
                    file_id: change.id.clone(),
                    path: change.path.clone(),
                    previous_path: change.previous_path.clone(),
                    status: change.status.clone(),
                    range_kind: change.range.kind.clone(),
                    added: change.range.added,
                    removed: change.range.removed,
                    ignored_for_review: change.ignored_for_review,
                    binary: change.binary,
                    content_version: change.content_version.clone(),
                    patch,
                    skip_reason,
                }
            })
            .collect();

        Ok(ExtractedCPushScope {
            branch: live_tree.branch,
            initial_head: live_tree.head,
            recent_commits,
            unselected_paths,
            files,
        })
    }

    pub async fn resolve_cpush_step_selections(
        &self,
        target: &ReadTarget,
        expected_branch: &str,
        expected_head: Option<&str>,
        step_files: &[FrozenCPushFile],
    ) -> Result<(RepositorySnapshot, Vec<CommitSelection>)> {
        let snapshot = self.refresh(target).await?;
        let live_tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.worktree_id == target.worktree_id)
            .ok_or("当前工作树不在最新 Git 快照中，请重新打开核对")?;
        if let Some(err) = &live_tree.read_error {
            return Err(format!("工作树状态不可用：{err}"));
        }
        if live_tree.detached || live_tree.branch != expected_branch {
            return Err(format!(
                "工作树当前分支已变为“{}”（原计划分支为“{expected_branch}”），已暂停写入以保护改动",
                if live_tree.detached {
                    "分离 HEAD"
                } else {
                    &live_tree.branch
                }
            ));
        }
        if live_tree.head.as_deref() != expected_head {
            return Err(
                "仓库 HEAD 已在当前会话之外发生变化，已暂停继续提交；请核对最新 Git 历史".into(),
            );
        }

        let mut selections = Vec::with_capacity(step_files.len());
        for frozen in step_files {
            let Some(live_file) = live_tree
                .files
                .iter()
                .find(|change| change.id == frozen.file_id)
            else {
                return Err(format!(
                    "待提交文件“{}”已不在当前变更列表中，已暂停当前步骤",
                    frozen.path
                ));
            };
            if live_file.content_version != frozen.content_version
                || live_file.range.kind != frozen.range_kind
            {
                return Err(format!(
                    "待提交文件“{}”的内容或暂存范围已在外部发生变化，已暂停当前步骤以防止误提交",
                    frozen.path
                ));
            }
            selections.push(CommitSelection {
                file_id: live_file.id.clone(),
                commit_token: live_file.commit_token.clone(),
            });
        }
        Ok((snapshot, selections))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::smoke::Fixture;

    #[test]
    fn head_tail_truncation_preserves_both_ends_and_utf8_boundaries() {
        let mut raw =
            String::from("--- a/src/views/sofDialog.vue\n+++ b/src/views/sofDialog.vue\n");
        for idx in 0..300 {
            raw.push_str(&format!(
                "+  <div>表单字段 {idx}：泊位与装卸作业时间校验</div>\n"
            ));
        }
        raw.push_str("+export function submitSofDialog() { return true; }\n");

        let slimmed = truncate_patch_head_tail(&raw, 1024);
        assert!(slimmed.len() <= 1024);
        assert!(slimmed.starts_with("--- a/src/views/sofDialog.vue"));
        assert!(slimmed.contains("[省略中间"));
        assert!(slimmed.contains("+export function submitSofDialog()"));
    }

    #[tokio::test]
    async fn cpush_scope_distributes_patch_budget_across_many_large_files() {
        let fixture = Fixture::new().unwrap();
        for idx in 0..7 {
            let mut content = format!("<template><section id=\"view-{idx}\">\n");
            for line in 0..320 {
                content.push_str(&format!(
                    "  <p>业务字段 {idx}-{line}：装卸港作业节点记录</p>\n"
                ));
            }
            content.push_str(&format!(
                "</section></template>\n<script setup>\nconst pageId = {idx};\n</script>\n"
            ));
            fs::write(fixture.root.join(format!("page-{idx}.vue")), content).unwrap();
        }

        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let tree = &snapshot.worktrees[0];
        let selections: Vec<CommitSelection> = tree
            .files
            .iter()
            .filter(|f| f.path.starts_with("page-"))
            .map(|f| CommitSelection {
                file_id: f.id.clone(),
                commit_token: f.commit_token.clone(),
            })
            .collect();
        assert_eq!(selections.len(), 7);

        let target = ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "sess-budget".into(),
            scope_version: tree.status_version.clone(),
        };
        let extracted = store
            .extract_cpush_scope(&target, &selections)
            .await
            .unwrap();
        assert_eq!(extracted.files.len(), 7);

        let total_bytes: usize = extracted
            .files
            .iter()
            .map(|f| f.patch.as_ref().map_or(0, String::len))
            .sum();
        assert!(total_bytes <= CPUSH_MAX_TOTAL_PATCH_BYTES);

        for file in &extracted.files {
            let patch = file
                .patch
                .as_deref()
                .expect("every file gets fair patch share");
            assert!(patch.contains("<template><section"));
            assert!(patch.contains("<script setup>"));
            assert_eq!(
                file.skip_reason.as_deref(),
                Some("大文件改动较大，补丁已精简")
            );
        }
    }
}
