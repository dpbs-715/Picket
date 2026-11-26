//! Custom Actions execution, generation, and approval prompt localization.

use super::{LanguageKind, tr};

pub const fn action_cpush_busy_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "A custom action is currently running in this repository; wait for it to finish before starting C-Push.",
        "当前仓库有自定义按钮正在执行，请等待完成或取消后再使用 C-Push。",
    )
}

pub const fn action_pull_busy_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "A custom action is currently running in this repository; wait for it to finish before pulling.",
        "当前仓库有自定义按钮正在执行，请等待完成后再拉取变基。",
    )
}

pub const fn action_already_running_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Another custom action is already running in this repository.",
        "当前仓库已有自定义动作正在执行，请等待其完成或先取消。",
    )
}

pub const fn action_cpush_active_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "A C-Push operation is already active in this repository; wait for it to finish first.",
        "当前仓库有正在进行的 C-Push 操作，请完成后再运行自定义按钮。",
    )
}

pub const fn action_empty_fields_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Custom action label and prompt must not be empty.",
        "自定义按钮名称和指令不能为空。",
    )
}

pub const fn action_question_inactive_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "This interactive question is no longer active.",
        "该交互问题已结束或被取消。",
    )
}

pub const fn action_question_settled_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "The action has already settled; response could not be delivered.",
        "任务已终止，无法投递回答。",
    )
}

pub const fn action_create_empty_request_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Please describe the workflow you want this button to perform.",
        "请先描述你想要创建的自定义按钮流程。",
    )
}

pub const fn action_tier2_header(locale: LanguageKind) -> &'static str {
    tr(locale, "Confirm Write", "确认写入")
}

pub const fn action_tier2_edit_header(locale: LanguageKind) -> &'static str {
    tr(locale, "Confirm File Edit", "确认修改文件")
}

pub const fn action_tier2_create_header(locale: LanguageKind) -> &'static str {
    tr(locale, "Confirm New File", "确认新建文件")
}

pub fn action_tier2_question(locale: LanguageKind, reason: &str) -> String {
    match locale {
        LanguageKind::En => format!("Confirm before running this write command: {reason}"),
        LanguageKind::Zh => format!("即将执行高风险或自定义写操作：{reason}"),
    }
}

pub fn action_tier2_edit_question(locale: LanguageKind, reason: &str) -> String {
    match locale {
        LanguageKind::En => format!("Confirm before modifying this repository file: {reason}"),
        LanguageKind::Zh => format!("即将修改当前仓库中的文件：{reason}"),
    }
}

pub fn action_tier2_create_question(locale: LanguageKind, reason: &str) -> String {
    match locale {
        LanguageKind::En => format!("Confirm before creating this new repository file: {reason}"),
        LanguageKind::Zh => format!("即将在当前仓库中新建文件：{reason}"),
    }
}

pub const fn action_tier2_approve_label(locale: LanguageKind) -> &'static str {
    tr(locale, "Run command", "允许执行")
}

pub const fn action_tier2_approve_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Execute the exact command shown in the preview below",
        "在当前仓库执行下方预览中的命令",
    )
}

pub const fn action_tier2_edit_approve_label(locale: LanguageKind) -> &'static str {
    tr(locale, "Apply edit", "允许修改")
}

pub const fn action_tier2_edit_approve_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Write the exact text replacement shown in the preview below",
        "将下方预览中的修改写入当前仓库文件",
    )
}

pub const fn action_tier2_create_approve_label(locale: LanguageKind) -> &'static str {
    tr(locale, "Create file", "允许新建")
}

pub const fn action_tier2_create_approve_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Create the new repository file shown in the preview below",
        "在当前仓库创建下方预览中的新文件",
    )
}

pub const fn action_tier2_cancel_label(locale: LanguageKind) -> &'static str {
    tr(locale, "Cancel", "取消操作")
}

pub const fn action_tier2_cancel_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Decline this operation without modifying the repository",
        "取消本次操作，不改动当前仓库",
    )
}

pub const fn action_tier2_dont_ask_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Don't ask again for this button (resets if prompt is edited)",
        "此按钮不再提示（修改指令后自动失效）",
    )
}

pub fn action_preparing_label(locale: LanguageKind, action_label: &str) -> String {
    match locale {
        LanguageKind::En => format!("Preparing \"{action_label}\""),
        LanguageKind::Zh => format!("准备执行「{action_label}」"),
    }
}

pub const fn action_analyzing_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Analyzing repository context and running controlled tools…",
        "正在分析仓库上下文并按需执行受控工具…",
    )
}

pub const fn action_started_label(locale: LanguageKind) -> &'static str {
    tr(locale, "Picket started", "Picket 已开始执行")
}

pub const fn action_resuming_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Received your response, resuming…",
        "已收到你的选择，正在继续执行…",
    )
}

pub fn action_installed_cli_label(locale: LanguageKind, tool: &str) -> String {
    match locale {
        LanguageKind::En => format!("Installed {tool}"),
        LanguageKind::Zh => format!("已安装 {tool}"),
    }
}

pub fn action_pushed_remote_label(locale: LanguageKind, remote_branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Pushed to {remote_branch}"),
        LanguageKind::Zh => format!("已推送至 {remote_branch}"),
    }
}

pub fn action_completed_title(locale: LanguageKind, action_label: &str) -> String {
    match locale {
        LanguageKind::En => format!("{action_label} completed"),
        LanguageKind::Zh => format!("{action_label} 已完成"),
    }
}

pub const fn action_cancelled_summary(locale: LanguageKind) -> &'static str {
    tr(locale, "Custom action cancelled.", "已取消本次自定义动作。")
}

pub const fn action_create_inspecting_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Inspecting repository context and built-in skills…",
        "正在探测当前仓库上下文与内置技能…",
    )
}

pub const fn action_create_needs_input_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Picket needs your input on a few details",
        "Picket 需要你确认几个细节",
    )
}

pub const fn action_create_drafting_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Drafting button configuration…",
        "正在根据你的选择生成按钮草稿…",
    )
}

pub fn action_interrupted_title(locale: LanguageKind, action_label: &str) -> String {
    match locale {
        LanguageKind::En => format!("{action_label} interrupted"),
        LanguageKind::Zh => format!("{action_label} 已中断"),
    }
}

pub fn action_interrupted_summary(locale: LanguageKind, action_label: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "\"{action_label}\" was interrupted before finishing. Created Issues, branches, and PRs were preserved; click the button again to reconcile remote state and resume from the next incomplete step."
        ),
        LanguageKind::Zh => format!(
            "「{action_label}」上次未执行完已中断，已创建的 Issue、分支和 PR 均已保留；再次点击按钮将自动对账远端状态并从断点续跑。"
        ),
    }
}

pub fn action_edited_file_label(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("Edited {path}"),
        LanguageKind::Zh => format!("已修改 {path}"),
    }
}

pub fn action_created_file_label(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("Created {path}"),
        LanguageKind::Zh => format!("已新建 {path}"),
    }
}

pub fn action_tool_status_label(locale: LanguageKind, tool_name: &str, detail: &str) -> String {
    match (locale, tool_name) {
        (LanguageKind::En, "check_cli_env") => "Checking local Git / gh / glab environment".into(),
        (LanguageKind::Zh, "check_cli_env") => "检查本机 Git / gh / glab 环境".into(),
        (LanguageKind::En, "install_platform_cli") => format!("Installing {detail} via Homebrew"),
        (LanguageKind::Zh, "install_platform_cli") => format!("正在通过 Homebrew 安装 {detail}"),
        (LanguageKind::En, "run_git") => format!("Running git {detail}"),
        (LanguageKind::Zh, "run_git") => format!("执行 git {detail}"),
        (LanguageKind::En, "run_platform_cli") => format!("Running {detail}"),
        (LanguageKind::Zh, "run_platform_cli") => format!("执行 {detail}"),
        (LanguageKind::En, "run_repo_command") => format!("Running {detail}"),
        (LanguageKind::Zh, "run_repo_command") => format!("执行 {detail}"),
        (LanguageKind::En, "edit_repo_file") => format!("Editing {detail}"),
        (LanguageKind::Zh, "edit_repo_file") => format!("修改 {detail}"),
        (LanguageKind::En, "write_repo_file") => format!("Creating {detail}"),
        (LanguageKind::Zh, "write_repo_file") => format!("新建 {detail}"),
        (LanguageKind::En, "commit_exact_scope") => "Creating exact-scope local commit".into(),
        (LanguageKind::Zh, "commit_exact_scope") => "写入精确范围本地提交".into(),
        (LanguageKind::En, "sync_and_push") => "Syncing and pushing to remote".into(),
        (LanguageKind::Zh, "sync_and_push") => "同步并推送到远端".into(),
        (LanguageKind::En, "manage_submit_worktree") => {
            format!("Managing isolated worktree ({detail})")
        }
        (LanguageKind::Zh, "manage_submit_worktree") => format!("管理隔离工作树（{detail}）"),
        (LanguageKind::En, "cleanup_submit_duplicates") => {
            "Backing up and cleaning verified local duplicates".into()
        }
        (LanguageKind::Zh, "cleanup_submit_duplicates") => "备份并清理已验证的本地重复改动".into(),
        (LanguageKind::En, "record_action_checkpoint") => "Saving submission checkpoint".into(),
        (LanguageKind::Zh, "record_action_checkpoint") => "记录断点进度".into(),
        (LanguageKind::En, "read_repo_file") => format!("Reading {detail}"),
        (LanguageKind::Zh, "read_repo_file") => format!("读取 {detail}"),
        (LanguageKind::En, "search_repo") => "Searching repository".into(),
        (LanguageKind::Zh, "search_repo") => "检索仓库代码".into(),
        (LanguageKind::En, "ask_user_question") => "Waiting for your decision".into(),
        (LanguageKind::Zh, "ask_user_question") => "等待你的确认".into(),
        (LanguageKind::En, other) => format!("Running {other}"),
        (LanguageKind::Zh, other) => format!("调用工具 {other}"),
    }
}

pub const fn action_install_label_keyword_zh() -> &'static str {
    "安装"
}
