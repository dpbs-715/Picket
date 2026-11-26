//! C-Push push, upstream selection, pull & rebase, and completion localization.

use crate::i18n::{LanguageKind, join_list, tr};

pub const fn cpush_option_retry_push(locale: LanguageKind) -> &'static str {
    tr(locale, "Retry push", "重试推送")
}

pub const fn cpush_option_continue_push(locale: LanguageKind) -> &'static str {
    tr(locale, "Continue push", "继续推送")
}

pub const fn cpush_option_skip_push_keep_local(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Do not push yet (keep local commits)",
        "暂不推送（保留本地提交）",
    )
}

pub const fn cpush_option_keep_local_only(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Keep local commits only (do not push)",
        "仅保留本地提交（暂不推送）",
    )
}

pub fn cpush_timeline_push_failed(locale: LanguageKind, remote_target: &str) -> String {
    match locale {
        LanguageKind::En => format!("Push to {remote_target} failed"),
        LanguageKind::Zh => format!("推送至 {remote_target} 失败"),
    }
}

pub const fn cpush_push_failed_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Committed locally, push failed",
        "已在本地提交，推送失败",
    )
}

pub fn cpush_push_failed_description(
    locale: LanguageKind,
    remote_target: &str,
    reason: &str,
) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commits were written to branch history, but pushing to {remote_target} failed ({reason}). Click \"Retry push\" to push existing commits without recommitting."
        ),
        LanguageKind::Zh => format!(
            "本地提交已成功写入历史，推送至 {remote_target} 未成功（{reason}）。点击「重试推送」仅推送已有提交，不会重复提交。"
        ),
    }
}

pub fn cpush_push_stage_scope(locale: LanguageKind, remote_target: &str) -> String {
    match locale {
        LanguageKind::En => format!("Stage: push to {remote_target}"),
        LanguageKind::Zh => format!("阶段：推送至 {remote_target}"),
    }
}

pub fn cpush_push_track_label(locale: LanguageKind, remote: &str, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Push and track {remote}/{branch}"),
        LanguageKind::Zh => format!("推送并跟踪 {remote}/{branch}"),
    }
}

pub fn cpush_pushed_done_title(locale: LanguageKind, count: usize, remote_branch: &str) -> String {
    match locale {
        LanguageKind::En => {
            if count == 1 {
                format!("1 commit pushed to {remote_branch}")
            } else {
                format!("{count} commits pushed to {remote_branch}")
            }
        }
        LanguageKind::Zh => format!("{count} 个提交已推送至 {remote_branch}"),
    }
}

pub fn cpush_pushed_done_description(locale: LanguageKind, skipped_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if skipped_count == 0 {
                "Unselected files and unstaged drafts of partially staged files remain safely preserved in the working tree.".into()
            } else if skipped_count == 1 {
                "Skipped 1 file as chosen; remaining unselected changes and unstaged drafts remain safely preserved in the working tree.".into()
            } else {
                format!(
                    "Skipped {skipped_count} files as chosen; remaining unselected changes and unstaged drafts remain safely preserved in the working tree."
                )
            }
        }
        LanguageKind::Zh => {
            if skipped_count == 0 {
                "未选文件与部分暂存文件的未暂存草稿均安全保留在工作区。".into()
            } else {
                format!(
                    "已按你的选择跳过 {skipped_count} 个文件；其余未选改动与未暂存草稿均安全保留在工作区。"
                )
            }
        }
    }
}

pub fn cpush_synced_scope(locale: LanguageKind, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("{branch} · Synced"),
        LanguageKind::Zh => format!("{branch} · 已同步"),
    }
}

pub fn cpush_timeline_terminal_pushed(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => format!("Detected push to {display} completed in terminal"),
        LanguageKind::Zh => format!("检测到终端已完成推送至 {display}"),
    }
}

pub fn cpush_terminal_pushed_title(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => format!("Pushed to {display} (completed in terminal)"),
        LanguageKind::Zh => format!("已推送至 {display}（终端已完成）"),
    }
}

pub const fn cpush_terminal_pushed_from_failed_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified with remote that the latest commits are already pushed; collapsed stale notice.",
        "经联网核对远端分支已包含最新提交，已自动收起旧提示。",
    )
}

pub const fn cpush_terminal_pushed_from_manual_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified remote branch includes the merged commits; archived session and collapsed previous prompt.",
        "经核对远端分支已包含合并后的最新提交，已自动归档会话并收起旧提问。",
    )
}

pub fn cpush_timeline_awaiting_continue_push(locale: LanguageKind, target_label: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Local commits not yet pushed to {target_label}; waiting for confirmation")
        }
        LanguageKind::Zh => format!("本地提交尚未推送至 {target_label}，等待确认继续推送"),
    }
}

pub const fn cpush_resolved_continue_push_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Conflict resolved — continue pushing?",
        "冲突已解决，是否继续推送？",
    )
}

pub fn cpush_resolved_continue_push_desc(locale: LanguageKind, target_label: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Conflicts are resolved, and commits have not yet been pushed to {target_label}. Click \"Continue push\" to push:"
        ),
        LanguageKind::Zh => {
            format!("检测到冲突已解决但尚未推送至 {target_label}。确认后点击「继续推送」：")
        }
    }
}

pub fn pull_rebase_missing_upstream_error(locale: LanguageKind, remotes: &[String]) -> String {
    if remotes.is_empty() {
        tr(
            locale,
            "No Git remote is configured for this repository; cannot pull and rebase. Draft and selection were preserved.",
            "当前仓库尚未配置任何 Git 远端（remote），无法执行拉取并变基；草稿与选择已保留。",
        )
        .into()
    } else {
        let joined = join_list(locale, remotes);
        match locale {
            LanguageKind::En => format!(
                "Current branch has no upstream tracking branch (available remotes: {joined}); cannot determine pull target. Draft and selection were preserved."
            ),
            LanguageKind::Zh => format!(
                "当前分支尚未设置跟踪远端分支（可用远端：{joined}），无法确定拉取目标；草稿与选择已保留。"
            ),
        }
    }
}

pub fn pull_rebase_dirty_overlap_error(
    locale: LanguageKind,
    upstream: &str,
    overlapping_paths: &[String],
) -> String {
    let joined = join_list(locale, overlapping_paths);
    match locale {
        LanguageKind::En => format!(
            "Updates from {upstream} touch uncommitted local changes in {joined} and cannot rebase without disturbing staged/unstaged boundaries; paused safely with drafts and selections intact."
        ),
        LanguageKind::Zh => format!(
            "远端 {upstream} 的更新涉及当前存在未提交改动的 {joined}，无法在不损伤暂存/草稿边界的前提下自动变基；已暂停操作，草稿与选择完整保留。"
        ),
    }
}

pub const fn cpush_enter_remote_or_select_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter a remote name or click a push target option above",
        "请输入远端名称或点击上方推送目标选项",
    )
}

pub fn cpush_ambiguous_remote_input_error(locale: LanguageKind, available: &[String]) -> String {
    let joined = join_list(locale, available);
    match locale {
        LanguageKind::En => format!(
            "Could not uniquely identify a target remote from input (available remotes: {joined}); click the remote button or type its exact name."
        ),
        LanguageKind::Zh => format!(
            "未能从输入中唯一确认远端目标（当前可用远端：{joined}），请直接点击对应远端按钮或明确写出远端名。"
        ),
    }
}

pub const fn cpush_timeline_pushing(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Checking remote status and pushing local commits…",
        "正在核对远端状态并推送本地提交…",
    )
}

pub const fn cpush_progress_pushing_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pushing local commits to remote…",
        "正在推送本地提交至远端…",
    )
}

pub const fn cpush_progress_pushing_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pushing only local commits on the current branch without creating duplicate commits.",
        "仅推送当前分支上的本地提交，不会重复创建提交。",
    )
}

pub const fn cpush_progress_pushing_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Check remote -> Safe sync -> Push branch",
        "核对远端 → 安全同步 → 推送分支",
    )
}

pub const fn cpush_no_remote_configured_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No Git remote is configured for this repository",
        "当前仓库尚未配置任何 Git 远端（remote）",
    )
}

pub const fn cpush_no_remote_configured_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Committed locally, no Git remote configured",
        "已在本地提交，尚未配置 Git 远端",
    )
}

pub const fn cpush_timeline_waiting_upstream_choice(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Waiting to confirm upstream remote target",
        "等待确认新推送远端目标",
    )
}

pub const fn cpush_missing_upstream_question_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No upstream tracking branch configured — confirm push target?",
        "尚未配置跟踪远端分支，确认推送目标？",
    )
}

pub fn cpush_missing_upstream_question_desc(locale: LanguageKind, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commits were written to {branch}, which has no upstream yet. Confirm the remote target to track and push to:"
        ),
        LanguageKind::Zh => format!(
            "本地提交已写入 {branch}，但尚未配置 upstream。请明确确认要建立跟踪并推送的远端目标："
        ),
    }
}

pub const fn cpush_missing_upstream_short_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No upstream tracking branch configured",
        "尚未配置跟踪远端分支",
    )
}

pub fn cpush_branch_scope(locale: LanguageKind, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Branch: {branch}"),
        LanguageKind::Zh => format!("分支：{branch}"),
    }
}

pub const fn cpush_rebased_push_failed_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Sync rebase completed, but push failed",
        "已完成同步变基，但推送失败",
    )
}

pub fn cpush_dirty_overlap_push_failed_reason(
    locale: LanguageKind,
    overlapping_paths: &[String],
) -> String {
    let joined = join_list(locale, overlapping_paths);
    match locale {
        LanguageKind::En => format!(
            "Remote has new commits touching uncommitted local files ({joined}); paused automatic rebase to preserve staged/unstaged drafts. Handle those files and retry push"
        ),
        LanguageKind::Zh => format!(
            "远端有新提交且涉及当前未提交文件（{joined}），为保全暂存/未暂存草稿已暂停自动变基；请处理该文件后重试推送"
        ),
    }
}

pub const fn cpush_dirty_overlap_push_failed_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Committed locally; resolve overlapping draft before sync rebase",
        "已在本地提交，同步变基需先处理重叠草稿",
    )
}

pub const fn cpush_published_history_risk_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Published history risk detected; paused automatic rebase and push",
        "检测到已发布历史风险，已暂停自动变基与推送",
    )
}

pub fn cpush_timeline_rebased_synced(locale: LanguageKind, remote_branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Safely rebased and synced onto {remote_branch}"),
        LanguageKind::Zh => format!("已安全变基并同步至 {remote_branch}"),
    }
}
