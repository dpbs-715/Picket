//! Centralized user-facing localization for the Rust desktop layer.
//! This is the ONLY Rust source file permitted to contain Chinese characters (`[\p{Han}]`).

use crate::models::types::ProviderKind;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageKind {
    #[default]
    En,
    Zh,
}

pub fn deserialize_language_or_default<'de, D>(deserializer: D) -> Result<LanguageKind, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<LanguageKind>::deserialize(deserializer)?.unwrap_or_default())
}

pub const fn tr(locale: LanguageKind, en: &'static str, zh: &'static str) -> &'static str {
    match locale {
        LanguageKind::En => en,
        LanguageKind::Zh => zh,
    }
}

pub fn join_list<S: AsRef<str>>(locale: LanguageKind, items: &[S]) -> String {
    let sep = tr(locale, ", ", "、");
    items
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(sep)
}

// ============================================================================
// Main Window & Folder Picker
// ============================================================================

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

// ============================================================================
// Pi Supervisor (`foundation/pi.rs`)
// ============================================================================

pub const fn pi_invalid_request_id(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Invalid model request identity",
        "无效的模型请求身份",
    )
}

pub const fn pi_registry_unavailable(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Model request coordinator is unavailable",
        "模型请求协调器不可用",
    )
}

pub const fn pi_shutting_down(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Application is exiting; model request was stopped",
        "应用正在退出，已停止模型请求",
    )
}

pub const fn pi_duplicate_request(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "An identical model request is already running",
        "相同的模型请求已在运行中",
    )
}

pub const fn pi_runtime_missing(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Bundled Pi Core runtime is missing; prepare the desktop runtime first",
        "随包 Pi Core 运行时缺失，请先准备桌面运行时",
    )
}

pub fn pi_cannot_start_runtime(locale: LanguageKind, err: &dyn std::fmt::Display) -> String {
    match locale {
        LanguageKind::En => format!("Cannot start bundled Pi runtime: {err}"),
        LanguageKind::Zh => format!("无法启动随包 Pi 运行时：{err}"),
    }
}

pub const fn pi_missing_pid(locale: LanguageKind) -> &'static str {
    tr(locale, "Pi process has no PID", "Pi 进程缺少 PID")
}

pub const fn pi_missing_stdin(locale: LanguageKind) -> &'static str {
    tr(locale, "Missing Pi stdin", "缺少 Pi 标准输入")
}

pub const fn pi_missing_stdout(locale: LanguageKind) -> &'static str {
    tr(locale, "Missing Pi stdout", "缺少 Pi 标准输出")
}

pub const fn pi_missing_stderr(locale: LanguageKind) -> &'static str {
    tr(locale, "Missing Pi stderr", "缺少 Pi 错误输出")
}

pub const fn pi_handshake_timeout(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pi Core startup handshake timed out",
        "Pi Core 启动握手超时",
    )
}

pub const fn pi_invalid_handshake(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pi Core did not return a valid readiness handshake",
        "Pi Core 未返回有效就绪握手",
    )
}

pub const fn pi_incompatible_version(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Incompatible Pi Core protocol or version",
        "Pi Core 协议或版本不兼容",
    )
}

pub const fn pi_cancel_channel_closed(locale: LanguageKind) -> &'static str {
    tr(locale, "Cancellation channel closed", "取消通道已关闭")
}

pub const fn pi_request_timed_out(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Model request timed out; current draft and selection were preserved",
        "模型请求超时，当前草稿与选择已保留",
    )
}

pub fn pi_protocol_rejected(locale: LanguageKind, message: &str) -> String {
    match locale {
        LanguageKind::En => format!("Pi protocol rejected request: {message}"),
        LanguageKind::Zh => format!("Pi 协议拒绝请求：{message}"),
    }
}

pub const fn pi_unexpected_control_event(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Received unexpected Pi control event",
        "收到未预期的 Pi 控制事件",
    )
}

pub const fn pi_mismatched_target(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pi response did not match target worktree or request identity",
        "Pi 响应的目标工作树或请求身份不匹配",
    )
}

pub const fn pi_terminated_on_exit_timeout(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pi process did not exit in time and was terminated",
        "Pi 进程未按时退出，已被终止",
    )
}

pub fn pi_exited_abnormally(
    locale: LanguageKind,
    status: &dyn std::fmt::Display,
    diagnostic: &str,
) -> String {
    let trimmed = diagnostic.trim();
    match (locale, trimmed.is_empty()) {
        (LanguageKind::En, true) => format!("Pi process exited abnormally ({status})"),
        (LanguageKind::En, false) => format!("Pi process exited abnormally ({status}): {trimmed}"),
        (LanguageKind::Zh, true) => format!("Pi 进程异常退出（{status}）"),
        (LanguageKind::Zh, false) => format!("Pi 进程异常退出（{status}）：{trimmed}"),
    }
}

pub fn pi_append_diagnostic(locale: LanguageKind, message: &str, diagnostic: &str) -> String {
    let trimmed = diagnostic.trim();
    if trimmed.is_empty() {
        return message.to_owned();
    }
    match locale {
        LanguageKind::En => format!("{message} ({trimmed})"),
        LanguageKind::Zh => format!("{message}（{trimmed}）"),
    }
}

// ============================================================================
// Repository Read, Parse, Runner, Persistence, Branches, Commit, Editor, Remote
// ============================================================================

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

// ============================================================================
// AI Context & Skipped File Reasons (`repository/ai_context.rs`)
// ============================================================================

pub fn patch_omitted_trailing(locale: LanguageKind, head: &str, omitted_lines: usize) -> String {
    match locale {
        LanguageKind::En => {
            format!("{head}\n... [{omitted_lines} trailing changed lines omitted] ...")
        }
        LanguageKind::Zh => format!("{head}\n... [省略后续 {omitted_lines} 行改动] ..."),
    }
}

pub fn patch_omitted_middle(
    locale: LanguageKind,
    head: &str,
    omitted_lines: usize,
    tail: &str,
) -> String {
    match locale {
        LanguageKind::En => {
            format!("{head}\n... [{omitted_lines} middle changed lines omitted] ...\n{tail}")
        }
        LanguageKind::Zh => {
            format!("{head}\n... [省略中间 {omitted_lines} 行改动] ...\n{tail}")
        }
    }
}

pub fn ai_skipped_lockfile_review(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (lockfile skipped by default)"),
        LanguageKind::Zh => format!("{path}（锁文件默认忽略审查）"),
    }
}

pub fn ai_skipped_binary(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (binary file)"),
        LanguageKind::Zh => format!("{path}（二进制文件）"),
    }
}

pub fn ai_skipped_non_utf8(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (non-UTF-8 text encoding, partially covered)"),
        LanguageKind::Zh => format!("{path}（非 UTF-8 文本编码，未完整覆盖）"),
    }
}

pub fn ai_skipped_diff_too_large(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (diff too large, skipped)"),
        LanguageKind::Zh => format!("{path}（大文件超出单文件审查上限）"),
    }
}

pub fn ai_skipped_total_too_large(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (diff too large, skipped)"),
        LanguageKind::Zh => format!("{path}（所选总改动较大，超出单次上下文上限）"),
    }
}

pub fn ai_skipped_sensitive_redacted(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (local env or sensitive values redacted locally)"),
        LanguageKind::Zh => format!("{path}（本地环境或敏感凭据值已本地脱敏）"),
    }
}

pub fn ai_skipped_large_partial(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!("{path} (large diff, partially covered)"),
        LanguageKind::Zh => format!("{path}（大文件改动较大，仅部分覆盖）"),
    }
}

pub fn cpush_skip_lockfile(locale: LanguageKind, added: u64, removed: u64) -> String {
    match locale {
        LanguageKind::En => {
            format!("Lockfile (+{added} -{removed}, group with related dependency changes)")
        }
        LanguageKind::Zh => format!("锁文件（+{added} -{removed}，按关联依赖变更归组）"),
    }
}

pub const fn cpush_skip_binary_patch(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Binary file (patch omitted)",
        "二进制文件（不展开文本补丁）",
    )
}

pub const fn cpush_skip_binary(locale: LanguageKind) -> &'static str {
    tr(locale, "Binary file", "二进制文件")
}

pub const fn cpush_skip_non_utf8(locale: LanguageKind) -> &'static str {
    tr(locale, "Non-UTF-8 text", "非 UTF-8 编码文本")
}

pub const fn cpush_skip_large_untracked(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Large file (exceeds inline limit)",
        "大文件（超出单文件内联上限）",
    )
}

pub const fn cpush_skip_sensitive(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Suspected local env or credential file (values redacted locally; confirm exclusion with user before committing)",
        "疑似本地环境或敏感凭据文件（值已本地脱敏，提交前需向用户确认是否排除）",
    )
}

pub const fn cpush_skip_slimmed_patch(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Large diff, patch truncated",
        "大文件改动较大，补丁已精简",
    )
}

pub const fn cpush_skip_omitted_budget(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Total selected diff is large, inline patch omitted",
        "所选总改动较大，省略内联补丁",
    )
}

pub fn cpush_step_branch_changed_error(
    locale: LanguageKind,
    actual_branch: &str,
    detached: bool,
    expected_branch: &str,
) -> String {
    match locale {
        LanguageKind::En => {
            let current = if detached {
                "detached HEAD"
            } else {
                actual_branch
            };
            format!(
                "Active worktree branch changed to \"{current}\" (planned branch was \"{expected_branch}\"); paused writes to protect changes"
            )
        }
        LanguageKind::Zh => {
            let current = if detached {
                "分离 HEAD"
            } else {
                actual_branch
            };
            format!(
                "工作树当前分支已变为“{current}”（原计划分支为“{expected_branch}”），已暂停写入以保护改动"
            )
        }
    }
}

pub fn cpush_step_missing_file_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Planned file \"{path}\" is no longer in the current change list; paused this step"
        ),
        LanguageKind::Zh => format!("待提交文件“{path}”已不在当前变更列表中，已暂停当前步骤"),
    }
}

pub fn cpush_step_stale_file_error(locale: LanguageKind, path: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Content or staging scope of planned file \"{path}\" changed externally; paused this step to prevent unintended commits"
        ),
        LanguageKind::Zh => format!(
            "待提交文件“{path}”的内容或暂存范围已在外部发生变化，已暂停当前步骤以防止误提交"
        ),
    }
}

// ============================================================================
// Models, Validation & Credentials (`models/**`)
// ============================================================================

pub fn provider_label(locale: LanguageKind, provider: ProviderKind) -> &'static str {
    match locale {
        LanguageKind::En => provider.label(),
        LanguageKind::Zh => match provider {
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Openai => "OpenAI",
            ProviderKind::Deepseek => "DeepSeek",
            ProviderKind::Ollama => "Ollama（本地）",
            ProviderKind::Custom => "自定义兼容",
        },
    }
}

pub fn token_context_range_error(locale: LanguageKind, min: u32, max: u32) -> String {
    match locale {
        LanguageKind::En => format!("Context window must be an integer between {min} and {max}"),
        LanguageKind::Zh => format!("上下文长度须为 {min} 至 {max} 之间的整数"),
    }
}

pub fn token_output_range_error(locale: LanguageKind, min: u32, max: u32) -> String {
    match locale {
        LanguageKind::En => {
            format!("Max output tokens must be an integer between {min} and {max}")
        }
        LanguageKind::Zh => format!("最大输出 Token 须为 {min} 至 {max} 之间的整数"),
    }
}

pub const fn token_output_exceeds_context_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Max output tokens cannot exceed context window",
        "最大输出 Token 不能超过上下文长度",
    )
}

pub const fn url_required_error(locale: LanguageKind) -> &'static str {
    tr(locale, "Base URL is required", "请填写接口地址")
}

pub const fn url_invalid_http_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter a valid HTTP / HTTPS base URL",
        "请填写有效的 HTTP / HTTPS 接口地址",
    )
}

pub const fn url_no_userinfo_http_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Base URL must be an HTTP / HTTPS address without embedded username or password",
        "接口地址须为不含用户名和密码的 HTTP / HTTPS 地址",
    )
}

pub const fn model_invalid_id_record_error(locale: LanguageKind) -> &'static str {
    tr(locale, "Invalid model identifier", "模型标识无效")
}

pub const fn model_id_required_error(locale: LanguageKind) -> &'static str {
    tr(locale, "Model ID is required", "请填写模型 ID")
}

pub const fn model_id_invalid_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Model ID format is invalid or exceeds length limit",
        "模型 ID 格式无效或长度超出限制",
    )
}

pub const fn model_display_name_invalid_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Display name format is invalid or exceeds length limit",
        "显示名称格式无效或长度超出限制",
    )
}

pub const fn model_api_key_required_error(locale: LanguageKind) -> &'static str {
    tr(locale, "API Key is required", "请填写 API Key")
}

pub const fn model_api_key_invalid_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "API Key contains invalid control characters or exceeds length limit",
        "API Key 包含非法控制字符或长度超出限制",
    )
}

pub const fn models_file_unreadable_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Could not read local model configuration; fell back to zero-model state without overwriting the existing file.",
        "无法读取本地模型配置，已回退为零模型状态；原有文件未被覆盖。",
    )
}

pub const fn models_file_corrupted_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local model configuration file is corrupted or incompatible; fell back to zero-model state without overwriting the existing file.",
        "本地模型配置文件已损坏或版本不兼容，已回退为零模型状态；原有文件未被覆盖。",
    )
}

pub fn model_connection_summary(
    locale: LanguageKind,
    provider: ProviderKind,
    model_id: &str,
    latency_ms: u64,
) -> String {
    let provider_name = provider_label(locale, provider);
    match locale {
        LanguageKind::En => format!("Connected · {provider_name} · {model_id} ({latency_ms} ms)"),
        LanguageKind::Zh => format!("连接成功：{provider_name} · {model_id}（{latency_ms} ms）"),
    }
}

pub fn model_save_aborted_error(locale: LanguageKind, error: &str, editing: bool) -> String {
    match (locale, editing) {
        (LanguageKind::En, true) => {
            format!("{error}; model edits were not saved, and your input was preserved.")
        }
        (LanguageKind::En, false) => {
            format!("{error}; model configuration was not saved, and your input was preserved.")
        }
        (LanguageKind::Zh, true) => format!("{error}；没有保存模型修改，输入已保留。"),
        (LanguageKind::Zh, false) => format!("{error}；没有保存模型配置，输入已保留。"),
    }
}

// ============================================================================
// C-Push Coordinator & Remote Sync Messages (`cpush/**`, `repository/remote.rs`)
// ============================================================================

pub fn cpush_scope_badge(locale: LanguageKind, total: usize, partial_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if total == 1 { "file" } else { "files" };
            if partial_count > 0 {
                format!("{total} selected {noun} · {partial_count} partially staged")
            } else {
                format!("{total} selected {noun}")
            }
        }
        LanguageKind::Zh => {
            if partial_count > 0 {
                format!("{total} 个已选文件 · 含 {partial_count} 项部分暂存")
            } else {
                format!("{total} 个已选文件")
            }
        }
    }
}

pub const fn cpush_conflict_badge(locale: LanguageKind) -> &'static str {
    tr(locale, "Conflict", "冲突")
}

pub const fn cpush_partial_badge(locale: LanguageKind) -> &'static str {
    tr(locale, "Staged only", "仅已暂存")
}

pub const fn cpush_needs_confirm_badge(locale: LanguageKind) -> &'static str {
    tr(locale, "Confirm", "待确认")
}

pub const fn cpush_option_agent_merge(locale: LanguageKind) -> &'static str {
    tr(locale, "Merge with Agent", "让 Agent 合并")
}

pub const fn cpush_option_manual_merge(locale: LanguageKind) -> &'static str {
    tr(locale, "Merge manually", "我手动合并")
}

pub const fn cpush_option_abort_rebase(locale: LanguageKind, has_commits: bool) -> &'static str {
    match (locale, has_commits) {
        (LanguageKind::En, true) => "Abort rebase (keep local commits)",
        (LanguageKind::En, false) => "Abort rebase and restore",
        (LanguageKind::Zh, true) => "中止变基（保留本地提交）",
        (LanguageKind::Zh, false) => "中止变基并恢复原状",
    }
}

pub const fn cpush_option_retry_push(locale: LanguageKind) -> &'static str {
    tr(locale, "Retry push", "重试推送")
}

pub const fn cpush_option_continue_push(locale: LanguageKind) -> &'static str {
    tr(locale, "Continue push", "继续推送")
}

pub const fn cpush_option_keep_local_end(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Keep local commits and finish",
        "保留本地提交并结束",
    )
}

pub const fn cpush_option_skip_push_keep_local(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Do not push yet (keep local commits)",
        "暂不推送（保留本地提交）",
    )
}

pub fn cpush_timeline_local_commit_titled(
    locale: LanguageKind,
    short_oid: &str,
    title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Local commit {short_oid}: {title}"),
        LanguageKind::Zh => format!("本地提交 {short_oid}：{title}"),
    }
}

pub fn cpush_timeline_local_commit_written(locale: LanguageKind, short_oid: &str) -> String {
    match locale {
        LanguageKind::En => format!("Local commit {short_oid} written"),
        LanguageKind::Zh => format!("本地提交 {short_oid} 写入成功"),
    }
}

pub fn cpush_timeline_commit_item(locale: LanguageKind, short_oid: &str, title: &str) -> String {
    match locale {
        LanguageKind::En => format!("Commit {short_oid}: {title}"),
        LanguageKind::Zh => format!("提交 {short_oid}：{title}"),
    }
}

pub fn cpush_timeline_sync_conflict(
    locale: LanguageKind,
    upstream: &str,
    names: &[&str],
) -> String {
    let joined = join_list(locale, names);
    match locale {
        LanguageKind::En => format!("Conflict while syncing {upstream}: {joined}"),
        LanguageKind::Zh => format!("同步 {upstream} 时遇到冲突：{joined}"),
    }
}

pub fn cpush_conflict_description(
    locale: LanguageKind,
    upstream: &str,
    names: &[&str],
    extra_reason: Option<&str>,
) -> String {
    let joined = join_list(locale, names);
    match locale {
        LanguageKind::En => {
            let base = format!(
                "Code conflict in {joined} while rebasing onto {upstream}. Click any file to inspect in your editor, then choose an action or enter merge instructions:"
            );
            match extra_reason.map(str::trim).filter(|s| !s.is_empty()) {
                Some(reason) => format!("{reason}. {base}"),
                None => base,
            }
        }
        LanguageKind::Zh => {
            let base = format!(
                "变基至 {upstream} 时 {joined} 发生代码冲突。可点击文件在编辑器中查看冲突，选择处理方式或输入合并要求："
            );
            match extra_reason.map(str::trim).filter(|s| !s.is_empty()) {
                Some(reason) => format!("{reason}。{base}"),
                None => base,
            }
        }
    }
}

pub const fn cpush_conflict_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Merge conflict while syncing remote changes",
        "同步远端改动遇到代码冲突",
    )
}

pub fn cpush_conflict_scope(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Stage: rebase {upstream}"),
        LanguageKind::Zh => format!("阶段：rebase {upstream}"),
    }
}

pub fn cpush_timeline_push_failed(locale: LanguageKind, remote_target: &str) -> String {
    match locale {
        LanguageKind::En => format!("Push to {remote_target} failed"),
        LanguageKind::Zh => format!("推送至 {remote_target} 失败"),
    }
}

pub const fn cpush_push_failed_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Committed locally, push failed",
        "已在本地提交，推送失败",
    )
}

pub fn cpush_push_failed_description(
    locale: LanguageKind,
    remote_target: &str,
    reason: &str,
) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commits were written to branch history, but pushing to {remote_target} failed ({reason}). Click \"Retry push\" to push existing commits without recommitting."
        ),
        LanguageKind::Zh => format!(
            "本地提交已成功写入历史，推送至 {remote_target} 未成功（{reason}）。点击「重试推送」仅推送已有提交，不会重复提交。"
        ),
    }
}

pub fn cpush_push_stage_scope(locale: LanguageKind, remote_target: &str) -> String {
    match locale {
        LanguageKind::En => format!("Stage: push to {remote_target}"),
        LanguageKind::Zh => format!("阶段：推送至 {remote_target}"),
    }
}

pub const fn cpush_sessions_unreadable_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Could not read local C-Push session history; started with empty session history.",
        "无法读取本地 C-Push 会话记录，已从空白会话记录启动。",
    )
}

pub const fn cpush_sessions_corrupted_warning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local C-Push session file is corrupted or incompatible; ignored stale entries without overwriting the original file.",
        "本地 C-Push 会话记录损坏或版本不兼容，已忽略旧记录；原文件未被覆盖。",
    )
}

pub fn cpush_sensitive_question(
    locale: LanguageKind,
    names: &[String],
    has_other_files: bool,
) -> (
    String,
    String,
    String,
    Vec<crate::foundation::pi::PiCPushQuestionOption>,
) {
    let joined = join_list(locale, names);
    let options = match (locale, has_other_files) {
        (LanguageKind::En, true) => vec![
            crate::foundation::pi::PiCPushQuestionOption {
                id: "skip".into(),
                label: "Exclude these files and commit the rest".into(),
                recommended: Some(true),
            },
            crate::foundation::pi::PiCPushQuestionOption {
                id: "include".into(),
                label: "Confirmed safe, commit and push together".into(),
                recommended: None,
            },
        ],
        (LanguageKind::En, false) => vec![
            crate::foundation::pi::PiCPushQuestionOption {
                id: "skip".into(),
                label: "Skip for now and keep locally".into(),
                recommended: Some(true),
            },
            crate::foundation::pi::PiCPushQuestionOption {
                id: "include".into(),
                label: "Confirmed safe, commit and push".into(),
                recommended: None,
            },
        ],
        (LanguageKind::Zh, true) => vec![
            crate::foundation::pi::PiCPushQuestionOption {
                id: "skip".into(),
                label: "排除这些文件，仅提交其余改动".into(),
                recommended: Some(true),
            },
            crate::foundation::pi::PiCPushQuestionOption {
                id: "include".into(),
                label: "确认安全，一并提交并推送".into(),
                recommended: None,
            },
        ],
        (LanguageKind::Zh, false) => vec![
            crate::foundation::pi::PiCPushQuestionOption {
                id: "skip".into(),
                label: "暂不提交，保留在本地".into(),
                recommended: Some(true),
            },
            crate::foundation::pi::PiCPushQuestionOption {
                id: "include".into(),
                label: "确认安全，继续提交并推送".into(),
                recommended: None,
            },
        ],
    };
    match locale {
        LanguageKind::En => (
            "Local environment or sensitive config detected — exclude?".into(),
            format!("Involves {joined}"),
            "Selected scope includes local environment or credential files (such as .env or key files) that usually should not be committed or pushed. Confirm how to proceed:".into(),
            options,
        ),
        LanguageKind::Zh => (
            "检测到本地环境或敏感配置文件，是否排除？".into(),
            format!("涉及 {joined}"),
            "已选范围包含本地环境或敏感凭据文件（如 .env / 密钥文件），通常不应入库推送。请确认处理方式：".into(),
            options,
        ),
    }
}

pub fn cpush_pushed_done_title(locale: LanguageKind, count: usize, remote_branch: &str) -> String {
    match locale {
        LanguageKind::En => {
            if count == 1 {
                format!("1 commit pushed to {remote_branch}")
            } else {
                format!("{count} commits pushed to {remote_branch}")
            }
        }
        LanguageKind::Zh => format!("{count} 个提交已推送至 {remote_branch}"),
    }
}

pub fn cpush_pushed_done_description(locale: LanguageKind, skipped_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if skipped_count == 0 {
                "Unselected files and unstaged drafts of partially staged files remain safely preserved in the working tree.".into()
            } else if skipped_count == 1 {
                "Skipped 1 file as chosen; remaining unselected changes and unstaged drafts remain safely preserved in the working tree.".into()
            } else {
                format!(
                    "Skipped {skipped_count} files as chosen; remaining unselected changes and unstaged drafts remain safely preserved in the working tree."
                )
            }
        }
        LanguageKind::Zh => {
            if skipped_count == 0 {
                "未选文件与部分暂存文件的未暂存草稿均安全保留在工作区。".into()
            } else {
                format!(
                    "已按你的选择跳过 {skipped_count} 个文件；其余未选改动与未暂存草稿均安全保留在工作区。"
                )
            }
        }
    }
}

pub fn cpush_synced_scope(locale: LanguageKind, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("{branch} · Synced"),
        LanguageKind::Zh => format!("{branch} · 已同步"),
    }
}

// ============================================================================
// Additional Repository, Branch, Commit, Editor, Remote & Model Messages
// ============================================================================

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
        "Executable Git hook detected; branch checkout was not executed. Switch branches in Git / IDE",
        "检测到可执行 Git hook，当前版本不支持安全执行，未创建或切换分支；请在 Git / IDE 中操作",
    )
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
        "Index contains sparse-checkout or skip-worktree / assume-unchanged flags; handle them in Git / IDE before committing",
        "当前暂存区含稀疏检出或隐藏变更标记，暂不执行提交写入；请先在 Git / IDE 中处理",
    )
}

pub const fn commit_submodule_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Repositories with submodule entries are not supported for commit writes yet; commit in Git / IDE",
        "含子模块的仓库尚未通过提交写入验证，请在 Git / IDE 中操作",
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

pub const fn editor_invalid_line_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Invalid line number; line numbers must start at 1. Current draft and selection were preserved",
        "跳转行号无效，行号必须从 1 开始；当前草稿与选择已保留",
    )
}

pub const fn ai_selection_required_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Select at least one changed file first; current draft was preserved."
        }
        (LanguageKind::En, true) => {
            "Select at least one changed file before starting C-Push; current draft was preserved."
        }
        (LanguageKind::Zh, false) => "请先选择至少一个变更文件；当前草稿已保留。",
        (LanguageKind::Zh, true) => "请先选择至少一个变更文件后再发起 C-Push；当前草稿已保留。",
    }
}

pub const fn ai_invalid_token_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Model request contains invalid or duplicate file scope tokens; refresh and try again."
        }
        (LanguageKind::En, true) => {
            "C-Push request contains invalid or duplicate file scope tokens; refresh and try again."
        }
        (LanguageKind::Zh, false) => "模型请求包含无效或重复的文件范围凭证，请刷新后重试。",
        (LanguageKind::Zh, true) => "C-Push 请求包含无效或重复的文件范围凭证，请刷新后重试。",
    }
}

pub const fn ai_file_missing_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Selected file is no longer in the worktree change list; refresh and confirm before running model tasks. Draft and selection were preserved."
        }
        (LanguageKind::En, true) => {
            "Selected file is no longer in the worktree change list; refresh and confirm before starting C-Push. Draft and selection were preserved."
        }
        (LanguageKind::Zh, false) => {
            "所选文件已不在当前工作树变更列表中，请刷新确认后再执行模型任务；当前草稿与选择已保留。"
        }
        (LanguageKind::Zh, true) => {
            "所选文件已不在当前工作树变更列表中，请刷新确认后再发起 C-Push；草稿与选择已保留。"
        }
    }
}

pub const fn ai_conflict_in_scope_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Selected scope contains unresolved merge conflicts; resolve conflicts before generating messages or reviewing. Draft and selection were preserved."
        }
        (LanguageKind::En, true) => {
            "Selected scope contains unresolved merge conflicts; resolve conflicts before running C-Push. Draft and selection were preserved."
        }
        (LanguageKind::Zh, false) => {
            "所选范围包含未解决的合并冲突文件，请先处理冲突后再生成说明或审查；草稿与选择已保留。"
        }
        (LanguageKind::Zh, true) => {
            "所选范围包含未解决的合并冲突文件，请先处理冲突后再执行 C-Push；草稿与选择已保留。"
        }
    }
}

pub const fn ai_scope_stale_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Selected file scope, staged content, or branch has changed; paused this model request. Refresh to verify latest changes; draft and selection were preserved."
        }
        (LanguageKind::En, true) => {
            "Selected file scope, staged content, or branch has changed; paused C-Push. Refresh to verify latest changes; draft and selection were preserved."
        }
        (LanguageKind::Zh, false) => {
            "所选文件范围、暂存内容或分支已变化，已暂停本次模型请求；请刷新核对最新变更，当前草稿与选择已保留。"
        }
        (LanguageKind::Zh, true) => {
            "所选文件范围、暂存内容或分支已变化，已暂停本次 C-Push；请刷新核对最新变更，草稿与选择已保留。"
        }
    }
}

pub const fn cpush_detached_head_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Currently on detached HEAD; switch to a branch in the header first before running C-Push. Draft and selection were preserved.",
        "当前处于分离 HEAD，请先在顶部新建或切换分支后再使用 C-Push；草稿与选择已保留。",
    )
}

pub const fn model_not_found_edit_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "The model being edited does not exist or was deleted.",
        "要编辑的模型不存在或已被删除。",
    )
}

pub const fn provider_not_found_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "The provider connection does not exist or was deleted.",
        "该供应商连接不存在或已被删除。",
    )
}

pub const fn provider_limit_reached_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Saved provider connection count reached the maximum limit; delete unused connections first.",
        "已保存的供应商连接数量已达上限，请先删除不再使用的连接。",
    )
}

pub const fn provider_test_needs_model_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Add at least one model under this connection before testing it.",
        "请先在该连接下至少添加一个模型后再测试连接。",
    )
}

pub const fn model_limit_reached_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Saved model count reached the maximum limit; delete unused models first.",
        "已保存的模型数量已达上限，请先删除不再使用的模型。",
    )
}

pub const fn model_select_missing_id_error(locale: LanguageKind) -> &'static str {
    tr(locale, "No model specified to select", "未指定要切换的模型")
}

pub const fn model_selected_not_found_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected model does not exist or was deleted",
        "所选模型不存在或已被删除",
    )
}

pub const fn model_delete_missing_id_error(locale: LanguageKind) -> &'static str {
    tr(locale, "No model specified to delete", "未指定要删除的模型")
}

pub const fn model_delete_not_found_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Model to delete does not exist",
        "要删除的模型不存在",
    )
}

pub const fn model_active_missing_runtime_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Currently selected model no longer exists; reselect a model in the footer or Settings. Draft and selection were preserved.",
        "当前选中的模型已不存在，请在底栏或设置中重新选择模型；草稿与选择已保留。",
    )
}

pub const fn model_missing_stored_key_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "API key for this provider connection is missing from ~/.picket/credentials.json; update the key in Settings. Draft and selection were preserved.",
        "该供应商连接在 ~/.picket/credentials.json 中的 API Key 已缺失，请在设置中重新填写密钥；草稿与选择已保留。",
    )
}

pub const fn model_test_cancelled_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Connection test cancelled; current input was preserved.",
        "连接测试已取消；当前输入已保留。",
    )
}

pub const fn model_review_cancelled_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Code review cancelled; current prompt and selection were preserved.",
        "已取消代码审查；当前提示词与选择已保留。",
    )
}

pub fn model_reveal_auth_reason(locale: LanguageKind, connection_name: &str) -> String {
    match locale {
        LanguageKind::En => format!("reveal the saved API key for \"{connection_name}\" in Picket"),
        LanguageKind::Zh => {
            format!("查看 Picket 中供应商连接“{connection_name}”已保存的完整 API Key")
        }
    }
}

pub const fn model_reveal_auth_cancelled_message(locale: LanguageKind) -> &'static str {
    tr(locale, "Authentication cancelled.", "已取消身份验证。")
}

pub const fn model_reveal_auth_failed_message(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Authentication failed; API key was not shown.",
        "身份验证未通过，未显示密钥。",
    )
}

pub const fn model_reveal_auth_unavailable_message(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "macOS authentication is unavailable on this device.",
        "当前设备无法调用 macOS 身份验证。",
    )
}

pub const fn model_reveal_no_key_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "This provider connection does not use an API key.",
        "该供应商连接不需要保存 API Key。",
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

// ============================================================================
// Remaining C-Push Coordinator Strings (`cpush/mod.rs`)
// ============================================================================

pub const fn remote_branch_fallback(locale: LanguageKind) -> &'static str {
    tr(locale, "remote branch", "远端分支")
}

pub const fn cpush_commit_completed_fallback(locale: LanguageKind) -> &'static str {
    tr(locale, "Commit completed", "提交已完成")
}

pub const fn cpush_commit_changes_fallback(locale: LanguageKind) -> &'static str {
    tr(locale, "Commit changes", "提交改动")
}

pub const fn cpush_restart_conflict_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Unresolved merge conflicts were found after restart; writes did not continue automatically",
        "应用重启后检测到未完成的合并冲突，未自动继续写入",
    )
}

pub const fn cpush_interrupted_push_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Previous session interrupted after local commits; push not yet confirmed",
        "上次会话已完成本地提交后中断，尚未确认推送",
    )
}

pub const fn cpush_interrupted_push_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified live Git history and kept completed commits without auto-rerunning push. Click \"Retry push\" to push or keep local commits.",
        "已核对真实 Git 历史并保留已完成提交，未自动重跑推送。请确认后点击「重试推送」或保留本地提交。",
    )
}

pub const fn cpush_option_confirm_continue_remaining(locale: LanguageKind) -> &'static str {
    tr(locale, "Continue remaining commits", "确认继续剩余提交")
}

pub const fn cpush_option_dismiss_interrupted(
    locale: LanguageKind,
    has_commits: bool,
) -> &'static str {
    match (locale, has_commits) {
        (LanguageKind::En, false) => "End interrupted session",
        (LanguageKind::En, true) => "End session (keep completed commits)",
        (LanguageKind::Zh, false) => "结束中断会话",
        (LanguageKind::Zh, true) => "结束会话（保留已完成提交）",
    }
}

pub fn cpush_interrupted_commits_title(locale: LanguageKind, completed_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if completed_count == 0 {
                "Previous C-Push session was interrupted before writing".into()
            } else if completed_count == 1 {
                "Previous session interrupted after 1 commit".into()
            } else {
                format!("Previous session interrupted after {completed_count} commits")
            }
        }
        LanguageKind::Zh => {
            if completed_count == 0 {
                "上次 C-Push 会话在写入前中断".into()
            } else {
                format!("上次会话已完成 {completed_count} 个提交后中断")
            }
        }
    }
}

pub const fn cpush_interrupted_commits_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified live Git state without auto-rerunning writes. Confirm whether to continue remaining steps or end this session.",
        "已核对真实 Git 状态，未自动重跑写入。请确认继续剩余步骤或结束本次会话。",
    )
}

pub fn cpush_manual_still_conflicted_desc(locale: LanguageKind, names: &[&str]) -> String {
    let joined = join_list(locale, names);
    match locale {
        LanguageKind::En => format!(
            "Unresolved conflicts remain in {joined}. Continue resolving in your external editor and finish rebase before switching back."
        ),
        LanguageKind::Zh => {
            format!("检测到 {joined} 仍存在未解决冲突，请在外部编辑器继续处理并完成变基后切回。")
        }
    }
}

pub const fn cpush_manual_rebase_incomplete_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Conflict markers are cleared, but rebase is still in progress; finish rebase (`git rebase --continue`) in your IDE or terminal and switch back.",
        "冲突标记已清除，但变基尚未结束；请在 IDE 或终端完成变基（git rebase --continue）后切回。",
    )
}

pub fn cpush_timeline_terminal_pushed(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => format!("Detected push to {display} completed in terminal"),
        LanguageKind::Zh => format!("检测到终端已完成推送至 {display}"),
    }
}

pub fn cpush_terminal_pushed_title(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => format!("Pushed to {display} (completed in terminal)"),
        LanguageKind::Zh => format!("已推送至 {display}（终端已完成）"),
    }
}

pub const fn cpush_terminal_pushed_from_failed_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified with remote that the latest commits are already pushed; collapsed stale notice.",
        "经联网核对远端分支已包含最新提交，已自动收起旧提示。",
    )
}

pub const fn cpush_qa_manual_ext_question(locale: LanguageKind) -> &'static str {
    tr(locale, "Merge conflict resolution", "合并冲突处理")
}

pub const fn cpush_qa_manual_ext_answer(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Merged externally and pushed in terminal",
        "在外部完成合并并在终端推送",
    )
}

pub fn cpush_timeline_manual_resolved_and_pushed(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Detected conflicts resolved and pushed to {display} in terminal")
        }
        LanguageKind::Zh => format!("检测到冲突已解决且终端已推送至 {display}"),
    }
}

pub const fn cpush_terminal_pushed_from_manual_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Verified remote branch includes the merged commits; archived session and collapsed previous prompt.",
        "经核对远端分支已包含合并后的最新提交，已自动归档会话并收起旧提问。",
    )
}

pub const fn cpush_timeline_manual_rebase_done(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Detected manual conflict merge and rebase completed",
        "检测到手动冲突合并与变基已完成",
    )
}

pub const fn cpush_manual_rebase_done_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Conflict resolved, rebase completed",
        "冲突已解决，变基已完成",
    )
}

pub const fn cpush_manual_rebase_done_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Manual conflict merge and rebase completed externally; standalone pull does not push automatically. Trigger push explicitly when ready.",
        "已在外部完成冲突合并与变基；单独发起的拉取不包含自动推送权限，如需推送请明确触发。",
    )
}

pub fn cpush_timeline_awaiting_continue_push(locale: LanguageKind, target_label: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Local commits not yet pushed to {target_label}; waiting for confirmation")
        }
        LanguageKind::Zh => format!("本地提交尚未推送至 {target_label}，等待确认继续推送"),
    }
}

pub const fn cpush_resolved_continue_push_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Conflict resolved — continue pushing?",
        "冲突已解决，是否继续推送？",
    )
}

pub fn cpush_resolved_continue_push_desc(locale: LanguageKind, target_label: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Conflicts are resolved, and commits have not yet been pushed to {target_label}. Click \"Continue push\" to push:"
        ),
        LanguageKind::Zh => {
            format!("检测到冲突已解决但尚未推送至 {target_label}。确认后点击「继续推送」：")
        }
    }
}

pub const fn cpush_active_session_busy_error(locale: LanguageKind, is_cpush: bool) -> &'static str {
    match (locale, is_cpush) {
        (LanguageKind::En, false) => {
            "Current worktree already has an active session item; handle it in the message bar below first."
        }
        (LanguageKind::En, true) => {
            "Current worktree already has an active C-Push session; continue handling it in the message bar below."
        }
        (LanguageKind::Zh, false) => "当前工作树已有未结束的会话事项，请先在下方消息栏处理。",
        (LanguageKind::Zh, true) => "当前工作树已有未结束的 C-Push 会话，请在下方消息栏继续处理。",
    }
}

pub fn pull_rebase_missing_upstream_error(locale: LanguageKind, remotes: &[String]) -> String {
    if remotes.is_empty() {
        tr(
            locale,
            "No Git remote is configured for this repository; cannot pull and rebase. Draft and selection were preserved.",
            "当前仓库尚未配置任何 Git 远端（remote），无法执行拉取并变基；草稿与选择已保留。",
        )
        .into()
    } else {
        let joined = join_list(locale, remotes);
        match locale {
            LanguageKind::En => format!(
                "Current branch has no upstream tracking branch (available remotes: {joined}); cannot determine pull target. Draft and selection were preserved."
            ),
            LanguageKind::Zh => format!(
                "当前分支尚未设置跟踪远端分支（可用远端：{joined}），无法确定拉取目标；草稿与选择已保留。"
            ),
        }
    }
}

pub fn pull_rebase_dirty_overlap_error(
    locale: LanguageKind,
    upstream: &str,
    overlapping_paths: &[String],
) -> String {
    let joined = join_list(locale, overlapping_paths);
    match locale {
        LanguageKind::En => format!(
            "Updates from {upstream} touch uncommitted local changes in {joined} and cannot rebase without disturbing staged/unstaged boundaries; paused safely with drafts and selections intact."
        ),
        LanguageKind::Zh => format!(
            "远端 {upstream} 的更新涉及当前存在未提交改动的 {joined}，无法在不损伤暂存/草稿边界的前提下自动变基；已暂停操作，草稿与选择完整保留。"
        ),
    }
}

pub fn cpush_timeline_scope_frozen(locale: LanguageKind, badge: &str) -> String {
    match locale {
        LanguageKind::En => format!("Scope frozen: {badge}"),
        LanguageKind::Zh => format!("已固定范围：{badge}"),
    }
}

pub const fn cpush_timeline_planning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Analyzing file changes and planning split commits…",
        "正在分析改动归属并规划拆分提交…",
    )
}

pub const fn cpush_progress_planning_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Analyzing and splitting selected changes…",
        "正在分析并拆分已选改动…",
    )
}

pub const fn cpush_progress_planning_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Authorized scope is frozen; unselected files and unstaged drafts of partially staged files remain untouched.",
        "已固定本次授权范围；未选文件与部分暂存文件的未暂存草稿将保留在原处。",
    )
}

pub const fn cpush_progress_planning_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Read scope -> Plan split -> Local commits -> Push remote",
        "读取范围 → 规划拆分 → 本地提交 → 远端推送",
    )
}

pub const fn cpush_no_pending_session_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No pending C-Push session found for the current worktree",
        "未找到当前工作树的待处理会话",
    )
}

pub const fn cpush_mismatched_reply_target_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Session reply did not match target worktree or run identity; ignored request",
        "会话回复的目标工作树或任务标识不匹配，已忽略该请求",
    )
}

pub const fn cpush_not_interactive_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session is not in an interactive state",
        "当前会话不在可交互处理状态",
    )
}

pub const fn cpush_missing_active_message_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session has no pending message item",
        "当前会话缺少待处理事项",
    )
}

pub const fn cpush_stale_message_id_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Session prompt has already updated; confirm again from the latest prompt",
        "当前事项状态已更新，请基于最新提示重新确认",
    )
}

pub fn cpush_dismiss_kept_local_title(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            if count == 1 {
                "Kept 1 local commit and finished session".into()
            } else {
                format!("Kept {count} local commits and finished session")
            }
        }
        LanguageKind::Zh => format!("已保留 {count} 个本地提交并结束本次会话"),
    }
}

pub const fn cpush_dismiss_kept_local_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Local commits remain on the current branch without pushing to remote; remaining working tree drafts are unchanged.",
        "本地提交已保留在当前分支，未继续推送远端；其余工作区草稿保持不变。",
    )
}

pub const fn cpush_no_retry_remaining_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session has no remaining local commit steps to retry",
        "当前会话没有可重试的剩余本地提交步骤",
    )
}

pub fn cpush_push_track_label(locale: LanguageKind, remote: &str, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Push and track {remote}/{branch}"),
        LanguageKind::Zh => format!("推送并跟踪 {remote}/{branch}"),
    }
}

pub const fn cpush_enter_remote_or_select_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter a remote name or click a push target option above",
        "请输入远端名称或点击上方推送目标选项",
    )
}

pub fn cpush_ambiguous_remote_input_error(locale: LanguageKind, available: &[String]) -> String {
    let joined = join_list(locale, available);
    match locale {
        LanguageKind::En => format!(
            "Could not uniquely identify a target remote from input (available remotes: {joined}); click the remote button or type its exact name."
        ),
        LanguageKind::Zh => format!(
            "未能从输入中唯一确认远端目标（当前可用远端：{joined}），请直接点击对应远端按钮或明确写出远端名。"
        ),
    }
}

pub const fn cpush_aborted_rebase_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Rebase aborted; local commits were kept intact",
        "已中止变基，本地提交保持原状",
    )
}

pub const fn cpush_aborted_rebase_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Rebase aborted, local commits kept",
        "已中止变基，保留本地提交",
    )
}

pub const fn cpush_timeline_chose_manual_merge(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected: resolve merge conflicts manually in external editor",
        "已选择：前往外部编辑器手动合并冲突",
    )
}

pub const fn cpush_timeline_waiting_return_reconcile(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Writes paused; waiting to reconcile Git and remote status when you return to Picket",
        "已暂停写入操作，等待切回 Picket 后自动核对 Git 与远端状态",
    )
}

pub const fn cpush_waiting_manual_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Waiting for manual merge in IDE",
        "等待你在 IDE 完成手动合并",
    )
}

pub const fn cpush_waiting_manual_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Resolve conflicts and finish rebase in your external editor, then switch back to Picket to reconcile status automatically.",
        "请在外部编辑器解决冲突并完成变基后切回 Picket，桌面层将自动核对状态。",
    )
}

pub const fn cpush_waiting_manual_scope(locale: LanguageKind) -> &'static str {
    tr(locale, "Stage: merge conflict", "阶段：合并冲突")
}

pub const fn cpush_enter_merge_instruction_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter merge instructions or choose a resolution option above",
        "请输入具体的冲突合并要求或选择上方处理方式",
    )
}

pub const fn cpush_unsupported_reply_state_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current session state does not accept this reply",
        "当前会话状态不支持该回复操作",
    )
}

pub const fn cpush_invalid_option_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected option does not belong to the current prompt",
        "所选回复项不在当前问题选项中",
    )
}

pub const fn cpush_empty_reply_text_error(locale: LanguageKind) -> &'static str {
    tr(locale, "Enter a non-empty reply", "请输入具体的回复内容")
}

pub const fn cpush_choose_option_or_text_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Choose an option or enter instructions",
        "请选择一个处理选项或输入具体要求",
    )
}

pub fn cpush_timeline_confirmed_decision(
    locale: LanguageKind,
    question: &str,
    answer: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Confirmed: {question} -> {answer}"),
        LanguageKind::Zh => format!("已确认：{question} → {answer}"),
    }
}

pub const fn cpush_timeline_replanning(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Generating split commit plan from your decision…",
        "正在按你的决定生成拆分提交方案…",
    )
}

pub const fn cpush_progress_replanning_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Updating commit plan from your confirmation…",
        "正在按你的确认更新提交计划…",
    )
}

pub fn cpush_progress_replanning_desc(locale: LanguageKind, answer: &str) -> String {
    match locale {
        LanguageKind::En => format!("Recorded choice: {answer}"),
        LanguageKind::Zh => format!("已记录选择：{answer}"),
    }
}

pub const fn cpush_progress_replanning_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Read scope -> Replan -> Local commits -> Push remote",
        "读取范围 → 重新规划 → 本地提交 → 远端推送",
    )
}

pub const fn cpush_agent_merge_no_model_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No model is configured or selected for Agent merge; add a model first or choose \"Merge manually\".",
        "当前尚未配置或选择模型，无法执行 Agent 合并；请先添加模型或选择「我手动合并」。",
    )
}

pub fn cpush_timeline_authorized_agent_merge(locale: LanguageKind, answer_text: &str) -> String {
    match locale {
        LanguageKind::En => format!("Authorized Agent conflict merge: {answer_text}"),
        LanguageKind::Zh => format!("已授权 Agent 处理冲突：{answer_text}"),
    }
}

pub fn cpush_timeline_merging_files(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if count == 1 { "file" } else { "files" };
            format!("Merging {count} conflicted {noun} and validating markers…")
        }
        LanguageKind::Zh => format!("正在合并 {count} 个冲突文件并校验结果…"),
    }
}

pub const fn cpush_progress_merging_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Agent is merging conflicted files…",
        "Agent 正在合并冲突文件…",
    )
}

pub fn cpush_progress_merging_desc(locale: LanguageKind, instruction: &str) -> String {
    if instruction.is_empty() {
        tr(
            locale,
            "Modifying only the conflicted files; conflict markers will be verified before continuing rebase.",
            "仅修改当前冲突文件，合并后将校验冲突标记并继续变基。",
        )
        .into()
    } else {
        match locale {
            LanguageKind::En => {
                format!("Merging conflicted files with instruction \"{instruction}\"…")
            }
            LanguageKind::Zh => format!("正在按要求「{instruction}」合并冲突文件…"),
        }
    }
}

pub fn cpush_merging_scope(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Stage: merge {upstream}"),
        LanguageKind::Zh => format!("阶段：合并 {upstream}"),
    }
}

pub const fn cpush_progress_merging_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Merge conflict -> Verify markers -> Continue rebase",
        "冲突合并 → 校验标记 → 完成变基",
    )
}

pub fn cpush_merge_resolver_failed_desc(locale: LanguageKind, err: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{err}. Conflicted files remain unchanged; adjust instructions to retry or choose \"Merge manually\"."
        ),
        LanguageKind::Zh => {
            format!("{err}。冲突文件保持原状，可调整要求重试或选择「我手动合并」。")
        }
    }
}

pub fn cpush_merge_apply_failed_desc(locale: LanguageKind, err: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{err}. Subsequent steps were paused; inspect conflict state in your editor or merge manually."
        ),
        LanguageKind::Zh => {
            format!("{err}。未自动继续后续步骤，请在编辑器核对冲突状态或手动合并。")
        }
    }
}

pub const fn cpush_next_commit_conflict_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current commit conflict was merged; rebasing the next commit encountered another conflict",
        "当前提交冲突已合并，下一笔提交变基时遇到新的冲突",
    )
}

pub fn cpush_timeline_agent_merge_done(locale: LanguageKind, summary: &str) -> String {
    match locale {
        LanguageKind::En => format!("Agent merged conflicts: {summary}"),
        LanguageKind::Zh => format!("Agent 已完成冲突合并：{summary}"),
    }
}

pub fn cpush_timeline_rebase_done_no_push(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Rebased onto {upstream} (not auto-pushed)"),
        LanguageKind::Zh => format!("已完成变基至 {upstream}（未自动推送）"),
    }
}

pub fn cpush_done_merge_pull_title(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Merged conflicts and rebased onto {upstream}"),
        LanguageKind::Zh => format!("已完成冲突合并并变基至 {upstream}"),
    }
}

pub fn cpush_done_merge_pull_desc(locale: LanguageKind, summary: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{summary}. Standalone pull does not push automatically; remaining working tree drafts were preserved."
        ),
        LanguageKind::Zh => {
            format!("{summary}。单独发起的拉取不自动推送远端，工作区其余草稿均完整保留。")
        }
    }
}

pub const fn cpush_conflict_cancelled_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Agent conflict merge cancelled",
        "已取消 Agent 冲突合并",
    )
}

pub const fn cpush_plan_cancelled_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push planning cancelled; draft and selection were preserved.",
        "已取消 C-Push 规划；草稿与选择已保留。",
    )
}

pub fn cpush_timeline_frozen_count(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if count == 1 { "file" } else { "files" };
            format!("Frozen scope of {count} selected {noun}")
        }
        LanguageKind::Zh => format!("已固定 {count} 个已选文件范围"),
    }
}

pub fn cpush_timeline_waiting_decision(locale: LanguageKind, title: &str) -> String {
    match locale {
        LanguageKind::En => format!("Waiting for decision: {title}"),
        LanguageKind::Zh => format!("等待决定：{title}"),
    }
}

pub const fn cpush_timeline_all_skipped(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Skipped all selected files in this run as chosen",
        "已按你的选择跳过本次全部已选文件",
    )
}

pub const fn cpush_all_skipped_title(locale: LanguageKind) -> &'static str {
    tr(locale, "Skipped commit as confirmed", "已按确认跳过提交")
}

pub const fn cpush_all_skipped_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected files and working tree drafts remain in place; no new commit was written.",
        "所选文件与工作区草稿均保留在原处，未写入新提交。",
    )
}

pub fn cpush_timeline_step_done(
    locale: LanguageKind,
    index: usize,
    total: usize,
    short_oid: &str,
    title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Commit {index}/{total}: {short_oid} {title}"),
        LanguageKind::Zh => format!("提交 {index}/{total}：{short_oid} {title}"),
    }
}

pub fn cpush_timeline_step_writing(
    locale: LanguageKind,
    index: usize,
    total: usize,
    step_title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Writing commit {index}/{total}: {step_title}"),
        LanguageKind::Zh => format!("正在写入提交 {index}/{total}：{step_title}"),
    }
}

pub fn cpush_progress_step_title(locale: LanguageKind, index: usize, total: usize) -> String {
    match locale {
        LanguageKind::En => format!("Writing local commit {index}/{total}…"),
        LanguageKind::Zh => format!("正在写入本地提交 {index}/{total}…"),
    }
}

pub fn cpush_progress_step_scope(locale: LanguageKind, branch: &str, file_count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if file_count == 1 { "file" } else { "files" };
            format!("{branch} · {file_count} {noun}")
        }
        LanguageKind::Zh => format!("{branch} · {file_count} 个文件"),
    }
}

pub fn cpush_progress_step_label(locale: LanguageKind, index: usize, total: usize) -> String {
    match locale {
        LanguageKind::En => format!("Write commit {index}/{total} · Verify scope & Git hooks"),
        LanguageKind::Zh => format!("写入提交 {index}/{total} · 核对范围与 Git 钩子"),
    }
}

pub fn cpush_timeline_step_paused(
    locale: LanguageKind,
    index: usize,
    total: usize,
    step_title: &str,
) -> String {
    match locale {
        LanguageKind::En => format!("Commit {index}/{total} paused: {step_title}"),
        LanguageKind::Zh => format!("提交 {index}/{total} 暂停：{step_title}"),
    }
}

pub fn cpush_step_paused_title(locale: LanguageKind, completed: usize, total: usize) -> String {
    let current = completed + 1;
    match locale {
        LanguageKind::En => {
            if completed > 0 {
                format!("Completed {completed}/{total} commits; commit {current} paused")
            } else {
                format!("Commit 1/{total} paused")
            }
        }
        LanguageKind::Zh => {
            if completed > 0 {
                format!("已完成 {completed}/{total} 个提交，提交 {current} 暂停")
            } else {
                format!("提交 1/{total} 暂停")
            }
        }
    }
}

pub fn cpush_step_paused_desc(locale: LanguageKind, err: &str, has_completed: bool) -> String {
    match (locale, has_completed) {
        (LanguageKind::En, true) => format!(
            "{err}. Completed local commits are safely preserved; retry remaining steps after fixing the issue without duplicating finished commits."
        ),
        (LanguageKind::En, false) => format!(
            "{err}. No new commit was created, and your draft and selection were preserved; retry remaining steps after fixing the issue."
        ),
        (LanguageKind::Zh, true) => format!(
            "{err}。已完成的本地提交已安全保留；修复后可重试剩余步骤，不会重复创建已有提交。"
        ),
        (LanguageKind::Zh, false) => format!(
            "{err}。未创建新提交，草稿与选择已保留；修复后可重试剩余步骤，不会重复创建已有提交。"
        ),
    }
}

pub fn cpush_step_scope(locale: LanguageKind, current: usize, total: usize) -> String {
    match locale {
        LanguageKind::En => format!("Commit {current}/{total}"),
        LanguageKind::Zh => format!("提交 {current}/{total}"),
    }
}

pub const fn cpush_option_retry_remaining(locale: LanguageKind) -> &'static str {
    tr(locale, "Retry remaining commits", "重试剩余提交")
}

pub const fn cpush_option_close_or_keep_completed(
    locale: LanguageKind,
    has_completed: bool,
) -> &'static str {
    match (locale, has_completed) {
        (LanguageKind::En, true) => "End session (keep completed commits)",
        (LanguageKind::En, false) => "Close session",
        (LanguageKind::Zh, true) => "结束会话（保留已完成提交）",
        (LanguageKind::Zh, false) => "关闭本次会话",
    }
}

pub const fn cpush_timeline_pushing(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Checking remote status and pushing local commits…",
        "正在核对远端状态并推送本地提交…",
    )
}

pub const fn cpush_progress_pushing_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pushing local commits to remote…",
        "正在推送本地提交至远端…",
    )
}

pub const fn cpush_progress_pushing_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Pushing only local commits on the current branch without creating duplicate commits.",
        "仅推送当前分支上的本地提交，不会重复创建提交。",
    )
}

pub const fn cpush_progress_pushing_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Check remote -> Safe sync -> Push branch",
        "核对远端 → 安全同步 → 推送分支",
    )
}

pub const fn cpush_no_remote_configured_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No Git remote is configured for this repository",
        "当前仓库尚未配置任何 Git 远端（remote）",
    )
}

pub const fn cpush_no_remote_configured_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Committed locally, no Git remote configured",
        "已在本地提交，尚未配置 Git 远端",
    )
}

pub const fn cpush_timeline_waiting_upstream_choice(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Waiting to confirm upstream remote target",
        "等待确认新推送远端目标",
    )
}

pub const fn cpush_option_keep_local_only(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Keep local commits only (do not push)",
        "仅保留本地提交（暂不推送）",
    )
}

pub const fn cpush_missing_upstream_question_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No upstream tracking branch configured — confirm push target?",
        "尚未配置跟踪远端分支，确认推送目标？",
    )
}

pub fn cpush_missing_upstream_question_desc(locale: LanguageKind, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Local commits were written to {branch}, which has no upstream yet. Confirm the remote target to track and push to:"
        ),
        LanguageKind::Zh => format!(
            "本地提交已写入 {branch}，但尚未配置 upstream。请明确确认要建立跟踪并推送的远端目标："
        ),
    }
}

pub fn cpush_branch_scope(locale: LanguageKind, branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Branch: {branch}"),
        LanguageKind::Zh => format!("分支：{branch}"),
    }
}

pub const fn cpush_rebased_push_failed_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Sync rebase completed, but push failed",
        "已完成同步变基，但推送失败",
    )
}

pub fn cpush_dirty_overlap_push_failed_reason(
    locale: LanguageKind,
    overlapping_paths: &[String],
) -> String {
    let joined = join_list(locale, overlapping_paths);
    match locale {
        LanguageKind::En => format!(
            "Remote has new commits touching uncommitted local files ({joined}); paused automatic rebase to preserve staged/unstaged drafts. Handle those files and retry push"
        ),
        LanguageKind::Zh => format!(
            "远端有新提交且涉及当前未提交文件（{joined}），为保全暂存/未暂存草稿已暂停自动变基；请处理该文件后重试推送"
        ),
    }
}

pub const fn cpush_dirty_overlap_push_failed_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Committed locally; resolve overlapping draft before sync rebase",
        "已在本地提交，同步变基需先处理重叠草稿",
    )
}

pub const fn cpush_published_history_risk_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Published history risk detected; paused automatic rebase and push",
        "检测到已发布历史风险，已暂停自动变基与推送",
    )
}

pub fn cpush_timeline_rebased_synced(locale: LanguageKind, remote_branch: &str) -> String {
    match locale {
        LanguageKind::En => format!("Safely rebased and synced onto {remote_branch}"),
        LanguageKind::Zh => format!("已安全变基并同步至 {remote_branch}"),
    }
}

pub const fn cpush_plan_incomplete_question_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Model returned an incomplete C-Push question",
        "模型返回的 C-Push 提问内容不完整",
    )
}

pub const fn cpush_plan_unconfirmed_skip_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Model cannot skip selected files without user confirmation",
        "未经确认，模型不能跳过已勾选的变更文件",
    )
}

pub const fn cpush_plan_unauthorized_skip_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push plan contains unauthorized skipped files",
        "C-Push 规划包含未经授权的跳过文件",
    )
}

pub const fn cpush_plan_duplicate_file_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push plan assigned a file more than once",
        "同一文件被重复分配到多个提交分组",
    )
}

pub const fn cpush_plan_empty_group_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push plan contains an empty file group or empty commit message",
        "C-Push 规划存在空文件组或空提交说明",
    )
}

pub const fn cpush_plan_unselected_file_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push plan attempted to commit an unselected file and was blocked by the desktop layer",
        "C-Push 规划尝试提交未勾选文件，已被桌面层拦截",
    )
}

pub const fn cpush_plan_incomplete_coverage_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "C-Push plan did not cover all selected files",
        "C-Push 规划未完整覆盖全部已勾选文件",
    )
}

pub const fn cpush_missing_upstream_short_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No upstream tracking branch configured",
        "尚未配置跟踪远端分支",
    )
}

pub const fn cpush_plan_invalid_message_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Commit message in the confirmed plan is empty or exceeds length limits",
        "预生成提交计划中的提交说明为空或超出长度限制",
    )
}

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
