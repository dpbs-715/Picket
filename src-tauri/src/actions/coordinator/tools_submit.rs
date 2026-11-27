//! Isolated submit-worktree management, duplicate cleanup, and checkpoint recording tool handlers.

use super::context::{ActionToolContext, append_unique_links, push_timeline_step};
use crate::{
    actions::{
        session_store::{
            self, SubmitGroupCheckpoint, SubmitPromotionCheckpoint, SubmitSessionLedger,
        },
        submit_cleanup::cleanup_verified_submit_duplicates,
        submit_worktree::{
            commit_in_isolated_worktree, create_isolated_submit_worktree, parse_worktree_list,
            remove_verified_submit_worktree,
        },
        types::CustomActionLink,
    },
    cpush::types::CPushLandedCommit,
    foundation::pi::PiToolExecutionResult,
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

fn find_group_for_worktree_or_id(
    storage_dir: &Path,
    repository_id: &str,
    ledger: &SubmitSessionLedger,
    group_id: Option<&str>,
    worktree_path: Option<&str>,
) -> Option<SubmitGroupCheckpoint> {
    if let Some(wt) = worktree_path {
        let canon = std::fs::canonicalize(wt).unwrap_or_else(|_| PathBuf::from(wt));
        if let Some(group) = ledger.groups.iter().find(|g| {
            g.worktree_path.as_deref().is_some_and(|p| {
                std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)) == canon
            })
        }) {
            return Some(group.clone());
        }
        if let Some((_, group)) =
            session_store::find_owned_worktree_group(storage_dir, repository_id, Path::new(wt))
        {
            return Some(group);
        }
    }
    if let Some(gid) = group_id {
        if let Some(group) = ledger.groups.iter().find(|g| g.group_id == gid) {
            return Some(group.clone());
        }
        for session in session_store::load_all_sessions(storage_dir) {
            if session.repository_id == repository_id
                && let Some(group) = session
                    .submit_ledger
                    .groups
                    .into_iter()
                    .find(|g| g.group_id == gid)
            {
                return Some(group);
            }
        }
    }
    None
}

pub(super) async fn handle_manage_submit_worktree(
    ctx: &ActionToolContext,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let op = args.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "create" => handle_worktree_create(ctx, args, status_label).await,
        "commit" => handle_worktree_commit(ctx, args).await,
        "list" => handle_worktree_list(ctx, status_label).await,
        "remove" => handle_worktree_remove(ctx, args, status_label).await,
        other => PiToolExecutionResult::error(format!(
            "Unsupported manage_submit_worktree op `{other}` (expected create | commit | list | remove)."
        )),
    }
}

async fn handle_worktree_create(
    ctx: &ActionToolContext,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let group_id = args
        .get("groupId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let branch = args
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let base_ref = args
        .get("baseRef")
        .and_then(Value::as_str)
        .unwrap_or("HEAD")
        .trim()
        .to_owned();
    let reuse_existing = args
        .get("reuseExistingBranch")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let requested_ids: Vec<String> = args
        .get("fileIds")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if group_id.is_empty() || branch.is_empty() || requested_ids.is_empty() {
        return PiToolExecutionResult::error(
            "manage_submit_worktree(create) requires non-empty `groupId`, `branch`, and `fileIds`.",
        );
    }
    let mut matched_changes = Vec::with_capacity(requested_ids.len());
    for fid in &requested_ids {
        if !ctx.frozen_selections.iter().any(|s| s.file_id == *fid) {
            return PiToolExecutionResult::error(format!(
                "File `{fid}` is outside the user's selected scope (or is in Local Keep). Never transfer unselected files."
            ));
        }
        let Some(change) = ctx.live_changes.iter().find(|c| c.id == *fid) else {
            return PiToolExecutionResult::error(format!(
                "Selected file `{fid}` was not found in the active worktree snapshot."
            ));
        };
        matched_changes.push(change.clone());
    }
    match create_isolated_submit_worktree(
        &ctx.storage_dir,
        &ctx.target.repository_id,
        &ctx.root,
        &group_id,
        &branch,
        &base_ref,
        reuse_existing,
        &matched_changes,
    )
    .await
    {
        Ok(created) => {
            if let Ok(mut st) = ctx.run_state.lock() {
                st.wrote_repo = true;
                if st.submit_ledger.workflow_kind.is_empty() {
                    st.submit_ledger.workflow_kind = "submit".into();
                }
                if st.submit_ledger.original_branch.is_none() {
                    st.submit_ledger.original_branch = Some(created.original_branch.clone());
                }
                if st.submit_ledger.original_head.is_none() {
                    st.submit_ledger.original_head = Some(created.original_head.clone());
                }
                if let Some(existing) = st
                    .submit_ledger
                    .groups
                    .iter_mut()
                    .find(|g| g.group_id == created.group_id)
                {
                    if !title.is_empty() {
                        existing.title = title;
                    }
                    existing.branch = Some(created.branch.clone());
                    existing.base_ref = Some(created.base_ref.clone());
                    existing.worktree_path = Some(created.worktree_path.clone());
                    existing.worktree_removed = false;
                    existing.transferred_paths = created.transferred_paths.clone();
                    existing.preflight_snapshots = created.preflight_snapshots.clone();
                } else {
                    st.submit_ledger.groups.push(SubmitGroupCheckpoint {
                        group_id: created.group_id.clone(),
                        title,
                        issue_url: None,
                        issue_number: None,
                        issue_closed: false,
                        branch: Some(created.branch.clone()),
                        base_ref: Some(created.base_ref.clone()),
                        remote: None,
                        verified_head_sha: None,
                        worktree_path: Some(created.worktree_path.clone()),
                        worktree_removed: false,
                        pr_url: None,
                        pr_number: None,
                        pr_state: None,
                        transferred_paths: created.transferred_paths.clone(),
                        preflight_snapshots: created.preflight_snapshots.clone(),
                    });
                }
                push_timeline_step(&mut st.timeline, "done", status_label);
            }
            (ctx.persist)("running", String::new());
            let body = serde_json::to_string_pretty(&created).unwrap_or_else(|_| "{}".into());
            PiToolExecutionResult::success(body)
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

async fn handle_worktree_commit(ctx: &ActionToolContext, args: &Value) -> PiToolExecutionResult {
    let group_id = args.get("groupId").and_then(Value::as_str);
    let worktree_path = args.get("worktreePath").and_then(Value::as_str);
    let message = args
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let push = args.get("push").and_then(Value::as_bool).unwrap_or(false);
    let remote = args
        .get("remote")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let ledger = ctx
        .run_state
        .lock()
        .map(|st| st.submit_ledger.clone())
        .unwrap_or_default();
    let Some(group) = find_group_for_worktree_or_id(
        &ctx.storage_dir,
        &ctx.target.repository_id,
        &ledger,
        group_id,
        worktree_path,
    ) else {
        return PiToolExecutionResult::error(
            "No Picket-owned isolated submit worktree found for the specified groupId/worktreePath.",
        );
    };
    let Some(ref wt_path) = group.worktree_path else {
        return PiToolExecutionResult::error("Recorded submit group has no worktreePath.");
    };
    match commit_in_isolated_worktree(
        Path::new(wt_path),
        &group.transferred_paths,
        &message,
        push,
        remote,
    )
    .await
    {
        Ok(res) => {
            let remote_name = remote.unwrap_or("origin").to_owned();
            if let Ok(mut st) = ctx.run_state.lock() {
                st.wrote_repo = true;
                if let Some(existing) = st
                    .submit_ledger
                    .groups
                    .iter_mut()
                    .find(|g| g.group_id == group.group_id)
                {
                    existing.verified_head_sha = Some(res.oid.clone());
                    existing.branch = Some(res.branch.clone());
                    if res.pushed {
                        existing.remote = Some(remote_name);
                    }
                } else {
                    let mut updated = group.clone();
                    updated.verified_head_sha = Some(res.oid.clone());
                    updated.branch = Some(res.branch.clone());
                    if res.pushed {
                        updated.remote = Some(remote_name);
                    }
                    st.submit_ledger.groups.push(updated);
                }
                st.commits.push(CPushLandedCommit {
                    oid: res.oid.clone(),
                    short_oid: res.short_oid.clone(),
                    title: res.title.clone(),
                    branch: res.branch.clone(),
                    file_ids: Vec::new(),
                });
                push_timeline_step(
                    &mut st.timeline,
                    "done",
                    format!("{} {}", res.short_oid, res.title),
                );
            }
            (ctx.persist)("running", String::new());
            let body = serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into());
            PiToolExecutionResult::success(body)
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

async fn handle_worktree_list(
    ctx: &ActionToolContext,
    status_label: String,
) -> PiToolExecutionResult {
    let mut all_groups = ctx
        .run_state
        .lock()
        .map(|st| st.submit_ledger.groups.clone())
        .unwrap_or_default();
    for session in session_store::load_all_sessions(&ctx.storage_dir) {
        if session.repository_id != ctx.target.repository_id {
            continue;
        }
        for g in session.submit_ledger.groups {
            if !all_groups.iter().any(|existing| {
                existing.worktree_path == g.worktree_path && existing.group_id == g.group_id
            }) {
                all_groups.push(g);
            }
        }
    }
    match parse_worktree_list(&ctx.root, &all_groups).await {
        Ok(entries) => {
            if let Ok(mut st) = ctx.run_state.lock() {
                push_timeline_step(&mut st.timeline, "done", status_label);
            }
            let body = serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".into());
            PiToolExecutionResult::success(body)
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

async fn handle_worktree_remove(
    ctx: &ActionToolContext,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let group_id = args.get("groupId").and_then(Value::as_str);
    let worktree_path = args.get("worktreePath").and_then(Value::as_str);
    let expected_head_sha = args
        .get("expectedHeadSha")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let delete_local_branch = args
        .get("deleteLocalBranch")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ledger = ctx
        .run_state
        .lock()
        .map(|st| st.submit_ledger.clone())
        .unwrap_or_default();
    let Some(group) = find_group_for_worktree_or_id(
        &ctx.storage_dir,
        &ctx.target.repository_id,
        &ledger,
        group_id,
        worktree_path,
    ) else {
        return PiToolExecutionResult::error(
            "Refused to remove worktree: not owned by any Picket submit session.",
        );
    };
    match remove_verified_submit_worktree(&ctx.root, &group, expected_head_sha, delete_local_branch)
        .await
    {
        Ok(res) => {
            if let Ok(mut st) = ctx.run_state.lock() {
                st.wrote_repo = true;
                if let Some(existing) = st
                    .submit_ledger
                    .groups
                    .iter_mut()
                    .find(|g| g.group_id == group.group_id)
                {
                    existing.worktree_removed = true;
                }
                push_timeline_step(&mut st.timeline, "done", status_label);
            }
            (ctx.persist)("running", String::new());
            if let Ok(snap) = ctx.store.refresh_with_locale(&ctx.target, ctx.locale).await {
                (ctx.emit)("running", None, None, Some(snap));
            }
            let body = serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into());
            PiToolExecutionResult::success(body)
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

pub(super) async fn handle_cleanup_submit_duplicates(
    ctx: &ActionToolContext,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let user_confirmed = args
        .get("userConfirmed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !user_confirmed {
        return PiToolExecutionResult::error(
            "Step 5 duplicate cleanup is opt-in: ask the user via `message_user` (with `questions`) first, then invoke `cleanup_submit_duplicates` with `userConfirmed: true`.",
        );
    }
    let filter_ids: HashSet<String> = args
        .get("groupIds")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let ledger = ctx
        .run_state
        .lock()
        .map(|st| st.submit_ledger.clone())
        .unwrap_or_default();
    let Some(orig_branch) = ledger.original_branch.as_deref() else {
        return PiToolExecutionResult::error(
            "No recorded original branch for Step 5 cleanup; create isolated submit worktrees first.",
        );
    };
    let Some(orig_head) = ledger.original_head.as_deref() else {
        return PiToolExecutionResult::error(
            "No recorded original HEAD for Step 5 cleanup; create isolated submit worktrees first.",
        );
    };
    let groups: Vec<SubmitGroupCheckpoint> = ledger
        .groups
        .iter()
        .filter(|g| filter_ids.is_empty() || filter_ids.contains(&g.group_id))
        .cloned()
        .collect();
    match cleanup_verified_submit_duplicates(&ctx.root, orig_branch, orig_head, &groups).await {
        Ok(outcome) => {
            if let Ok(mut st) = ctx.run_state.lock() {
                if !outcome.cleaned_paths.is_empty() {
                    st.wrote_repo = true;
                }
                st.submit_ledger.cleanup = Some(outcome.to_record());
                push_timeline_step(&mut st.timeline, "done", status_label);
            }
            (ctx.persist)("running", String::new());
            if let Ok(snap) = ctx.store.refresh_with_locale(&ctx.target, ctx.locale).await {
                (ctx.emit)("running", None, None, Some(snap));
            }
            let body = serde_json::to_string_pretty(&outcome).unwrap_or_else(|_| "{}".into());
            PiToolExecutionResult::success(body)
        }
        Err(err) => PiToolExecutionResult::error(err),
    }
}

pub(super) fn handle_record_action_checkpoint(
    ctx: &ActionToolContext,
    args: &Value,
    status_label: String,
) -> PiToolExecutionResult {
    let mut added_links: Vec<CustomActionLink> = Vec::new();
    if let Ok(mut st) = ctx.run_state.lock() {
        if let Some(wk) = args.get("workflowKind").and_then(Value::as_str)
            && !wk.trim().is_empty()
        {
            st.submit_ledger.workflow_kind = wk.trim().to_owned();
        }
        if let Some(ft) = args.get("firstTarget").and_then(Value::as_str)
            && !ft.trim().is_empty()
        {
            st.submit_ledger.first_target = Some(ft.trim().to_owned());
        }
        if let Some(fin) = args.get("finalTarget").and_then(Value::as_str)
            && !fin.trim().is_empty()
        {
            st.submit_ledger.final_target = Some(fin.trim().to_owned());
        }
        if let Some(gobj) = args.get("group").and_then(Value::as_object)
            && let Some(gid) = gobj
                .get("groupId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            let entry = if let Some(existing) = st
                .submit_ledger
                .groups
                .iter_mut()
                .find(|g| g.group_id == gid)
            {
                existing
            } else {
                st.submit_ledger.groups.push(SubmitGroupCheckpoint {
                    group_id: gid.to_owned(),
                    title: String::new(),
                    issue_url: None,
                    issue_number: None,
                    issue_closed: false,
                    branch: None,
                    base_ref: None,
                    remote: None,
                    verified_head_sha: None,
                    worktree_path: None,
                    worktree_removed: false,
                    pr_url: None,
                    pr_number: None,
                    pr_state: None,
                    transferred_paths: Vec::new(),
                    preflight_snapshots: Vec::new(),
                });
                st.submit_ledger.groups.last_mut().unwrap()
            };
            if let Some(v) = gobj.get("title").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.title = v.trim().to_owned();
            }
            if let Some(v) = gobj.get("issueUrl").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.issue_url = Some(v.trim().to_owned());
                let label = gobj
                    .get("issueNumber")
                    .and_then(Value::as_str)
                    .map(|n| format!("Issue #{n}"))
                    .unwrap_or_else(|| "Issue".into());
                added_links.push(CustomActionLink {
                    label,
                    url: v.trim().to_owned(),
                });
            }
            if let Some(v) = gobj.get("issueNumber").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.issue_number = Some(v.trim().to_owned());
            }
            if let Some(v) = gobj.get("issueClosed").and_then(Value::as_bool) {
                entry.issue_closed = v;
            }
            if let Some(v) = gobj.get("branch").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.branch = Some(v.trim().to_owned());
            }
            if let Some(v) = gobj.get("baseRef").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.base_ref = Some(v.trim().to_owned());
            }
            if let Some(v) = gobj.get("remote").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.remote = Some(v.trim().to_owned());
            }
            if let Some(v) = gobj.get("verifiedHeadSha").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.verified_head_sha = Some(v.trim().to_owned());
            }
            if let Some(v) = gobj.get("prUrl").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.pr_url = Some(v.trim().to_owned());
                let label = gobj
                    .get("prNumber")
                    .and_then(Value::as_str)
                    .map(|n| format!("PR #{n}"))
                    .unwrap_or_else(|| "PR/MR".into());
                added_links.push(CustomActionLink {
                    label,
                    url: v.trim().to_owned(),
                });
            }
            if let Some(v) = gobj.get("prNumber").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.pr_number = Some(v.trim().to_owned());
            }
            if let Some(v) = gobj.get("prState").and_then(Value::as_str)
                && !v.trim().is_empty()
            {
                entry.pr_state = Some(v.trim().to_owned());
            }
        }
        if let Some(pobj) = args.get("promotion").and_then(Value::as_object) {
            let src = pobj
                .get("sourceBranch")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            let dst = pobj
                .get("targetBranch")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if !src.is_empty() && !dst.is_empty() {
                let prev = st.submit_ledger.promotion.clone();
                let pr_url = pobj
                    .get("prUrl")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_owned())
                    .or_else(|| prev.as_ref().and_then(|p| p.pr_url.clone()));
                let pr_number = pobj
                    .get("prNumber")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_owned())
                    .or_else(|| prev.as_ref().and_then(|p| p.pr_number.clone()));
                let pr_state = pobj
                    .get("prState")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_owned())
                    .or_else(|| prev.as_ref().and_then(|p| p.pr_state.clone()));
                let verified_head_sha = pobj
                    .get("verifiedHeadSha")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_owned())
                    .or_else(|| prev.as_ref().and_then(|p| p.verified_head_sha.clone()));
                if let Some(ref url) = pr_url {
                    added_links.push(CustomActionLink {
                        label: format!("{src} -> {dst}"),
                        url: url.clone(),
                    });
                }
                st.submit_ledger.promotion = Some(SubmitPromotionCheckpoint {
                    source_branch: src,
                    target_branch: dst,
                    pr_url,
                    pr_number,
                    pr_state,
                    verified_head_sha,
                });
            }
        }
        if let Some(links_arr) = args.get("links").and_then(Value::as_array) {
            for item in links_arr {
                if let (Some(label), Some(url)) = (
                    item.get("label").and_then(Value::as_str),
                    item.get("url").and_then(Value::as_str),
                ) && !url.trim().is_empty()
                {
                    added_links.push(CustomActionLink {
                        label: label.trim().to_owned(),
                        url: url.trim().to_owned(),
                    });
                }
            }
        }
        append_unique_links(&mut st.links, added_links);
        push_timeline_step(&mut st.timeline, "done", status_label);
    }
    (ctx.persist)("running", String::new());
    let summary_json = ctx
        .run_state
        .lock()
        .map(|st| {
            json!({
                "saved": true,
                "groupCount": st.submit_ledger.groups.len(),
                "linkCount": st.links.len(),
                "hasPromotion": st.submit_ledger.promotion.is_some(),
            })
            .to_string()
        })
        .unwrap_or_else(|_| "{\"saved\":true}".into());
    PiToolExecutionResult::success(summary_json)
}
