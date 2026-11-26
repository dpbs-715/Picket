//! C-Push commit step execution, session persistence, and recovery localization.

use crate::i18n::{LanguageKind, tr};

pub const fn cpush_partial_badge(locale: LanguageKind) -> &'static str {
    tr(locale, "Staged only", "仅已暂存")
}

pub const fn cpush_commit_completed_fallback(locale: LanguageKind) -> &'static str {
    tr(locale, "Commit completed", "提交已完成")
}

pub const fn cpush_commit_changes_fallback(locale: LanguageKind) -> &'static str {
    tr(locale, "Commit changes", "提交改动")
}

pub const fn cpush_sessions_unreadable_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Could not read local C-Push session history; started with empty session history.",
        "无法读取本地 C-Push 会话记录，已从空白会话记录启动。",
    )
}

pub const fn cpush_sessions_corrupted_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local C-Push session file is corrupted or incompatible; ignored stale entries without overwriting the original file.",
        "本地 C-Push 会话记录损坏或版本不兼容，已忽略旧记录；原文件未被覆盖。",
    )
}

pub const fn cpush_active_session_busy_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Current worktree already has an active session item; handle it in the message bar below first."
        }
        (LanguageKind::En, true) => {
            "Current worktree already has an active C-Push session; continue handling it in the message bar below."
        }
        (LanguageKind::Zh, false) => "当前工作树已有未结束的会话事项，请先在下方消息栏处理。",
        (LanguageKind::Zh, true) => "当前工作树已有未结束的 C-Push 会话，请在下方消息栏继续处理。",
    }
}

pub fn cpush_timeline_local_commit_titled(
    locale: LanguageKind,
    short_oid: &str,
    title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Local commit {short_oid}: {title}"),
        LanguageKind::Zh => format!("本地提交 {short_oid}：{title}"),
    }
}

pub fn cpush_timeline_local_commit_written(locale: LanguageKind, short_oid: &str) -> String {
    match locale {
        LanguageKind::En => format!("Local commit {short_oid} written"),
        LanguageKind::Zh => format!("本地提交 {short_oid} 写入成功"),
    }
}

pub fn cpush_timeline_commit_item(locale: LanguageKind, short_oid: &str, title: &str) -> String {
    match locale {
        LanguageKind::En => format!("Commit {short_oid}: {title}"),
        LanguageKind::Zh => format!("提交 {short_oid}：{title}"),
    }
}

pub fn cpush_timeline_step_done(
    locale: LanguageKind,
    index: usize,
    total: usize,
    short_oid: &str,
    title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Commit {index}/{total}: {short_oid} {title}"),
        LanguageKind::Zh => format!("提交 {index}/{total}：{short_oid} {title}"),
    }
}

pub fn cpush_timeline_step_writing(
    locale: LanguageKind,
    index: usize,
    total: usize,
    step_title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Writing commit {index}/{total}: {step_title}"),
        LanguageKind::Zh => format!("正在写入提交 {index}/{total}：{step_title}"),
    }
}

pub fn cpush_progress_step_title(locale: LanguageKind, index: usize, total: usize) -> String {
    match locale {
        LanguageKind::En => format!("Writing local commit {index}/{total}…"),
        LanguageKind::Zh => format!("正在写入本地提交 {index}/{total}…"),
    }
}

pub fn cpush_progress_step_scope(locale: LanguageKind, branch: &str, file_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if file_count == 1 { "file" } else { "files" };
            format!("{branch} · {file_count} {noun}")
        }
        LanguageKind::Zh => format!("{branch} · {file_count} 个文件"),
    }
}

pub fn cpush_progress_step_label(locale: LanguageKind, index: usize, total: usize) -> String {
    match locale {
        LanguageKind::En => format!("Write commit {index}/{total} · Verify scope & Git hooks"),
        LanguageKind::Zh => format!("写入提交 {index}/{total} · 核对范围与 Git 钩子"),
    }
}

pub fn cpush_timeline_step_paused(
    locale: LanguageKind,
    index: usize,
    total: usize,
    step_title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Commit {index}/{total} paused: {step_title}"),
        LanguageKind::Zh => format!("提交 {index}/{total} 暂停：{step_title}"),
    }
}

pub fn cpush_step_paused_title(locale: LanguageKind, completed: usize, total: usize) -> String {
    let current = completed + 1;
    match locale {
        LanguageKind::En => {
            if completed > 0 {
                format!("Completed {completed}/{total} commits; commit {current} paused")
            } else {
                format!("Commit 1/{total} paused")
            }
        }
        LanguageKind::Zh => {
            if completed > 0 {
                format!("已完成 {completed}/{total} 个提交，提交 {current} 暂停")
            } else {
                format!("提交 1/{total} 暂停")
            }
        }
    }
}

pub fn cpush_step_paused_desc(locale: LanguageKind, err: &str, has_completed: bool) -> String {
    match (locale, has_completed) {
        (LanguageKind::En, true) => format!(
            "{err}. Completed local commits are safely preserved; retry remaining steps after fixing the issue without duplicating finished commits."
        ),
        (LanguageKind::En, false) => format!(
            "{err}. No new commit was created, and your draft and selection were preserved; retry remaining steps after fixing the issue."
        ),
        (LanguageKind::Zh, true) => format!(
            "{err}。已完成的本地提交已安全保留；修复后可重试剩余步骤，不会重复创建已有提交。"
        ),
        (LanguageKind::Zh, false) => format!(
            "{err}。未创建新提交，草稿与选择已保留；修复后可重试剩余步骤，不会重复创建已有提交。"
        ),
    }
}

pub fn cpush_step_blocked_desc(locale: LanguageKind, err: &str, has_completed: bool) -> String {
    match (locale, has_completed) {
        (LanguageKind::En, true) => format!(
            "{err}. Completed local commits are safely preserved; handle the remaining changes in Git / IDE."
        ),
        (LanguageKind::En, false) => format!(
            "{err}. No new commit was created, and your draft and selection were preserved."
        ),
        (LanguageKind::Zh, true) => {
            format!("{err}。已完成的本地提交已安全保留；剩余变更请在 Git / IDE 中处理。")
        }
        (LanguageKind::Zh, false) => format!("{err}。未创建新提交，草稿与选择已保留。"),
    }
}

pub fn cpush_step_scope(locale: LanguageKind, current: usize, total: usize) -> String {
    match locale {
        LanguageKind::En => format!("Commit {current}/{total}"),
        LanguageKind::Zh => format!("提交 {current}/{total}"),
    }
}

pub const fn cpush_interrupted_push_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Previous session interrupted after local commits; push not yet confirmed",
        "上次会话已完成本地提交后中断，尚未确认推送",
    )
}

pub const fn cpush_interrupted_push_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified live Git history and kept completed commits without auto-rerunning push. Click \"Retry push\" to push or keep local commits.",
        "已核对真实 Git 历史并保留已完成提交，未自动重跑推送。请确认后点击「重试推送」或保留本地提交。",
    )
}

pub fn cpush_interrupted_commits_title(locale: LanguageKind, completed_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if completed_count == 0 {
                "Previous C-Push session was interrupted before writing".into()
            } else if completed_count == 1 {
                "Previous session interrupted after 1 commit".into()
            } else {
                format!("Previous session interrupted after {completed_count} commits")
            }
        }
        LanguageKind::Zh => {
            if completed_count == 0 {
                "上次 C-Push 会话在写入前中断".into()
            } else {
                format!("上次会话已完成 {completed_count} 个提交后中断")
            }
        }
    }
}

pub const fn cpush_interrupted_commits_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified live Git state without auto-rerunning writes. Confirm whether to continue remaining steps or end this session.",
        "已核对真实 Git 状态，未自动重跑写入。请确认继续剩余步骤或结束本次会话。",
    )
}

pub fn cpush_dismiss_kept_local_title(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if count == 1 {
                "Kept 1 local commit and finished session".into()
            } else {
                format!("Kept {count} local commits and finished session")
            }
        }
        LanguageKind::Zh => format!("已保留 {count} 个本地提交并结束本次会话"),
    }
}

pub const fn cpush_dismiss_kept_local_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local commits remain on the current branch without pushing to remote; remaining working tree drafts are unchanged.",
        "本地提交已保留在当前分支，未继续推送远端；其余工作区草稿保持不变。",
    )
}

pub const fn cpush_option_confirm_continue_remaining(locale: LanguageKind) -> &'static str {
    tr(locale, "Continue remaining commits", "确认继续剩余提交")
}

pub const fn cpush_option_dismiss_interrupted(
    locale: LanguageKind,
    has_commits: bool,
) -> &'static str {
    match (locale, has_commits) {
        (LanguageKind::En, false) => "End interrupted session",
        (LanguageKind::En, true) => "End session (keep completed commits)",
        (LanguageKind::Zh, false) => "结束中断会话",
        (LanguageKind::Zh, true) => "结束会话（保留已完成提交）",
    }
}

pub const fn cpush_option_retry_remaining(locale: LanguageKind) -> &'static str {
    tr(locale, "Retry remaining commits", "重试剩余提交")
}

pub const fn cpush_option_close_or_keep_completed(
    locale: LanguageKind,
    has_completed: bool,
) -> &'static str {
    match (locale, has_completed) {
        (LanguageKind::En, true) => "End session (keep completed commits)",
        (LanguageKind::En, false) => "Close session",
        (LanguageKind::Zh, true) => "结束会话（保留已完成提交）",
        (LanguageKind::Zh, false) => "关闭本次会话",
    }
}

pub const fn cpush_option_keep_local_end(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Keep local commits and finish",
        "保留本地提交并结束",
    )
}

pub const fn cpush_no_retry_remaining_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session has no remaining local commit steps to retry",
        "当前会话没有可重试的剩余本地提交步骤",
    )
}
