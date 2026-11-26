//! External IDE / editor launcher localization.

use crate::i18n::{LanguageKind, tr};

pub fn editor_not_found_error(locale: LanguageKind, display_name: &str, cli: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{display_name} executable ({cli}) was not found. Make sure {display_name} is installed or enable its CLI launcher, or switch editors in Settings; draft and selection were preserved."
        ),
        LanguageKind::Zh => format!(
            "未找到 {display_name} 可执行入口（{cli}）。请确认已安装 {display_name} 或启用其命令行工具，也可在设置中切换其他编辑器；当前草稿与选择已保留。"
        ),
    }
}

pub fn editor_launch_failed_error(
    locale: LanguageKind,
    display_name: &str,
    error: &dyn std::fmt::Display,
) -> String {
    match locale {
        LanguageKind::En => format!(
            "Failed to launch {display_name} ({error}); draft and selection were preserved."
        ),
        LanguageKind::Zh => format!("启动 {display_name} 失败（{error}）；当前草稿与选择已保留。"),
    }
}

pub fn editor_returned_error(locale: LanguageKind, display_name: &str, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{display_name} returned an error ({detail}); draft and selection were preserved."
        ),
        LanguageKind::Zh => format!("{display_name} 返回错误（{detail}）；当前草稿与选择已保留。"),
    }
}

pub fn editor_wait_failed_error(
    locale: LanguageKind,
    display_name: &str,
    error: &dyn std::fmt::Display,
) -> String {
    match locale {
        LanguageKind::En => format!(
            "Failed while waiting for {display_name} to launch ({error}); draft and selection were preserved."
        ),
        LanguageKind::Zh => {
            format!("等待 {display_name} 启动结果失败（{error}）；当前草稿与选择已保留。")
        }
    }
}

pub const fn editor_invalid_line_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Invalid line number; line numbers must start at 1. Current draft and selection were preserved",
        "跳转行号无效，行号必须从 1 开始；当前草稿与选择已保留",
    )
}
