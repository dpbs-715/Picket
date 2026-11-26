//! Remote tracking, fetch, and merge conflict verification localization.

use crate::i18n::{LanguageKind, tr};

pub const fn remote_missing_tracking_ref_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local tracking ref not found; ahead/behind cannot be determined without fetching",
        "本地找不到有效的跟踪引用，无法确认领先 / 落后；尚未联网刷新",
    )
}

pub const fn remote_detached_head_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Detached HEAD; no tracking branch",
        "当前为分离 HEAD，未确定跟踪分支",
    )
}

pub const fn remote_no_remotes_configured_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No Git remote or tracking branch configured",
        "尚未配置 Git 远端与跟踪分支",
    )
}

pub const fn remote_no_upstream_configured_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No upstream tracking branch configured",
        "尚未配置跟踪远端分支",
    )
}

pub fn remote_fetch_failed_error(locale: LanguageKind, remote: &str, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Cannot connect to remote {remote} to fetch refs ({detail})"),
        LanguageKind::Zh => format!("无法连接远端 {remote} 拉取最新引用（{detail}）"),
    }
}

pub fn remote_rewound_history_reason(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Remote branch {display} was rewritten non-fast-forward; automatic rebase could rewrite published commits and was paused. Inspect remote history manually."
        ),
        LanguageKind::Zh => format!(
            "检测到远端分支 {display} 的历史发生非快进重写，直接自动变基可能改写已发布提交；已暂停，请在核对远端历史后手动处理。"
        ),
    }
}

pub const fn remote_merge_commit_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local commits to rebase include a merge commit; paused automatic rebase to avoid flattening branch history.",
        "待变基的本地提交中包含合并提交（merge commit），为避免扁平化改写分支历史已暂停自动变基。",
    )
}

pub fn remote_published_on_other_ref_reason(
    locale: LanguageKind,
    short_oid: &str,
    other_ref: &str,
) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commit {short_oid} already exists on remote ref {other_ref}; paused automatic rebase to avoid rewriting published history."
        ),
        LanguageKind::Zh => format!(
            "本地提交 {short_oid} 已存在于远端引用 {other_ref} 中，为防止改写已发布历史已暂停自动变基。"
        ),
    }
}

pub fn remote_conflict_leftover_markers_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Merged result for {path} still contains conflict markers; refused to write")
        }
        LanguageKind::Zh => format!("模型对 {path} 的合并结果仍残留冲突标记，已拒绝写入"),
    }
}

pub fn remote_conflict_unauthorized_file_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("Blocked unauthorized file modification request: {path}"),
        LanguageKind::Zh => format!("拦截到未授权的文件修改请求：{path}"),
    }
}

pub fn remote_conflict_empty_result_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Model returned empty content for {path}; refused to overwrite the conflicted file"
        ),
        LanguageKind::Zh => format!("模型对 {path} 返回了空内容，已拒绝覆盖原冲突文件"),
    }
}

pub const fn remote_branch_fallback(locale: LanguageKind) -> &'static str {
    tr(locale, "remote branch", "远端分支")
}

pub const fn remote_rebase_submodule_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Upstream or local commits modify submodule revisions; pull and rebase in Git / IDE so submodule checkouts stay in sync",
        "远端或本地提交包含子模块指针变更，暂不自动变基以免子模块目录不同步；请在 Git / IDE 中处理",
    )
}
