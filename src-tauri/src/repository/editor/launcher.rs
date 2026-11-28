//! External IDE executable discovery, read-only snapshot directory lifecycle, and argv launch.

use super::snapshot::safe_snapshot_filename;
use crate::{
    i18n::{self, LanguageKind},
    repository::{
        Result,
        types::{EditorKind, OpenFileAction},
    },
};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
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

    #[cfg(feature = "smoke-harness")]
    pub fn set_search_dirs(&self, dirs: Option<Vec<PathBuf>>) -> Result<()> {
        *self
            .inner
            .override_dirs
            .lock()
            .map_err(|_| "Editor configuration lock is unavailable")? = dirs;
        Ok(())
    }

    pub fn cleanup(&self) -> Result<()> {
        let temp = self
            .inner
            .temp
            .lock()
            .map_err(|_| "Editor temporary directory lock is unavailable")?
            .take();
        if let Some(directory) = temp {
            directory
                .close()
                .map_err(|error| format!("Failed to clean up temporary diff snapshots: {error}"))?;
        }
        Ok(())
    }

    pub fn resolve_executable(&self, editor: EditorKind) -> Result<PathBuf> {
        self.resolve_executable_with_locale(editor, LanguageKind::En)
    }

    pub fn resolve_executable_with_locale(
        &self,
        editor: EditorKind,
        locale: LanguageKind,
    ) -> Result<PathBuf> {
        let (cli, display_name, bundles) = editor_metadata(editor);
        let override_dirs = self
            .inner
            .override_dirs
            .lock()
            .map_err(|_| "Editor search directory lock is unavailable")?
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
            .ok_or_else(|| i18n::repository::editor_not_found_error(locale, display_name, cli))
    }

    pub fn detect_installed_editors(&self) -> Vec<EditorKind> {
        EditorKind::ALL
            .into_iter()
            .filter(|kind| self.resolve_executable(*kind).is_ok())
            .collect()
    }

    pub(super) fn write_snapshot(
        &self,
        role: &str,
        repo_path: &str,
        bytes: &[u8],
    ) -> Result<PathBuf> {
        let mut guard = self
            .inner
            .temp
            .lock()
            .map_err(|_| "Editor temporary directory is unavailable")?;
        if guard.is_none() {
            let created = tempfile::Builder::new()
                .prefix("picket-ide-")
                .tempdir()
                .map_err(|_| "Cannot create temporary editor diff directory")?;
            fs::set_permissions(created.path(), fs::Permissions::from_mode(0o700))
                .map_err(|_| "Cannot set temporary editor directory permissions")?;
            *guard = Some(created);
        }
        let base = guard.as_ref().unwrap().path();
        let slot = self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let folder = base.join(format!("s-{slot}")).join(role);
        fs::create_dir_all(&folder)
            .map_err(|_| "Cannot prepare temporary diff snapshot directory")?;
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot set temporary diff snapshot permissions")?;
        let target = folder.join(safe_snapshot_filename(repo_path));
        fs::write(&target, bytes).map_err(|_| "Cannot write temporary diff snapshot")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))
            .map_err(|_| "Cannot mark temporary diff snapshot read-only")?;
        Ok(target)
    }

    pub(super) async fn launch(
        &self,
        editor: EditorKind,
        executable: &Path,
        worktree_root: &Path,
        args: &[OsString],
        locale: LanguageKind,
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
            i18n::repository::editor_launch_failed_error(locale, display_name, &error)
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
                    detail = format!("exit code {:?}", status.code());
                }
                Err(i18n::repository::editor_returned_error(
                    locale,
                    display_name,
                    &detail,
                ))
            }
            Ok(Err(error)) => Err(i18n::repository::editor_wait_failed_error(
                locale,
                display_name,
                &error,
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
