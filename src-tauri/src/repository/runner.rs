use crate::i18n::{self, LanguageKind};
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
    locale: LanguageKind,
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
            locale: LanguageKind::En,
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
    pub fn with_locale(&self, locale: LanguageKind) -> Self {
        let mut cloned = self.clone();
        cloned.locale = locale;
        cloned
    }
    pub fn cancelled(&self) -> bool {
        *self.inner.cancelled.borrow()
    }
    pub fn has_blocked_filters(&self) -> bool {
        !self.filter_settings.is_empty()
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
            return Err("Cannot inspect Git filter configuration; stopped reading".into());
        }
        let mut drivers = std::collections::BTreeSet::new();
        for key in output
            .bytes
            .split(|byte| *byte == 0)
            .filter(|key| !key.is_empty())
        {
            let key =
                std::str::from_utf8(key).map_err(|_| "Git filter driver name is not UTF-8")?;
            let (driver, _) = key
                .rsplit_once('.')
                .ok_or("Cannot parse Git filter driver name")?;
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
    pub async fn run_with_input(
        &self,
        root: &Path,
        args: &[&str],
        input: &[u8],
    ) -> Result<GitOutput, String> {
        self.execute(root, args, ExecutionMode::Read, None, Some(input))
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
        let locale = self.locale;
        self.inner.active.fetch_add(1, Ordering::SeqCst);
        let _active = Active(self.inner.clone());
        let mut cancelled = self.inner.cancelled.subscribe();
        if *cancelled.borrow() {
            return Err(i18n::repository::repo_shutting_down_error(locale).into());
        }
        let mut command = Command::new("/usr/bin/git");
        command
            .current_dir(root)
            .args([
                "--no-pager",
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
        // Preserve the user's local Git configuration (and `GIT_SSH*` on remote operations),
        // but never inherit an injected repository/index or caller Git config overrides.
        crate::git_env::strip_git_overrides(&mut command, mode == ExecutionMode::Remote);
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
        // Use `GIT_NO_LAZY_FETCH=1` instead of the Git 2.45+ `--no-lazy-fetch` CLI flag so
        // Apple Git 2.39.x on macOS 14/15 never aborts with exit code 129 (`unknown option`),
        // while `GIT_ALLOW_PROTOCOL=""` also blocks promisor fetches on older Git releases.
        command
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_ALLOW_PROTOCOL", allow_protocol)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", ":");
        command.args(args);
        let mut child = command.spawn().map_err(|_| {
            "Cannot start /usr/bin/git; verify that Xcode Command Line Tools are installed"
                .to_string()
        })?;
        let writer = if let Some(input) = input {
            let mut stdin = child.stdin.take().ok_or("Git stdin is unavailable")?;
            let payload = input.to_vec();
            Some(tokio::spawn(async move {
                stdin.write_all(&payload).await?;
                stdin.shutdown().await
            }))
        } else {
            None
        };
        let stdout = child.stdout.take().ok_or("Git stdout is unavailable")?;
        let stderr = child.stderr.take().ok_or("Git stderr is unavailable")?;
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
            status = child.wait() => status.map_err(|_| "Git process exited unexpectedly; refresh to verify live status".to_string()),
            _ = cancelled.changed() => { let _ = child.kill().await; Err(i18n::repository::repo_shutting_down_error(locale).into()) },
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => { let _ = child.kill().await; Err("Git command timed out; refresh to verify live status (writes are never retried automatically)".into()) },
        };
        let stdin_result = match writer {
            Some(writer) => Some(writer.await.map_err(|_| "Cannot join Git stdin writer")?),
            None => None,
        };
        let bytes = output
            .await
            .map_err(|_| "Cannot join Git stdout task")?
            .map_err(|_| "Cannot read Git stdout")?;
        let stderr = errors
            .await
            .map_err(|_| "Cannot join Git stderr task")?
            .map_err(|_| "Cannot read Git stderr")?;
        let status = result?;
        if status.success()
            && let Some(Err(_)) = stdin_result
        {
            return Err("Failed to write complete Git stdin; operation did not complete".into());
        }
        if bytes.len() as u64 > OUTPUT_LIMIT || stderr.len() as u64 > OUTPUT_LIMIT {
            return Err(
                "Repository status output is too large; incomplete results were not loaded".into(),
            );
        }
        if !status.success() && stderr.windows(12).any(|part| part == b"clean filter") {
            return Err(if mode == ExecutionMode::Read {
                i18n::repository::runner_external_filter_read_error(locale).into()
            } else {
                i18n::repository::runner_external_filter_write_error(locale).into()
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
                "Git {} read failed; check that the repository is accessible (draft and selection were preserved)",
                args.first().unwrap_or(&"status")
            ));
        }
        Ok(output.bytes)
    }
    pub async fn text(&self, root: &Path, args: &[&str]) -> Result<String, String> {
        let bytes = self.bytes(root, args).await?;
        let mut text = String::from_utf8(bytes)
            .map_err(|_| i18n::repository::parse_non_utf8_path_error(self.locale).to_string())?;
        if text.ends_with('\n') {
            text.pop();
        }
        Ok(text)
    }
}
