//! Skills repository sync and validation localization.

use super::{LanguageKind, tr};

pub const fn skills_invalid_source_url_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter a valid Git repository URL (HTTPS, SSH, or local path).",
        "请输入有效的 Git 技能仓库地址（支持 HTTPS、SSH 或本地路径）。",
    )
}

pub fn skills_sync_clone_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Could not fetch skills repository: {detail}"),
        LanguageKind::Zh => format!("无法拉取技能仓库：{detail}"),
    }
}

pub fn skills_sync_validation_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!(
                "Skills repository validation failed (active skills were kept unchanged): {detail}"
            )
        }
        LanguageKind::Zh => {
            format!("技能校验未通过，已保留原有可用技能：{detail}")
        }
    }
}

pub const fn skills_not_synced_yet_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Sync a skills repository first before customizing enabled skills.",
        "请先同步一次技能仓库，再勾选启用或停用单个技能。",
    )
}
