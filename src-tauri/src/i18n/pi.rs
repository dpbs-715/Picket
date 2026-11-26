//! Pi Core supervisor and protocol localization.

use super::{LanguageKind, tr};

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
