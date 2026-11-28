//! Safe worktree target inspection, temporary diff snapshot naming, and IDE CLI argument builders.

use super::super::{Result, display_with_locale, parse, types::EditorKind};
use crate::i18n::LanguageKind;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

pub(super) fn safe_snapshot_filename(repo_path: &str) -> String {
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

pub(super) fn ensure_safe_repo_path(path: &str) -> Result<()> {
    parse::relative(path)?;
    if path
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err("Opening internal .git files in an external editor is not allowed".into());
    }
    Ok(())
}

pub(super) enum WorkingTarget {
    RealFile(PathBuf),
    SymlinkTarget(Vec<u8>),
    Missing,
}

pub(super) fn inspect_working_target(root: &Path, relative_path: &str) -> Result<WorkingTarget> {
    ensure_safe_repo_path(relative_path)?;
    let full = root.join(relative_path);
    let metadata = match fs::symlink_metadata(&full) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorkingTarget::Missing);
        }
        Err(_) => {
            return Err(
                "Cannot inspect worktree file status; current draft and selection were preserved"
                    .into(),
            );
        }
    };
    let canon_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let parent = fs::canonicalize(
        full.parent()
            .ok_or("File path is missing parent directory")?,
    )
    .map_err(|_| "Cannot verify parent directory; current draft and selection were preserved")?;
    if !parent.starts_with(&canon_root) {
        return Err("File parent directory escapes worktree boundary; refused to open".into());
    }
    if metadata.file_type().is_symlink() {
        let link = fs::read_link(&full).map_err(|_| "Cannot read symbolic link target")?;
        return Ok(WorkingTarget::SymlinkTarget(
            link.as_os_str().as_encoded_bytes().to_vec(),
        ));
    }
    if !metadata.is_file() {
        return Err(
            "Target path is not a regular file and cannot open in an external editor; draft and selection were preserved"
                .into(),
        );
    }
    Ok(WorkingTarget::RealFile(full))
}

pub(super) fn build_diff_args(editor: EditorKind, left: &Path, right: &Path) -> Vec<OsString> {
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

pub(super) fn build_goto_args(
    editor: EditorKind,
    file: &Path,
    line: u32,
    locale: LanguageKind,
) -> Result<Vec<OsString>> {
    match editor {
        EditorKind::Idea | EditorKind::Rustrover | EditorKind::Webstorm => Ok(vec![
            "--line".into(),
            line.to_string().into(),
            file.as_os_str().to_owned(),
        ]),
        EditorKind::Vscode | EditorKind::Cursor => {
            let rendered = display_with_locale(file, locale)?;
            Ok(vec!["--goto".into(), format!("{rendered}:{line}:1").into()])
        }
    }
}
