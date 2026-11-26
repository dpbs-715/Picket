//! Branch and worktree switching localization.

use crate::i18n::{LanguageKind, tr};

pub fn branch_in_progress_or_lock_error(locale: LanguageKind, marker: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Git operation or lock in progress ({marker}); resolve it in Git / IDE first. No branch changes were written"
        ),
        LanguageKind::Zh => {
            format!("存在 Git 进行中操作或锁（{marker}），请先在 Git / IDE 中处理；未执行分支写入")
        }
    }
}

pub fn branch_switch_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Git operation did not complete: {detail}. Refresh to verify actual state; it will not auto-retry or roll back."
        ),
        LanguageKind::Zh => {
            format!("Git 操作未完成：{detail}。请刷新核对实际状态；不会自动重试或回滚。")
        }
    }
}

pub fn branch_switched_read_failed_error(locale: LanguageKind, error: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Git switched branches, but reading the updated state failed: {error}. Refresh to verify actual state."
        ),
        LanguageKind::Zh => {
            format!("Git 已切换分支，但读取结果失败：{error}。请刷新核对实际状态。")
        }
    }
}

pub const fn branch_token_stale_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Branch state token is outdated; refresh and try again, and your draft and selection were preserved",
        "分支、HEAD、暂存区或工作区内容已变化，请刷新后重试；草稿与选择已保留",
    )
}

pub const fn branch_duplicate_request_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "This branch request was already accepted and will not run twice; refresh to verify live Git status",
        "此分支请求已经接收，不会重复执行；请刷新核对 Git 实际状态",
    )
}

pub const fn branch_unborn_head_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Repository has no initial commit yet; create the initial commit first before switching branches",
        "仓库尚无首次提交，请先在 Git / IDE 中完成首次提交后再切换分支",
    )
}

pub const fn branch_dirty_cross_commit_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Working tree has staged or unstaged changes and cannot switch safely to a different commit; handle current changes in Git / IDE first",
        "当前有暂存或未暂存改动，不能安全切换到不同提交；请先在 Git / IDE 中处理当前改动后再切换分支",
    )
}

pub const fn branch_external_filter_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Files on the current or target branch require an external Git filter; switch branches in Git / IDE",
        "当前或目标分支的文件需要外部 Git 过滤器，当前版本不能保证安全检出，请在 Git / IDE 中操作",
    )
}

pub const fn branch_custom_hooks_path_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Repository configures a custom core.hooksPath; switch branches in Git / IDE, no branch write was performed",
        "仓库配置了自定义 hooksPath，当前版本尚未验证其安全检出，未执行分支写入；请在 Git / IDE 中操作",
    )
}

pub const fn branch_executable_hook_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Executable Git checkout hook detected; branch checkout was not executed. Switch branches in Git / IDE",
        "检测到可执行 Git 检出钩子，当前版本不支持安全执行，未创建或切换分支；请在 Git / IDE 中操作",
    )
}

pub const fn branch_submodule_diff_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Target branch changes submodule revisions or working tree has submodule changes; switch branches in Git / IDE",
        "目标分支与当前分支的子模块版本不同或子模块存在未提交改动，暂不自动切换；请在 Git / IDE 中切分支",
    )
}
