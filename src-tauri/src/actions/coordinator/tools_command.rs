//! Discrete-argv repository command execution (`run_repo_command`) with automatic
//! pre/post worktree diff tracking for safe commit closure via `commit_exact_scope`.

use super::{
    context::{ActionToolContext, append_unique_links, push_timeline_step},
    tools_workspace::{MAX_EDIT_FILE_BYTES, sha256_hex},
};
use crate::{
    actions::{
        executor::{exec_controlled_git, exec_controlled_repo_command},
        policy::{CommandDecision, classify_repo_command},
    },
    pi_runtime::PiToolExecutionResult,
    repository::{ai_context::is_sensitive_local_file, read_tools::validate_safe_repo_path},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

fn parse_porcelain_z_status(stdout: &str) -> HashMap<String, [u8; 2]> {
    let mut entries = HashMap::new();
    let mut parts = stdout.split('\0');
    while let Some(raw) = parts.next() {
        if raw.len() < 4 {
            continue;
        }
        let bytes = raw.as_bytes();
        let xy = [bytes[0], bytes[1]];
        let path = raw[3..].to_owned();
        if xy[0] == b'R' || xy[0] == b'C' || xy[1] == b'R' || xy[1] == b'C' {
            let _orig = parts.next();
        }
        entries.insert(path, xy);
    }
    entries
}

async fn snapshot_worktree_porcelain(root: &Path) -> Option<HashMap<String, [u8; 2]>> {
    let out = exec_controlled_git(
        root,
        &[
            "status".into(),
            "--porcelain=v1".into(),
            "-z".into(),
            "-uall".into(),
        ],
        false,
        true,
    )
    .await
    .ok()?;
    if !out.success {
        return None;
    }
    Some(parse_porcelain_z_status(&out.stdout))
}

fn read_regular_worktree_file_hash(canon_root: &Path, path: &str) -> Option<String> {
    let full = canon_root.join(path);
    let meta = fs::symlink_metadata(&full).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_EDIT_FILE_BYTES {
        return None;
    }
    let parent = full.parent()?;
    let canon_parent = fs::canonicalize(parent).ok()?;
    if !canon_parent.starts_with(canon_root) {
        return None;
    }
    let bytes = fs::read(&full).ok()?;
    Some(sha256_hex(&bytes))
}

fn is_partially_staged(xy: [u8; 2]) -> bool {
    xy[0] != b' ' && xy[0] != b'?' && xy[1] != b' '
}

pub(super) async fn handle_run_repo_command(
    ctx: &ActionToolContext,
    call_id: &str,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let command = args
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let raw_args: Vec<String> = args
        .get("args")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let timeout_secs = args.get("timeoutSecs").and_then(Value::as_u64);

    let decision = classify_repo_command(command, &raw_args);
    match decision {
        CommandDecision::DenyRedline { code, reason } => {
            return PiToolExecutionResult::error(format!(
                "Blocked by Picket hard redline ({code}): {reason}"
            ));
        }
        CommandDecision::AllowTier0 | CommandDecision::AllowTier1 => {}
        CommandDecision::RequireConfirmTier2 {
            reason,
            preview,
            allow_dont_ask_again,
        } => {
            if let Err(err_result) = ctx
                .confirm_tier2_write(
                    call_id,
                    &reason,
                    &preview,
                    allow_dont_ask_again,
                    "User declined to run the repository command.",
                )
                .await
            {
                return err_result;
            }
        }
    }

    let canon_root = fs::canonicalize(&ctx.root).ok();
    let pre_status = snapshot_worktree_porcelain(&ctx.root).await;
    let pre_hashes: HashMap<String, String> = match (canon_root.as_deref(), pre_status.as_ref()) {
        (Some(root), Some(status)) => status
            .keys()
            .filter_map(|path| {
                read_regular_worktree_file_hash(root, path).map(|hash| (path.clone(), hash))
            })
            .collect(),
        _ => HashMap::new(),
    };
    let already_tracked: HashSet<String> = ctx
        .run_state
        .lock()
        .map(|st| st.action_edited_hashes.keys().cloned().collect())
        .unwrap_or_default();

    let override_dirs = ctx.coordinator.override_cli_dirs();
    match exec_controlled_repo_command(
        &ctx.root,
        command,
        &raw_args,
        Some(&ctx.bundled_node_path),
        override_dirs.as_deref(),
        timeout_secs,
    )
    .await
    {
        Ok(out) => {
            let mut tracked_paths: Vec<String> = Vec::new();
            let mut tracked_updates: Vec<(String, String)> = Vec::new();
            if out.success
                && let (Some(root), Some(pre_map), Some(post_map)) = (
                    canon_root.as_deref(),
                    pre_status.as_ref(),
                    snapshot_worktree_porcelain(&ctx.root).await,
                )
            {
                for (raw_path, xy) in post_map {
                    if is_partially_staged(xy) {
                        continue;
                    }
                    let Ok(clean) = validate_safe_repo_path(&raw_path) else {
                        continue;
                    };
                    if is_sensitive_local_file(&clean) || ctx.local_keep_paths.contains(&clean) {
                        continue;
                    }
                    let was_dirty_before = pre_map.contains_key(&raw_path);
                    if was_dirty_before {
                        let is_selected_non_partial = ctx
                            .live_changes
                            .iter()
                            .find(|c| c.path == clean)
                            .is_some_and(|c| {
                                c.range.kind != "partial"
                                    && ctx.frozen_selections.iter().any(|s| s.file_id == c.id)
                            });
                        if !already_tracked.contains(&clean) && !is_selected_non_partial {
                            continue;
                        }
                    }
                    let Some(post_hash) = read_regular_worktree_file_hash(root, &clean) else {
                        continue;
                    };
                    if pre_hashes
                        .get(&raw_path)
                        .is_some_and(|prev| prev == &post_hash)
                    {
                        continue;
                    }
                    tracked_paths.push(clean.clone());
                    tracked_updates.push((clean, post_hash));
                }
                tracked_paths.sort();
            }

            let has_links = !out.extracted_links.is_empty();
            let has_tracked_edits = !tracked_updates.is_empty();
            if let Ok(mut st) = ctx.run_state.lock() {
                append_unique_links(&mut st.links, out.extracted_links.clone());
                for (path, hash) in tracked_updates {
                    st.wrote_repo = true;
                    st.action_edited_hashes.insert(path.clone(), hash);
                    st.committed_edited_paths.remove(&path);
                }
                push_timeline_step(
                    &mut st.timeline,
                    if out.success { "done" } else { "failed" },
                    status_label,
                );
            }
            if has_tracked_edits {
                let refreshed_snapshot = ctx
                    .store
                    .refresh_with_locale(&ctx.target, ctx.locale)
                    .await
                    .ok();
                (ctx.emit)("running", None, None, refreshed_snapshot);
                ctx.persist("running", String::new()).await;
            } else if out.success && has_links {
                ctx.persist("running", String::new()).await;
            }

            let mut body = format!(
                "exitCode: {:?}\nstdout:\n{}\nstderr:\n{}",
                out.exit_code, out.stdout, out.stderr
            );
            if !tracked_paths.is_empty() {
                body.push_str(&format!(
                    "\nmodifiedPaths (tracked for commit_exact_scope): {:?}",
                    tracked_paths
                ));
            }
            if out.success {
                PiToolExecutionResult::success(body)
            } else {
                PiToolExecutionResult::error(body)
            }
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}
