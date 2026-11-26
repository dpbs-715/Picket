use crate::{
    i18n::LanguageKind, repository::ai_context::ExtractedDiffFile,
    skills::RuntimeSkillOverrideCatalog,
};
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCPushQaTurn {
    pub question_id: String,
    pub question: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiConflictFile {
    pub file_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_content: Option<String>,
    pub upstream_content: String,
    pub local_content: String,
    pub conflicted_content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiCommand {
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
    ReviewScope {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        prompt: String,
        files: Vec<ExtractedDiffFile>,
        skipped_paths: Vec<String>,
        locale: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        enable_tools: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    #[serde(rename = "planCPush")]
    PlanCPush {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        recent_commits: Vec<String>,
        prompt: String,
        files: Vec<PiPlanFileEntry>,
        unselected_paths: Vec<String>,
        qa_history: Vec<PiCPushQaTurn>,
        locale: LanguageKind,
        commit_language: LanguageKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        enable_tools: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill_overrides: Option<RuntimeSkillOverrideCatalog>,
    },
    ResolveConflict {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        upstream: String,
        user_instruction: String,
        commits: Vec<String>,
        files: Vec<PiConflictFile>,
        locale: LanguageKind,
    },
    RunCustomAction {
        request_id: String,
        context: RequestContext,
        model: PiProviderSpec,
        branch: String,
        detached: bool,
        remote_branches: Vec<String>,
        action_label: String,
        action_prompt: String,
        extra_prompt: String,
        selected_files: Vec<PiPlanFileEntry>,
        unselected_paths: Vec<String>,
        local_keep_paths: Vec<String>,
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
            | Self::ReviewScope { locale, .. }
            | Self::PlanCPush { locale, .. }
            | Self::ResolveConflict { locale, .. }
            | Self::RunCustomAction { locale, .. }
            | Self::GenerateCustomAction { locale, .. } => *locale,
            Self::Probe { .. } | Self::ToolResult { .. } | Self::Cancel { .. } | Self::Shutdown => {
                LanguageKind::En
            }
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiReviewFinding {
    pub file_id: String,
    pub path: String,
    pub line: u32,
    pub severity: String,
    pub title: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiCPushCommitGroup {
    pub id: String,
    pub message: String,
    pub file_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiCPushQuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub recommended: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PiCPushPlan {
    Ready {
        summary: String,
        groups: Vec<PiCPushCommitGroup>,
        skipped_file_ids: Vec<String>,
        #[serde(default)]
        skip_push: bool,
    },
    Question {
        title: String,
        scope: String,
        description: String,
        affected_file_ids: Vec<String>,
        options: Vec<PiCPushQuestionOption>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiResolvedConflictFile {
    pub file_id: String,
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiConflictResolution {
    pub summary: String,
    pub resolved_files: Vec<PiResolvedConflictFile>,
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
    ReviewCompleted {
        request_id: String,
        context: RequestContext,
        findings: Vec<PiReviewFinding>,
    },
    #[serde(rename = "cPushPlanCompleted")]
    CPushPlanCompleted {
        request_id: String,
        context: RequestContext,
        plan: PiCPushPlan,
    },
    ConflictResolved {
        request_id: String,
        context: RequestContext,
        resolution: PiConflictResolution,
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
            | Self::ReviewCompleted {
                request_id,
                context,
                ..
            }
            | Self::CPushPlanCompleted {
                request_id,
                context,
                ..
            }
            | Self::ConflictResolved {
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
    ReviewCompleted {
        findings: Vec<PiReviewFinding>,
    },
    CPushPlanCompleted {
        plan: PiCPushPlan,
    },
    ConflictResolved {
        resolution: PiConflictResolution,
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
