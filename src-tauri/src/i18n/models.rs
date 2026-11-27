//! Model catalog, provider validation, credential reveal, and review localization.

use super::{LanguageKind, tr};
use crate::models::types::ProviderKind;

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

pub fn model_auth_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!(
                "Authentication failed (401/403). Verify your API key and permissions ({detail})"
            )
        }
        LanguageKind::Zh => format!("鉴权失败，请检查 API Key 或服务权限（{detail}）"),
    }
}

pub fn model_endpoint_not_found_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!(
                "Endpoint or model not found (404). Verify the model ID and Base URL ({detail})"
            )
        }
        LanguageKind::Zh => format!("未找到对应接口或模型，请检查模型 ID 与接口地址（{detail}）"),
    }
}

pub fn model_rate_limited_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Rate limit or quota exceeded (402/429). Check your account quota or retry shortly ({detail})"
        ),
        LanguageKind::Zh => format!("请求过于频繁或账户额度不足，请稍后重试（{detail}）"),
    }
}

pub fn model_service_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Model provider service error (5xx). Please retry shortly ({detail})")
        }
        LanguageKind::Zh => format!("模型服务异常（5xx），请稍后重试（{detail}）"),
    }
}

pub fn model_connection_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "Unable to connect to model service. Check your network or local server ({detail})"
        ),
        LanguageKind::Zh => format!("无法连接到模型服务，请确认网络或本地服务已启动（{detail}）"),
    }
}

pub fn model_request_timed_out_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Model request timed out. Please retry shortly ({detail})"),
        LanguageKind::Zh => format!("连接模型服务超时，请稍后重试（{detail}）"),
    }
}

pub fn model_request_failed_error(locale: LanguageKind, detail: &str) -> String {
    match locale {
        LanguageKind::En => format!("Model request failed: {detail}"),
        LanguageKind::Zh => format!("模型请求未完成：{detail}"),
    }
}
