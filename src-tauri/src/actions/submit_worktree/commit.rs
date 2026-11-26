use super::inspect::{run_git_bytes, validate_repo_relative_path};
use serde::Serialize;
use std::{collections::HashSet, io::Write, path::Path};
use tempfile::NamedTempFile;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IsolatedCommitResult {
    pub oid: String,
    pub short_oid: String,
    pub title: String,
    pub branch: String,
    pub pushed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_error: Option<String>,
}

pub async fn commit_in_isolated_worktree(
    worktree_path: &Path,
    allowed_paths: &[String],
    message: &str,
    push: bool,
    remote: Option<&str>,
) -> Result<IsolatedCommitResult, String> {
    if !worktree_path.exists() {
        return Err(format!(
            "Isolated submit worktree does not exist: {}",
            worktree_path.display()
        ));
    }
    let clean_msg = message.trim();
    if clean_msg.is_empty() {
        return Err("Commit message cannot be empty.".into());
    }
    if allowed_paths.is_empty() {
        return Err("No transferred paths recorded for this isolated submit worktree.".into());
    }
    let allowed_set: HashSet<&str> = allowed_paths.iter().map(String::as_str).collect();
    for p in allowed_paths {
        validate_repo_relative_path(p)?;
    }

    // Verify worktree only has changes inside `allowed_paths`.
    let (ok_st, st_out, st_err) = run_git_bytes(
        worktree_path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        false,
        true,
    )
    .await?;
    if !ok_st {
        return Err(format!(
            "Failed to inspect isolated worktree status: {st_err}"
        ));
    }
    let mut dirty_paths = Vec::new();
    let mut iter = st_out.split(|b| *b == 0).filter(|r| !r.is_empty());
    while let Some(rec) = iter.next() {
        if rec.len() < 4 {
            continue;
        }
        let xy = std::str::from_utf8(&rec[..2]).unwrap_or("");
        let path = std::str::from_utf8(&rec[3..])
            .map_err(|_| "Non-UTF-8 path in isolated worktree status")?;
        dirty_paths.push(path.to_owned());
        if (xy.starts_with('R') || xy.starts_with('C'))
            && let Some(prev) = iter.next()
            && let Ok(prev_str) = std::str::from_utf8(prev)
        {
            dirty_paths.push(prev_str.to_owned());
        }
    }
    if dirty_paths.is_empty() {
        return Err("Isolated submit worktree has no changes to commit.".into());
    }
    for dirty in &dirty_paths {
        if !allowed_set.contains(dirty.as_str()) {
            return Err(format!(
                "Isolated worktree contains unexpected path `{dirty}` outside group scope; refusing commit."
            ));
        }
    }

    // Stage ONLY literal allowed paths (`git add -A -- <paths>` stages modifications, additions, and deletions for ONLY those exact paths).
    let mut add_args: Vec<&str> = vec!["add", "-A", "--"];
    for p in allowed_paths {
        add_args.push(p.as_str());
    }
    let (ok_add, _, err_add) = run_git_bytes(worktree_path, &add_args, false, true).await?;
    if !ok_add {
        return Err(format!(
            "Failed to stage exact paths in isolated worktree: {err_add}"
        ));
    }

    // Commit via temp message file with REAL Git hooks enabled (never `--no-verify`!).
    let mut msg_file = NamedTempFile::new()
        .map_err(|err| format!("Failed to create temporary commit message file: {err}"))?;
    msg_file
        .write_all(clean_msg.as_bytes())
        .map_err(|err| format!("Failed to write commit message file: {err}"))?;
    msg_file
        .flush()
        .map_err(|err| format!("Failed to flush commit message file: {err}"))?;
    let msg_path_str = msg_file.path().to_string_lossy().into_owned();

    let (ok_commit, _, err_commit) = run_git_bytes(
        worktree_path,
        &["commit", "-F", &msg_path_str],
        false,
        false, // Run real repository hooks!
    )
    .await?;
    if !ok_commit {
        return Err(format!(
            "Isolated worktree commit failed (or rejected by Git hook): {err_commit}"
        ));
    }

    let (_, oid_out, _) = run_git_bytes(worktree_path, &["rev-parse", "HEAD"], false, true).await?;
    let oid = String::from_utf8_lossy(&oid_out).trim().to_owned();
    let short_oid: String = oid.chars().take(7).collect();
    let (_, br_out, _) = run_git_bytes(
        worktree_path,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        false,
        true,
    )
    .await?;
    let branch = String::from_utf8_lossy(&br_out).trim().to_owned();
    let title = clean_msg
        .lines()
        .next()
        .unwrap_or(clean_msg)
        .trim()
        .to_owned();

    let mut pushed = false;
    let mut remote_branch = None;
    let mut push_error = None;
    if push {
        let remote_name = remote
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("origin");
        let (ok_push, _, err_push) = run_git_bytes(
            worktree_path,
            &["push", "-u", remote_name, &branch],
            true,
            false,
        )
        .await?;
        if ok_push {
            pushed = true;
            remote_branch = Some(format!("{remote_name}/{branch}"));
        } else {
            push_error = Some(err_push);
        }
    }

    Ok(IsolatedCommitResult {
        oid,
        short_oid,
        title,
        branch,
        pushed,
        remote_branch,
        push_error,
    })
}
