//! Git-aware file opening, diff snapshot exporting, and conflict merge launch workflow.

use super::{
    launcher::{EditorLauncher, OpenFileOutcome},
    snapshot::{
        WorkingTarget, build_diff_args, build_goto_args, ensure_safe_repo_path,
        inspect_working_target,
    },
};
use crate::{
    i18n,
    repository::{
        RepositoryStore, Result, hash, parse,
        runner::GitReader,
        staged,
        types::{EditorKind, OpenFileAction, OpenFileRequest},
        validate_request_id_with_locale,
    },
};
use std::path::Path;

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
        let locale = request.locale;
        validate_request_id_with_locale(&request.request_id, locale)?;
        if request.line == Some(0) {
            return Err(i18n::repository::editor_invalid_line_error(locale).into());
        }
        let executable = launcher.resolve_executable_with_locale(request.editor, locale)?;
        let (registered, root) = self
            .registered_worktree_root(&request.target, locale)
            .await?;
        let worktree_reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;
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
        let live_status = parse::status_with_locale(&status_bytes, locale)?;

        let explicit_file_id = request
            .file_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let explicit_path = request
            .path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let (path, previous_path, live_entry, direct_path_open) =
            if let Some(file_id) = explicit_file_id {
                let authorized = registered
                    .commit_states
                    .get(&request.target.worktree_id)
                    .and_then(|state| state.files.get(file_id));
                let live_entry = live_status.entries.iter().find(|entry| {
                    authorized.is_some_and(|item| item.path == entry.path)
                        || format!("file-{}", hash(&entry.path)) == file_id
                });
                let path = live_entry
                    .map(|entry| entry.path.clone())
                    .or_else(|| authorized.map(|item| item.path.clone()))
                    .ok_or_else(|| i18n::repository::ai_file_missing_error(locale, false))?;
                let previous_path = live_entry
                    .and_then(|entry| entry.previous.clone())
                    .or_else(|| authorized.and_then(|item| item.previous_path.clone()));
                (path, previous_path, live_entry, false)
            } else if let Some(raw_path) = explicit_path {
                let mut normalized = raw_path.replace('\\', "/");
                while let Some(rest) = normalized.strip_prefix("./") {
                    normalized = rest.to_owned();
                }
                ensure_safe_repo_path(&normalized)?;
                let live_entry = live_status
                    .entries
                    .iter()
                    .find(|entry| entry.path == normalized);
                let previous_path = live_entry.and_then(|entry| entry.previous.clone());
                (normalized, previous_path, live_entry, true)
            } else {
                return Err(i18n::repository::ai_file_missing_error(locale, false).into());
            };
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
                            .ok_or("Target file does not exist in working tree or HEAD; cannot jump to line (draft and selection were preserved)")?;
                        launcher.write_snapshot("HEAD", &path, &head_bytes)?
                    }
                }
            };
            let args = build_goto_args(request.editor, &target_file, line, locale)?;
            launcher
                .launch(request.editor, &executable, &root, &args, locale)
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::Goto,
                line: Some(line),
            });
        }

        if direct_path_open {
            let target_file = match working {
                WorkingTarget::RealFile(real) => real,
                WorkingTarget::SymlinkTarget(bytes) => {
                    launcher.write_snapshot("SYMLINK", &path, &bytes)?
                }
                WorkingTarget::Missing => {
                    let head_source = previous_path.as_deref().unwrap_or(&path);
                    let head_bytes = self
                        .read_git_blob(&worktree_reader, &root, &format!("HEAD:{head_source}"))
                        .await?
                        .ok_or(
                            "Target file does not exist in working tree or HEAD (draft and selection were preserved)",
                        )?;
                    launcher.write_snapshot("HEAD", &path, &head_bytes)?
                }
            };
            launcher
                .launch(
                    request.editor,
                    &executable,
                    &root,
                    &[target_file.as_os_str().to_owned()],
                    locale,
                )
                .await?;
            return Ok(OpenFileOutcome {
                file_path: path,
                action: OpenFileAction::Open,
                line: None,
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
                        "File has no pending changes and does not exist in the working tree; refresh the change list (draft and selection were preserved)"
                            .into(),
                    );
                }
            };
            launcher
                .launch(
                    request.editor,
                    &executable,
                    &root,
                    &[target_file.as_os_str().to_owned()],
                    locale,
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
                    "Conflicted file does not exist in the working tree; inspect merge status in Git / terminal first (draft and selection were preserved)"
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
                .launch(request.editor, &executable, &root, &args, locale)
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
                .launch(request.editor, &executable, &root, &args, locale)
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
                            "Added file does not exist in working tree or index; refresh and retry (draft and selection were preserved)",
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
                    locale,
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
            .launch(request.editor, &executable, &root, &args, locale)
            .await?;
        Ok(OpenFileOutcome {
            file_path: path,
            action: OpenFileAction::Diff,
            line: None,
        })
    }
}
