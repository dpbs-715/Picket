//! Controlled repository file editing (`edit_repo_file`) and creation (`write_repo_file`)
//! handlers for Custom Actions, plus verified hash hydration across resumed runs.

use super::context::{ActionToolContext, Tier2RiskLevel, push_timeline_step};
use crate::{
    actions::executor::exec_controlled_git,
    i18n,
    pi_runtime::PiToolExecutionResult,
    repository::{ai_context::is_sensitive_local_file, read_tools::validate_safe_repo_path},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use tempfile::NamedTempFile;

pub(super) const MAX_EDIT_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EDIT_TEXT_BYTES: usize = 256 * 1024;
const MAX_PREVIEW_CHARS: usize = 1600;

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn format_edit_preview(path: &str, original: &str, old_text: &str, new_text: &str) -> String {
    let start_byte = original.find(old_text).unwrap_or(0);
    let start_line = original[..start_byte]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1;
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();
    let end_line = start_line.saturating_add(old_lines.len().saturating_sub(1));

    let mut out = format!("File: {path} (L{start_line}-L{end_line})\n");
    for line in old_lines.iter().take(24) {
        out.push_str("- ");
        out.push_str(line);
        out.push('\n');
    }
    if old_lines.len() > 24 {
        out.push_str(&format!(
            "- … ({} more removed lines)\n",
            old_lines.len() - 24
        ));
    }
    for line in new_lines.iter().take(24) {
        out.push_str("+ ");
        out.push_str(line);
        out.push('\n');
    }
    if new_lines.len() > 24 {
        out.push_str(&format!(
            "+ … ({} more added lines)\n",
            new_lines.len() - 24
        ));
    }
    if out.chars().count() > MAX_PREVIEW_CHARS {
        let head: String = out.chars().take(MAX_PREVIEW_CHARS).collect();
        format!("{head}\n… (preview truncated)")
    } else {
        out.trim_end().to_owned()
    }
}

fn resolve_editable_worktree_file(root: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    let canon_root = fs::canonicalize(root)
        .map_err(|_| "Cannot verify active worktree root directory.".to_owned())?;
    let full = canon_root.join(path);
    let meta =
        fs::symlink_metadata(&full).map_err(|_| format!("File not found in worktree: `{path}`"))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Refused to edit symlink `{path}`; only regular repository files can be edited."
        ));
    }
    if !meta.is_file() {
        return Err(format!("Target path is not a regular file: `{path}`"));
    }
    let parent = full
        .parent()
        .ok_or_else(|| format!("Invalid parent directory for `{path}`"))?;
    let canon_parent = fs::canonicalize(parent)
        .map_err(|_| format!("Cannot verify parent directory for `{path}`"))?;
    if !canon_parent.starts_with(&canon_root) {
        return Err(format!("Path escapes active worktree boundary: `{path}`"));
    }
    if meta.len() > MAX_EDIT_FILE_BYTES {
        return Err(format!("File `{path}` exceeds the 4 MB edit safety limit."));
    }
    Ok(full)
}

async fn live_porcelain_status_for_path(root: &Path, path: &str) -> Result<Option<String>, String> {
    let out = exec_controlled_git(
        root,
        &[
            "status".into(),
            "--porcelain=v1".into(),
            "-z".into(),
            "--".into(),
            path.into(),
        ],
        false,
        true,
    )
    .await?;
    if !out.success {
        return Err(format!(
            "Failed to verify live Git status for `{path}`: {}",
            out.stderr.trim()
        ));
    }
    let entry = out
        .stdout
        .split('\0')
        .find(|segment| !segment.is_empty())
        .map(str::to_owned);
    Ok(entry)
}

fn write_file_atomically(full: &Path, bytes: &[u8]) -> Result<(), String> {
    let perms = fs::metadata(full)
        .map_err(|err| format!("Cannot read file permissions: {err}"))?
        .permissions();
    let parent = full
        .parent()
        .ok_or_else(|| "Missing parent directory for atomic write".to_owned())?;
    let mut temp = NamedTempFile::new_in(parent)
        .map_err(|err| format!("Cannot create temporary file for atomic edit: {err}"))?;
    temp.write_all(bytes)
        .map_err(|err| format!("Failed writing edited content: {err}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|err| format!("Failed syncing edited file to disk: {err}"))?;
    fs::set_permissions(temp.path(), perms)
        .map_err(|err| format!("Failed preserving file permissions: {err}"))?;
    temp.persist(full)
        .map_err(|err| format!("Failed persisting edited file atomically: {}", err.error))?;
    Ok(())
}

pub(super) async fn handle_edit_repo_file(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
) -> PiToolExecutionResult {
    let raw_path = args.get("path").and_then(Value::as_str).unwrap_or("");
    let path = match validate_safe_repo_path(raw_path) {
        Ok(clean) => clean,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    if is_sensitive_local_file(&path) {
        return PiToolExecutionResult::error(format!(
            "Refused to edit sensitive local credential/environment file: `{path}`"
        ));
    }
    if ctx.local_keep_paths.contains(&path) {
        return PiToolExecutionResult::error(format!(
            "Refused to edit `{path}` because it is in Local Keep."
        ));
    }

    let Some(old_text) = args.get("oldText").and_then(Value::as_str) else {
        return PiToolExecutionResult::error(
            "Missing required `oldText` string for `edit_repo_file`.",
        );
    };
    let Some(new_text) = args.get("newText").and_then(Value::as_str) else {
        return PiToolExecutionResult::error(
            "Missing required `newText` string for `edit_repo_file`.",
        );
    };
    if old_text.is_empty() {
        return PiToolExecutionResult::error(
            "`oldText` must be a non-empty exact snippet to replace.",
        );
    }
    if old_text.contains('\0') || new_text.contains('\0') {
        return PiToolExecutionResult::error("`oldText` and `newText` cannot contain NUL bytes.");
    }
    if old_text.len() > MAX_EDIT_TEXT_BYTES || new_text.len() > MAX_EDIT_TEXT_BYTES {
        return PiToolExecutionResult::error("Replacement text exceeds the 256 KB per-edit limit.");
    }
    if old_text == new_text {
        return PiToolExecutionResult::error(
            "`newText` is identical to `oldText`; no edit needed.",
        );
    }

    let full = match resolve_editable_worktree_file(&ctx.root, &path) {
        Ok(f) => f,
        Err(err) => return PiToolExecutionResult::error(err),
    };

    let prior_action_hash = ctx
        .run_state
        .lock()
        .ok()
        .and_then(|st| st.action_edited_hashes.get(&path).cloned());

    // Protect pre-existing unselected user edits, partial staging, and concurrent external changes.
    if prior_action_hash.is_none() {
        if let Some(initial_change) = ctx.live_changes.iter().find(|c| c.path == path) {
            let was_selected = ctx
                .frozen_selections
                .iter()
                .any(|sel| sel.file_id == initial_change.id);
            if !was_selected {
                return PiToolExecutionResult::error(format!(
                    "Refused to edit `{path}` because it already had unselected local modifications before this action started. Never overwrite unselected user drafts."
                ));
            }
            if initial_change.range.kind == "partial" {
                return PiToolExecutionResult::error(format!(
                    "Refused to edit `{path}` because it is partially staged; editing the working tree would risk corrupting unstaged drafts."
                ));
            }
            if initial_change.range.kind == "staged" {
                return PiToolExecutionResult::error(format!(
                    "Refused to edit `{path}` because it is already staged; editing the working tree would diverge from the staged index."
                ));
            }
            if let Ok(Some(live_entry)) = live_porcelain_status_for_path(&ctx.root, &path).await {
                let xy = live_entry.as_bytes();
                let is_staged = xy.first().is_some_and(|&b| b != b' ' && b != b'?');
                if is_staged && initial_change.range.kind == "unstaged" {
                    return PiToolExecutionResult::error(format!(
                        "Refused to edit `{path}` because its Git staging state changed since the action started."
                    ));
                }
            }
        } else {
            match live_porcelain_status_for_path(&ctx.root, &path).await {
                Ok(Some(_)) => {
                    return PiToolExecutionResult::error(format!(
                        "Refused to edit `{path}` because it has uncommitted local modifications that were not present or selected when the action started."
                    ));
                }
                Ok(None) => {}
                Err(err) => return PiToolExecutionResult::error(err),
            }
        }
    }

    let original_bytes = match fs::read(&full) {
        Ok(bytes) => bytes,
        Err(err) => return PiToolExecutionResult::error(format!("Cannot read `{path}`: {err}")),
    };
    if original_bytes.contains(&0) {
        return PiToolExecutionResult::error(format!("Refused to edit binary file `{path}`."));
    }
    let original_hash = sha256_hex(&original_bytes);
    if let Some(expected_prev) = prior_action_hash.as_deref()
        && expected_prev != original_hash
    {
        return PiToolExecutionResult::error(format!(
            "Refused to edit `{path}` because it was modified externally after an earlier edit in this run."
        ));
    }

    let original_text = match String::from_utf8(original_bytes) {
        Ok(text) => text,
        Err(_) => {
            return PiToolExecutionResult::error(format!("File `{path}` is not valid UTF-8 text."));
        }
    };

    let occurrences = original_text.matches(old_text).count();
    if occurrences == 0 {
        return PiToolExecutionResult::error(format!(
            "Exact `oldText` was not found in `{path}`. Read the file with `read_repo_file` and supply exact matching lines."
        ));
    }
    if occurrences > 1 {
        return PiToolExecutionResult::error(format!(
            "`oldText` matched {occurrences} locations in `{path}`. Include more surrounding context so the replacement target is unique."
        ));
    }

    let updated_text = original_text.replacen(old_text, new_text, 1);
    let preview = format_edit_preview(&path, &original_text, old_text, new_text);
    let reason = format!("Applying exact text edit to `{path}`.");

    if let Err(err_result) = ctx
        .confirm_tier2_edit(
            call_id,
            Tier2RiskLevel::RoutineWrite,
            &reason,
            &preview,
            "User declined to edit the repository file.",
        )
        .await
    {
        return err_result;
    }

    // TOCTOU re-check: verify the file on disk did not change while waiting for confirmation.
    let current_bytes = match fs::read(&full) {
        Ok(bytes) => bytes,
        Err(err) => return PiToolExecutionResult::error(format!("Cannot re-read `{path}`: {err}")),
    };
    if sha256_hex(&current_bytes) != original_hash {
        return PiToolExecutionResult::error(format!(
            "Aborted editing `{path}` because the file changed on disk while waiting for confirmation."
        ));
    }

    let updated_bytes = updated_text.as_bytes();
    if let Err(err) = write_file_atomically(&full, updated_bytes) {
        return PiToolExecutionResult::error(err);
    }
    let written_hash = sha256_hex(updated_bytes);

    if let Ok(mut st) = ctx.run_state.lock() {
        st.wrote_repo = true;
        st.action_edited_hashes.insert(path.clone(), written_hash);
        st.committed_edited_paths.remove(&path);
        push_timeline_step(
            &mut st.timeline,
            "done",
            i18n::actions::action_edited_file_label(ctx.locale, &path),
        );
    }

    let refreshed_snapshot = ctx
        .store
        .refresh_with_locale(&ctx.target, ctx.locale)
        .await
        .ok();
    (ctx.emit)("running", None, None, refreshed_snapshot);
    ctx.persist("running", String::new()).await;

    PiToolExecutionResult::success(
        json!({
            "edited": true,
            "path": path,
            "preview": preview,
        })
        .to_string(),
    )
}

fn format_create_preview(path: &str, content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = format!("New file: {path} ({} lines)\n", lines.len());
    for line in lines.iter().take(32) {
        out.push_str("+ ");
        out.push_str(line);
        out.push('\n');
    }
    if lines.len() > 32 {
        out.push_str(&format!("+ … ({} more lines)\n", lines.len() - 32));
    }
    if out.chars().count() > MAX_PREVIEW_CHARS {
        let head: String = out.chars().take(MAX_PREVIEW_CHARS).collect();
        format!("{head}\n… (preview truncated)")
    } else {
        out.trim_end().to_owned()
    }
}

fn resolve_creatable_worktree_file(root: &Path, path: &str) -> Result<PathBuf, String> {
    let canon_root = fs::canonicalize(root)
        .map_err(|_| "Cannot verify active worktree root directory.".to_owned())?;
    let rel = Path::new(path);
    let mut components = rel.components().peekable();
    let mut cursor = canon_root.clone();
    while let Some(comp) = components.next() {
        let Component::Normal(seg) = comp else {
            return Err(format!("Invalid repository path: `{path}`"));
        };
        cursor.push(seg);
        let is_last = components.peek().is_none();
        if is_last {
            if fs::symlink_metadata(&cursor).is_ok() {
                return Err(format!(
                    "File `{path}` already exists. Use `edit_repo_file` for targeted edits to existing files."
                ));
            }
            break;
        }
        if let Ok(meta) = fs::symlink_metadata(&cursor) {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "Refused to create `{path}` through symlinked parent directory."
                ));
            }
            if !meta.is_dir() {
                return Err(format!(
                    "Cannot create `{path}` because a parent path component is not a directory."
                ));
            }
            let canon_dir = fs::canonicalize(&cursor)
                .map_err(|_| format!("Cannot verify parent directory for `{path}`"))?;
            if !canon_dir.starts_with(&canon_root) {
                return Err(format!("Path escapes active worktree boundary: `{path}`"));
            }
        } else {
            break;
        }
    }
    Ok(canon_root.join(rel))
}

fn create_new_file_atomically(full: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = full
        .parent()
        .ok_or_else(|| "Missing parent directory for atomic file creation".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("Failed creating parent directories: {err}"))?;
    let mut temp = NamedTempFile::new_in(parent)
        .map_err(|err| format!("Cannot create temporary file for atomic write: {err}"))?;
    temp.write_all(bytes)
        .map_err(|err| format!("Failed writing new file content: {err}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|err| format!("Failed syncing new file to disk: {err}"))?;
    temp.persist_noclobber(full).map_err(|err| {
        format!(
            "Failed creating file atomically without overwriting existing file: {}",
            err.error
        )
    })?;
    Ok(())
}

pub(super) fn hydrate_verified_edited_hashes(
    root: &Path,
    persisted: &BTreeMap<String, String>,
    local_keep_paths: &HashSet<String>,
) -> HashMap<String, String> {
    let mut verified = HashMap::new();
    for (path, expected_hash) in persisted {
        let Ok(clean) = validate_safe_repo_path(path) else {
            continue;
        };
        if clean != *path || is_sensitive_local_file(&clean) || local_keep_paths.contains(&clean) {
            continue;
        }
        let Ok(full) = resolve_editable_worktree_file(root, &clean) else {
            continue;
        };
        let Ok(bytes) = fs::read(&full) else {
            continue;
        };
        if sha256_hex(&bytes) == *expected_hash {
            verified.insert(clean, expected_hash.clone());
        }
    }
    verified
}

pub(super) async fn handle_write_repo_file(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
) -> PiToolExecutionResult {
    let raw_path = args.get("path").and_then(Value::as_str).unwrap_or("");
    let path = match validate_safe_repo_path(raw_path) {
        Ok(clean) => clean,
        Err(err) => return PiToolExecutionResult::error(err),
    };
    if is_sensitive_local_file(&path) {
        return PiToolExecutionResult::error(format!(
            "Refused to create sensitive local credential/environment file: `{path}`"
        ));
    }
    if ctx.local_keep_paths.contains(&path) {
        return PiToolExecutionResult::error(format!(
            "Refused to create `{path}` because it is in Local Keep."
        ));
    }

    let Some(content) = args.get("content").and_then(Value::as_str) else {
        return PiToolExecutionResult::error(
            "Missing required `content` string for `write_repo_file`.",
        );
    };
    if content.contains('\0') {
        return PiToolExecutionResult::error("`content` cannot contain NUL bytes.");
    }
    if content.len() > MAX_EDIT_TEXT_BYTES {
        return PiToolExecutionResult::error(
            "New file content exceeds the 256 KB per-file creation limit.",
        );
    }

    if let Err(err) = resolve_creatable_worktree_file(&ctx.root, &path) {
        return PiToolExecutionResult::error(err);
    }

    let preview = format_create_preview(&path, content);
    let reason = format!("Creating new repository file `{path}`.");
    if let Err(err_result) = ctx
        .confirm_tier2_create(
            call_id,
            Tier2RiskLevel::RoutineWrite,
            &reason,
            &preview,
            "User declined to create the repository file.",
        )
        .await
    {
        return err_result;
    }

    // TOCTOU re-check after confirmation.
    let full = match resolve_creatable_worktree_file(&ctx.root, &path) {
        Ok(f) => f,
        Err(err) => return PiToolExecutionResult::error(err),
    };

    let bytes = content.as_bytes();
    if let Err(err) = create_new_file_atomically(&full, bytes) {
        return PiToolExecutionResult::error(err);
    }
    let written_hash = sha256_hex(bytes);

    if let Ok(mut st) = ctx.run_state.lock() {
        st.wrote_repo = true;
        st.action_edited_hashes.insert(path.clone(), written_hash);
        st.committed_edited_paths.remove(&path);
        push_timeline_step(
            &mut st.timeline,
            "done",
            i18n::actions::action_created_file_label(ctx.locale, &path),
        );
    }

    let refreshed_snapshot = ctx
        .store
        .refresh_with_locale(&ctx.target, ctx.locale)
        .await
        .ok();
    (ctx.emit)("running", None, None, refreshed_snapshot);
    ctx.persist("running", String::new()).await;

    PiToolExecutionResult::success(
        json!({
            "created": true,
            "path": path,
            "preview": preview,
        })
        .to_string(),
    )
}
