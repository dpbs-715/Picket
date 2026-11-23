//! Controlled local commit coordinator for M2.
//! Freezes and re-verifies selected per-file scopes in Rust, runs real Git hooks without
//! `--no-verify` or `hooksPath` overrides, aborts before writing if hooks mutate content,
//! reconciles only selected paths in `.git/index`, and keeps unstaged/unselected changes intact.
use super::{branches::BranchState, parse::StatusEntry, runner::GitOutput, *};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write,
};
use tempfile::NamedTempFile;

#[derive(Clone)]
pub(super) struct CommitRegistration {
    pub branch: String,
    pub detached: bool,
    pub head: Option<String>,
    pub files: HashMap<String, AuthorizedFile>,
}

#[derive(Clone)]
pub(super) struct AuthorizedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: bool,
    pub range_kind: String,
    pub conflicted: bool,
    pub stage_entries: Vec<u8>,
    pub working_hash: Option<String>,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionRecord {
    repository_id: String,
    worktree_id: String,
    parent: Option<String>,
    branch: String,
    tree: String,
    paths: Vec<String>,
    selections: Vec<CommitSelection>,
    raw_message: String,
    commit: Option<CommitResult>,
}

struct IndexLock {
    path: PathBuf,
    file: File,
    replaced: bool,
}

impl IndexLock {
    fn acquire(index: &Path) -> Result<Self> {
        let path = index.with_file_name("index.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| {
                "存在 Git 索引锁（index.lock），可能有其他 Git 进程正在操作；未执行提交，草稿与选择已保留"
                    .to_string()
            })?;
        Ok(Self {
            path,
            file,
            replaced: false,
        })
    }

    fn replace(&mut self, index: &Path, bytes: &[u8]) -> Result<()> {
        self.file
            .write_all(bytes)
            .map_err(|_| "写入更新后的暂存区失败".to_string())?;
        self.file
            .sync_all()
            .map_err(|_| "同步暂存区文件失败".to_string())?;
        fs::rename(&self.path, index).map_err(|_| "原子替换暂存区索引失败".to_string())?;
        self.replaced = true;
        Ok(())
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.replaced {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) fn is_protected_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master")
}

fn ensure_safe_repo_path(path: &str) -> Result<()> {
    parse::relative(path)?;
    if path
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err("提交范围包含不安全的 .git 路径".into());
    }
    Ok(())
}

pub(super) fn stage_entries_by_path(index_bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    for record in index_bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("Git 暂存区条目格式不匹配".into());
        };
        let header = parse::utf8(&record[..tab])?;
        if header.split_whitespace().count() != 3 {
            return Err("Git 暂存区头部格式不匹配".into());
        }
        let path = parse::utf8(&record[tab + 1..])?;
        parse::relative(&path)?;
        let slot = entries.entry(path).or_default();
        slot.extend_from_slice(record);
        slot.push(0);
    }
    Ok(entries)
}

fn collect_unit_stage_entries(
    stage_map: &BTreeMap<String, Vec<u8>>,
    unit_paths: &BTreeSet<String>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for path in unit_paths {
        if let Some(records) = stage_map.get(path) {
            bytes.extend_from_slice(records);
        }
    }
    bytes
}

impl AuthorizedFile {
    pub fn unit_paths(&self) -> BTreeSet<String> {
        let mut paths = BTreeSet::new();
        paths.insert(self.path.clone());
        if let Some(previous) = &self.previous_path {
            paths.insert(previous.clone());
        }
        paths
    }
}

impl CommitRegistration {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        repository_id: &str,
        worktree_id: &str,
        branch: &str,
        detached: bool,
        head: Option<&str>,
        entries: &[StatusEntry],
        changes: &mut [Change],
        working_hashes: &HashMap<String, String>,
        index_bytes: &[u8],
    ) -> Result<Self> {
        let stage_map = stage_entries_by_path(index_bytes)?;
        let mut files = HashMap::with_capacity(changes.len());
        for (entry, change) in entries.iter().zip(changes.iter_mut()) {
            let is_staged = staged(entry);
            let previous_path = (is_staged && entry.xy.starts_with('R'))
                .then(|| entry.previous.clone())
                .flatten();
            let mut unit_paths = BTreeSet::new();
            unit_paths.insert(entry.path.clone());
            if let Some(previous) = &previous_path {
                unit_paths.insert(previous.clone());
            }
            let stage_entries = collect_unit_stage_entries(&stage_map, &unit_paths);
            let working_hash = if is_staged {
                None
            } else {
                Some(
                    working_hashes
                        .get(&entry.path)
                        .cloned()
                        .ok_or("缺少未暂存文件的内容指纹")?,
                )
            };
            let payload = serde_json::to_vec(&(
                "picket-commit-v1",
                repository_id,
                worktree_id,
                branch,
                detached,
                head,
                &change.id,
                &entry.path,
                &previous_path,
                is_staged,
                &change.range.kind,
                entry.conflicted,
                &stage_entries,
                &working_hash,
            ))
            .map_err(|error| error.to_string())?;
            let token = hash(payload);
            change.commit_token = token.clone();
            files.insert(
                change.id.clone(),
                AuthorizedFile {
                    path: entry.path.clone(),
                    previous_path,
                    staged: is_staged,
                    range_kind: change.range.kind.clone(),
                    conflicted: entry.conflicted,
                    stage_entries,
                    working_hash,
                    token,
                },
            );
        }
        Ok(Self {
            branch: branch.to_owned(),
            detached,
            head: head.map(str::to_owned),
            files,
        })
    }
}

fn output_detail(output: &GitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.bytes);
    let combined = match (stderr.trim(), stdout.trim()) {
        ("", "") => format!("退出码 {:?}", output.code),
        (err, "") => err.to_owned(),
        ("", out) => out.to_owned(),
        (err, out) => format!("{err}；{out}"),
    };
    let single_line = combined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let truncated: String = chars.by_ref().take(320).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

async fn run_index_checked(
    reader: &GitReader,
    root: &Path,
    args: &[&str],
    index: &Path,
    input: Option<&[u8]>,
    context: &str,
) -> Result<Vec<u8>> {
    let output = reader.run_with_index(root, args, index, input).await?;
    if !output.success {
        return Err(format!(
            "{context}（{}）；未创建提交，草稿与选择已保留",
            output_detail(&output)
        ));
    }
    Ok(output.bytes)
}

async fn write_tree(reader: &GitReader, root: &Path, index: &Path) -> Result<String> {
    let bytes =
        run_index_checked(reader, root, &["write-tree"], index, None, "生成提交树失败").await?;
    let tree = parse::utf8(&bytes)?.trim().to_owned();
    if tree.is_empty() {
        return Err("Git 未返回有效的提交树哈希".into());
    }
    Ok(tree)
}

impl RepositoryStore {
    async fn registered_worktree_root(
        &self,
        target: &ReadTarget,
    ) -> Result<(Registration, PathBuf)> {
        let registered = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "仓库注册表不可用")?
            .get(&target.repository_id)
            .cloned()
            .ok_or("仓库未在当前窗口打开，请重新选择目录；草稿与选择已保留")?;
        let root = registered
            .paths
            .get(&target.worktree_id)
            .cloned()
            .ok_or("工作树未在当前窗口打开，请刷新后重试；草稿与选择已保留")?;
        if fs::canonicalize(&root).map_err(|_| "工作树目录不可访问")? != root {
            return Err("工作树路径已变化，请重新打开目录；草稿与选择已保留".into());
        }
        let reader = &self.inner.reader;
        let common = reader
            .text(
                &root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "Git 公共目录不可访问")?;
        let toplevel = reader
            .text(&root, &["rev-parse", "--show-toplevel"])
            .await?;
        if identity("repo", &common) != target.repository_id
            || fs::canonicalize(toplevel).map_err(|_| "Git 工作目录不可访问")? != root
        {
            return Err("工作树所属仓库已变化，请重新打开；草稿与选择已保留".into());
        }
        Ok((registered, root))
    }

    async fn ensure_commit_write_allowed(&self, root: &Path) -> Result<()> {
        let reader = &self.inner.reader;
        for lock in ["index.lock", "HEAD.lock"] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", lock],
                )
                .await?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(format!(
                    "存在 Git 锁文件（{lock}），可能有其他 Git 进程正在操作；未执行提交，草稿与选择已保留"
                ));
            }
        }
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "BISECT_LOG",
        ] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .await?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(format!(
                    "存在未完成的 Git 操作（{marker}），请先在 Git / IDE 中处理；未执行提交，草稿与选择已保留"
                ));
            }
        }
        if !reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await?
            .is_empty()
        {
            return Err(
                "存在未解决的合并冲突，请先在 Git / IDE 中处理；未执行提交，草稿与选择已保留"
                    .into(),
            );
        }
        let flags = reader.bytes(root, &["ls-files", "-v", "-z"]).await?;
        if flags
            .split(|byte| *byte == 0)
            .any(|entry| !entry.is_empty() && entry[0] != b'H')
        {
            return Err(
                "当前暂存区含稀疏检出或隐藏变更标记，暂不执行提交写入；请先在 Git / IDE 中处理"
                    .into(),
            );
        }
        if reader
            .bytes(root, &["ls-files", "--stage", "-z"])
            .await?
            .split(|byte| *byte == 0)
            .any(|entry| entry.starts_with(b"160000 "))
        {
            return Err("含子模块的仓库尚未通过提交写入验证，请在 Git / IDE 中操作".into());
        }
        Ok(())
    }

    async fn ensure_commit_filters_allowed(
        &self,
        root: &Path,
        unstaged_paths: &[String],
    ) -> Result<()> {
        if unstaged_paths.is_empty() {
            return Ok(());
        }
        let reader = &self.inner.reader;
        let filters = reader
            .run(
                root,
                &[
                    "config",
                    "--null",
                    "--name-only",
                    "--get-regexp",
                    r"^filter\..*\.(clean|process)$",
                    ".+",
                ],
            )
            .await?;
        if filters.code == Some(1) {
            return Ok(());
        }
        if !filters.success {
            return Err("无法核对 Git 文件过滤器配置，未执行提交；草稿与选择已保留".into());
        }
        let mut drivers = BTreeSet::new();
        for key in filters
            .bytes
            .split(|byte| *byte == 0)
            .filter(|key| !key.is_empty())
        {
            let key = parse::utf8(key)?;
            let (driver, _) = key
                .strip_prefix("filter.")
                .ok_or("无法解析 Git 过滤器名称")?
                .rsplit_once('.')
                .ok_or("无法解析 Git 过滤器名称")?;
            drivers.insert(driver.to_owned());
        }
        for batch in unstaged_paths.chunks(128) {
            let mut args = vec!["check-attr", "-z", "filter", "--"];
            args.extend(batch.iter().map(String::as_str));
            let attributes = reader.bytes(root, &args).await?;
            let fields: Vec<_> = attributes.split(|byte| *byte == 0).collect();
            for record in fields.chunks(3).filter(|record| record.len() == 3) {
                if drivers.contains(&parse::utf8(record[2])?) {
                    return Err("所选未暂存文件需要外部 Git 过滤器转换，当前版本不能保证安全写入，请在 Git / IDE 中提交；草稿与选择已保留".into());
                }
            }
        }
        Ok(())
    }

    fn verify_live_selected_scope(
        &self,
        commit_state: &CommitRegistration,
        selected_files: &[AuthorizedFile],
        live_state: &BranchState,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        let live_status = parse::status(&live_state.status)?;
        if live_status.branch == "(detached)" {
            return Err(
                "当前处于分离 HEAD，请先在顶部新建或切换开发分支后再提交；草稿与选择已保留".into(),
            );
        }
        if is_protected_branch(&live_status.branch) {
            return Err(
                "当前为保护分支（main / master），请先在顶部新建或切换开发分支后再提交；草稿与选择已保留"
                    .into(),
            );
        }
        if live_status.branch != commit_state.branch || live_status.head != commit_state.head {
            return Err(
                "分支或 HEAD 已在外部发生变化，已暂停提交；当前草稿与选择已保留，请刷新后重新确认范围"
                    .into(),
            );
        }
        if live_status.entries.iter().any(|entry| entry.conflicted) {
            return Err(
                "存在未解决的合并冲突，请先在 Git / IDE 中处理；未执行提交，草稿与选择已保留"
                    .into(),
            );
        }
        if live_status
            .entries
            .iter()
            .any(|entry| entry.mode == "160000")
        {
            return Err("存在子模块变更，当前版本尚不支持提交；请在 Git / IDE 中操作".into());
        }
        let live_stage_map = stage_entries_by_path(&live_state.index)?;
        let selected_paths: HashSet<&str> = selected_files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        for entry in &live_status.entries {
            if !selected_paths.contains(entry.path.as_str())
                && staged(entry)
                && entry.xy.starts_with('R')
                && let Some(previous) = entry.previous.as_deref()
                && selected_paths.contains(previous)
            {
                return Err("所选文件与未勾选的重命名原路径共用暂存位置，请将对应重命名文件一并勾选或先在 Git / IDE 中调整暂存；草稿与选择已保留".into());
            }
        }
        let working_map: HashMap<&str, &str> = live_state
            .working
            .iter()
            .map(|(path, digest)| (path.as_str(), digest.as_str()))
            .collect();
        for authorized in selected_files {
            let Some(live_entry) = live_status
                .entries
                .iter()
                .find(|entry| entry.path == authorized.path)
            else {
                return Err(
                    "所选文件的变更状态已在外部变化，已暂停提交；当前草稿与选择已保留，请刷新后重新确认范围"
                        .into(),
                );
            };
            let live_staged = staged(live_entry);
            let live_kind = if live_staged {
                if live_entry
                    .xy
                    .as_bytes()
                    .get(1)
                    .is_some_and(|byte| *byte != b'.')
                {
                    "partial"
                } else {
                    "staged"
                }
            } else {
                "unstaged"
            };
            let live_previous = (live_staged && live_entry.xy.starts_with('R'))
                .then(|| live_entry.previous.clone())
                .flatten();
            let live_stage_entries =
                collect_unit_stage_entries(&live_stage_map, &authorized.unit_paths());
            if live_staged != authorized.staged
                || live_kind != authorized.range_kind
                || live_previous != authorized.previous_path
                || live_stage_entries != authorized.stage_entries
            {
                return Err(
                    "所选文件的暂存范围已在外部变化，已暂停提交；当前草稿与选择已保留，请刷新后重新确认范围"
                        .into(),
                );
            }
            if !authorized.staged
                && working_map.get(authorized.path.as_str()).copied()
                    != authorized.working_hash.as_deref()
            {
                return Err(
                    "所选未暂存文件的内容已在外部变化，已暂停提交；当前草稿与选择已保留，请刷新后重新确认范围"
                        .into(),
                );
            }
        }
        Ok(live_stage_map)
    }

    async fn hook_failure_error(
        &self,
        root: &Path,
        hook_name: &str,
        output: &GitOutput,
        pre_hook_state: &BranchState,
    ) -> Result<(RepositorySnapshot, CommitResult)> {
        let detail = output_detail(output);
        let mutated = match self.branch_state(root).await {
            Ok(current) => {
                current.status != pre_hook_state.status
                    || current.index != pre_hook_state.index
                    || current.working != pre_hook_state.working
            }
            Err(_) => true,
        };
        if mutated {
            Err(format!(
                "Git {hook_name} 钩子未通过且修改了仓库内容：{detail}。未创建提交，当前草稿与选择已保留；请核对更新后的变更并重新确认范围。"
            ))
        } else {
            Err(format!(
                "Git {hook_name} 钩子拒绝了本次提交：{detail}。未创建提交，当前草稿与选择已保留。"
            ))
        }
    }

    async fn verify_recorded_commit(
        &self,
        root: &Path,
        record: &ExecutionRecord,
        commit: &CommitResult,
    ) -> Result<()> {
        let reader = &self.inner.reader;
        if !reader
            .run(root, &["merge-base", "--is-ancestor", &commit.oid, "HEAD"])
            .await?
            .success
        {
            return Err(
                "记录的提交已不在当前分支历史中，请刷新核对实际状态；不会自动重复提交".into(),
            );
        }
        if reader
            .text(root, &["rev-parse", &format!("{}^{{tree}}", commit.oid)])
            .await?
            != record.tree
        {
            return Err("已记录提交的内容树与授权范围不一致，请停止并在 Git 中核对".into());
        }
        match &record.parent {
            Some(expected_parent) => {
                if reader
                    .text(root, &["rev-parse", &format!("{}^1", commit.oid)])
                    .await?
                    != *expected_parent
                {
                    return Err("已记录提交的父提交与授权起点不一致，请停止并在 Git 中核对".into());
                }
            }
            None => {
                if reader
                    .run(
                        root,
                        &[
                            "rev-parse",
                            "--verify",
                            "--quiet",
                            &format!("{}^", commit.oid),
                        ],
                    )
                    .await?
                    .success
                {
                    return Err("已记录的初始提交存在意外父提交，请停止并在 Git 中核对".into());
                }
            }
        }
        Ok(())
    }

    pub async fn commit(
        &self,
        request: &CommitRequest,
    ) -> Result<(RepositorySnapshot, CommitResult)> {
        validate_request_id(&request.request_id)?;
        if request.push {
            return Err(
                "远端推送将在 M5 接入；本次未执行提交或推送，草稿与选择已保留。当前请使用「仅本地提交」。"
                    .into(),
            );
        }
        if request.message.trim().is_empty() {
            return Err("请先填写提交说明，当前草稿与选择已保留".into());
        }
        if request.message.contains('\0') {
            return Err("提交说明不能包含空字节，当前草稿与选择已保留".into());
        }
        if request.message.len() > 1024 * 1024 {
            return Err("提交说明过长，请精简后重试；当前草稿与选择已保留".into());
        }
        if request.selections.is_empty() {
            return Err("请至少选择一项要提交的变更；当前草稿已保留".into());
        }
        let mut seen_ids = HashSet::with_capacity(request.selections.len());
        for selection in &request.selections {
            if selection.file_id.is_empty()
                || selection.commit_token.is_empty()
                || !seen_ids.insert(selection.file_id.as_str())
            {
                return Err(
                    "提交范围包含无效或重复的文件项，请刷新后重试；草稿与选择已保留".into(),
                );
            }
        }

        let _guard = self
            .repository_gate(&request.target.repository_id)?
            .lock_owned()
            .await;
        let (registered, root) = self.registered_worktree_root(&request.target).await?;
        let reader = &self.inner.reader;
        let git_dir = fs::canonicalize(
            reader
                .text(&root, &["rev-parse", "--path-format=absolute", "--git-dir"])
                .await?,
        )
        .map_err(|_| "无法访问工作树 Git 元数据目录")?;
        let journal_dir = git_dir.join("picket").join("commits");
        let record_path = journal_dir.join(format!("{}.json", request.request_id));
        if record_path.exists() {
            let previous: ExecutionRecord = serde_json::from_slice(
                &fs::read(&record_path).map_err(|_| "无法读取提交执行记录")?,
            )
            .map_err(|_| "提交执行记录格式损坏，请先在 Git 中核对实际历史")?;
            if previous.repository_id != request.target.repository_id
                || previous.worktree_id != request.target.worktree_id
                || previous.selections != request.selections
                || previous.raw_message != request.message
            {
                return Err(
                    "提交请求身份已被其他范围或说明占用，不会重复写入；请刷新核对 Git 状态".into(),
                );
            }
            let commit = previous.commit.clone().ok_or(
                "检测到上次提交写入中断，请先刷新并在 Git / IDE 中核对实际状态；不会自动重复提交",
            )?;
            self.verify_recorded_commit(&root, &previous, &commit)
                .await?;
            let snapshot = self.read_repository_locked(&root).await?;
            return Ok((snapshot, commit));
        }
        if !self
            .inner
            .commit_requests
            .lock()
            .map_err(|_| "提交请求记录不可用")?
            .insert(request.request_id.clone())
        {
            return Err(
                "此提交请求已接收且未完成，不会重复执行；请刷新核对实际状态后重新发起".into(),
            );
        }

        let commit_state = registered
            .commit_states
            .get(&request.target.worktree_id)
            .ok_or("当前工作树缺少有效提交授权，请刷新后重新确认范围；草稿与选择已保留")?;
        if commit_state.detached {
            return Err(
                "当前处于分离 HEAD，请先在顶部新建或切换开发分支后再提交；草稿与选择已保留".into(),
            );
        }
        if is_protected_branch(&commit_state.branch) {
            return Err(
                "当前为保护分支（main / master），请先在顶部新建或切换开发分支后再提交；草稿与选择已保留"
                    .into(),
            );
        }
        let mut selected_files = Vec::with_capacity(request.selections.len());
        for selection in &request.selections {
            let Some(authorized) = commit_state.files.get(&selection.file_id) else {
                return Err(
                    "所选提交范围已过期或不属于当前工作树，请刷新并重新确认；草稿与选择已保留"
                        .into(),
                );
            };
            if authorized.token != selection.commit_token {
                return Err(
                    "所选提交范围凭证已过期或无效，不能使用旧范围写入；请刷新并重新确认，草稿与选择已保留"
                        .into(),
                );
            }
            if authorized.conflicted {
                return Err(
                    "所选范围包含未解决冲突文件，请先在 Git / IDE 中解决冲突；草稿与选择已保留"
                        .into(),
                );
            }
            ensure_safe_repo_path(&authorized.path)?;
            if let Some(previous) = &authorized.previous_path {
                ensure_safe_repo_path(previous)?;
            }
            selected_files.push(authorized.clone());
        }

        self.ensure_commit_write_allowed(&root).await?;
        let unstaged_paths: Vec<String> = selected_files
            .iter()
            .filter(|file| !file.staged)
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.ensure_commit_filters_allowed(&root, &unstaged_paths)
            .await?;

        let initial_live_state = self.branch_state(&root).await?;
        self.verify_live_selected_scope(commit_state, &selected_files, &initial_live_state)?;

        let worktree_reader = reader.for_worktree(&root).await?;
        let temp = tempfile::tempdir().map_err(|_| "无法创建受控提交临时目录")?;
        let selected_index = temp.path().join("selected.index");
        if let Some(parent) = commit_state.head.as_deref() {
            run_index_checked(
                &worktree_reader,
                &root,
                &["read-tree", parent],
                &selected_index,
                None,
                "初始化待提交索引失败",
            )
            .await?;
        } else {
            run_index_checked(
                &worktree_reader,
                &root,
                &["read-tree", "--empty"],
                &selected_index,
                None,
                "初始化首次提交索引失败",
            )
            .await?;
        }
        for batch in unstaged_paths.chunks(128) {
            let mut args = vec!["add", "--all", "--"];
            args.extend(batch.iter().map(String::as_str));
            run_index_checked(
                &worktree_reader,
                &root,
                &args,
                &selected_index,
                None,
                "构建所选未暂存范围失败",
            )
            .await?;
        }
        let mut staged_unit_paths = BTreeSet::new();
        let mut staged_index_info = Vec::new();
        let mut staged_by_unit_path = BTreeMap::<String, Vec<u8>>::new();
        let initial_stage_map = stage_entries_by_path(&initial_live_state.index)?;
        for file in selected_files.iter().filter(|file| file.staged) {
            for path in file.unit_paths() {
                if let Some(records) = initial_stage_map.get(&path) {
                    staged_by_unit_path.insert(path.clone(), records.clone());
                }
                staged_unit_paths.insert(path);
            }
        }
        if !staged_unit_paths.is_empty() {
            let mut remove_stdin = Vec::new();
            for path in &staged_unit_paths {
                remove_stdin.extend_from_slice(path.as_bytes());
                remove_stdin.push(0);
            }
            run_index_checked(
                &worktree_reader,
                &root,
                &["update-index", "--force-remove", "-z", "--stdin"],
                &selected_index,
                Some(&remove_stdin),
                "移除临时索引旧路径失败",
            )
            .await?;
            for records in staged_by_unit_path.values() {
                staged_index_info.extend_from_slice(records);
            }
            if !staged_index_info.is_empty() {
                run_index_checked(
                    &worktree_reader,
                    &root,
                    &["update-index", "-z", "--index-info"],
                    &selected_index,
                    Some(&staged_index_info),
                    "写入所选已暂存条目失败",
                )
                .await?;
            }
        }
        let selected_stage = run_index_checked(
            &worktree_reader,
            &root,
            &["ls-files", "--stage", "-z"],
            &selected_index,
            None,
            "核对待提交索引失败",
        )
        .await?;
        if selected_stage
            .split(|byte| *byte == 0)
            .any(|entry| entry.starts_with(b"160000 "))
        {
            return Err("所选内容包含子模块条目，当前版本尚不支持提交；草稿与选择已保留".into());
        }
        let tree = write_tree(&worktree_reader, &root, &selected_index).await?;
        if let Some(parent) = commit_state.head.as_deref() {
            let base_tree = reader
                .text(&root, &["rev-parse", &format!("{parent}^{{tree}}")])
                .await?;
            if tree == base_tree {
                return Err(
                    "所选范围相对当前分支没有实际变更，未创建空提交；草稿与选择已保留".into(),
                );
            }
        } else if selected_stage.is_empty() {
            return Err("所选范围没有可写入首次提交的文件，未创建空提交；草稿与选择已保留".into());
        }

        let index_path = PathBuf::from(
            reader
                .text(
                    &root,
                    &["rev-parse", "--path-format=absolute", "--git-path", "index"],
                )
                .await?,
        );
        self.ensure_commit_write_allowed(&root).await?;
        let mut index_lock = IndexLock::acquire(&index_path)?;
        let pre_hook_state = self.branch_state(&root).await?;
        let pre_stage_map =
            self.verify_live_selected_scope(commit_state, &selected_files, &pre_hook_state)?;

        let pre_commit = reader
            .run_hook(
                &root,
                &["hook", "run", "--ignore-missing", "pre-commit"],
                Some(&selected_index),
            )
            .await?;
        if !pre_commit.success {
            return self
                .hook_failure_error(&root, "pre-commit", &pre_commit, &pre_hook_state)
                .await;
        }
        let cleaned = reader
            .write_with(
                &root,
                &["stripspace"],
                None,
                Some(request.message.as_bytes()),
            )
            .await?;
        if !cleaned.success {
            return Err("规范化提交说明失败，未创建提交；当前草稿与选择已保留".into());
        }
        let msg_path = temp.path().join("COMMIT_EDITMSG");
        fs::write(&msg_path, &cleaned.bytes).map_err(|_| "无法准备提交说明临时文件")?;
        let msg_arg = display(&msg_path)?;
        let prepare_msg = reader
            .run_hook(
                &root,
                &[
                    "hook",
                    "run",
                    "--ignore-missing",
                    "prepare-commit-msg",
                    "--",
                    &msg_arg,
                    "message",
                ],
                Some(&selected_index),
            )
            .await?;
        if !prepare_msg.success {
            return self
                .hook_failure_error(&root, "prepare-commit-msg", &prepare_msg, &pre_hook_state)
                .await;
        }
        let commit_msg = reader
            .run_hook(
                &root,
                &[
                    "hook",
                    "run",
                    "--ignore-missing",
                    "commit-msg",
                    "--",
                    &msg_arg,
                ],
                Some(&selected_index),
            )
            .await?;
        if !commit_msg.success {
            return self
                .hook_failure_error(&root, "commit-msg", &commit_msg, &pre_hook_state)
                .await;
        }
        let hook_msg_bytes = fs::read(&msg_path).map_err(|_| "无法读取钩子校验后的提交说明")?;
        let final_cleaned = reader
            .write_with(&root, &["stripspace"], None, Some(&hook_msg_bytes))
            .await?;
        if !final_cleaned.success || parse::utf8(&final_cleaned.bytes)?.trim().is_empty() {
            return Err("提交说明经钩子处理后为空，未创建提交；当前草稿与选择已保留".into());
        }
        fs::write(&msg_path, &final_cleaned.bytes).map_err(|_| "无法更新提交说明临时文件")?;

        let post_hook_tree = write_tree(&worktree_reader, &root, &selected_index).await?;
        let post_hook_state = self.branch_state(&root).await?;
        if post_hook_tree != tree
            || post_hook_state.status != pre_hook_state.status
            || post_hook_state.index != pre_hook_state.index
            || post_hook_state.working != pre_hook_state.working
        {
            return Err(
                "Git 钩子修改了待提交或工作区内容，不能继续沿用旧范围；未创建提交，当前草稿与选择已保留，请核对更新后的变更后重试。"
                    .into(),
            );
        }
        let head_lock = reader
            .text(
                &root,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "HEAD.lock",
                ],
            )
            .await?;
        if fs::symlink_metadata(&head_lock).is_ok() {
            return Err("存在 Git 锁文件（HEAD.lock），未执行提交；当前草稿与选择已保留".into());
        }
        let symbolic_head = reader
            .write(&root, &["symbolic-ref", "--quiet", "HEAD"])
            .await?;
        let expected_ref = format!("refs/heads/{}", commit_state.branch);
        if !symbolic_head.success || parse::utf8(&symbolic_head.bytes)?.trim() != expected_ref {
            return Err(
                "当前分支引用已在外部变化，未执行提交；请刷新后重试，草稿与选择已保留".into(),
            );
        }

        let all_unit_paths_set: BTreeSet<String> = selected_files
            .iter()
            .flat_map(AuthorizedFile::unit_paths)
            .collect();
        let all_unit_paths: Vec<String> = all_unit_paths_set.iter().cloned().collect();
        fs::create_dir_all(&journal_dir).map_err(|_| "无法初始化提交协调记录目录")?;
        let mut record = ExecutionRecord {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            parent: commit_state.head.clone(),
            branch: commit_state.branch.clone(),
            tree: tree.clone(),
            paths: all_unit_paths.clone(),
            selections: request.selections.clone(),
            raw_message: request.message.clone(),
            commit: None,
        };
        let mut record_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&record_path)
            .map_err(|_| "提交执行记录已存在，不会重复提交；请刷新核对实际状态")?;
        record_file
            .write_all(&serde_json::to_vec(&record).map_err(|error| error.to_string())?)
            .map_err(|_| "无法写入提交执行记录")?;
        record_file.sync_all().map_err(|_| "无法同步提交执行记录")?;
        drop(record_file);

        let sign_config = reader
            .write(&root, &["config", "--type=bool", "--get", "commit.gpgSign"])
            .await?;
        let gpg_sign = sign_config.success && parse::utf8(&sign_config.bytes)?.trim() == "true";
        let mut commit_args = vec!["commit-tree", tree.as_str()];
        if let Some(parent) = commit_state.head.as_deref() {
            commit_args.extend(["-p", parent]);
        }
        if gpg_sign {
            commit_args.push("-S");
        }
        commit_args.extend(["-F", &msg_arg]);
        let created = match reader.write(&root, &commit_args).await {
            Ok(output) => output,
            Err(error) => {
                let _ = fs::remove_file(&record_path);
                return Err(error);
            }
        };
        if !created.success {
            let _ = fs::remove_file(&record_path);
            let detail = output_detail(&created);
            let lower = detail.to_ascii_lowercase();
            if lower.contains("ident")
                || lower.contains("user.name")
                || lower.contains("user.email")
                || lower.contains("email address")
            {
                return Err(format!(
                    "Git 尚未配置提交者身份（user.name / user.email）：{detail}。请先在 Git 中配置后再提交；未创建提交，草稿与选择已保留。"
                ));
            }
            return Err(format!(
                "创建提交对象未成功：{detail}。未修改当前分支，草稿与选择已保留。"
            ));
        }
        let commit_oid = parse::utf8(&created.bytes)?.trim().to_owned();
        if commit_oid.is_empty()
            || reader
                .text(&root, &["rev-parse", &format!("{commit_oid}^{{tree}}")])
                .await?
                != tree
        {
            let _ = fs::remove_file(&record_path);
            return Err(
                "生成的提交对象内容树与授权范围不符，已中止更新分支；草稿与选择已保留".into(),
            );
        }
        let short_oid = reader
            .text(&root, &["rev-parse", "--short", &commit_oid])
            .await?;
        let title = reader
            .text(&root, &["show", "-s", "--format=%s", &commit_oid])
            .await?;
        let reflog = if commit_state.head.is_some() {
            format!("commit: {title}")
        } else {
            format!("commit (initial): {title}")
        };
        let expected_old = commit_state.head.as_deref().unwrap_or("");
        let updated = reader
            .write(
                &root,
                &[
                    "update-ref",
                    "-m",
                    &reflog,
                    "HEAD",
                    &commit_oid,
                    expected_old,
                ],
            )
            .await?;
        if !updated.success {
            let current_head = reader
                .run(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])
                .await?
                .bytes;
            let unchanged = match commit_state.head.as_deref() {
                Some(parent) => parse::utf8(&current_head)?.trim() == parent,
                None => current_head.is_empty(),
            };
            if unchanged {
                let _ = fs::remove_file(&record_path);
            }
            return Err(format!(
                "更新分支引用未完成：{}。请刷新核对实际状态，不要盲目重复提交；草稿与选择已保留。",
                output_detail(&updated)
            ));
        }

        let commit_result = CommitResult {
            oid: commit_oid.clone(),
            short_oid: short_oid.clone(),
            title,
            branch: commit_state.branch.clone(),
            parent: commit_state.head.clone(),
        };
        let reconcile = async {
            let remaining_index = temp.path().join("remaining.index");
            if index_path.exists() {
                fs::copy(&index_path, &remaining_index).map_err(|_| "无法复制当前暂存区索引")?;
            } else {
                run_index_checked(
                    &worktree_reader,
                    &root,
                    &["read-tree", "--empty"],
                    &remaining_index,
                    None,
                    "初始化剩余暂存区索引失败",
                )
                .await?;
            }
            for batch in all_unit_paths.chunks(128) {
                let mut reset_args = vec!["reset", "--quiet", commit_oid.as_str(), "--"];
                reset_args.extend(batch.iter().map(String::as_str));
                run_index_checked(
                    &worktree_reader,
                    &root,
                    &reset_args,
                    &remaining_index,
                    None,
                    "同步已提交路径到暂存区失败",
                )
                .await?;
            }
            let remaining_bytes =
                fs::read(&remaining_index).map_err(|_| "无法读取更新后的暂存区索引")?;
            index_lock.replace(&index_path, &remaining_bytes)?;
            record.commit = Some(commit_result.clone());
            let mut completed =
                NamedTempFile::new_in(&journal_dir).map_err(|_| "无法创建提交完成记录")?;
            completed
                .write_all(&serde_json::to_vec(&record).map_err(|error| error.to_string())?)
                .map_err(|_| "无法写入提交完成记录")?;
            completed
                .as_file()
                .sync_all()
                .map_err(|_| "无法同步提交完成记录")?;
            completed
                .persist(&record_path)
                .map_err(|_| "无法保存提交完成记录")?;
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = reconcile {
            return Err(format!(
                "本地提交 {short_oid} 已写入分支历史，但同步暂存区或记录未完成：{error}。请刷新并在 Git / IDE 中核对，不要重复提交。"
            ));
        }

        if reader.text(&root, &["rev-parse", "HEAD"]).await? != commit_oid
            || reader.text(&root, &["rev-parse", "HEAD^{tree}"]).await? != tree
        {
            return Err(format!(
                "提交 {short_oid} 已存在，但分支指向或内容树与预期不符；已停止后续动作，请在 Git 中核对。"
            ));
        }
        for (path, expected_hash) in &pre_hook_state.working {
            if &self.working_file(&root, path)? != expected_hash {
                return Err(format!(
                    "提交 {short_oid} 已完成，但检测到工作区文件 {path} 发生变化；已停止后续动作，请刷新并在 IDE 中核对。"
                ));
            }
        }
        let after_stage_map =
            stage_entries_by_path(&reader.bytes(&root, &["ls-files", "--stage", "-z"]).await?)?;
        let pre_unselected: BTreeMap<_, _> = pre_stage_map
            .iter()
            .filter(|(path, _)| !all_unit_paths_set.contains(*path))
            .collect();
        let after_unselected: BTreeMap<_, _> = after_stage_map
            .iter()
            .filter(|(path, _)| !all_unit_paths_set.contains(*path))
            .collect();
        if pre_unselected != after_unselected {
            return Err(format!(
                "提交 {short_oid} 已完成，但未选文件的暂存状态与预期不符；请刷新并在 Git / IDE 中核对。"
            ));
        }

        let _ = reader
            .run_hook(
                &root,
                &["hook", "run", "--ignore-missing", "post-commit"],
                None,
            )
            .await;

        let snapshot = self.read_repository_locked(&root).await.map_err(|error| {
            format!(
                "本地提交 {short_oid} 已写入，但读取最新状态失败：{error}。请手动刷新核对，不要重复提交。"
            )
        })?;
        Ok((snapshot, commit_result))
    }
}
