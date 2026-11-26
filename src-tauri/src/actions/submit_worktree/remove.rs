use super::inspect::run_git_bytes;
use crate::actions::session_store::SubmitGroupCheckpoint;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedWorktreeEntry {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub is_original_checkout: bool,
    pub owned_by_picket_submit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

pub async fn parse_worktree_list(
    original_root: &Path,
    recorded_groups: &[SubmitGroupCheckpoint],
) -> Result<Vec<ParsedWorktreeEntry>, String> {
    let (ok, stdout, stderr) = run_git_bytes(
        original_root,
        &["worktree", "list", "--porcelain"],
        false,
        true,
    )
    .await?;
    if !ok {
        return Err(format!("Failed to list Git worktrees: {stderr}"));
    }
    let canon_original =
        fs::canonicalize(original_root).unwrap_or_else(|_| original_root.to_path_buf());
    let text = String::from_utf8_lossy(&stdout);
    let mut entries = Vec::new();
    let mut cur_path: Option<String> = None;
    let mut cur_head: Option<String> = None;
    let mut cur_branch: Option<String> = None;
    let mut cur_locked = false;
    let mut cur_prunable = false;

    let flush = |entries: &mut Vec<ParsedWorktreeEntry>,
                 path_opt: &mut Option<String>,
                 head_opt: &mut Option<String>,
                 branch_opt: &mut Option<String>,
                 locked: &mut bool,
                 prunable: &mut bool| {
        if let Some(p) = path_opt.take() {
            let pb = PathBuf::from(&p);
            let canon = fs::canonicalize(&pb).unwrap_or(pb);
            let matched_group = recorded_groups.iter().find(|g| {
                g.worktree_path.as_deref().is_some_and(|gp| {
                    let gpb = PathBuf::from(gp);
                    fs::canonicalize(&gpb).unwrap_or(gpb) == canon
                })
            });
            entries.push(ParsedWorktreeEntry {
                path: canon.to_string_lossy().into_owned(),
                head: head_opt.take(),
                branch: branch_opt.take(),
                locked: *locked,
                prunable: *prunable,
                is_original_checkout: canon == canon_original,
                owned_by_picket_submit: matched_group.is_some(),
                group_id: matched_group.map(|g| g.group_id.clone()),
            });
            *locked = false;
            *prunable = false;
        }
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            flush(
                &mut entries,
                &mut cur_path,
                &mut cur_head,
                &mut cur_branch,
                &mut cur_locked,
                &mut cur_prunable,
            );
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            cur_path = Some(p.trim().to_owned());
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            cur_head = Some(h.trim().to_owned());
        } else if let Some(b) = line.strip_prefix("branch ") {
            let trimmed = b.trim();
            cur_branch = Some(
                trimmed
                    .strip_prefix("refs/heads/")
                    .unwrap_or(trimmed)
                    .to_owned(),
            );
        } else if line == "locked" || line.starts_with("locked ") {
            cur_locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            cur_prunable = true;
        }
    }
    flush(
        &mut entries,
        &mut cur_path,
        &mut cur_head,
        &mut cur_branch,
        &mut cur_locked,
        &mut cur_prunable,
    );
    Ok(entries)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSubmitWorktreeResult {
    pub worktree_path: String,
    pub removed: bool,
    pub already_absent: bool,
    pub branch_deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_note: Option<String>,
}

/// Strictly implements `references/merge-chain.md` temporary worktree & branch cleanup rules:
/// verifies ownership, unlocked status, verified HEAD SHA, zero in-progress Git operations,
/// completely empty status (including untracked & ignored files), non-force `git worktree remove`,
/// post-removal re-check, and optional safe `git branch -d` (never `-D`).
pub async fn remove_verified_submit_worktree(
    original_root: &Path,
    recorded_group: &SubmitGroupCheckpoint,
    expected_head_sha: Option<&str>,
    delete_local_branch: bool,
) -> Result<RemoveSubmitWorktreeResult, String> {
    let Some(ref raw_wt_path) = recorded_group.worktree_path else {
        return Err("No isolated submit worktree path is recorded for this group.".into());
    };
    let wt_path = PathBuf::from(raw_wt_path);
    let canon_original =
        fs::canonicalize(original_root).unwrap_or_else(|_| original_root.to_path_buf());
    let canon_target = fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
    if canon_target == canon_original {
        return Err(
            "Refused to remove the original user checkout; only temporary Picket submit worktrees can be removed."
                .into(),
        );
    }

    let list = parse_worktree_list(original_root, std::slice::from_ref(recorded_group)).await?;
    let matched = list
        .iter()
        .find(|entry| Path::new(&entry.path) == canon_target);

    if matched.is_none() && !wt_path.exists() {
        let (branch_deleted, branch_note) = if delete_local_branch {
            try_safe_delete_local_branch(original_root, recorded_group.branch.as_deref()).await
        } else {
            (false, None)
        };
        return Ok(RemoveSubmitWorktreeResult {
            worktree_path: canon_target.to_string_lossy().into_owned(),
            removed: false,
            already_absent: true,
            branch_deleted,
            branch_note,
        });
    }

    let Some(entry) = matched else {
        return Err(format!(
            "Directory `{}` exists on disk but is not a registered Git worktree; keeping it in place.",
            canon_target.display()
        ));
    };
    if entry.locked {
        return Err(format!(
            "Worktree `{}` is locked; keeping it in place.",
            entry.path
        ));
    }
    if let Some(ref expected_branch) = recorded_group.branch
        && entry.branch.as_deref() != Some(expected_branch.as_str())
    {
        return Err(format!(
            "Worktree `{}` is currently on branch {:?} instead of recorded branch `{expected_branch}`; keeping it in place.",
            entry.path, entry.branch
        ));
    }
    let verified_sha = expected_head_sha.or(recorded_group.verified_head_sha.as_deref());
    if let Some(expected_sha) = verified_sha
        && entry.head.as_deref() != Some(expected_sha)
    {
        return Err(format!(
            "Worktree `{}` HEAD {:?} does not match verified revision `{expected_sha}`; keeping it in place.",
            entry.path, entry.head
        ));
    }

    // Check for in-progress Git operations inside the worktree's git dir.
    let (ok_gd, gd_out, gd_err) = run_git_bytes(
        &canon_target,
        &["rev-parse", "--absolute-git-dir"],
        false,
        true,
    )
    .await?;
    if !ok_gd {
        return Err(format!(
            "Could not resolve git dir for worktree `{}`: {gd_err}",
            canon_target.display()
        ));
    }
    let wt_git_dir = PathBuf::from(String::from_utf8_lossy(&gd_out).trim());
    for sentinel in [
        "MERGE_HEAD",
        "REBASE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
    ] {
        if wt_git_dir.join(sentinel).exists() {
            return Err(format!(
                "Worktree `{}` has an in-progress Git operation (`{sentinel}`); keeping it in place.",
                canon_target.display()
            ));
        }
    }

    // Exact cleanliness check from `references/merge-chain.md` line 98:
    let (ok_st, st_out, st_err) = run_git_bytes(
        &canon_target,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored",
            "--ignore-submodules=none",
        ],
        false,
        true,
    )
    .await?;
    if !ok_st {
        return Err(format!(
            "Failed to verify worktree cleanliness for `{}`: {st_err}",
            canon_target.display()
        ));
    }
    if !st_out.is_empty() {
        return Err(format!(
            "Worktree `{}` is not clean (contains staged, unstaged, untracked, or ignored files); keeping it in place.",
            canon_target.display()
        ));
    }

    // Remove without `--force`:
    let target_str = canon_target.to_string_lossy().into_owned();
    let (ok_rm, _, err_rm) = run_git_bytes(
        original_root,
        &["worktree", "remove", "--", &target_str],
        false,
        true,
    )
    .await?;
    if !ok_rm {
        return Err(format!(
            "Git refused non-force `git worktree remove` for `{target_str}`: {err_rm}"
        ));
    }

    // Recheck worktree list & filesystem path after removal.
    let list_after =
        parse_worktree_list(original_root, std::slice::from_ref(recorded_group)).await?;
    if list_after
        .iter()
        .any(|item| Path::new(&item.path) == canon_target)
        || canon_target.exists()
    {
        return Err(format!(
            "Worktree `{target_str}` still appears present after `git worktree remove`."
        ));
    }

    let (branch_deleted, branch_note) = if delete_local_branch {
        try_safe_delete_local_branch(original_root, recorded_group.branch.as_deref()).await
    } else {
        (false, None)
    };

    Ok(RemoveSubmitWorktreeResult {
        worktree_path: target_str,
        removed: true,
        already_absent: false,
        branch_deleted,
        branch_note,
    })
}

async fn try_safe_delete_local_branch(
    original_root: &Path,
    branch_opt: Option<&str>,
) -> (bool, Option<String>) {
    let Some(branch) = branch_opt.map(str::trim).filter(|b| !b.is_empty()) else {
        return (false, None);
    };
    if matches!(branch, "main" | "master" | "HEAD") {
        return (false, Some(format!("Kept protected branch `{branch}`.")));
    }
    let ref_name = format!("refs/heads/{branch}");
    let Ok((exists, _, _)) = run_git_bytes(
        original_root,
        &["show-ref", "--verify", "--quiet", &ref_name],
        false,
        true,
    )
    .await
    else {
        return (
            false,
            Some(format!("Could not verify local branch `{branch}`.")),
        );
    };
    if !exists {
        return (
            false,
            Some(format!("Local branch `{branch}` already absent.")),
        );
    }
    // Safe merged-only delete (`-d`, NEVER `-D`):
    match run_git_bytes(original_root, &["branch", "-d", "--", branch], false, true).await {
        Ok((true, _, _)) => (true, None),
        Ok((false, _, err)) => (
            false,
            Some(format!(
                "Kept local branch `{branch}` because safe `git branch -d` refused removal: {err}"
            )),
        ),
        Err(err) => (false, Some(err)),
    }
}
