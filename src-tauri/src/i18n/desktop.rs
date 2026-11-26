//! Main window, folder picker, and menu bar tray localization.

use super::{LanguageKind, tr};

pub const fn picker_busy(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Directory selection or repository read is already in progress; please wait",
        "目录选择或仓库读取正在进行，请勿重复打开",
    )
}

pub const fn main_window_unavailable(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Workbench window is unavailable",
        "工作台窗口不可用",
    )
}

pub const fn select_git_working_directory_title(locale: LanguageKind) -> &'static str {
    tr(locale, "Select Git Working Directory", "选择 Git 工作目录")
}

pub const fn picker_incomplete(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Directory selection did not complete; please reopen",
        "目录选择未完成，请重新打开",
    )
}

pub const fn picker_must_be_local_dir(locale: LanguageKind) -> &'static str {
    tr(locale, "Please select a local directory", "请选择本机目录")
}

pub const fn tray_tooltip_needs_decision(locale: LanguageKind) -> &'static str {
    tr(locale, "Picket · Needs decision", "Picket · 待你决定")
}

pub const fn tray_tooltip_conflict(locale: LanguageKind) -> &'static str {
    tr(locale, "Picket · Merge conflict", "Picket · 有代码冲突")
}

pub const fn tray_tooltip_push_failed(locale: LanguageKind) -> &'static str {
    tr(locale, "Picket · Push failed", "Picket · 推送失败")
}

pub const fn tray_tooltip_running(locale: LanguageKind) -> &'static str {
    tr(locale, "Picket · C-Push running", "Picket · 正在执行")
}

pub fn tray_tooltip_changed_repos(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if count == 1 {
                "Picket · 1 repository with changes".into()
            } else {
                format!("Picket · {count} repositories with changes")
            }
        }
        LanguageKind::Zh => format!("Picket · {count} 个仓库有变更"),
    }
}

pub const fn tray_tooltip_update_available(locale: LanguageKind) -> &'static str {
    tr(locale, "Picket · Update available", "Picket · 发现新版本")
}
