use crate::{
    i18n::{self, LanguageKind},
    repository::{Result, runner::GitReader, types::Change},
};
use std::{collections::HashMap, fs, path::Path};

const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
pub(super) const MAX_FILE_PATCH_BYTES: usize = 64 * 1024;
pub(super) const MAX_SCALED_FILE_PATCH_BYTES: usize = 256 * 1024;
pub(super) const REVIEW_MIN_PATCH_BYTES: usize = 32 * 1024;
pub(super) const REVIEW_MAX_PATCH_BYTES: usize = 1536 * 1024;
const REVIEW_PROMPT_RESERVE_TOKENS: usize = 8_000;
const REVIEW_INLINE_DIFF_SHARE_NUM: usize = 55;
const REVIEW_INLINE_DIFF_SHARE_DEN: usize = 100;
const REVIEW_BYTES_PER_TOKEN_CONSERVATIVE: usize = 3;
pub(super) const MIN_PARTIAL_PATCH_BUDGET: usize = 2 * 1024;
const SCOPE_DIFF_MAX_LINES: usize = 250;
const SCOPE_DIFF_MAX_BYTES: usize = 24 * 1024;
pub(super) const CPUSH_MAX_FILE_PATCH_BYTES: usize = 8 * 1024;
pub(super) const CPUSH_MAX_TOTAL_PATCH_BYTES: usize = 64 * 1024;
pub(super) const CPUSH_MIN_PARTIAL_PATCH_BUDGET: usize = 512;
pub(super) const MAX_UNTRACKED_READ_BYTES: usize = 256 * 1024;
pub(super) const MAX_REVIEW_UNTRACKED_READ_BYTES: usize = 512 * 1024;

pub(super) enum RawPatchOutcome {
    Patch(String),
    Binary,
    NonUtf8,
    UntrackedTooLarge,
}

/// Computes the initial inline diff byte budget for code review directly from the
/// active model's configured `context_window` and `max_output_tokens`:
/// - Reserves `max_output_tokens` plus ~8k tokens for system prompt, `dcr` skill, and tool schemas.
/// - Allocates 55% of remaining input tokens to the initial inline diff (leaving ~45% for
///   multi-turn `read_scope_diff` / `read_repo_file` / `search_repo` tool calls and reasoning).
/// - Uses a conservative 3 bytes/token estimate (safe for mixed English code and Chinese UTF-8)
///   and clamps to `[32 KB, 1.5 MB]` to keep desktop stdio IPC responsive.
pub fn review_patch_budget_for_model(context_window: u32, max_output_tokens: u32) -> usize {
    let available_input_tokens = (context_window as usize)
        .saturating_sub(max_output_tokens as usize)
        .saturating_sub(REVIEW_PROMPT_RESERVE_TOKENS);
    let diff_tokens = available_input_tokens.saturating_mul(REVIEW_INLINE_DIFF_SHARE_NUM)
        / REVIEW_INLINE_DIFF_SHARE_DEN;
    let budget_bytes = diff_tokens.saturating_mul(REVIEW_BYTES_PER_TOKEN_CONSERVATIVE);
    budget_bytes.clamp(REVIEW_MIN_PATCH_BYTES, REVIEW_MAX_PATCH_BYTES)
}

fn format_untracked_patch(path: &str, content: &str) -> String {
    let normalized = content.strip_suffix('\n').unwrap_or(content);
    if normalized.is_empty() {
        return format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +0,0 @@\n");
    }
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut out = format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub(super) async fn read_change_raw_patch(
    worktree_reader: &GitReader,
    root: &Path,
    head: Option<&str>,
    change: &Change,
    unified_flag: &str,
    max_untracked_bytes: usize,
) -> Result<RawPatchOutcome> {
    if change.range.kind == "partial" || change.range.kind == "staged" {
        let base_tree = head.unwrap_or(EMPTY_TREE_OID);
        let mut args = vec![
            "diff-index",
            "--cached",
            "-p",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
            unified_flag,
            base_tree,
            "--",
        ];
        if let Some(previous) = change.previous_path.as_deref() {
            args.push(previous);
        }
        args.push(&change.path);
        let bytes = worktree_reader.bytes(root, &args).await?;
        if bytes.contains(&0) {
            return Ok(RawPatchOutcome::Binary);
        }
        return Ok(match String::from_utf8(bytes) {
            Ok(text) => RawPatchOutcome::Patch(text),
            Err(_) => RawPatchOutcome::NonUtf8,
        });
    }
    if change.git_status == "??" {
        let full_path = root.join(&change.path);
        let metadata = fs::symlink_metadata(&full_path).map_err(
            |_| "Cannot read untracked file metadata; current draft and selection were preserved",
        )?;
        if metadata.file_type().is_symlink() {
            let link = fs::read_link(&full_path).map_err(|_| "Cannot read symbolic link target")?;
            return Ok(RawPatchOutcome::Patch(format_untracked_patch(
                &change.path,
                &format!("symlink -> {}", link.display()),
            )));
        }
        if metadata.len() as usize > max_untracked_bytes {
            return Ok(RawPatchOutcome::UntrackedTooLarge);
        }
        let bytes = fs::read(&full_path).map_err(
            |_| "Cannot read untracked file content; current draft and selection were preserved",
        )?;
        if bytes.contains(&0) {
            return Ok(RawPatchOutcome::Binary);
        }
        return Ok(match String::from_utf8(bytes) {
            Ok(text) => RawPatchOutcome::Patch(format_untracked_patch(&change.path, &text)),
            Err(_) => RawPatchOutcome::NonUtf8,
        });
    }
    let bytes = worktree_reader
        .bytes(
            root,
            &[
                "diff-files",
                "-p",
                "--no-ext-diff",
                "--no-textconv",
                unified_flag,
                "--",
                &change.path,
            ],
        )
        .await?;
    if bytes.contains(&0) {
        return Ok(RawPatchOutcome::Binary);
    }
    Ok(match String::from_utf8(bytes) {
        Ok(text) => RawPatchOutcome::Patch(text),
        Err(_) => RawPatchOutcome::NonUtf8,
    })
}

pub fn is_sensitive_local_file(path: &str) -> bool {
    let Some(raw_name) = Path::new(path).file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = raw_name.to_ascii_lowercase();
    if lower == ".env" {
        return true;
    }
    if let Some(suffix) = lower.strip_prefix(".env.") {
        return !matches!(
            suffix,
            "example" | "sample" | "template" | "dist" | "schema" | "defaults"
        ) && !suffix.ends_with(".example")
            && !suffix.ends_with(".sample")
            && !suffix.ends_with(".template");
    }
    if matches!(
        lower.as_str(),
        ".npmrc"
            | ".pypirc"
            | ".netrc"
            | "id_rsa"
            | "id_ed25519"
            | "id_ecdsa"
            | "id_dsa"
            | "credentials.json"
            | "client_secret.json"
    ) {
        return true;
    }
    matches!(
        Path::new(&lower).extension().and_then(|e| e.to_str()),
        Some("pem" | "key" | "p12" | "pfx" | "keystore" | "jks")
    )
}

pub(super) fn redact_sensitive_patch(path: &str, raw_patch: &str) -> Option<String> {
    let lower = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !(lower == ".env" || lower.starts_with(".env.") || lower == ".npmrc") {
        return None;
    }
    let mut out = String::with_capacity(raw_patch.len());
    for line in raw_patch.lines() {
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("@@ ")
            || line.starts_with("new file mode")
            || line.starts_with("deleted file mode")
            || line.starts_with("\\ ")
        {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some(first) = line.chars().next()
            && matches!(first, '+' | '-' | ' ')
        {
            let body = &line[first.len_utf8()..];
            let trimmed = body.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                out.push_str(line);
            } else if let Some((key_part, val_part)) = body.split_once('=') {
                if val_part.trim().is_empty() {
                    out.push_str(line);
                } else {
                    out.push(first);
                    out.push_str(key_part);
                    out.push_str("=[REDACTED_LOCAL_VALUE]");
                }
            } else {
                out.push(first);
                out.push_str("[REDACTED_SENSITIVE_LINE]");
            }
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Some(out)
}

pub(super) fn truncate_patch_head_tail_with_ranges(
    raw_patch: &str,
    max_bytes: usize,
    locale: LanguageKind,
) -> (String, Vec<(usize, usize)>, usize) {
    let total_lines = raw_patch.lines().count().max(1);
    if raw_patch.len() <= max_bytes {
        return (raw_patch.to_owned(), vec![(1, total_lines)], total_lines);
    }
    const MARKER_RESERVE: usize = 64;
    if max_bytes < MARKER_RESERVE + 128 {
        let cut = raw_patch.floor_char_boundary(max_bytes.saturating_sub(20));
        let covered_head = raw_patch[..cut].lines().count();
        let ranges = if covered_head > 0 {
            vec![(1, covered_head)]
        } else {
            Vec::new()
        };
        return (
            format!("{}\n... [truncated]", &raw_patch[..cut]),
            ranges,
            total_lines,
        );
    }
    let usable = max_bytes - MARKER_RESERVE;
    let head_budget = (usable * 13) / 20;
    let tail_budget = usable - head_budget;

    let max_head = raw_patch.floor_char_boundary(head_budget);
    let head_end = raw_patch[..max_head]
        .rfind('\n')
        .filter(|&idx| idx > 0)
        .unwrap_or(max_head);

    let min_tail = raw_patch.ceil_char_boundary(raw_patch.len().saturating_sub(tail_budget));
    let tail_search_start = min_tail.max(head_end);
    let tail_start = raw_patch[tail_search_start..]
        .find('\n')
        .map(|offset| tail_search_start + offset + 1)
        .filter(|&idx| idx < raw_patch.len())
        .unwrap_or(tail_search_start);

    let omitted_lines = raw_patch[head_end..tail_start].lines().count().max(1);
    let head = raw_patch[..head_end].trim_end_matches('\n');
    let tail = raw_patch[tail_start..].trim_start_matches('\n');
    let head_line_count = head.lines().count();
    let mut ranges = Vec::new();
    if head_line_count > 0 {
        ranges.push((1, head_line_count));
    }
    if !tail.is_empty() {
        let tail_line_count = tail.lines().count();
        let tail_start_line = total_lines
            .saturating_sub(tail_line_count)
            .saturating_add(1);
        if tail_start_line <= total_lines {
            ranges.push((tail_start_line, total_lines));
        }
    }
    let formatted = if tail.is_empty() {
        i18n::repository::patch_omitted_trailing(locale, head, omitted_lines)
    } else {
        i18n::repository::patch_omitted_middle(locale, head, omitted_lines, tail)
    };
    (formatted, ranges, total_lines)
}

pub(crate) fn truncate_patch_head_tail(
    raw_patch: &str,
    max_bytes: usize,
    locale: LanguageKind,
) -> String {
    truncate_patch_head_tail_with_ranges(raw_patch, max_bytes, locale).0
}

pub fn ranges_cover_all_lines(total_lines: usize, ranges: &[(usize, usize)]) -> bool {
    if total_lines == 0 {
        return true;
    }
    let mut valid: Vec<(usize, usize)> = ranges
        .iter()
        .copied()
        .filter(|&(start, end)| start > 0 && end >= start)
        .map(|(start, end)| (start.min(total_lines), end.min(total_lines)))
        .collect();
    if valid.is_empty() {
        return false;
    }
    valid.sort_unstable();
    let (cur_start, mut cur_end) = valid[0];
    if cur_start > 1 {
        return false;
    }
    for (start, end) in valid.into_iter().skip(1) {
        if start <= cur_end.saturating_add(1) {
            cur_end = cur_end.max(end);
        } else {
            return false;
        }
    }
    cur_end >= total_lines
}

pub fn read_authorized_scope_diff(
    patches: &HashMap<String, String>,
    raw_path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> std::result::Result<(String, usize, usize, usize), String> {
    let path = raw_path.trim();
    if path.is_empty() {
        return Err("Missing required repository-relative path".into());
    }
    let Some(raw_patch) = patches.get(path) else {
        return Err(format!(
            "File `{path}` is not part of the authorized review scope"
        ));
    };
    let lines: Vec<&str> = raw_patch.lines().collect();
    let total = lines.len();
    if total == 0 {
        return Ok((format!("Authorized diff for {path} (0 lines)"), 1, 0, 0));
    }
    let start = start_line.unwrap_or(1).max(1);
    if start > total {
        return Err(format!(
            "startLine {start} exceeds authorized diff line count ({total}) for {path}"
        ));
    }
    let requested_end = end_line
        .unwrap_or(start.saturating_add(SCOPE_DIFF_MAX_LINES - 1))
        .max(start);
    let max_end = requested_end
        .min(start.saturating_add(SCOPE_DIFF_MAX_LINES - 1))
        .min(total);

    let mut out = String::new();
    let mut actual_end = start.saturating_sub(1);
    let mut byte_count = 0usize;
    let mut truncated_bytes = false;

    for (offset, line) in lines[start - 1..max_end].iter().enumerate() {
        let line_no = start + offset;
        let formatted = format!("{line_no}: {line}\n");
        if byte_count + formatted.len() > SCOPE_DIFF_MAX_BYTES && actual_end >= start {
            truncated_bytes = true;
            break;
        }
        out.push_str(&formatted);
        byte_count += formatted.len();
        actual_end = line_no;
    }

    let mut header = format!("Authorized diff: {path} (Lines {start}-{actual_end} of {total})\n");
    if actual_end < total {
        header.push_str(&format!(
            "[More diff lines available; call read_scope_diff with startLine={} to continue{}]\n",
            actual_end + 1,
            if truncated_bytes {
                ", byte cap reached"
            } else {
                ""
            }
        ));
    }
    Ok((format!("{header}{out}"), start, actual_end, total))
}
