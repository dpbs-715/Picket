//! Exact-scope local commit localization.

use crate::i18n::{LanguageKind, tr};

pub fn commit_lock_file_error(locale: LanguageKind, lock: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Git lock file exists ({lock}); another Git process may be running. No commit was created; draft and selection were preserved"
        ),
        LanguageKind::Zh => format!(
            "存在 Git 锁文件（{lock}），可能有其他 Git 进程正在操作；未执行提交，草稿与选择已保留"
        ),
    }
}

pub fn commit_ongoing_op_error(locale: LanguageKind, marker: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Unfinished Git operation detected ({marker}); resolve it in Git / IDE first. No commit was created; draft and selection were preserved"
        ),
        LanguageKind::Zh => format!(
            "存在未完成的 Git 操作（{marker}），请先在 Git / IDE 中处理；未执行提交，草稿与选择已保留"
        ),
    }
}

pub fn commit_hook_failed_error(
    locale: LanguageKind,
    hook_name: &str,
    detail: &str,
    mutated: bool,
) -> String {
    match (locale, mutated) {
        (LanguageKind::En, true) => format!(
            "Git {hook_name} hook failed and modified repository files: {detail}. No commit was created, and your draft and selection were preserved; inspect the updated changes and re-confirm scope."
        ),
        (LanguageKind::En, false) => format!(
            "Git {hook_name} hook rejected this commit: {detail}. No commit was created; draft and selection were preserved."
        ),
        (LanguageKind::Zh, true) => format!(
            "Git {hook_name} 钩子未通过且修改了仓库内容：{detail}。未创建提交，当前草稿与选择已保留；请核对更新后的变更并重新确认范围。"
        ),
        (LanguageKind::Zh, false) => format!(
            "Git {hook_name} 钩子拒绝了本次提交：{detail}。未创建提交，当前草稿与选择已保留。"
        ),
    }
}

pub fn commit_missing_identity_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Git author identity (user.name / user.email) is not configured: {detail}. Configure it in Git before committing; no commit was created, and draft and selection were preserved."
        ),
        LanguageKind::Zh => format!(
            "Git 尚未配置提交者身份（user.name / user.email）：{detail}。请先在 Git 中配置后再提交；未创建提交，草稿与选择已保留。"
        ),
    }
}

pub fn commit_object_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Failed to create commit object: {detail}. Current branch was not modified; draft and selection were preserved."
        ),
        LanguageKind::Zh => {
            format!("创建提交对象未成功：{detail}。未修改当前分支，草稿与选择已保留。")
        }
    }
}

pub fn commit_update_ref_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Updating branch ref did not complete: {detail}. Refresh to verify actual Git state before retrying; draft and selection were preserved."
        ),
        LanguageKind::Zh => format!(
            "更新分支引用未完成：{detail}。请刷新核对实际状态，不要盲目重复提交；草稿与选择已保留。"
        ),
    }
}

pub fn commit_reconcile_failed_error(locale: LanguageKind, short_oid: &str, error: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commit {short_oid} was written to branch history, but syncing the index or journal did not complete: {error}. Refresh and verify in Git / IDE; do not repeat the commit."
        ),
        LanguageKind::Zh => format!(
            "本地提交 {short_oid} 已写入分支历史，但同步暂存区或记录未完成：{error}。请刷新并在 Git / IDE 中核对，不要重复提交。"
        ),
    }
}

pub fn commit_post_verify_tree_error(locale: LanguageKind, short_oid: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Commit {short_oid} exists, but branch ref or tree differs from expectation; stopped subsequent actions. Verify in Git."
        ),
        LanguageKind::Zh => format!(
            "提交 {short_oid} 已存在，但分支指向或内容树与预期不符；已停止后续动作，请在 Git 中核对。"
        ),
    }
}

pub fn commit_post_verify_working_error(
    locale: LanguageKind,
    short_oid: &str,
    path: &str,
) -> String {
    match locale {
        LanguageKind::En => format!(
            "Commit {short_oid} completed, but working tree file {path} changed unexpectedly; stopped subsequent actions. Refresh and verify in IDE."
        ),
        LanguageKind::Zh => format!(
            "提交 {short_oid} 已完成，但检测到工作区文件 {path} 发生变化；已停止后续动作，请刷新并在 IDE 中核对。"
        ),
    }
}

pub fn commit_post_verify_unselected_error(locale: LanguageKind, short_oid: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Commit {short_oid} completed, but staging state of unselected files changed unexpectedly; refresh and verify in Git / IDE."
        ),
        LanguageKind::Zh => format!(
            "提交 {short_oid} 已完成，但未选文件的暂存状态与预期不符；请刷新并在 Git / IDE 中核对。"
        ),
    }
}

pub fn commit_post_read_failed_error(locale: LanguageKind, short_oid: &str, error: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commit {short_oid} was written, but reading latest state failed: {error}. Refresh manually; do not repeat the commit."
        ),
        LanguageKind::Zh => format!(
            "本地提交 {short_oid} 已写入，但读取最新状态失败：{error}。请手动刷新核对，不要重复提交。"
        ),
    }
}

pub const fn commit_local_only_no_push_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local-only commit endpoint does not push; use the controlled Commit & Push command when pushing. Neither commit nor push was executed.",
        "仅本地提交入口不执行推送；需要提交并推送时请通过受控同步与推送命令执行，本次未执行提交或推送。",
    )
}

pub const fn commit_message_required_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter a commit message first; current draft and selection were preserved",
        "请先填写提交说明，当前草稿与选择已保留",
    )
}

pub const fn commit_selection_required_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Select at least one changed file to commit; current draft was preserved",
        "请至少选择一项要提交的变更；当前草稿已保留",
    )
}

pub const fn commit_detached_head_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Currently on detached HEAD; switch to an existing local branch in the header or in IDE / terminal before committing. Draft and selection were preserved",
        "当前处于分离 HEAD，请先在顶部切换已有分支，或在 IDE / 终端切好分支后再提交；草稿与选择已保留",
    )
}

pub const fn commit_scope_changed_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected file scope, staged content, or branch changed externally; paused commit and preserved your draft and selection. Refresh to re-confirm scope",
        "所选文件的变更状态或暂存范围已在外部变化，已暂停提交；当前草稿与选择已保留，请刷新后重新确认范围",
    )
}

pub const fn commit_token_invalid_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected commit scope token is outdated or invalid; refresh and re-confirm scope. Draft and selection were preserved",
        "所选提交范围凭证已过期或无效，不能使用旧范围写入；请刷新并重新确认，草稿与选择已保留",
    )
}

pub const fn commit_unresolved_conflict_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Unresolved merge conflicts exist; resolve them in Git / IDE first. No commit was created; draft and selection were preserved",
        "存在未解决的合并冲突，请先在 Git / IDE 中处理；未执行提交，草稿与选择已保留",
    )
}

pub const fn commit_rename_slot_collision_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected file shares its staged source path with an unselected rename; select the renamed file together or adjust staging in Git / IDE. Draft and selection were preserved",
        "所选文件与未勾选的重命名原路径共用暂存位置，请将对应重命名文件一并勾选或先在 Git / IDE 中调整暂存；草稿与选择已保留",
    )
}

pub const fn commit_sparse_or_hidden_flag_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected files or repository sparse-checkout use skip-worktree / assume-unchanged index flags; handle them in Git / IDE before committing",
        "所选文件含隐藏变更标记（skip-worktree / assume-unchanged）或仓库启用了稀疏检出，暂不执行提交写入；请先在 Git / IDE 中处理",
    )
}

pub const fn commit_submodule_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Submodule changes are not supported for commit writes yet; handle submodules in Git / IDE",
        "暂不支持提交子模块变更或对含未提交子模块改动的仓库写入，请在 Git / IDE 中处理子模块",
    )
}

pub const fn commit_hook_mutated_content_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Git hook modified staged or working tree content; aborted commit to protect scope. Draft and selection were preserved; inspect updated changes and retry.",
        "Git 钩子修改了待提交或工作区内容，不能继续沿用旧范围；未创建提交，当前草稿与选择已保留，请核对更新后的变更后重试。",
    )
}

pub const fn commit_request_id_reused_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Commit request identity is already bound to a different scope or message and will not write again; refresh to verify Git status",
        "提交请求身份已被其他范围或说明占用，不会重复写入；请刷新核对 Git 状态",
    )
}

pub const fn commit_interrupted_record_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Previous commit write was interrupted; refresh and verify actual history in Git / IDE. It will not repeat automatically",
        "检测到上次提交写入中断，请先刷新并在 Git / IDE 中核对实际状态；不会自动重复提交",
    )
}
