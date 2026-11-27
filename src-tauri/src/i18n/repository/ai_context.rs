//! AI diff extraction, patch trimming, and skipped-file localization.

use crate::i18n::{LanguageKind, tr};

pub fn patch_omitted_trailing(locale: LanguageKind, head: &str, omitted_lines: usize) -> String {
    match locale {
        LanguageKind::En => {
            format!("{head}\n... [{omitted_lines} trailing changed lines omitted] ...")
        }
        LanguageKind::Zh => format!("{head}\n... [省略后续 {omitted_lines} 行改动] ..."),
    }
}

pub fn patch_omitted_middle(
    locale: LanguageKind,
    head: &str,
    omitted_lines: usize,
    tail: &str,
) -> String {
    match locale {
        LanguageKind::En => {
            format!("{head}\n... [{omitted_lines} middle changed lines omitted] ...\n{tail}")
        }
        LanguageKind::Zh => {
            format!("{head}\n... [省略中间 {omitted_lines} 行改动] ...\n{tail}")
        }
    }
}

pub fn cpush_skip_lockfile(locale: LanguageKind, added: u64, removed: u64) -> String {
    match locale {
        LanguageKind::En => {
            format!("Lockfile (+{added} -{removed}, group with related dependency changes)")
        }
        LanguageKind::Zh => format!("锁文件（+{added} -{removed}，按关联依赖变更归组）"),
    }
}

pub const fn cpush_skip_binary_patch(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Binary file (patch omitted)",
        "二进制文件（不展开文本补丁）",
    )
}

pub const fn cpush_skip_binary(locale: LanguageKind) -> &'static str {
    tr(locale, "Binary file", "二进制文件")
}

pub const fn cpush_skip_non_utf8(locale: LanguageKind) -> &'static str {
    tr(locale, "Non-UTF-8 text", "非 UTF-8 编码文本")
}

pub const fn cpush_skip_large_untracked(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Large file (exceeds inline limit)",
        "大文件（超出单文件内联上限）",
    )
}

pub const fn cpush_skip_sensitive(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Suspected local env or credential file (values redacted locally; confirm exclusion with user before committing)",
        "疑似本地环境或敏感凭据文件（值已本地脱敏，提交前需向用户确认是否排除）",
    )
}

pub const fn cpush_skip_slimmed_patch(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Large diff, patch truncated",
        "大文件改动较大，补丁已精简",
    )
}

pub const fn cpush_skip_omitted_budget(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Total selected diff is large, inline patch omitted",
        "所选总改动较大，省略内联补丁",
    )
}

pub fn cpush_step_branch_changed_error(
    locale: LanguageKind,
    actual_branch: &str,
    detached: bool,
    expected_branch: &str,
) -> String {
    match locale {
        LanguageKind::En => {
            let current = if detached {
                "detached HEAD"
            } else {
                actual_branch
            };
            format!(
                "Active worktree branch changed to \"{current}\" (planned branch was \"{expected_branch}\"); paused writes to protect changes"
            )
        }
        LanguageKind::Zh => {
            let current = if detached {
                "分离 HEAD"
            } else {
                actual_branch
            };
            format!(
                "工作树当前分支已变为“{current}”（原计划分支为“{expected_branch}”），已暂停写入以保护改动"
            )
        }
    }
}

pub fn cpush_step_missing_file_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Planned file \"{path}\" is no longer in the current change list; paused this step"
        ),
        LanguageKind::Zh => format!("待提交文件“{path}”已不在当前变更列表中，已暂停当前步骤"),
    }
}

pub fn cpush_step_stale_file_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Content or staging scope of planned file \"{path}\" changed externally; paused this step to prevent unintended commits"
        ),
        LanguageKind::Zh => format!(
            "待提交文件“{path}”的内容或暂存范围已在外部发生变化，已暂停当前步骤以防止误提交"
        ),
    }
}

pub const fn ai_selection_required_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Select at least one changed file first; current draft was preserved."
        }
        (LanguageKind::En, true) => {
            "Select at least one changed file before starting C-Push; current draft was preserved."
        }
        (LanguageKind::Zh, false) => "请先选择至少一个变更文件；当前草稿已保留。",
        (LanguageKind::Zh, true) => "请先选择至少一个变更文件后再发起 C-Push；当前草稿已保留。",
    }
}

pub const fn ai_invalid_token_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Model request contains invalid or duplicate file scope tokens; refresh and try again."
        }
        (LanguageKind::En, true) => {
            "C-Push request contains invalid or duplicate file scope tokens; refresh and try again."
        }
        (LanguageKind::Zh, false) => "模型请求包含无效或重复的文件范围凭证，请刷新后重试。",
        (LanguageKind::Zh, true) => "C-Push 请求包含无效或重复的文件范围凭证，请刷新后重试。",
    }
}

pub const fn ai_file_missing_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Selected file is no longer in the worktree change list; refresh and confirm before running model tasks. Draft and selection were preserved."
        }
        (LanguageKind::En, true) => {
            "Selected file is no longer in the worktree change list; refresh and confirm before starting C-Push. Draft and selection were preserved."
        }
        (LanguageKind::Zh, false) => {
            "所选文件已不在当前工作树变更列表中，请刷新确认后再执行模型任务；当前草稿与选择已保留。"
        }
        (LanguageKind::Zh, true) => {
            "所选文件已不在当前工作树变更列表中，请刷新确认后再发起 C-Push；草稿与选择已保留。"
        }
    }
}

pub const fn ai_conflict_in_scope_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Selected scope contains unresolved merge conflicts; resolve conflicts before generating messages or reviewing. Draft and selection were preserved."
        }
        (LanguageKind::En, true) => {
            "Selected scope contains unresolved merge conflicts; resolve conflicts before running C-Push. Draft and selection were preserved."
        }
        (LanguageKind::Zh, false) => {
            "所选范围包含未解决的合并冲突文件，请先处理冲突后再生成说明或审查；草稿与选择已保留。"
        }
        (LanguageKind::Zh, true) => {
            "所选范围包含未解决的合并冲突文件，请先处理冲突后再执行 C-Push；草稿与选择已保留。"
        }
    }
}

pub const fn ai_scope_stale_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Selected file scope, staged content, or branch has changed; paused this model request. Refresh to verify latest changes; draft and selection were preserved."
        }
        (LanguageKind::En, true) => {
            "Selected file scope, staged content, or branch has changed; paused C-Push. Refresh to verify latest changes; draft and selection were preserved."
        }
        (LanguageKind::Zh, false) => {
            "所选文件范围、暂存内容或分支已变化，已暂停本次模型请求；请刷新核对最新变更，当前草稿与选择已保留。"
        }
        (LanguageKind::Zh, true) => {
            "所选文件范围、暂存内容或分支已变化，已暂停本次 C-Push；请刷新核对最新变更，草稿与选择已保留。"
        }
    }
}

pub const fn cpush_detached_head_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Currently on detached HEAD; switch to a branch in the header first before running C-Push. Draft and selection were preserved.",
        "当前处于分离 HEAD，请先在顶部新建或切换分支后再使用 C-Push；草稿与选择已保留。",
    )
}
