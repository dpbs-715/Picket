use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{Notify, watch},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecutionMode {
    Read,
    Write,
    Hook,
    Remote,
}
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
#[derive(Clone)]
pub struct GitReader {
    inner: Arc<Inner>,
    filter_settings: Vec<(String, &'static str)>,
}
struct Inner {
    cancelled: watch::Sender<bool>,
    active: AtomicUsize,
    finished: Notify,
}
impl Default for GitReader {
    fn default() -> Self {
        Self {
            filter_settings: Vec::new(),
            inner: Arc::new(Inner {
                cancelled: watch::channel(false).0,
                active: AtomicUsize::new(0),
                finished: Notify::new(),
            }),
        }
    }
}
struct Active(Arc<Inner>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
        self.0.finished.notify_waiters();
    }
}
pub struct GitOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub bytes: Vec<u8>,
    pub stderr: Vec<u8>,
}
impl GitReader {
    pub fn cancelled(&self) -> bool {
        *self.inner.cancelled.borrow()
    }
    pub async fn shutdown(&self) {
        self.inner.cancelled.send_replace(true);
        loop {
            let notified = self.inner.finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.inner.active.load(Ordering::SeqCst) == 0 {
                return;
            }
            notified.await;
        }
    }
    // Preserve Git's attribute handling. If a conversion would actually need
    // an external cleaner, Git must fail instead of diffing unconverted bytes.
    pub async fn for_worktree(&self, root: &Path) -> Result<Self, String> {
        self.block_filters(root, false).await
    }
    pub async fn for_checkout(&self, root: &Path) -> Result<Self, String> {
        self.block_filters(root, true).await
    }
    async fn block_filters(&self, root: &Path, checkout: bool) -> Result<Self, String> {
        let pattern = if checkout {
            r"^filter\..*\.(clean|smudge|process)$"
        } else {
            r"^filter\..*\.(clean|process)$"
        };
        let output = self
            .run(
                root,
                &[
                    "config",
                    "--null",
                    "--name-only",
                    "--get-regexp",
                    pattern,
                    ".+",
                ],
            )
            .await?;
        if output.code == Some(1) {
            return Ok(self.clone());
        }
        if !output.success {
            return Err("无法核对 Git 过滤器配置，已停止读取".into());
        }
        let mut drivers = std::collections::BTreeSet::new();
        for key in output
            .bytes
            .split(|byte| *byte == 0)
            .filter(|key| !key.is_empty())
        {
            let key = std::str::from_utf8(key).map_err(|_| "Git 过滤器名称不是 UTF-8")?;
            let (driver, _) = key.rsplit_once('.').ok_or("无法解析 Git 过滤器名称")?;
            drivers.insert(driver);
        }
        let mut reader = self.clone();
        for driver in drivers {
            reader.filter_settings.extend([
                (format!("{driver}.clean"), ""),
                (format!("{driver}.process"), ""),
                (format!("{driver}.required"), "true"),
            ]);
            if checkout {
                reader
                    .filter_settings
                    .push((format!("{driver}.smudge"), ""));
            }
        }
        Ok(reader)
    }
    pub async fn run(&self, root: &Path, args: &[&str]) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Read, None, None)
            .await
    }
    pub async fn run_with_index(
        &self,
        root: &Path,
        args: &[&str],
        index: &Path,
        input: Option<&[u8]>,
    ) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Read, Some(index), input)
            .await
    }
    // Only the branch and commit coordinators call write/hook runners after safety checks.
    // Unlike reads, writes and hooks must not override the user's hooksPath.
    pub async fn write(&self, root: &Path, args: &[&str]) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Write, None, None)
            .await
    }
    pub async fn write_with(
        &self,
        root: &Path,
        args: &[&str],
        index: Option<&Path>,
        input: Option<&[u8]>,
    ) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Write, index, input)
            .await
    }
    pub async fn run_hook(
        &self,
        root: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Hook, index, None)
            .await
    }
    pub async fn remote(&self, root: &Path, args: &[&str]) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Remote, None, None)
            .await
    }
    async fn execute(
        &self,
        root: &Path,
        args: &[&str],
        mode: ExecutionMode,
        index: Option<&Path>,
        input: Option<&[u8]>,
    ) -> Result<GitOutput, String> {
        self.inner.active.fetch_add(1, Ordering::SeqCst);
        let _active = Active(self.inner.clone());
        let mut cancelled = self.inner.cancelled.subscribe();
        if *cancelled.borrow() {
            return Err("应用正在退出，已停止 Git 读取".into());
        }
        let mut command = Command::new("/usr/bin/git");
        command
            .current_dir(root)
            .args([
                "--no-pager",
                "--no-lazy-fetch",
                "--no-optional-locks",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "color.ui=false",
                "-c",
                "status.renames=true",
            ])
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if mode == ExecutionMode::Read {
            command.args(["-c", "core.hooksPath=/dev/null"]);
        }
        // Preserve the user's local Git configuration, but never inherit an
        // injected repository/index or a caller's Git configuration overrides.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        if mode != ExecutionMode::Hook {
            command.env("GIT_CONFIG_COUNT", self.filter_settings.len().to_string());
            for (i, (key, value)) in self.filter_settings.iter().enumerate() {
                command.env(format!("GIT_CONFIG_KEY_{i}"), key);
                command.env(format!("GIT_CONFIG_VALUE_{i}"), value);
            }
            command.env("LC_ALL", "C").env("GIT_LITERAL_PATHSPECS", "1");
        }
        if let Some(index) = index {
            command.env("GIT_INDEX_FILE", index);
        }
        let allow_protocol = if mode == ExecutionMode::Remote {
            "file:https:http:ssh:git"
        } else {
            ""
        };
        let timeout_secs = if mode == ExecutionMode::Remote {
            30
        } else {
            15
        };
        command
            .env("GIT_ALLOW_PROTOCOL", allow_protocol)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", ":");
        command.args(args);
        let mut child = command.spawn().map_err(|_| {
            "无法启动 /usr/bin/git，请检查 Xcode Command Line Tools 是否可用".to_string()
        })?;
        let writer = if let Some(input) = input {
            let mut stdin = child.stdin.take().ok_or("Git 输入不可用")?;
            let payload = input.to_vec();
            Some(tokio::spawn(async move {
                stdin.write_all(&payload).await?;
                stdin.shutdown().await
            }))
        } else {
            None
        };
        let stdout = child.stdout.take().ok_or("Git 输出不可用")?;
        let stderr = child.stderr.take().ok_or("Git 错误输出不可用")?;
        let output = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout
                .take(OUTPUT_LIMIT + 1)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        let errors = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr
                .take(OUTPUT_LIMIT + 1)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        let result = tokio::select! {
            status = child.wait() => status.map_err(|_| "Git 进程异常退出，请刷新核对实际状态".to_string()),
            _ = cancelled.changed() => { let _ = child.kill().await; Err("应用正在退出，已停止 Git 读取".into()) },
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => { let _ = child.kill().await; Err("Git 操作超时，请刷新核对实际状态；不会自动重试写入".into()) },
        };
        let stdin_result = match writer {
            Some(writer) => Some(writer.await.map_err(|_| "无法回收 Git 输入任务")?),
            None => None,
        };
        let bytes = output
            .await
            .map_err(|_| "无法回收 Git 输出")?
            .map_err(|_| "无法读取 Git 输出")?;
        let stderr = errors
            .await
            .map_err(|_| "无法回收 Git 错误输出")?
            .map_err(|_| "无法读取 Git 错误输出")?;
        let status = result?;
        if status.success()
            && let Some(Err(_)) = stdin_result
        {
            return Err("无法完整写入 Git 标准输入，操作未完成".into());
        }
        if bytes.len() as u64 > OUTPUT_LIMIT || stderr.len() as u64 > OUTPUT_LIMIT {
            return Err("仓库状态输出过大，未载入不完整结果".into());
        }
        if !status.success() && stderr.windows(12).any(|part| part == b"clean filter") {
            return Err(if mode == ExecutionMode::Read {
                "文件需要 Git 外部过滤器，只读模式已停止读取；请在 Git 或 IDE 中查看".into()
            } else {
                "所选文件需要外部 Git 过滤器转换，当前版本不能保证安全写入，请在 Git / IDE 中操作；草稿与选择已保留".into()
            });
        }
        Ok(GitOutput {
            success: status.success(),
            code: status.code(),
            bytes,
            stderr,
        })
    }
    pub async fn bytes(&self, root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = self.run(root, args).await?;
        if !output.success {
            return Err(format!(
                "Git {} 读取失败，请检查仓库是否可访问；当前草稿与选择已保留",
                args.first().unwrap_or(&"状态")
            ));
        }
        Ok(output.bytes)
    }
    pub async fn text(&self, root: &Path, args: &[&str]) -> Result<String, String> {
        let bytes = self.bytes(root, args).await?;
        let mut text = String::from_utf8(bytes)
            .map_err(|_| "Git 返回了非 UTF-8 路径，当前版本不能安全显示该仓库".to_string())?;
        if text.ends_with('\n') {
            text.pop();
        }
        Ok(text)
    }
}
