//! Bounded read-only repository tools exposed to Pi Core over the Rust Tool Bridge
//! (`read_repo_file` and `search_repo`).
//! Strictly enforces registered worktree boundaries, blocks `.git` internals and symlinks
//! escaping the worktree, redacts or blocks local secret/credential files, and caps output size.

use super::{RepositoryStore, ai_context::is_sensitive_local_file, parse, types::ReadTarget};
use crate::i18n::LanguageKind;
use serde_json::Value;
use std::fs;

const MAX_READ_LINES: usize = 250;
const MAX_READ_BYTES: usize = 24 * 1024;
const MAX_SOURCE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_SEARCH_BYTES: usize = 24 * 1024;

pub(crate) fn validate_safe_repo_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Missing required repository-relative path".into());
    }
    parse::relative(trimmed)?;
    if trimmed
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err("Reading internal `.git` files is not allowed".into());
    }
    Ok(trimmed.to_owned())
}

fn validate_safe_git_rev(rev: &str) -> Result<String, String> {
    let trimmed = rev.trim();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return Err("Invalid Git revision".into());
    }
    if trimmed.starts_with('-')
        || trimmed.contains("..")
        || trimmed
            .bytes()
            .any(|b| b <= 0x20 || b == 0x7f || matches!(b, b':' | b'\\' | b'*' | b'?' | b'['))
    {
        return Err(format!("Unsupported Git revision syntax: {trimmed}"));
    }
    Ok(trimmed.to_owned())
}

fn slice_lines_bounded(content: &str, start_line: usize, end_line: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if total == 0 {
        return "(empty file)".into();
    }
    let start = start_line.max(1).min(total);
    let clamped_end = end_line
        .max(start)
        .min(start.saturating_add(MAX_READ_LINES - 1))
        .min(total);

    let mut out = String::new();
    let mut actual_end = start.saturating_sub(1);
    let mut byte_truncated = false;
    for (offset, line) in lines[start - 1..clamped_end].iter().enumerate() {
        let lineno = start + offset;
        let formatted = format!("{lineno}: {line}\n");
        if out.len() + formatted.len() > MAX_READ_BYTES && !out.is_empty() {
            byte_truncated = true;
            break;
        }
        out.push_str(&formatted);
        actual_end = lineno;
    }

    let mut header = format!("Lines {start}-{actual_end} of {total}");
    if byte_truncated {
        header.push_str(" (truncated at byte budget)");
    } else if clamped_end < total {
        header.push_str(&format!(" ({} more lines below)", total - clamped_end));
    }
    format!("{header}\n{out}")
}

impl RepositoryStore {
    pub async fn read_repo_file(
        &self,
        target: &ReadTarget,
        raw_path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
        rev: Option<&str>,
        locale: LanguageKind,
    ) -> Result<String, String> {
        let path = validate_safe_repo_path(raw_path)?;
        if is_sensitive_local_file(&path) {
            return Err(format!(
                "Refused to read sensitive local credential/environment file: {path}"
            ));
        }

        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let start = start_line.unwrap_or(1).max(1);
        let end = end_line
            .unwrap_or_else(|| start.saturating_add(MAX_READ_LINES - 1))
            .max(start);

        if let Some(raw_rev) = rev.map(str::trim).filter(|s| !s.is_empty()) {
            let safe_rev = validate_safe_git_rev(raw_rev)?;
            let reader = self
                .inner
                .reader
                .with_locale(locale)
                .for_worktree(&root)
                .await?;
            let spec = format!("{safe_rev}:{path}");
            let output = reader.run(&root, &["show", &spec]).await?;
            if !output.success {
                return Err(format!("File not found at revision `{safe_rev}`: {path}"));
            }
            if output.bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
                return Err(format!(
                    "File `{path}` at `{safe_rev}` exceeds the 1 MB read limit"
                ));
            }
            if output.bytes.contains(&0) {
                return Err(format!("Binary file cannot be read as text: {path}"));
            }
            let text = String::from_utf8(output.bytes)
                .map_err(|_| format!("File is not valid UTF-8 text: {path}"))?;
            return Ok(format!(
                "File: {path} (rev {safe_rev})\n{}",
                slice_lines_bounded(&text, start, end)
            ));
        }

        let full = root.join(&path);
        let meta = fs::symlink_metadata(&full)
            .map_err(|_| format!("File not found in worktree: {path}"))?;
        if meta.file_type().is_symlink() {
            let resolved = fs::canonicalize(&full)
                .map_err(|_| format!("Cannot resolve symlink target: {path}"))?;
            if !resolved.starts_with(&root) {
                return Err(format!("Symlink escapes worktree boundary: {path}"));
            }
            if !resolved.is_file() {
                return Err(format!("Target path is not a regular file: {path}"));
            }
        } else if !meta.is_file() {
            return Err(format!("Target path is not a regular file: {path}"));
        }

        let canonical_parent = fs::canonicalize(
            full.parent()
                .ok_or_else(|| format!("Invalid parent path for {path}"))?,
        )
        .map_err(|_| format!("Cannot verify parent directory for {path}"))?;
        if !canonical_parent.starts_with(&root) {
            return Err(format!("Path escapes worktree boundary: {path}"));
        }

        if meta.len() > MAX_SOURCE_FILE_BYTES {
            return Err(format!("File `{path}` exceeds the 1 MB read limit"));
        }
        let bytes = fs::read(&full).map_err(|_| format!("Cannot read file: {path}"))?;
        if bytes.contains(&0) {
            return Err(format!("Binary file cannot be read as text: {path}"));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("File is not valid UTF-8 text: {path}"))?;
        Ok(format!(
            "File: {path}\n{}",
            slice_lines_bounded(&text, start, end)
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_repo(
        &self,
        target: &ReadTarget,
        query: &str,
        path_prefix: Option<&str>,
        is_regex: bool,
        case_sensitive: bool,
        max_results: Option<usize>,
        locale: LanguageKind,
    ) -> Result<String, String> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() || trimmed_query.len() > 256 || trimmed_query.contains('\0') {
            return Err("Search query must be 1 to 256 non-NUL characters".into());
        }
        let validated_prefix = match path_prefix.map(str::trim).filter(|s| !s.is_empty()) {
            Some(prefix) => Some(validate_safe_repo_path(prefix)?),
            None => None,
        };
        let limit = max_results.unwrap_or(30).clamp(1, MAX_SEARCH_RESULTS);
        let (_, root) = self.registered_worktree_root(target, locale).await?;
        let reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;

        let mut args: Vec<&str> = vec!["grep", "-n", "-I", "--max-count", "15"];
        if is_regex {
            args.push("-E");
        } else {
            args.push("-F");
        }
        if !case_sensitive {
            args.push("-i");
        }
        args.push("-e");
        args.push(trimmed_query);
        args.push("--");
        if let Some(ref prefix) = validated_prefix {
            args.push(prefix.as_str());
        } else {
            args.push(".");
        }

        let output = reader.run(&root, &args).await?;
        if output.code == Some(1) {
            return Ok(format!("No matches found for `{trimmed_query}`."));
        }
        if !output.success {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if detail.is_empty() {
                "Repository search failed".into()
            } else {
                format!("Repository search failed: {detail}")
            });
        }

        let raw = String::from_utf8_lossy(&output.bytes);
        let mut results = Vec::new();
        let mut used_bytes = 0usize;
        let mut truncated = false;
        for line in raw.lines() {
            let Some((file_path, rest)) = line.split_once(':') else {
                continue;
            };
            if is_sensitive_local_file(file_path) {
                continue;
            }
            if results.len() >= limit {
                truncated = true;
                break;
            }
            let snippet = if rest.len() > 240 {
                let cut = rest.floor_char_boundary(240);
                format!("{file_path}:{}…", &rest[..cut])
            } else {
                format!("{file_path}:{rest}")
            };
            if used_bytes + snippet.len() + 1 > MAX_SEARCH_BYTES && !results.is_empty() {
                truncated = true;
                break;
            }
            used_bytes += snippet.len() + 1;
            results.push(snippet);
        }

        if results.is_empty() {
            return Ok(format!("No matches found for `{trimmed_query}`."));
        }
        let mut header = format!("Found {} match(es) for `{trimmed_query}`", results.len());
        if let Some(prefix) = validated_prefix {
            header.push_str(&format!(" in `{prefix}`"));
        }
        if truncated {
            header.push_str(" (truncated)");
        }
        Ok(format!("{header}:\n{}", results.join("\n")))
    }

    pub async fn execute_read_tool(
        &self,
        target: &ReadTarget,
        tool_name: &str,
        args: &Value,
        locale: LanguageKind,
    ) -> Result<String, String> {
        match tool_name {
            "read_repo_file" => {
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or("Missing `path` argument for read_repo_file")?;
                let start_line = args
                    .get("startLine")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                let end_line = args
                    .get("endLine")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                let rev = args.get("rev").and_then(Value::as_str);
                self.read_repo_file(target, path, start_line, end_line, rev, locale)
                    .await
            }
            "search_repo" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or("Missing `query` argument for search_repo")?;
                let path_prefix = args.get("pathPrefix").and_then(Value::as_str);
                let is_regex = args
                    .get("isRegex")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let case_sensitive = args
                    .get("caseSensitive")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let max_results = args
                    .get("maxResults")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                self.search_repo(
                    target,
                    query,
                    path_prefix,
                    is_regex,
                    case_sensitive,
                    max_results,
                    locale,
                )
                .await
            }
            other => Err(format!("Unsupported repository read tool: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    async fn init_temp_repo() -> (tempfile::TempDir, ReadTarget, RepositoryStore) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Picket Test"],
            vec!["config", "user.email", "test@picket.local"],
        ] {
            assert!(
                Command::new("/usr/bin/git")
                    .current_dir(root)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/auth.ts"),
            "export function verifyToken(token: string) {\n  return token.length > 0;\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("src/caller.ts"),
            "import { verifyToken } from './auth';\nexport const ok = verifyToken('abc');\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "API_SECRET=super-secret-value\n").unwrap();
        for args in [
            vec!["add", "src/auth.ts", "src/caller.ts"],
            vec!["commit", "-m", "feat: initial"],
        ] {
            assert!(
                Command::new("/usr/bin/git")
                    .current_dir(root)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let store = RepositoryStore::default();
        let snap = store.open(root).await.unwrap();
        let tree = &snap.worktrees[0];
        let target = ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "read-tools-test".into(),
            scope_version: tree.status_version.clone(),
        };
        (dir, target, store)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reads_files_and_searches_while_blocking_secrets_and_traversal() {
        let (_dir, target, store) = init_temp_repo().await;

        let content = store
            .read_repo_file(
                &target,
                "src/auth.ts",
                Some(1),
                Some(2),
                None,
                LanguageKind::En,
            )
            .await
            .unwrap();
        assert!(content.contains("1: export function verifyToken"));
        assert!(content.contains("Lines 1-2 of 3"));

        let at_head = store
            .read_repo_file(
                &target,
                "src/auth.ts",
                Some(1),
                Some(3),
                Some("HEAD"),
                LanguageKind::En,
            )
            .await
            .unwrap();
        assert!(at_head.contains("(rev HEAD)"));

        let search = store
            .search_repo(
                &target,
                "verifyToken",
                None,
                false,
                true,
                Some(10),
                LanguageKind::En,
            )
            .await
            .unwrap();
        assert!(search.contains("src/caller.ts:1:"));
        assert!(search.contains("src/auth.ts:1:"));

        assert!(
            store
                .read_repo_file(&target, ".env", None, None, None, LanguageKind::En)
                .await
                .unwrap_err()
                .contains("sensitive")
        );
        assert!(
            store
                .read_repo_file(
                    &target,
                    "../outside.txt",
                    None,
                    None,
                    None,
                    LanguageKind::En
                )
                .await
                .is_err()
        );
        assert!(
            store
                .read_repo_file(&target, ".git/config", None, None, None, LanguageKind::En)
                .await
                .is_err()
        );
        assert!(
            store
                .read_repo_file(
                    &target,
                    "src/auth.ts",
                    None,
                    None,
                    Some("--all"),
                    LanguageKind::En
                )
                .await
                .is_err()
        );
    }
}
