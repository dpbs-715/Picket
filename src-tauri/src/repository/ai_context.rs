//! Read-only Git scope verification and unified diff extraction for AI tasks
//! (`review_selected_scope`, `plan_cpush`, and `start_cpush`).
//! Holds the repository gate only while reading a consistent Git snapshot; releases it
//! before any external model request starts.
use super::*;
use crate::i18n::{self, LanguageKind};
use serde::Serialize;

const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const MAX_FILE_PATCH_BYTES: usize = 64 * 1024;
const MAX_SCALED_FILE_PATCH_BYTES: usize = 256 * 1024;
const REVIEW_MIN_PATCH_BYTES: usize = 32 * 1024;
const REVIEW_MAX_PATCH_BYTES: usize = 1536 * 1024;
const REVIEW_PROMPT_RESERVE_TOKENS: usize = 8_000;
const REVIEW_INLINE_DIFF_SHARE_NUM: usize = 55;
const REVIEW_INLINE_DIFF_SHARE_DEN: usize = 100;
const REVIEW_BYTES_PER_TOKEN_CONSERVATIVE: usize = 3;
const MIN_PARTIAL_PATCH_BUDGET: usize = 2 * 1024;
const SCOPE_DIFF_MAX_LINES: usize = 250;
const SCOPE_DIFF_MAX_BYTES: usize = 24 * 1024;
const CPUSH_MAX_FILE_PATCH_BYTES: usize = 8 * 1024;
const CPUSH_MAX_TOTAL_PATCH_BYTES: usize = 64 * 1024;
const CPUSH_MIN_PARTIAL_PATCH_BUDGET: usize = 512;
const MAX_UNTRACKED_READ_BYTES: usize = 256 * 1024;
const MAX_REVIEW_UNTRACKED_READ_BYTES: usize = 512 * 1024;

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

#[derive(Clone, Debug)]
pub struct DeferredPatchNotice {
    pub file_id: String,
    pub path: String,
    pub total_lines: usize,
    pub initial_ranges: Vec<(usize, usize)>,
    pub deferred_only: bool,
    pub partial_notice: String,
    pub omitted_notice: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedDiffFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub patch: String,
}

#[derive(Clone, Debug)]
pub struct ExtractedAiScope {
    pub branch: String,
    pub files: Vec<ExtractedDiffFile>,
    pub skipped_paths: Vec<String>,
    pub reviewed_file_ids: Vec<String>,
    pub authorized_patches: HashMap<String, String>,
    pub deferred_notices: Vec<DeferredPatchNotice>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenCPushFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub ignored_for_review: bool,
    pub binary: bool,
    pub content_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExtractedCPushScope {
    pub branch: String,
    pub initial_head: Option<String>,
    pub recent_commits: Vec<String>,
    pub unselected_paths: Vec<String>,
    pub files: Vec<FrozenCPushFile>,
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

fn redact_sensitive_patch(path: &str, raw_patch: &str) -> Option<String> {
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

fn truncate_patch_head_tail_with_ranges(
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
        i18n::patch_omitted_trailing(locale, head, omitted_lines)
    } else {
        i18n::patch_omitted_middle(locale, head, omitted_lines, tail)
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

impl RepositoryStore {
    pub async fn extract_ai_scope_with_budget(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
        locale: LanguageKind,
        max_total_patch_bytes: usize,
    ) -> Result<ExtractedAiScope> {
        if selections.is_empty() {
            return Err(i18n::ai_selection_required_error(locale, false).into());
        }
        let mut seen = HashSet::with_capacity(selections.len());
        for item in selections {
            if item.file_id.is_empty()
                || item.commit_token.is_empty()
                || !seen.insert(&item.file_id)
            {
                return Err(i18n::ai_invalid_token_error(locale, false).into());
            }
        }

        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let (registered, root) = self.registered_worktree_root(target, locale).await?;
        let keep_set = self.resolve_keep_set_for_common(&registered.common_dir)?;
        let entry = WorktreeEntry {
            path: root.clone(),
            head: None,
            branch: String::new(),
            bare: false,
        };
        let (live_tree, live_commit_state) = self
            .read_worktree(&target.repository_id, &entry, &keep_set, locale)
            .await?;

        let change_by_id: HashMap<&str, &Change> = live_tree
            .files
            .iter()
            .map(|change| (change.id.as_str(), change))
            .collect();

        for item in selections {
            let Some(authorized) = live_commit_state.files.get(&item.file_id) else {
                return Err(i18n::ai_file_missing_error(locale, false).into());
            };
            if authorized.conflicted {
                return Err(i18n::ai_conflict_in_scope_error(locale, false).into());
            }
            if authorized.token != item.commit_token {
                return Err(i18n::ai_scope_stale_error(locale, false).into());
            }
        }

        let worktree_reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;

        let mut skipped_paths = Vec::new();
        let mut authorized_patches = HashMap::new();
        let mut raw_candidates: Vec<(usize, String)> = Vec::new();

        for (selection_idx, item) in selections.iter().enumerate() {
            let change = change_by_id[item.file_id.as_str()];
            if change.ignored_for_review {
                skipped_paths.push(i18n::ai_skipped_lockfile_review(locale, &change.path));
                continue;
            }
            if change.binary {
                skipped_paths.push(i18n::ai_skipped_binary(locale, &change.path));
                continue;
            }

            let raw_patch = if change.range.kind == "partial" || change.range.kind == "staged" {
                let base_tree = live_tree.head.as_deref().unwrap_or(EMPTY_TREE_OID);
                let mut args = vec![
                    "diff-index",
                    "--cached",
                    "-p",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=3",
                    base_tree,
                    "--",
                ];
                if let Some(previous) = change.previous_path.as_deref() {
                    args.push(previous);
                }
                args.push(&change.path);
                let bytes = worktree_reader.bytes(&root, &args).await?;
                if bytes.contains(&0) {
                    skipped_paths.push(i18n::ai_skipped_binary(locale, &change.path));
                    continue;
                }
                match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        skipped_paths.push(i18n::ai_skipped_non_utf8(locale, &change.path));
                        continue;
                    }
                }
            } else if change.git_status == "??" {
                let full_path = root.join(&change.path);
                let metadata = fs::symlink_metadata(&full_path).map_err(|_| {
                    "Cannot read untracked file metadata; current draft and selection were preserved"
                })?;
                if metadata.file_type().is_symlink() {
                    let link = fs::read_link(&full_path)
                        .map_err(|_| "Cannot read symbolic link target")?;
                    format_untracked_patch(&change.path, &format!("symlink -> {}", link.display()))
                } else {
                    if metadata.len() as usize > MAX_REVIEW_UNTRACKED_READ_BYTES {
                        skipped_paths.push(i18n::ai_skipped_diff_too_large(locale, &change.path));
                        continue;
                    }
                    let bytes = fs::read(&full_path).map_err(|_| {
                        "Cannot read untracked file content; current draft and selection were preserved"
                    })?;
                    if bytes.contains(&0) {
                        skipped_paths.push(i18n::ai_skipped_binary(locale, &change.path));
                        continue;
                    }
                    match String::from_utf8(bytes) {
                        Ok(text) => format_untracked_patch(&change.path, &text),
                        Err(_) => {
                            skipped_paths.push(i18n::ai_skipped_non_utf8(locale, &change.path));
                            continue;
                        }
                    }
                }
            } else {
                let bytes = worktree_reader
                    .bytes(
                        &root,
                        &[
                            "diff-files",
                            "-p",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--unified=3",
                            "--",
                            &change.path,
                        ],
                    )
                    .await?;
                if bytes.contains(&0) {
                    skipped_paths.push(i18n::ai_skipped_binary(locale, &change.path));
                    continue;
                }
                match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        skipped_paths.push(i18n::ai_skipped_non_utf8(locale, &change.path));
                        continue;
                    }
                }
            };

            let raw_patch = if is_sensitive_local_file(&change.path) {
                skipped_paths.push(i18n::ai_skipped_sensitive_redacted(locale, &change.path));
                match redact_sensitive_patch(&change.path, &raw_patch) {
                    Some(redacted) => redacted,
                    None => continue,
                }
            } else {
                raw_patch
            };

            authorized_patches.insert(change.path.clone(), raw_patch.clone());
            raw_candidates.push((selection_idx, raw_patch));
        }

        // Distribute the inline diff budget smallest-first (water-filling) so early large
        // files never starve smaller files later in alphabetical order.
        let mut order: Vec<usize> = (0..raw_candidates.len()).collect();
        order.sort_by_key(|&pos| (raw_candidates[pos].1.len(), raw_candidates[pos].0));

        let mut remaining_budget = max_total_patch_bytes.max(MIN_PARTIAL_PATCH_BUDGET);
        let max_file_patch_bytes =
            (remaining_budget / 3).clamp(MAX_FILE_PATCH_BYTES, MAX_SCALED_FILE_PATCH_BYTES);
        let mut allocated: Vec<Option<(String, Option<DeferredPatchNotice>)>> =
            vec![None; raw_candidates.len()];

        for (step, &cand_pos) in order.iter().enumerate() {
            let files_left = order.len() - step;
            let fair_share = (remaining_budget / files_left).min(max_file_patch_bytes);
            let (selection_idx, ref raw_patch) = raw_candidates[cand_pos];
            let change = change_by_id[selections[selection_idx].file_id.as_str()];
            let total_lines = raw_patch.lines().count().max(1);

            if raw_patch.len() <= fair_share {
                remaining_budget = remaining_budget.saturating_sub(raw_patch.len());
                allocated[cand_pos] = Some((raw_patch.clone(), None));
            } else if fair_share >= MIN_PARTIAL_PATCH_BUDGET {
                let (mut slimmed, initial_ranges, total_lines) =
                    truncate_patch_head_tail_with_ranges(raw_patch, fair_share, locale);
                let first_omitted = initial_ranges
                    .first()
                    .map(|&(_, end)| end.saturating_add(1))
                    .unwrap_or(1)
                    .min(total_lines);
                let last_omitted = initial_ranges
                    .get(1)
                    .map(|&(start, _)| start.saturating_sub(1))
                    .unwrap_or(total_lines)
                    .max(first_omitted);
                slimmed.push_str(&format!(
                    "\n[Diff truncated ({total_lines} lines total). Call read_scope_diff(path=\"{}\", startLine={first_omitted}, endLine={last_omitted}) to inspect omitted hunks.]",
                    change.path
                ));
                remaining_budget = remaining_budget.saturating_sub(slimmed.len());
                allocated[cand_pos] = Some((
                    slimmed,
                    Some(DeferredPatchNotice {
                        file_id: change.id.clone(),
                        path: change.path.clone(),
                        total_lines,
                        initial_ranges,
                        deferred_only: false,
                        partial_notice: i18n::ai_skipped_large_partial(locale, &change.path),
                        omitted_notice: i18n::ai_skipped_total_too_large(locale, &change.path),
                    }),
                ));
            } else {
                let stub = format!(
                    "[Inline diff omitted due to large total changeset ({total_lines} lines, {} bytes). Call read_scope_diff(path=\"{}\", startLine=1, endLine=250) to review its exact authorized diff.]",
                    raw_patch.len(),
                    change.path
                );
                allocated[cand_pos] = Some((
                    stub,
                    Some(DeferredPatchNotice {
                        file_id: change.id.clone(),
                        path: change.path.clone(),
                        total_lines,
                        initial_ranges: Vec::new(),
                        deferred_only: true,
                        partial_notice: i18n::ai_skipped_large_partial(locale, &change.path),
                        omitted_notice: i18n::ai_skipped_total_too_large(locale, &change.path),
                    }),
                ));
            }
        }

        let mut files = Vec::with_capacity(raw_candidates.len());
        let mut reviewed_file_ids = Vec::with_capacity(raw_candidates.len());
        let mut deferred_notices = Vec::new();
        for (cand_pos, (selection_idx, _)) in raw_candidates.into_iter().enumerate() {
            let Some((final_patch, notice)) = allocated[cand_pos].take() else {
                continue;
            };
            let change = change_by_id[selections[selection_idx].file_id.as_str()];
            if let Some(deferred) = notice {
                deferred_notices.push(deferred);
            }
            reviewed_file_ids.push(change.id.clone());
            files.push(ExtractedDiffFile {
                file_id: change.id.clone(),
                path: change.path.clone(),
                previous_path: change.previous_path.clone(),
                status: change.status.clone(),
                range_kind: change.range.kind.clone(),
                added: change.range.added,
                removed: change.range.removed,
                patch: final_patch,
            });
        }

        let branch = if live_tree.detached {
            "(detached)".into()
        } else {
            live_tree.branch
        };
        Ok(ExtractedAiScope {
            branch,
            files,
            skipped_paths,
            reviewed_file_ids,
            authorized_patches,
            deferred_notices,
        })
    }

    pub async fn extract_cpush_scope(
        &self,
        target: &ReadTarget,
        selections: &[CommitSelection],
        locale: LanguageKind,
    ) -> Result<ExtractedCPushScope> {
        if selections.is_empty() {
            return Err(i18n::ai_selection_required_error(locale, true).into());
        }
        let mut seen = HashSet::with_capacity(selections.len());
        for item in selections {
            if item.file_id.is_empty()
                || item.commit_token.is_empty()
                || !seen.insert(item.file_id.as_str())
            {
                return Err(i18n::ai_invalid_token_error(locale, true).into());
            }
        }

        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        let (registered, root) = self.registered_worktree_root(target, locale).await?;
        let keep_set = self.resolve_keep_set_for_common(&registered.common_dir)?;
        let entry = WorktreeEntry {
            path: root.clone(),
            head: None,
            branch: String::new(),
            bare: false,
        };
        let (live_tree, live_commit_state) = self
            .read_worktree(&target.repository_id, &entry, &keep_set, locale)
            .await?;
        if live_tree.detached {
            return Err(i18n::cpush_detached_head_error(locale).into());
        }

        let change_by_id: HashMap<&str, &Change> = live_tree
            .files
            .iter()
            .map(|change| (change.id.as_str(), change))
            .collect();

        for item in selections {
            let Some(authorized) = live_commit_state.files.get(&item.file_id) else {
                return Err(i18n::ai_file_missing_error(locale, true).into());
            };
            if authorized.conflicted {
                return Err(i18n::ai_conflict_in_scope_error(locale, true).into());
            }
            if authorized.token != item.commit_token {
                return Err(i18n::ai_scope_stale_error(locale, true).into());
            }
        }

        let unselected_paths = live_tree
            .files
            .iter()
            .filter(|change| !seen.contains(change.id.as_str()))
            .map(|change| change.path.clone())
            .collect();

        let worktree_reader = self
            .inner
            .reader
            .with_locale(locale)
            .for_worktree(&root)
            .await?;
        let recent_commits = if live_tree.head.is_some() {
            let output = worktree_reader
                .run(&root, &["log", "-n", "8", "--format=%s%x00", "HEAD"])
                .await?;
            if output.success {
                output
                    .bytes
                    .split(|byte| *byte == 0)
                    .filter_map(|raw| std::str::from_utf8(raw).ok())
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut raw_entries: Vec<(Option<String>, Option<String>)> =
            Vec::with_capacity(selections.len());

        for item in selections {
            let change = change_by_id[item.file_id.as_str()];
            let mut raw_patch_opt = None;
            let mut skip_reason = None;

            if change.ignored_for_review {
                skip_reason = Some(i18n::cpush_skip_lockfile(
                    locale,
                    change.range.added,
                    change.range.removed,
                ));
            } else if change.binary {
                skip_reason = Some(i18n::cpush_skip_binary_patch(locale).into());
            } else if change.range.kind == "partial" || change.range.kind == "staged" {
                let base_tree = live_tree.head.as_deref().unwrap_or(EMPTY_TREE_OID);
                let mut args = vec![
                    "diff-index",
                    "--cached",
                    "-p",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=1",
                    base_tree,
                    "--",
                ];
                if let Some(previous) = change.previous_path.as_deref() {
                    args.push(previous);
                }
                args.push(&change.path);
                let bytes = worktree_reader.bytes(&root, &args).await?;
                if bytes.contains(&0) {
                    skip_reason = Some(i18n::cpush_skip_binary(locale).into());
                } else {
                    match String::from_utf8(bytes) {
                        Ok(text) => raw_patch_opt = Some(text),
                        Err(_) => skip_reason = Some(i18n::cpush_skip_non_utf8(locale).into()),
                    }
                }
            } else if change.git_status == "??" {
                let full_path = root.join(&change.path);
                let metadata = fs::symlink_metadata(&full_path).map_err(|_| {
                    "Cannot read untracked file metadata; current draft and selection were preserved"
                })?;
                if metadata.file_type().is_symlink() {
                    let link = fs::read_link(&full_path)
                        .map_err(|_| "Cannot read symbolic link target")?;
                    raw_patch_opt = Some(format_untracked_patch(
                        &change.path,
                        &format!("symlink -> {}", link.display()),
                    ));
                } else if metadata.len() as usize > MAX_UNTRACKED_READ_BYTES {
                    skip_reason = Some(i18n::cpush_skip_large_untracked(locale).into());
                } else {
                    let bytes = fs::read(&full_path).map_err(|_| {
                        "Cannot read untracked file content; current draft and selection were preserved"
                    })?;
                    if bytes.contains(&0) {
                        skip_reason = Some(i18n::cpush_skip_binary(locale).into());
                    } else {
                        match String::from_utf8(bytes) {
                            Ok(text) => {
                                raw_patch_opt = Some(format_untracked_patch(&change.path, &text))
                            }
                            Err(_) => skip_reason = Some(i18n::cpush_skip_non_utf8(locale).into()),
                        }
                    }
                }
            } else {
                let bytes = worktree_reader
                    .bytes(
                        &root,
                        &[
                            "diff-files",
                            "-p",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--unified=1",
                            "--",
                            &change.path,
                        ],
                    )
                    .await?;
                if bytes.contains(&0) {
                    skip_reason = Some(i18n::cpush_skip_binary(locale).into());
                } else {
                    match String::from_utf8(bytes) {
                        Ok(text) => raw_patch_opt = Some(text),
                        Err(_) => skip_reason = Some(i18n::cpush_skip_non_utf8(locale).into()),
                    }
                }
            }

            if is_sensitive_local_file(&change.path) {
                raw_patch_opt =
                    raw_patch_opt.and_then(|patch| redact_sensitive_patch(&change.path, &patch));
                skip_reason = Some(i18n::cpush_skip_sensitive(locale).into());
            }

            raw_entries.push((raw_patch_opt, skip_reason));
        }

        let mut patch_indices: Vec<usize> = raw_entries
            .iter()
            .enumerate()
            .filter_map(|(idx, (patch, _))| patch.as_ref().map(|p| (idx, p.len())))
            .map(|(idx, _)| idx)
            .collect();
        patch_indices.sort_by_key(|&idx| (raw_entries[idx].0.as_ref().map_or(0, String::len), idx));

        let mut remaining_budget = CPUSH_MAX_TOTAL_PATCH_BYTES;
        for (pos, &idx) in patch_indices.iter().enumerate() {
            let files_left = patch_indices.len() - pos;
            let fair_share = (remaining_budget / files_left).min(CPUSH_MAX_FILE_PATCH_BYTES);
            let Some(raw_patch) = raw_entries[idx].0.take() else {
                continue;
            };
            if raw_patch.len() <= fair_share {
                remaining_budget = remaining_budget.saturating_sub(raw_patch.len());
                raw_entries[idx].0 = Some(raw_patch);
            } else if fair_share >= CPUSH_MIN_PARTIAL_PATCH_BUDGET {
                let slimmed = truncate_patch_head_tail(&raw_patch, fair_share, locale);
                remaining_budget = remaining_budget.saturating_sub(slimmed.len());
                raw_entries[idx].0 = Some(slimmed);
                raw_entries[idx]
                    .1
                    .get_or_insert_with(|| i18n::cpush_skip_slimmed_patch(locale).into());
            } else {
                raw_entries[idx]
                    .1
                    .get_or_insert_with(|| i18n::cpush_skip_omitted_budget(locale).into());
            }
        }

        let files = selections
            .iter()
            .zip(raw_entries)
            .map(|(item, (patch, skip_reason))| {
                let change = change_by_id[item.file_id.as_str()];
                FrozenCPushFile {
                    file_id: change.id.clone(),
                    path: change.path.clone(),
                    previous_path: change.previous_path.clone(),
                    status: change.status.clone(),
                    range_kind: change.range.kind.clone(),
                    added: change.range.added,
                    removed: change.range.removed,
                    ignored_for_review: change.ignored_for_review,
                    binary: change.binary,
                    content_version: change.content_version.clone(),
                    patch,
                    skip_reason,
                }
            })
            .collect();

        Ok(ExtractedCPushScope {
            branch: live_tree.branch,
            initial_head: live_tree.head,
            recent_commits,
            unselected_paths,
            files,
        })
    }

    pub async fn resolve_cpush_step_selections(
        &self,
        target: &ReadTarget,
        expected_branch: &str,
        expected_head: Option<&str>,
        step_files: &[FrozenCPushFile],
        locale: LanguageKind,
    ) -> Result<(RepositorySnapshot, Vec<CommitSelection>)> {
        let snapshot = self.refresh_with_locale(target, locale).await?;
        let live_tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.worktree_id == target.worktree_id)
            .ok_or("Current worktree is missing from latest Git snapshot; please reopen")?;
        if let Some(err) = &live_tree.read_error {
            return Err(format!("Worktree status is unavailable: {err}"));
        }
        if live_tree.detached || live_tree.branch != expected_branch {
            return Err(i18n::cpush_step_branch_changed_error(
                locale,
                &live_tree.branch,
                live_tree.detached,
                expected_branch,
            ));
        }
        if live_tree.head.as_deref() != expected_head {
            return Err(
                "Repository HEAD changed outside the current session; paused subsequent commits. Verify latest Git history"
                    .into(),
            );
        }

        let mut selections = Vec::with_capacity(step_files.len());
        for frozen in step_files {
            let Some(live_file) = live_tree
                .files
                .iter()
                .find(|change| change.id == frozen.file_id)
            else {
                return Err(i18n::cpush_step_missing_file_error(locale, &frozen.path));
            };
            if live_file.content_version != frozen.content_version
                || live_file.range.kind != frozen.range_kind
            {
                return Err(i18n::cpush_step_stale_file_error(locale, &frozen.path));
            }
            selections.push(CommitSelection {
                file_id: live_file.id.clone(),
                commit_token: live_file.commit_token.clone(),
            });
        }
        Ok((snapshot, selections))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::smoke::Fixture;

    #[test]
    fn head_tail_truncation_preserves_both_ends_and_utf8_boundaries() {
        let mut raw =
            String::from("--- a/src/views/sofDialog.vue\n+++ b/src/views/sofDialog.vue\n");
        for idx in 0..300 {
            raw.push_str(&format!(
                "+  <div>field {idx}: \u{4e2d}\u{6587}-utf8-boundary-validation</div>\n"
            ));
        }
        raw.push_str("+export function submitSofDialog() { return true; }\n");

        let slimmed = truncate_patch_head_tail(&raw, 1024, LanguageKind::En);
        assert!(slimmed.len() <= 1024);
        assert!(slimmed.starts_with("--- a/src/views/sofDialog.vue"));
        assert!(slimmed.contains("middle changed lines omitted"));
        assert!(slimmed.contains("+export function submitSofDialog()"));

        let slimmed_zh = truncate_patch_head_tail(&raw, 1024, LanguageKind::Zh);
        assert!(slimmed_zh.len() <= 1024);
        assert!(slimmed_zh.contains("\u{7701}\u{7565}\u{4e2d}\u{95f4}"));
    }

    #[tokio::test]
    async fn cpush_scope_distributes_patch_budget_across_many_large_files() {
        let fixture = Fixture::new().unwrap();
        for idx in 0..7 {
            let mut content = format!("<template><section id=\"view-{idx}\">\n");
            for line in 0..320 {
                content.push_str(&format!(
                    "  <p>domain-field {idx}-{line}: port operation node record</p>\n"
                ));
            }
            content.push_str(&format!(
                "</section></template>\n<script setup>\nconst pageId = {idx};\n</script>\n"
            ));
            fs::write(fixture.root.join(format!("page-{idx}.vue")), content).unwrap();
        }

        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let tree = &snapshot.worktrees[0];
        let selections: Vec<CommitSelection> = tree
            .files
            .iter()
            .filter(|f| f.path.starts_with("page-"))
            .map(|f| CommitSelection {
                file_id: f.id.clone(),
                commit_token: f.commit_token.clone(),
            })
            .collect();
        assert_eq!(selections.len(), 7);

        let target = ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "sess-budget".into(),
            scope_version: tree.status_version.clone(),
        };
        let extracted = store
            .extract_cpush_scope(&target, &selections, LanguageKind::En)
            .await
            .unwrap();
        assert_eq!(extracted.files.len(), 7);

        let total_bytes: usize = extracted
            .files
            .iter()
            .map(|f| f.patch.as_ref().map_or(0, String::len))
            .sum();
        assert!(total_bytes <= CPUSH_MAX_TOTAL_PATCH_BYTES);

        for file in &extracted.files {
            let patch = file
                .patch
                .as_deref()
                .expect("every file gets fair patch share");
            assert!(patch.contains("<template><section"));
            assert!(patch.contains("<script setup>"));
            assert_eq!(
                file.skip_reason.as_deref(),
                Some("Large diff, patch truncated")
            );
        }
    }

    #[tokio::test]
    async fn cpush_scope_preserves_patch_context_across_ninety_five_files_within_budget() {
        let fixture = Fixture::new().unwrap();
        for idx in 0..95 {
            let content = format!(
                "export const moduleIndex = {idx};\nexport function runFeature{idx}() {{\n  return 'value-{idx}';\n}}\n"
            );
            fs::write(fixture.root.join(format!("mod-{idx:02}.ts")), content).unwrap();
        }

        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let tree = &snapshot.worktrees[0];
        let selections: Vec<CommitSelection> = tree
            .files
            .iter()
            .filter(|f| f.path.starts_with("mod-"))
            .map(|f| CommitSelection {
                file_id: f.id.clone(),
                commit_token: f.commit_token.clone(),
            })
            .collect();
        assert_eq!(selections.len(), 95);

        let target = ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "sess-95".into(),
            scope_version: tree.status_version.clone(),
        };
        let extracted = store
            .extract_cpush_scope(&target, &selections, LanguageKind::En)
            .await
            .unwrap();
        assert_eq!(extracted.files.len(), 95);
        let total_bytes: usize = extracted
            .files
            .iter()
            .map(|f| f.patch.as_ref().map_or(0, String::len))
            .sum();
        assert!(total_bytes <= CPUSH_MAX_TOTAL_PATCH_BYTES);
        assert!(
            extracted.files.iter().all(|f| f.patch.is_some()),
            "Every file in a 95-file scope should retain patch context without tail starvation"
        );
    }

    #[tokio::test]
    async fn review_scope_fair_budget_prevents_alphabetical_starvation_and_tracks_diff_tool_coverage()
     {
        let fixture = Fixture::new().unwrap();
        let mut giant = String::from("export const giantStart = true;\n");
        for idx in 0..900 {
            giant.push_str(&format!(
                "export const item_{idx:04} = 'payload-line-with-realistic-content-{idx:04}';\n"
            ));
        }
        giant.push_str("export const giantEnd = true;\n");
        fs::write(fixture.root.join("a-giant.ts"), giant).unwrap();
        for idx in 0..5 {
            fs::write(
                fixture.root.join(format!("z-small-{idx}.ts")),
                format!("export const small{idx} = {idx};\n"),
            )
            .unwrap();
        }

        let store = RepositoryStore::default();
        let snapshot = store.open(&fixture.root).await.unwrap();
        let tree = &snapshot.worktrees[0];
        let selections: Vec<CommitSelection> = tree
            .files
            .iter()
            .filter(|f| f.path == "a-giant.ts" || f.path.starts_with("z-small-"))
            .map(|f| CommitSelection {
                file_id: f.id.clone(),
                commit_token: f.commit_token.clone(),
            })
            .collect();
        assert_eq!(selections.len(), 6);

        let target = ReadTarget {
            repository_id: tree.repository_id.clone(),
            worktree_id: tree.worktree_id.clone(),
            session_id: "sess-review-fair".into(),
            scope_version: tree.status_version.clone(),
        };
        let scope = store
            .extract_ai_scope_with_budget(&target, &selections, LanguageKind::En, 24 * 1024)
            .await
            .unwrap();

        assert_eq!(scope.files.len(), 6);
        assert!(scope.skipped_paths.is_empty());
        // Every smaller `z-small-*` file got its 100% full patch without starvation:
        for idx in 0..5 {
            let path = format!("z-small-{idx}.ts");
            let entry = scope.files.iter().find(|f| f.path == path).unwrap();
            assert!(
                entry
                    .patch
                    .contains(&format!("export const small{idx} = {idx};"))
            );
            assert!(
                !scope.deferred_notices.iter().any(|n| n.path == path),
                "Small file {path} should not be truncated or starved by earlier large file"
            );
        }

        // Only `a-giant.ts` was truncated inline and carries a `read_scope_diff` hint + deferred notice:
        assert_eq!(scope.deferred_notices.len(), 1);
        let notice = &scope.deferred_notices[0];
        assert_eq!(notice.path, "a-giant.ts");
        assert!(!notice.deferred_only);
        assert!(!ranges_cover_all_lines(
            notice.total_lines,
            &notice.initial_ranges
        ));
        let giant_entry = scope.files.iter().find(|f| f.path == "a-giant.ts").unwrap();
        assert!(giant_entry.patch.contains("read_scope_diff"));

        // Reading an unauthorized file outside the scope is strictly refused:
        assert!(
            read_authorized_scope_diff(&scope.authorized_patches, "outside.ts", Some(1), Some(20))
                .is_err()
        );

        // Reading remaining diff chunks via `read_authorized_scope_diff` completes 100% line coverage:
        let mut combined = notice.initial_ranges.clone();
        let mut cursor = 1usize;
        while cursor <= notice.total_lines {
            let (chunk, start, end, total) = read_authorized_scope_diff(
                &scope.authorized_patches,
                "a-giant.ts",
                Some(cursor),
                Some(cursor + 249),
            )
            .unwrap();
            assert_eq!(total, notice.total_lines);
            assert!(chunk.contains("Authorized diff: a-giant.ts"));
            combined.push((start, end));
            if end >= total {
                break;
            }
            cursor = end + 1;
        }
        assert!(ranges_cover_all_lines(notice.total_lines, &combined));

        // Dynamic model-aware budget helper derives inline diff bytes directly from
        // (context_window - max_output_tokens - 8k reserve) * 55% * 3 bytes/token,
        // clamped to [32 KB, 1.5 MB]:
        assert_eq!(review_patch_budget_for_model(32_000, 4_096), 32_841);
        assert_eq!(review_patch_budget_for_model(128_000, 8_192), 184_482);
        assert_eq!(review_patch_budget_for_model(256_000, 16_384), 382_164);
        assert_eq!(
            review_patch_budget_for_model(1_000_000, 32_768),
            REVIEW_MAX_PATCH_BYTES
        );
        // Tiny/misconfigured windows clamp safely to the 32 KB floor without underflow:
        assert_eq!(
            review_patch_budget_for_model(8_000, 8_000),
            REVIEW_MIN_PATCH_BYTES
        );
    }
}
