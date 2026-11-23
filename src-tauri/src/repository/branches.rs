//! Branch writes are separate from commit authorization and never stage, stash,
//! reset, force checkout, fetch, or retry a partially completed operation.
use super::*;
use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};

pub(super) struct BranchState {
    pub status: Vec<u8>,
    pub branch: String,
    pub head: Option<String>,
    pub index: Vec<u8>,
    pub working: Vec<(String, String)>,
}
impl BranchState {
    pub fn token(&self) -> Result<String> {
        Ok(hash(
            serde_json::to_vec(&(&self.status, &self.index, &self.working))
                .map_err(|error| error.to_string())?,
        ))
    }
    fn preserves_changes(&self, next: &Self) -> bool {
        self.index == next.index && self.working == next.working
    }
}

impl RepositoryStore {
    pub(super) async fn branch_state(&self, root: &Path) -> Result<BranchState> {
        let reader = self.inner.reader.for_worktree(root).await?;
        let args = [
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ];
        let status = reader.bytes(root, &args).await?;
        let parsed = parse::status(&status)?;
        let index = reader.bytes(root, &["ls-files", "--stage", "-z"]).await?;
        let paths: BTreeSet<_> = parsed
            .entries
            .iter()
            .flat_map(|entry| std::iter::once(entry.path.clone()).chain(entry.previous.clone()))
            .collect();
        let mut working = Vec::new();
        for path in paths {
            working.push((path.clone(), self.working_file(root, &path)?));
        }
        if reader.bytes(root, &args).await? != status {
            return Err("Git 状态正在变化，请刷新后重试；草稿与选择已保留".into());
        }
        Ok(BranchState {
            status,
            index,
            working,
            branch: parsed.branch,
            head: parsed.head,
        })
    }

    async fn registered_branch_path(&self, request: &BranchRequest) -> Result<PathBuf> {
        let registered = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "仓库注册表不可用")?
            .get(&request.target.repository_id)
            .cloned()
            .ok_or("仓库未在当前窗口打开")?;
        if registered.branch_tokens.get(&request.target.worktree_id) != Some(&request.branch_token)
        {
            return Err("分支操作依据已过期，请刷新后重试；草稿与选择已保留".into());
        }
        let root = registered
            .paths
            .get(&request.target.worktree_id)
            .cloned()
            .ok_or("工作树未在当前窗口打开")?;
        if fs::canonicalize(&root).map_err(|_| "工作树目录不可访问")? != root {
            return Err("工作树路径已变化，请重新打开目录".into());
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
        if identity("repo", &common) != request.target.repository_id
            || fs::canonicalize(toplevel).map_err(|_| "Git 工作目录不可访问")? != root
        {
            return Err("工作树所属仓库已变化，请重新打开；草稿与选择已保留".into());
        }
        Ok(root)
    }

    async fn validate_branch_name(&self, root: &Path, branch: &str) -> Result<()> {
        if branch.is_empty()
            || branch.starts_with('-')
            || branch.trim() != branch
            || branch.chars().any(char::is_control)
        {
            return Err("分支名无效，请使用 Git 接受的完整本地分支名".into());
        }
        let checked = self
            .inner
            .reader
            .run(root, &["check-ref-format", "--branch", branch])
            .await?;
        if !checked.success || checked.bytes != format!("{branch}\n").as_bytes() {
            return Err("分支名无效，请检查空格、特殊字符或路径格式".into());
        }
        Ok(())
    }

    async fn ensure_branch_write_allowed(&self, root: &Path) -> Result<()> {
        let reader = &self.inner.reader;
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "BISECT_LOG",
            "index.lock",
            "HEAD.lock",
        ] {
            let path = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .await?;
            if fs::symlink_metadata(&path).is_ok() {
                return Err(format!(
                    "存在 Git 进行中操作或锁（{marker}），请先在 Git / IDE 中处理；未执行分支写入"
                ));
            }
        }
        if !reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await?
            .is_empty()
        {
            return Err("存在未解决冲突，请先在 Git / IDE 中处理；未执行分支写入".into());
        }
        let flags = reader.bytes(root, &["ls-files", "-v", "-z"]).await?;
        if flags
            .split(|byte| *byte == 0)
            .any(|entry| !entry.is_empty() && entry[0] != b'H')
        {
            return Err(
                "当前暂存区含稀疏检出或隐藏变更标记，暂不执行分支写入；请先在 Git / IDE 中处理"
                    .into(),
            );
        }
        if reader
            .bytes(root, &["ls-files", "--stage", "-z"])
            .await?
            .split(|byte| *byte == 0)
            .any(|entry| entry.starts_with(b"160000 "))
        {
            return Err("含子模块的仓库尚未通过分支写入验证，请在 Git / IDE 中操作".into());
        }
        // Query without the read runner's hooksPath override. The write runner
        // preserves this configuration too; executable hooks are refused, not skipped.
        let hooks = reader
            .write(
                root,
                &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
            )
            .await?;
        if !hooks.success {
            return Err("无法核对 Git hooks 目录，未执行分支写入".into());
        }
        let configured_hooks = reader
            .write(root, &["config", "--path", "--get", "core.hooksPath"])
            .await?;
        if configured_hooks.success {
            // A custom path can become executable only after checkout (for
            // example .husky). Checking the source directory alone is unsafe.
            return Err("仓库配置了自定义 hooksPath，当前版本尚未验证其安全检出，未执行分支写入；请在 Git / IDE 中操作".into());
        } else if configured_hooks.code != Some(1) {
            return Err("无法核对 Git hooksPath，未执行分支写入".into());
        }
        let hooks = parse::utf8(&hooks.bytes)?;
        let hooks = PathBuf::from(hooks.strip_suffix('\n').unwrap_or(&hooks));
        match fs::read_dir(&hooks) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry.map_err(|_| "无法检查 Git hook")?.path();
                    if path.extension().is_some_and(|ext| ext == "sample") {
                        continue;
                    }
                    let metadata =
                        fs::metadata(&path).map_err(|_| "无法检查 Git hook，未执行分支写入")?;
                    if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                        return Err("检测到可执行 Git hook，当前版本不支持安全执行，未创建或切换分支；请在 Git / IDE 中操作".into());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("无法检查 Git hooks 目录，未执行分支写入".into()),
        }
        Ok(())
    }

    async fn ensure_checkout_filters_allowed(&self, root: &Path, target_head: &str) -> Result<()> {
        let reader = &self.inner.reader;
        let filters = reader
            .run(
                root,
                &[
                    "config",
                    "--null",
                    "--name-only",
                    "--get-regexp",
                    r"^filter\..*\.(clean|smudge|process)$",
                    ".+",
                ],
            )
            .await?;
        if filters.code == Some(1) {
            return Ok(());
        }
        if !filters.success {
            return Err("无法检查 Git 文件过滤器，未执行分支写入".into());
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
        // Check working attributes, index attributes and target-tree attributes.
        // Merely installing Git LFS globally must not disable unrelated repos.
        let mut paths = reader
            .bytes(
                root,
                &[
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ],
            )
            .await?;
        paths.extend(
            reader
                .bytes(root, &["ls-tree", "-r", "--name-only", "-z", target_head])
                .await?,
        );
        let paths: Vec<_> = paths
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(parse::utf8)
            .collect::<Result<BTreeSet<_>>>()?
            .into_iter()
            .collect();
        for source in [
            None,
            Some("--cached".to_owned()),
            Some(format!("--source={target_head}")),
        ] {
            // Keep argument batches below macOS's command-line size limit.
            for batch in paths.chunks(128) {
                let mut args = vec!["check-attr", "-z"];
                if let Some(source) = &source {
                    args.push(source);
                }
                args.extend(["filter", "--"]);
                args.extend(batch.iter().map(String::as_str));
                let attributes = reader.bytes(root, &args).await?;
                let fields: Vec<_> = attributes.split(|byte| *byte == 0).collect();
                for record in fields.chunks(3).filter(|record| record.len() == 3) {
                    if drivers.contains(&parse::utf8(record[2])?) {
                        return Err("当前或目标分支的文件需要外部 Git 过滤器，当前版本不能保证安全检出，请在 Git / IDE 中操作".into());
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn change_branch(&self, request: &BranchRequest) -> Result<RepositorySnapshot> {
        validate_request_id(&request.request_id)?;
        if !self
            .inner
            .branch_requests
            .lock()
            .map_err(|_| "分支请求记录不可用")?
            .insert(request.request_id.clone())
        {
            return Err("此分支请求已经接收，不会重复执行；请刷新核对 Git 实际状态".into());
        }
        let _guard = self
            .repository_gate(&request.target.repository_id)?
            .lock_owned()
            .await;
        let root = self.registered_branch_path(request).await?;
        self.validate_branch_name(&root, &request.branch).await?;
        self.ensure_branch_write_allowed(&root).await?;
        let before = self.branch_state(&root).await?;
        if before.token()? != request.branch_token {
            return Err(
                "分支、HEAD、暂存区或工作区内容已变化，请刷新后重试；草稿与选择已保留".into(),
            );
        }
        let head = before
            .head
            .as_deref()
            .ok_or("仓库尚无首次提交，请先在 Git / IDE 中完成首次提交后再切换分支")?;
        let reader = &self.inner.reader;
        let entries = parse::worktrees(
            &reader
                .bytes(&root, &["worktree", "list", "--porcelain", "-z"])
                .await?,
        )?;
        let BranchAction::SwitchBranch = request.action;
        if let Some(entry) = entries.iter().find(|entry| entry.branch == request.branch) {
            let destination = fs::canonicalize(&entry.path)
                .map_err(|_| "分支占用的工作树不可访问，请先修复该工作树")?;
            // An occupied branch is a navigation result, never a forced checkout.
            return self.read_repository_locked(&destination).await;
        }
        let checkout_head = reader
            .text(
                &root,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("refs/heads/{}^{{commit}}", request.branch),
                ],
            )
            .await
            .map_err(|_| "所选本地分支已不存在，请刷新分支列表")?;
        if !before.working.is_empty() && checkout_head != head {
            return Err("当前有暂存或未暂存改动，不能安全切换到不同提交；请先在 Git / IDE 中处理当前改动后再切换分支".into());
        }
        // Recheck after all validation and any slow filesystem inspection.
        self.ensure_branch_write_allowed(&root).await?;
        self.ensure_checkout_filters_allowed(&root, &checkout_head)
            .await?;
        if self.branch_state(&root).await?.token()? != request.branch_token {
            return Err("Git 内容在校验期间变化，请刷新后重新操作；未执行写入".into());
        }
        let writer = reader.for_checkout(&root).await?;
        let output = writer
            .write(
                &root,
                &[
                    "switch",
                    "--no-guess",
                    "--no-overwrite-ignore",
                    &request.branch,
                ],
            )
            .await
            .map_err(|error| format!("{error}。请刷新核对实际分支状态；不会自动重试或回滚。"))?;
        if !output.success {
            return Err(format!(
                "Git 操作未完成：{}。请刷新核对实际状态；不会自动重试或回滚。",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let after = self.branch_state(&root).await?;
        if !before.working.is_empty() && !before.preserves_changes(&after) {
            return Err("Git 操作后暂存区或工作草稿与预期不符，已停止后续动作；请刷新并在 Git / IDE 中核对，不会自动回滚".into());
        }
        if after.branch != request.branch || after.head.as_deref() != Some(checkout_head.as_str()) {
            return Err("Git 操作后的分支或 HEAD 与预期不符，请刷新核对；不会自动重试".into());
        }
        self.read_repository_locked(&root).await.map_err(|error| {
            format!("Git 已切换分支，但读取结果失败：{error}。请刷新核对实际状态。")
        })
    }
}
