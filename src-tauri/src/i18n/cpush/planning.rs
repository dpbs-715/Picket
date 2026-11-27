//! C-Push planning, validation, and interactive decision localization.

use crate::i18n::{LanguageKind, join_list, tr};

pub fn cpush_scope_badge(locale: LanguageKind, total: usize, partial_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if total == 1 { "file" } else { "files" };
            if partial_count > 0 {
                format!("{total} selected {noun} · {partial_count} partially staged")
            } else {
                format!("{total} selected {noun}")
            }
        }
        LanguageKind::Zh => {
            if partial_count > 0 {
                format!("{total} 个已选文件 · 含 {partial_count} 项部分暂存")
            } else {
                format!("{total} 个已选文件")
            }
        }
    }
}

pub const fn cpush_needs_confirm_badge(locale: LanguageKind) -> &'static str {
    tr(locale, "Confirm", "待确认")
}

pub fn cpush_sensitive_question(
    locale: LanguageKind,
    names: &[String],
    has_other_files: bool,
) -> (
    String,
    String,
    String,
    Vec<crate::actions::types::CPushMessageOption>,
) {
    let joined = join_list(locale, names);
    let options = match (locale, has_other_files) {
        (LanguageKind::En, true) => vec![
            crate::actions::types::CPushMessageOption {
                id: "skip".into(),
                label: "Exclude these files and commit the rest".into(),
                recommended: Some(true),
            },
            crate::actions::types::CPushMessageOption {
                id: "include".into(),
                label: "Confirmed safe, commit and push together".into(),
                recommended: None,
            },
        ],
        (LanguageKind::En, false) => vec![
            crate::actions::types::CPushMessageOption {
                id: "skip".into(),
                label: "Skip for now and keep locally".into(),
                recommended: Some(true),
            },
            crate::actions::types::CPushMessageOption {
                id: "include".into(),
                label: "Confirmed safe, commit and push".into(),
                recommended: None,
            },
        ],
        (LanguageKind::Zh, true) => vec![
            crate::actions::types::CPushMessageOption {
                id: "skip".into(),
                label: "排除这些文件，仅提交其余改动".into(),
                recommended: Some(true),
            },
            crate::actions::types::CPushMessageOption {
                id: "include".into(),
                label: "确认安全，一并提交并推送".into(),
                recommended: None,
            },
        ],
        (LanguageKind::Zh, false) => vec![
            crate::actions::types::CPushMessageOption {
                id: "skip".into(),
                label: "暂不提交，保留在本地".into(),
                recommended: Some(true),
            },
            crate::actions::types::CPushMessageOption {
                id: "include".into(),
                label: "确认安全，继续提交并推送".into(),
                recommended: None,
            },
        ],
    };
    match locale {
        LanguageKind::En => (
            "Local environment or sensitive config detected — exclude?".into(),
            format!("Involves {joined}"),
            "Selected scope includes local environment or credential files (such as .env or key files) that usually should not be committed or pushed. Confirm how to proceed:".into(),
            options,
        ),
        LanguageKind::Zh => (
            "检测到本地环境或敏感配置文件，是否排除？".into(),
            format!("涉及 {joined}"),
            "已选范围包含本地环境或敏感凭据文件（如 .env / 密钥文件），通常不应入库推送。请确认处理方式：".into(),
            options,
        ),
    }
}

pub fn cpush_timeline_scope_frozen(locale: LanguageKind, badge: &str) -> String {
    match locale {
        LanguageKind::En => format!("Scope frozen: {badge}"),
        LanguageKind::Zh => format!("已固定范围：{badge}"),
    }
}

pub const fn cpush_timeline_planning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Analyzing file changes and planning split commits…",
        "正在分析改动归属并规划拆分提交…",
    )
}

pub const fn cpush_progress_planning_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Analyzing and splitting selected changes…",
        "正在分析并拆分已选改动…",
    )
}

pub const fn cpush_progress_planning_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Authorized scope is frozen; unselected files and unstaged drafts of partially staged files remain untouched.",
        "已固定本次授权范围；未选文件与部分暂存文件的未暂存草稿将保留在原处。",
    )
}

pub const fn cpush_progress_planning_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Read scope -> Plan split -> Local commits -> Push remote",
        "读取范围 → 规划拆分 → 本地提交 → 远端推送",
    )
}

pub const fn cpush_no_pending_session_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No pending C-Push session found for the current worktree",
        "未找到当前工作树的待处理会话",
    )
}

pub const fn cpush_mismatched_reply_target_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Session reply did not match target worktree or run identity; ignored request",
        "会话回复的目标工作树或任务标识不匹配，已忽略该请求",
    )
}

pub const fn cpush_not_interactive_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session is not in an interactive state",
        "当前会话不在可交互处理状态",
    )
}

pub const fn cpush_missing_active_message_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session has no pending message item",
        "当前会话缺少待处理事项",
    )
}

pub const fn cpush_stale_message_id_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Session prompt has already updated; confirm again from the latest prompt",
        "当前事项状态已更新，请基于最新提示重新确认",
    )
}

pub const fn cpush_unsupported_reply_state_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session state does not accept this reply",
        "当前会话状态不支持该回复操作",
    )
}

pub fn cpush_timeline_confirmed_decision(
    locale: LanguageKind,
    question: &str,
    answer: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Confirmed: {question} -> {answer}"),
        LanguageKind::Zh => format!("已确认：{question} → {answer}"),
    }
}

pub const fn cpush_timeline_replanning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Generating split commit plan from your decision…",
        "正在按你的决定生成拆分提交方案…",
    )
}

pub const fn cpush_progress_replanning_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Updating commit plan from your confirmation…",
        "正在按你的确认更新提交计划…",
    )
}

pub fn cpush_progress_replanning_desc(locale: LanguageKind, answer: &str) -> String {
    match locale {
        LanguageKind::En => format!("Recorded choice: {answer}"),
        LanguageKind::Zh => format!("已记录选择：{answer}"),
    }
}

pub const fn cpush_progress_replanning_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Read scope -> Replan -> Local commits -> Push remote",
        "读取范围 → 重新规划 → 本地提交 → 远端推送",
    )
}

pub fn cpush_timeline_frozen_count(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if count == 1 { "file" } else { "files" };
            format!("Frozen scope of {count} selected {noun}")
        }
        LanguageKind::Zh => format!("已固定 {count} 个已选文件范围"),
    }
}

pub fn cpush_timeline_waiting_decision(locale: LanguageKind, title: &str) -> String {
    match locale {
        LanguageKind::En => format!("Waiting for decision: {title}"),
        LanguageKind::Zh => format!("等待决定：{title}"),
    }
}

pub const fn cpush_timeline_all_skipped(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Skipped all selected files in this run as chosen",
        "已按你的选择跳过本次全部已选文件",
    )
}

pub const fn cpush_all_skipped_title(locale: LanguageKind) -> &'static str {
    tr(locale, "Skipped commit as confirmed", "已按确认跳过提交")
}

pub const fn cpush_all_skipped_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected files and working tree drafts remain in place; no new commit was written.",
        "所选文件与工作区草稿均保留在原处，未写入新提交。",
    )
}

pub const fn cpush_plan_empty_group_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push plan contains an empty file group or empty commit message",
        "C-Push 规划存在空文件组或空提交说明",
    )
}
