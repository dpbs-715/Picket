//! Localized user-facing strings for local Run commands, process lifecycle, and quit prompts.

use super::{LanguageKind, join_list, tr};

pub fn invalid_command_config(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Run command configuration is invalid. Check the name, command, working directory, and environment variables.",
        "运行命令配置无效，请检查名称、命令、工作目录和环境变量设置。",
    )
}

pub fn invalid_cwd(locale: LanguageKind, cwd: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Working directory `{cwd}` must stay inside the active repository worktree.")
        }
        LanguageKind::Zh => format!("工作目录 `{cwd}` 必须位于当前仓库工作树内部。"),
    }
}

pub fn cwd_not_found(locale: LanguageKind, cwd: &str) -> String {
    match locale {
        LanguageKind::En => format!("Working directory `{cwd}` was not found in this worktree."),
        LanguageKind::Zh => format!("当前工作树中不存在工作目录 `{cwd}`。"),
    }
}

pub fn already_running(locale: LanguageKind, label: &str) -> String {
    match locale {
        LanguageKind::En => format!("`{label}` is already running in this worktree."),
        LanguageKind::Zh => format!("`{label}` 已在当前工作树中运行。"),
    }
}

pub fn process_not_found(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No matching run process was found.",
        "未找到对应的运行进程。",
    )
}

pub fn spawn_failed(locale: LanguageKind, label: &str, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Failed to start `{label}`: {detail}"),
        LanguageKind::Zh => format!("启动 `{label}` 失败：{detail}"),
    }
}

pub fn invalid_local_url(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Only local http:// or https:// localhost URLs can be opened from Run logs.",
        "运行日志仅支持打开本机 localhost / 127.0.0.1 的 http(s) 链接。",
    )
}

pub fn open_local_url_failed(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Failed to open local URL in browser: {detail}"),
        LanguageKind::Zh => format!("在浏览器中打开本地地址失败：{detail}"),
    }
}

pub fn user_env_fallback_warning(locale: LanguageKind, shell: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Could not load login shell environment from `{shell}` within the timeout; using fallback PATH."
        ),
        LanguageKind::Zh => {
            format!("在超时时间内未能从 `{shell}` 加载登录 Shell 环境，已回退至基础环境 PATH。")
        }
    }
}

pub fn system_started_line(locale: LanguageKind, command: &str, cwd: &str, shell: &str) -> String {
    match locale {
        LanguageKind::En => format!("$ {command}  (cwd: {cwd}, shell: {shell})"),
        LanguageKind::Zh => format!("$ {command}  (目录: {cwd}, Shell: {shell})"),
    }
}

pub fn system_stopping_line(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Stopping process group (SIGTERM)...",
        "正在停止进程组 (SIGTERM)...",
    )
}

pub fn system_cancelling_startup_line(locale: LanguageKind) -> &'static str {
    tr(locale, "Cancelling startup...", "正在取消启动...")
}

pub fn system_startup_cancelled_line(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Startup cancelled before launch.",
        "已在启动阶段取消。",
    )
}

pub fn system_force_killed_line(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Process group did not exit in time; terminated with SIGKILL.",
        "进程组超时未退出，已强制终止 (SIGKILL)。",
    )
}

pub fn system_exited_line(
    locale: LanguageKind,
    exit_code: Option<i32>,
    signal: Option<i32>,
) -> String {
    match (locale, exit_code, signal) {
        (LanguageKind::En, Some(code), _) => format!("Process exited with code {code}."),
        (LanguageKind::Zh, Some(code), _) => format!("进程已退出，退出码 {code}。"),
        (LanguageKind::En, None, Some(sig)) => format!("Process stopped by signal {sig}."),
        (LanguageKind::Zh, None, Some(sig)) => format!("进程已由信号 {sig} 终止。"),
        (LanguageKind::En, None, None) => "Process exited.".into(),
        (LanguageKind::Zh, None, None) => "进程已退出。".into(),
    }
}

pub fn quit_confirm_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Stop Running Services and Quit?",
        "停止正在运行的服务并退出？",
    )
}

pub fn quit_confirm_message(locale: LanguageKind, names: &[String]) -> String {
    let preview: Vec<&str> = names.iter().take(4).map(String::as_str).collect();
    let joined = join_list(locale, &preview);
    let extra = names.len().saturating_sub(preview.len());
    match locale {
        LanguageKind::En => {
            if extra > 0 {
                format!(
                    "{} service(s) are currently running ({joined}, and {extra} more). Quitting Picket will stop all of them.",
                    names.len()
                )
            } else {
                format!(
                    "{} service(s) are currently running ({joined}). Quitting Picket will stop all of them.",
                    names.len()
                )
            }
        }
        LanguageKind::Zh => {
            if extra > 0 {
                format!(
                    "当前有 {} 个项目服务正在运行（{joined} 等共 {} 个）。退出 Picket 将停止这些服务进程，确定退出吗？",
                    names.len(),
                    names.len()
                )
            } else {
                format!(
                    "当前有 {} 个项目服务正在运行（{joined}）。退出 Picket 将停止这些服务进程，确定退出吗？",
                    names.len()
                )
            }
        }
    }
}

pub fn quit_confirm_ok_button(locale: LanguageKind) -> &'static str {
    tr(locale, "Stop & Quit", "停止并退出")
}

pub fn quit_confirm_cancel_button(locale: LanguageKind) -> &'static str {
    tr(locale, "Cancel", "取消")
}

pub fn discover_scanning_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Scanning repository project files...",
        "正在扫描项目配置与脚本...",
    )
}

pub fn discover_analyzing_label(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Analyzing candidate run commands...",
        "正在分析可运行的项目命令...",
    )
}

pub fn discover_reading_file_label(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("Reading {path}..."),
        LanguageKind::Zh => format!("正在读取 {path}..."),
    }
}

pub fn discover_completed_summary(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if count == 0 {
                "No runnable project commands were detected automatically.".into()
            } else {
                format!("Found {count} recommended run command(s).")
            }
        }
        LanguageKind::Zh => {
            if count == 0 {
                "未自动识别到可运行的项目命令，你可以手动添加。".into()
            } else {
                format!("已分析出 {count} 条推荐的运行命令，请勾选需要添加的项。")
            }
        }
    }
}
