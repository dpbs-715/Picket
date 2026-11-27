use crate::{i18n::LanguageKind, skills::RuntimeSkillOverrideCatalog};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestContext {
    pub repository_id: String,
    pub worktree_id: String,
    pub session_id: String,
    pub scope_version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderSpec {
    pub provider: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<&'static str>,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiPlanFileEntry {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: String,
    pub range_kind: String,
    pub added: u64,
    pub removed: u64,
    pub ignored_for_review: bool,
    pub binary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomActionRunMode {
    Cpush,
    ResolveConflict,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiConflictFileSummary {
    pub file_id: String,
    pub path: String,
    pub line_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_base: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiCommand {
    #[cfg(any(test, feature = "smoke-harness"))]
    Probe {
        request_id: String,
        context: RequestContext,
    },
    TestConnection {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        locale: LanguageKind,
    },
    ListModels {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        locale: LanguageKind,
    },
    RunCustomAction {
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<CustomActionRunMode>,
        request_id: String,
        context: Box<RequestContext>,
        model: Box<PiProviderSpec>,
        branch: String,
        detached: bool,
        remote_branches: Vec<String>,
        action_label: String,
        action_prompt: String,
        extra_prompt: String,
        selected_files: Vec<PiPlanFileEntry>,
        unselected_paths: Vec<String>,
        local_keep_paths: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recent_commits: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        push_authorized: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        upstream: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        commits: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        conflict_files: Option<Vec<PiConflictFileSummary>>,
        locale: LanguageKind,
        commit_language: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_checkpoint: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        follow_up_prompt: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    GenerateCustomAction {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        existing_labels: Vec<String>,
        user_request: String,
        locale: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    ToolResult {
        request_id: String,
        call_id: String,
        ok: bool,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    Cancel {
        request_id: String,
    },
    Shutdown,
}

impl PiCommand {
    pub const fn locale(&self) -> LanguageKind {
        match self {
            Self::TestConnection { locale, .. }
            | Self::ListModels { locale, .. }
            | Self::RunCustomAction { locale, .. }
            | Self::GenerateCustomAction { locale, .. } => *locale,
            Self::ToolResult { .. } | Self::Cancel { .. } | Self::Shutdown => LanguageKind::En,
            #[cfg(any(test, feature = "smoke-harness"))]
            Self::Probe { .. } => LanguageKind::En,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PiToolExecutionResult {
    pub ok: bool,
    pub content: String,
    pub details: Option<Value>,
    pub is_error: bool,
}

impl PiToolExecutionResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            ok: true,
            content: content.into(),
            details: None,
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            ok: false,
            content: content.into(),
            details: None,
            is_error: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiRemoteModelOption {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiActionLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiCustomActionDraft {
    pub label: String,
    pub prompt: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiEvent {
    Ready {
        protocol_version: u32,
        pi_core_version: String,
        node_version: String,
        pid: u32,
    },
    Started {
        request_id: String,
        context: RequestContext,
    },
    Activity {
        request_id: String,
        context: RequestContext,
    },
    TextDelta {
        request_id: String,
        context: RequestContext,
        delta: String,
    },
    ToolCall {
        request_id: String,
        context: RequestContext,
        call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolProgress {
        request_id: String,
        context: RequestContext,
        call_id: String,
        tool_name: String,
        status: String,
    },
    Completed {
        request_id: String,
        context: RequestContext,
        text: String,
    },
    ModelsListed {
        request_id: String,
        context: RequestContext,
        models: Vec<PiRemoteModelOption>,
    },
    CustomActionCompleted {
        request_id: String,
        context: RequestContext,
        summary: String,
        #[serde(default)]
        links: Vec<PiActionLink>,
    },
    CustomActionDrafted {
        request_id: String,
        context: RequestContext,
        draft: PiCustomActionDraft,
    },
    Cancelled {
        request_id: String,
        context: RequestContext,
    },
    Failed {
        request_id: String,
        context: RequestContext,
        message: String,
    },
    ProtocolError {
        message: String,
    },
}

impl PiEvent {
    pub(super) fn envelope(&self) -> Option<(&str, &RequestContext)> {
        match self {
            Self::Started {
                request_id,
                context,
            }
            | Self::Activity {
                request_id,
                context,
            }
            | Self::TextDelta {
                request_id,
                context,
                ..
            }
            | Self::ToolCall {
                request_id,
                context,
                ..
            }
            | Self::ToolProgress {
                request_id,
                context,
                ..
            }
            | Self::Completed {
                request_id,
                context,
                ..
            }
            | Self::ModelsListed {
                request_id,
                context,
                ..
            }
            | Self::CustomActionCompleted {
                request_id,
                context,
                ..
            }
            | Self::CustomActionDrafted {
                request_id,
                context,
                ..
            }
            | Self::Cancelled {
                request_id,
                context,
            }
            | Self::Failed {
                request_id,
                context,
                ..
            } => Some((request_id, context)),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum PiTaskOutcome {
    Completed {
        text: String,
    },
    ModelsListed {
        models: Vec<PiRemoteModelOption>,
    },
    CustomActionCompleted {
        summary: String,
        links: Vec<PiActionLink>,
    },
    CustomActionDrafted {
        draft: PiCustomActionDraft,
    },
    Cancelled,
}
