//! In-app updater status, blockers, and error localization.

use super::{LanguageKind, tr};

pub const fn updater_git_busy_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "A Git or C-Push task is currently running. Wait for it to finish before installing the update.",
        "当前有 Git 或 C-Push 任务正在执行，请等任务完成后再安装更新。",
    )
}

pub const fn updater_no_pending_update_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No matching update is ready to install. Check for updates again.",
        "没有可安装的对应更新，请重新检查更新。",
    )
}

pub const fn updater_busy_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "An update check or download is already in progress.",
        "正在检查或下载更新，请稍候。",
    )
}

pub const fn updater_blocker_dev_build(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "In-app updates are only available in the packaged macOS app bundle.",
        "应用内更新仅适用于已打包的 macOS 应用，开发模式下请手动更新。",
    )
}

pub const fn updater_blocker_read_only_volume(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Picket is running from a read-only disk image. Move Picket.app into Applications first, or download the latest DMG from GitHub.",
        "Picket 当前正从只读镜像中运行，请先将其拖入「应用程序」文件夹，或前往 GitHub 手动下载。",
    )
}

pub const fn updater_blocker_app_translocation(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Picket is running from a temporary macOS translocation path. Move Picket.app into Applications and reopen it to enable in-app updates.",
        "Picket 当前位于 macOS 临时隔离路径下，请移入「应用程序」并重新打开后再使用应用内更新。",
    )
}

pub const fn updater_blocker_signing_key_unconfigured(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "This build does not have an update signing public key configured yet. Download the latest DMG from GitHub instead.",
        "当前构建尚未配置更新验签公钥，暂不支持应用内自动安装，请前往 GitHub 手动下载。",
    )
}

pub const fn updater_signature_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Update signature verification failed. The downloaded package was discarded; try again later or download manually from GitHub.",
        "更新包签名校验失败，已丢弃本次下载内容；请稍后重试或前往 GitHub 手动下载。",
    )
}

pub fn updater_check_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Unable to check for updates ({detail}). Check your network connection or open GitHub Releases."
        ),
        LanguageKind::Zh => {
            format!("检查更新失败（{detail}），请检查网络连接或前往 GitHub 下载页面。")
        }
    }
}

pub fn updater_install_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Update installation did not complete ({detail}). Your existing Picket app was kept; you can retry or download from GitHub."
        ),
        LanguageKind::Zh => format!(
            "安装更新未完成（{detail}），原有应用已保留；可稍后重试或前往 GitHub 手动下载。"
        ),
    }
}

pub fn updater_open_release_page_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Unable to open GitHub Releases page: {detail}"),
        LanguageKind::Zh => format!("无法打开 GitHub 发布页面：{detail}"),
    }
}

pub const fn updater_state_lock_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Updater state lock is unavailable",
        "更新状态锁不可用",
    )
}
