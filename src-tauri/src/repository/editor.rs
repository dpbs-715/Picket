//! Controlled external IDE launcher for M2.
//! Resolves real worktree paths, exports `HEAD:<path>` / `:<path>` / conflict stages to
//! read-only temporary snapshots (never passing Git object refs directly to IDEs),
//! and invokes `idea`, `webstorm`, `code`, or `cursor` via argv without shell interpolation.
use super::*;
use std::{ffi::OsString, os::unix::fs::PermissionsExt, process::Stdio, time::Duration};
use tempfile::TempDir;
use tokio::{io::AsyncReadExt, process::Command};

#[derive(Clone, Default)]
pub struct EditorLauncher {
    inner: Arc<LauncherInner>,
}

#[derive(Default)]
struct LauncherInner {
    temp: Mutex<Option<TempDir>>,
    counter: AtomicU64,
    override_dirs: Mutex<Option<Vec<PathBuf>>>,
}

#[derive(Debug)]
pub struct OpenFileOutcome {
    pub file_path: String,
    pub action: OpenFileAction,
    pub line: Option<u32>,
}

fn editor_metadata(editor: EditorKind) -> (&'static str, &'static str, &'static [&'static str]) {
    match editor {
        EditorKind::Idea => (
            "idea",
            "IntelliJ IDEA",
            &[
                "IntelliJ IDEA.app/Contents/MacOS/idea",
                "IntelliJ IDEA CE.app/Contents/MacOS/idea",
            ],
        ),
        EditorKind::Rustrover => (
            "rustrover",
            "RustRover",
            &["RustRover.app/Contents/MacOS/rustrover"],
        ),
        EditorKind::Webstorm => (
            "webstorm",
            "WebStorm",
            &["WebStorm.app/Contents/MacOS/webstorm"],
        ),
        EditorKind::Vscode => (
            "code",
            "VS Code",
            &["Visual Studio Code.app/Contents/Resources/app/bin/code"],
        ),
        EditorKind::Cursor => (
            "cursor",
            "Cursor",
            &["Cursor.app/Contents/Resources/app/bin/cursor"],
        ),
    }
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

fn safe_snapshot_filename(repo_path: &str) -> String {
    let raw_name = Path::new(repo_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("snapshot");
    let cleaned: String = raw_name
        .chars()
        .map(|ch| {
            if ch == '/' || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "snapshot".into()
    } else {
        cleaned
    }
}

fn ensure_safe_repo_path(path: &str) -> Result<()> {
    parse::relative(path)?;
    if path
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err("不允许通过编辑器打开 .git 内部文件".into());
    }
    Ok(())
}

enum WorkingTarget {
    RealFile(PathBuf),
    SymlinkTarget(Vec<u8>),
    Missing,
}

fn inspect_working_target(root: &Path, relative_path: &str) -> Result<WorkingTarget> {
    ensure_safe_repo_path(relative_path)?;
    let full = root.join(relative_path);
    let metadata = match fs::symlink_metadata(&full) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorkingTarget::Missing);
        }
        Err(_) => return Err("无法读取工作树文件状态；当前草稿与选择已保留".into()),
    };
    let parent = fs::canonicalize(full.parent().ok_or("文件路径缺少父目录")?)
        .map_err(|_| "无法核对文件所在目录；当前草稿与选择已保留")?;
    if !parent.starts_with(root) {
        return Err("文件父目录越出当前工作树边界，已拒绝打开".into());
    }
    if metadata.file_type().is_symlink() {
        let link = fs::read_link(&full).map_err(|_| "无法读取符号链接目标")?;
        return Ok(WorkingTarget::SymlinkTarget(
            link.as_os_str().as_encoded_bytes().to_vec(),
        ));
    }
    if !metadata.is_file() {
        return Err("目标路径不是普通文件，无法在外部编辑器中打开；草稿与选择已保留".into());
    }
    Ok(WorkingTarget::RealFile(full))
}

impl EditorLauncher {
    #[cfg(test)]
    pub fn with_search_dirs(dirs: Vec<PathBuf>) -> Self {
        Self {
            inner: Arc::new(LauncherInner {
                temp: Mutex::new(None),
                counter: AtomicU64::new(0),
                override_dirs: Mutex::new(Some(dirs)),
            }),
        }
    }

    pub fn set_search_dirs(&self, dirs: Option<Vec<PathBuf>>) -> Result<()> {
        *self
            .inner
            .override_dirs
            .lock()
            .map_err(|_| "编辑器配置锁不可用")? = dirs;
        Ok(())
    }

    pub fn cleanup(&self) -> Result<()> {
        let temp = self
            .inner
            .temp
            .lock()
            .map_err(|_| "编辑器临时目录锁不可用")?
            .take();
        if let Some(directory) = temp {
            directory
                .close()
                .map_err(|error| format!("清理临时 Diff 快照失败：{error}"))?;
        }
        Ok(())
    }

    pub fn resolve_executable(&self, editor: EditorKind) -> Result<PathBuf> {
        let (cli, display_name, bundles) = editor_metadata(editor);
        let override_dirs = self
            .inner
            .override_dirs
            .lock()
            .map_err(|_| "编辑器查找目录锁不可用")?
            .clone();
        let candidates = if let Some(dirs) = override_dirs {
            let mut list = Vec::new();
            for dir in dirs {
                list.push(dir.join(cli));
                for bundle in bundles {
                    list.push(dir.join(bundle));
                }
            }
            list
        } else {
            let mut list = Vec::new();
            if let Some(path_env) = std::env::var_os("PATH") {
                list.extend(std::env::split_paths(&path_env).map(|dir| dir.join(cli)));
            }
            list.push(PathBuf::from("/opt/homebrew/bin").join(cli));
            list.push(PathBuf::from("/usr/local/bin").join(cli));
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                list.push(home.join(".local/bin").join(cli));
                if matches!(
                    editor,
                    EditorKind::Idea | EditorKind::Rustrover | EditorKind::Webstorm
                ) {
                    list.push(
                        home.join("Library/Application Support/JetBrains/Toolbox/scripts")
                            .join(cli),
                    );
                }
                for bundle in bundles {
                    list.push(home.join("Applications").join(bundle));
                }
            }
            for bundle in bundles {
                list.push(PathBuf::from("/Applications").join(bundle));
            }
            list
        };
        candidates
            .into_iter()
            .find(|candidate| is_executable_file(candidate))
            .ok_or_else(|| {
                format!(
                    "未找到 {display_name} 可执行入口（{cli}）。请确认已安装 {display_name} 或启用其命令行工具，也可在设置中切换其他编辑器；当前草稿与选择已保留。"
                )
            })
    }

    pub fn detect_installed_editors(&self) -> Vec<EditorKind> {
        EditorKind::ALL
            .into_iter()
            .filter(|kind| self.resolve_executable(*kind).is_ok())
            .collect()
    }

    fn write_snapshot(&self, role: &str, repo_path: &str, bytes: &[u8]) -> Result<PathBuf> {
        let mut guard = self.inner.temp.lock().map_err(|_| "编辑器临时目录不可用")?;
        if guard.is_none() {
            let created = tempfile::Builder::new()
                .prefix("picket-ide-")
                .tempdir()
                .map_err(|_| "无法创建编辑器 Diff 临时目录")?;
            fs::set_permissions(created.path(), fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法设置编辑器临时目录权限")?;
            *guard = Some(created);
        }
        let base = guard.as_ref().unwrap().path();
        let slot = self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let folder = base.join(format!("s-{slot}")).join(role);
        fs::create_dir_all(&folder).map_err(|_| "无法准备 Diff 临时快照子目录")?;
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法设置 Diff 快照目录权限")?;
        let target = folder.join(safe_snapshot_filename(repo_path));
        fs::write(&target, bytes).map_err(|_| "无法写入临时快照文件")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))
            .map_err(|_| "无法将临时快照设为只读")?;
        Ok(target)
    }

    async fn launch(
        &self,
        editor: EditorKind,
        executable: &Path,
        worktree_root: &Path,
        args: &[OsString],
    ) -> Result<()> {
        let (_, display_name, _) = editor_metadata(editor);
        let mut command = Command::new(executable);
        command
            .current_dir(worktree_root)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        let mut child = command.spawn().map_err(|error| {
            format!("启动 {display_name} 失败（{error}）；当前草稿与选择已保留。")
        })?;
        let stderr = child.stderr.take();
        let wait_window = if self
            .inner
            .override_dirs
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
        {
            Duration::from_secs(5)
        } else {
            Duration::from_millis(600)
        };
        match tokio::time::timeout(wait_window, child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(status)) => {
                let mut detail = String::new();
                if let Some(pipe) = stderr {
                    let mut buf = Vec::new();
                    let _ = pipe.take(8 * 1024).read_to_end(&mut buf).await;
                    detail = String::from_utf8_lossy(&buf)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                }
                if detail.is_empty() {
                    detail = format!("退出码 {:?}", status.code());
                }
                Err(format!(
                    "{display_name} 返回错误（{detail}）；当前草稿与选择已保留。"
                ))
            }
            Ok(Err(error)) => Err(format!(
                "等待 {display_name} 启动结果失败（{error}）；当前草稿与选择已保留。"
            )),
            Err(_) => {
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                Ok(())
            }
        }
    }
}

fn build_diff_args(editor: EditorKind, left: &Path, right: &Path) -> Vec<OsString> {
    match editor {
        EditorKind::Idea | EditorKind::Rustrover | EditorKind::Webstorm => {
            vec![
                "diff".into(),
                left.as_os_str().to_owned(),
                right.as_os_str().to_owned(),
            ]
        }
        EditorKind::Vscode | EditorKind::Cursor => {
            vec![
                "--diff".into(),
                left.as_os_str().to_owned(),
                right.as_os_str().to_owned(),
            ]
        }
    }
}

fn build_goto_args(editor: EditorKind, file: &Path, line: u32) -> Result<Vec<OsString>> {
    match editor {
        EditorKind::Idea | EditorKind::Rustrover | EditorKind::Webstorm => Ok(vec![
            "--line".into(),
            line.to_string().into(),
            file.as_os_str().to_owned(),
        ]),
        EditorKind::Vscode | EditorKind::Cursor => {
            let rendered = display(file)?;
            Ok(vec!["--goto".into(), format!("{rendered}:{line}:1").into()])
        }
    }
}

impl RepositoryStore {
    async fn read_git_blob(
        &self,
        reader: &GitReader,
        root: &Path,
        spec: &str,
    ) -> Result<Option<Vec<u8>>> {
        let output = reader.run(root, &["cat-file", "blob", spec]).await?;
        if output.success {
            Ok(Some(output.bytes))
        } else {
            Ok(None)
        }
    }

    pub async fn open_in_editor(
        &self,
        launcher: &EditorLauncher,
        request: &OpenFileRequest,
    ) -> Result<OpenFileOutcome> {
        validate_request_id(&request.request_id)?;
        if request.line == Some(0) {
            return Err("跳转行号无效，行号必须从 1 开始；当前草稿与选择已保留".into());
        }
        let executable = launcher.resolve_executable(request.editor)?;
        let (registered, root) = self.registered_worktree_root(&request.target).await?;
        let worktree_reader = self.inner.reader.for_worktree(&root).await?;
        let status_bytes = worktree_reader
            .bytes(
                &root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let live_status = parse::status(&status_bytes)?;

        let authorized = registered
            .commit_states
            .get(&request.target.worktree_id)
            .and_then(|state| state.files.get(&request.file_id));
        let live_entry = live_status.entries.iter().find(|entry| {
            authorized.is_some_and(|item| item.path == entry.path)
                || format!("file-{}", hash(&entry.path)) == request.file_id
        });
        let path = live_entry
            .map(|entry| entry.path.clone())
            .or_else(|| authorized.map(|item| item.path.clone()))
            .ok_or("所选文件已不在当前工作树的变更列表中，请刷新后重试；草稿与选择已保留")?;
        let previous_path = live_entry
            .and_then(|entry| entry.previous.clone())
            .or_else(|| authorized.and_then(|item| item.previous_path.clone()));
        ensure_safe_repo_path(&path)?;
        if let Some(previous) = &previous_path {
            ensure_safe_repo_path(previous)?;
        }

        let working = inspect_working_target(&root, &path)?;
        if let Some(line) = request.line {
            let target_file = match working {
                WorkingTarget::RealFile(real) => real,
                WorkingTarget::SymlinkTarget(bytes) => {
                    launcher.write_snapshot("SYMLINK", &path, &bytes)?
                }
                WorkingTarget::Missing => {
                    if let Some(staged_bytes) = self
                        .read_git_blob(&worktree_reader, &root, &format!(":0:{path}"))
                        .await?
                    {
                        launcher.write_snapshot("STAGED", &path, &staged_bytes)?
                    } else {
                        let head_source = previous_path.as_deref().unwrap_or(&path);
                        let head_bytes = self
                            .read_git_blob(&worktree_reader, &root, &format!("HEAD:{head_source}"))
                            .await?
                            .ok_or("目标文件在工作区与历史版本中均不存在，无法定位行号；草稿与选择已保留")?;
                        launcher.write_snapshot("HEAD", &path, &head_bytes)?
                    }
                }
            };
            let args = build_goto_args(request.editor, &target_file, line)?;
            launcher
                .launch(request.editor, &executable, &root, &args)
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::Goto,
                line: Some(line),
            });
        }

        let Some(entry) = live_entry else {
            let target_file = match working {
                WorkingTarget::RealFile(real) => real,
                WorkingTarget::SymlinkTarget(bytes) => {
                    launcher.write_snapshot("SYMLINK", &path, &bytes)?
                }
                WorkingTarget::Missing => {
                    return Err(
                        "文件当前没有变更且不在工作区中，请刷新变更列表；草稿与选择已保留".into(),
                    );
                }
            };
            launcher
                .launch(
                    request.editor,
                    &executable,
                    &root,
                    &[target_file.as_os_str().to_owned()],
                )
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::Open,
                line: None,
            });
        };

        if entry.conflicted {
            let WorkingTarget::RealFile(working_file) = working else {
                return Err(
                    "冲突文件在工作区中不存在，请先在 Git / 终端核对合并状态；草稿与选择已保留"
                        .into(),
                );
            };
            let args = match request.editor {
                EditorKind::Vscode | EditorKind::Cursor => {
                    vec![working_file.as_os_str().to_owned()]
                }
                EditorKind::Idea | EditorKind::Rustrover | EditorKind::Webstorm => {
                    let ours = self
                        .read_git_blob(&worktree_reader, &root, &format!(":2:{path}"))
                        .await?;
                    let theirs = self
                        .read_git_blob(&worktree_reader, &root, &format!(":3:{path}"))
                        .await?;
                    if let (Some(ours_bytes), Some(theirs_bytes)) = (ours, theirs) {
                        let ours_file = launcher.write_snapshot("OURS", &path, &ours_bytes)?;
                        let theirs_file =
                            launcher.write_snapshot("THEIRS", &path, &theirs_bytes)?;
                        if let Some(base_bytes) = self
                            .read_git_blob(&worktree_reader, &root, &format!(":1:{path}"))
                            .await?
                        {
                            let base_file =
                                launcher.write_snapshot("ANCESTOR", &path, &base_bytes)?;
                            vec![
                                "merge".into(),
                                ours_file.as_os_str().to_owned(),
                                theirs_file.as_os_str().to_owned(),
                                base_file.as_os_str().to_owned(),
                                working_file.as_os_str().to_owned(),
                            ]
                        } else {
                            vec![
                                "merge".into(),
                                ours_file.as_os_str().to_owned(),
                                theirs_file.as_os_str().to_owned(),
                                working_file.as_os_str().to_owned(),
                            ]
                        }
                    } else {
                        vec![working_file.as_os_str().to_owned()]
                    }
                }
            };
            launcher
                .launch(request.editor, &executable, &root, &args)
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::Merge,
                line: None,
            });
        }

        let is_staged = staged(entry);
        let is_partial = is_staged && entry.xy.as_bytes().get(1).is_some_and(|byte| *byte != b'.');
        let effective = if entry.xy == "??" {
            b'A'
        } else {
            entry.xy.as_bytes()[usize::from(!is_staged)]
        };

        if is_partial {
            let head_source = previous_path.as_deref().unwrap_or(&path);
            let head_bytes = self
                .read_git_blob(&worktree_reader, &root, &format!("HEAD:{head_source}"))
                .await?
                .unwrap_or_default();
            let staged_bytes = self
                .read_git_blob(&worktree_reader, &root, &format!(":0:{path}"))
                .await?
                .unwrap_or_default();
            let left = launcher.write_snapshot("HEAD", head_source, &head_bytes)?;
            let right = launcher.write_snapshot("STAGED", &path, &staged_bytes)?;
            let args = build_diff_args(request.editor, &left, &right);
            launcher
                .launch(request.editor, &executable, &root, &args)
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::StagedDiff,
                line: None,
            });
        }

        if effective == b'A' {
            let target_file = match working {
                WorkingTarget::RealFile(real) => real,
                WorkingTarget::SymlinkTarget(bytes) => {
                    launcher.write_snapshot("SYMLINK", &path, &bytes)?
                }
                WorkingTarget::Missing => {
                    let staged_bytes = self
                        .read_git_blob(&worktree_reader, &root, &format!(":0:{path}"))
                        .await?
                        .ok_or(
                            "新增文件在工作区与暂存区均不存在，请刷新后重试；草稿与选择已保留",
                        )?;
                    launcher.write_snapshot("STAGED", &path, &staged_bytes)?
                }
            };
            launcher
                .launch(
                    request.editor,
                    &executable,
                    &root,
                    &[target_file.as_os_str().to_owned()],
                )
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::Open,
                line: None,
            });
        }

        let (left, right) = if is_staged {
            let head_source = previous_path.as_deref().unwrap_or(&path);
            let head_bytes = self
                .read_git_blob(&worktree_reader, &root, &format!("HEAD:{head_source}"))
                .await?
                .unwrap_or_default();
            let left = launcher.write_snapshot("HEAD", head_source, &head_bytes)?;
            let right = if effective == b'D' {
                launcher.write_snapshot("STAGED", &path, &[])?
            } else {
                match working {
                    WorkingTarget::RealFile(real) => real,
                    WorkingTarget::SymlinkTarget(bytes) => {
                        launcher.write_snapshot("SYMLINK", &path, &bytes)?
                    }
                    WorkingTarget::Missing => {
                        let staged_bytes = self
                            .read_git_blob(&worktree_reader, &root, &format!(":0:{path}"))
                            .await?
                            .unwrap_or_default();
                        launcher.write_snapshot("STAGED", &path, &staged_bytes)?
                    }
                }
            };
            (left, right)
        } else {
            let base_bytes = match self
                .read_git_blob(&worktree_reader, &root, &format!(":0:{path}"))
                .await?
            {
                Some(bytes) => bytes,
                None => self
                    .read_git_blob(&worktree_reader, &root, &format!("HEAD:{path}"))
                    .await?
                    .unwrap_or_default(),
            };
            let left = launcher.write_snapshot("BASE", &path, &base_bytes)?;
            let right = if effective == b'D' {
                launcher.write_snapshot("WORKTREE", &path, &[])?
            } else {
                match working {
                    WorkingTarget::RealFile(real) => real,
                    WorkingTarget::SymlinkTarget(bytes) => {
                        launcher.write_snapshot("SYMLINK", &path, &bytes)?
                    }
                    WorkingTarget::Missing => launcher.write_snapshot("WORKTREE", &path, &[])?,
                }
            };
            (left, right)
        };
        let args = build_diff_args(request.editor, &left, &right);
        launcher
            .launch(request.editor, &executable, &root, &args)
            .await?;
        Ok(OpenFileOutcome {
            file_path: path,
            action: OpenFileAction::Diff,
            line: None,
        })
    }
}
