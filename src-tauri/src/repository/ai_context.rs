//! Read-only Git scope verification and unified diff extraction for M3 AI tasks
//! (`generate_commit_message` and `review_selected_scope`).
//! Holds the repository gate only while reading a consistent Git snapshot; releases it
//! before any external model request starts.
use super::*;
use serde::Serialize;

const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const MAX_FILE_PATCH_BYTES: usize = 48 * 1024;
const MAX_TOTAL_PATCH_BYTES: usize = 192 * 1024;
const MIN_PARTIAL_PATCH_BUDGET: usize = 2 * 1024;

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

            let remaining = MAX_TOTAL_PATCH_BYTES.saturating_sub(total_patch_bytes);
            if remaining < MIN_PARTIAL_PATCH_BUDGET {
                skipped_paths.push(format!(
                    "{}（所选总改动较大，超出单次上下文上限）",
                    change.path
                ));
                continue;
            }
            let allowed = MAX_FILE_PATCH_BYTES.min(remaining);
            let final_patch = if raw_patch.len() > allowed {
                let cut = raw_patch.floor_char_boundary(allowed);
                skipped_paths.push(format!("{}（大文件改动较大，仅部分覆盖）", change.path));
                format!("{}\n... [truncated]", &raw_patch[..cut])
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

        let mut files = Vec::with_capacity(selections.len());
        let mut total_patch_bytes = 0usize;

        for item in selections {
            let change = change_by_id[item.file_id.as_str()];
            let mut patch = None;
            let mut skip_reason = None;

            if change.ignored_for_review {
                skip_reason = Some(format!(
                    "锁文件（+{} -{}，按关联依赖变更归组）",
                    change.range.added, change.range.removed
                ));
            } else if change.binary {
                skip_reason = Some("二进制文件（不展开文本补丁）".into());
            } else {
                let raw_patch_opt: Option<String> = if change.range.kind == "partial"
                    || change.range.kind == "staged"
                {
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
                        skip_reason = Some("二进制文件".into());
                        None
                    } else {
                        match String::from_utf8(bytes) {
                            Ok(text) => Some(text),
                            Err(_) => {
                                skip_reason = Some("非 UTF-8 编码文本".into());
                                None
                            }
                        }
                    }
                } else if change.git_status == "??" {
                    let full_path = root.join(&change.path);
                    let metadata = fs::symlink_metadata(&full_path)
                        .map_err(|_| "无法读取未跟踪变更文件；当前草稿与选择已保留")?;
                    if metadata.file_type().is_symlink() {
                        let link = fs::read_link(&full_path).map_err(|_| "无法读取符号链接目标")?;
                        Some(format_untracked_patch(
                            &change.path,
                            &format!("symlink -> {}", link.display()),
                        ))
                    } else if metadata.len() as usize > MAX_FILE_PATCH_BYTES {
                        skip_reason = Some("大文件（超出单文件内联上限）".into());
                        None
                    } else {
                        let bytes = fs::read(&full_path)
                            .map_err(|_| "无法读取未跟踪文件内容；当前草稿与选择已保留")?;
                        if bytes.contains(&0) {
                            skip_reason = Some("二进制文件".into());
                            None
                        } else {
                            match String::from_utf8(bytes) {
                                Ok(text) => Some(format_untracked_patch(&change.path, &text)),
                                Err(_) => {
                                    skip_reason = Some("非 UTF-8 编码文本".into());
                                    None
                                }
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
                        skip_reason = Some("二进制文件".into());
                        None
                    } else {
                        match String::from_utf8(bytes) {
                            Ok(text) => Some(text),
                            Err(_) => {
                                skip_reason = Some("非 UTF-8 编码文本".into());
                                None
                            }
                        }
                    }
                };

                if let Some(raw_patch) = raw_patch_opt {
                    let remaining = MAX_TOTAL_PATCH_BYTES.saturating_sub(total_patch_bytes);
                    if remaining < MIN_PARTIAL_PATCH_BUDGET {
                        skip_reason = Some("所选总改动较大，省略内联补丁".into());
                    } else {
                        let allowed = MAX_FILE_PATCH_BYTES.min(remaining);
                        let final_patch = if raw_patch.len() > allowed {
                            let cut = raw_patch.floor_char_boundary(allowed);
                            skip_reason = Some("大文件改动较大，补丁已截断".into());
                            format!("{}\n... [truncated]", &raw_patch[..cut])
                        } else {
                            raw_patch
                        };
                        total_patch_bytes += final_patch.len();
                        patch = Some(final_patch);
                    }
                }
            }

            files.push(FrozenCPushFile {
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
            });
        }

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
