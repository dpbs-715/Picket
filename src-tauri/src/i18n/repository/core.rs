//! Repository open/read, persistence, local keep, parser, and runner localization.

use crate::i18n::{LanguageKind, tr};

pub const fn default_worktree_label(locale: LanguageKind) -> &'static str {
    tr(locale, "Worktree", "工作树")
}

pub const fn non_utf8_directory(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Directory path is not valid UTF-8 and cannot be opened safely",
        "存在非 UTF-8 目录，当前版本不能安全打开",
    )
}

pub const fn invalid_repo_request_id(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Invalid repository request identity",
        "无效的仓库读取请求身份",
    )
}

pub fn open_bare_repo_error(locale: LanguageKind, dir_name: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Cannot open \"{dir_name}\": this is a bare Git repository without a working tree; select a standard Git working directory"
        ),
        LanguageKind::Zh => format!(
            "无法打开“{dir_name}”：该目录是 Git 裸仓库（无工作区文件），请选择普通 Git 工作目录"
        ),
    }
}

pub fn open_not_git_repo_error(locale: LanguageKind, dir_name: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Cannot open \"{dir_name}\": directory is not managed by Git (.git not found); run git init first or select another Git repository"
        ),
        LanguageKind::Zh => format!(
            "无法打开“{dir_name}”：该目录未受 Git 管理（未找到 .git），请先执行 git init 或选择其他 Git 仓库"
        ),
    }
}

pub const fn persistence_unreadable_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Could not read saved local workbench state; started from an empty workbench.",
        "无法读取本地工作台保存记录，已从空白状态启动。",
    )
}

pub const fn persistence_incompatible_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Saved workbench state version or format is incompatible; started safely from an empty state without overwriting the existing file.",
        "本地工作台保存记录版本或格式不兼容，已从安全空状态启动；原记录未被覆盖。",
    )
}

pub fn persistence_unavailable_repos_warning(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if count == 1 {
                "1 previously opened repository directory is currently inaccessible; its saved drafts and selections were preserved.".into()
            } else {
                format!(
                    "{count} previously opened repository directories are currently inaccessible; their saved drafts and selections were preserved."
                )
            }
        }
        LanguageKind::Zh => {
            format!("有 {count} 个上次打开的仓库目录当前不可访问，已保留其已保存的草稿与选择。")
        }
    }
}

pub const fn persistence_exceeds_limits(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Workbench draft or persisted state exceeds storage limits",
        "工作台草稿或工作树状态超出持久化上限",
    )
}

pub const fn persistence_invalid_worktree_entry(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Invalid or duplicate persisted worktree state entry",
        "工作树持久化条目无效或存在重复身份",
    )
}

pub const fn persistence_invalid_repo_settings_entry(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Invalid or duplicate persisted repository settings entry",
        "仓库级设置持久化条目无效或存在重复身份",
    )
}

pub const fn local_keep_empty_paths_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Select at least one relative file path to update Local Keep",
        "请至少指定一个仓库内相对路径以更新本地保留状态",
    )
}

pub const fn local_keep_invalid_path_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local Keep accepts only safe repository-relative file paths",
        "本地保留仅支持安全的仓库内相对文件路径",
    )
}

pub const fn local_keep_limit_exceeded_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local Keep file list exceeds the per-repository storage limit",
        "本地保留路径数量超出单仓库存储上限",
    )
}

pub const fn repo_directory_inaccessible(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected directory is inaccessible; choose a local Git working directory",
        "所选目录不可访问，请选择本机 Git 工作目录",
    )
}

pub const fn repo_must_select_directory(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Select a directory, not a file",
        "请选择目录，而不是文件",
    )
}

pub const fn repo_not_opened_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Repository or worktree is not open in the current window; select the directory again, and your draft and selection were preserved",
        "仓库或工作树未在当前窗口打开，请重新选择目录；草稿与选择已保留",
    )
}

pub const fn repo_identity_changed_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Worktree repository identity changed; reopen it, and your draft and selection were preserved",
        "工作树所属仓库已变化，请重新打开；已保留原草稿和选择",
    )
}

pub const fn repo_too_many_changes_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "More than 10,000 changed files detected; incomplete list was not loaded",
        "变更文件超过 10000 项，未载入不完整列表",
    )
}

pub const fn repo_submodule_read_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Submodule changes detected; inspect them in Git / IDE",
        "存在子模块变更，当前只读阶段尚不能安全计算其范围；请在 Git / IDE 中查看",
    )
}

pub const fn repo_changed_during_read_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Git status changed while reading; refresh again, and your draft and selection were preserved",
        "Git 状态在读取期间发生变化，请重新刷新；当前草稿与选择已保留",
    )
}

pub const fn repo_shutting_down_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Application is exiting; Git operation stopped",
        "应用正在退出，已停止 Git 读取",
    )
}

pub const fn runner_external_filter_read_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "File requires an external Git filter; read-only mode stopped reading. Inspect it in Git or IDE",
        "文件需要 Git 外部过滤器，只读模式已停止读取；请在 Git 或 IDE 中查看",
    )
}

pub const fn runner_external_filter_write_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected file requires an external Git filter conversion; commit or switch in Git / IDE, and your draft and selection were preserved",
        "所选文件需要外部 Git 过滤器转换，当前版本不能保证安全写入，请在 Git / IDE 中操作；草稿与选择已保留",
    )
}

pub const fn parse_non_utf8_path_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Non-UTF-8 Git path detected; incomplete results were not loaded",
        "存在非 UTF-8 Git 路径，未载入不完整结果",
    )
}
