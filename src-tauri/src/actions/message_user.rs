//! Structured `message_user` protocol types, validation, and card projection shared between
//! Pi Core tool calls, Rust session persistence, and the React `MessageDock`.

use super::types::CustomActionLink;
use serde::{Deserialize, Serialize};

pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SUBTITLE_CHARS: usize = 200;
pub const MAX_BLOCKS: usize = 12;
pub const MAX_MARKDOWN_CHARS: usize = 6_000;
pub const MAX_CALLOUT_TITLE_CHARS: usize = 120;
pub const MAX_CALLOUT_CONTENT_CHARS: usize = 1_200;
pub const MAX_BLOCK_HEADING_CHARS: usize = 80;
pub const MAX_FACTS_ITEMS: usize = 12;
pub const MAX_FACT_LABEL_CHARS: usize = 48;
pub const MAX_FACT_VALUE_CHARS: usize = 160;
pub const MAX_FINDINGS_ITEMS: usize = 24;
pub const MAX_FINDING_TAG_CHARS: usize = 24;
pub const MAX_FINDING_TITLE_CHARS: usize = 160;
pub const MAX_FINDING_DETAIL_CHARS: usize = 1_200;
pub const MAX_SKIPPED_ITEMS: usize = 24;
pub const MAX_SKIPPED_REASON_CHARS: usize = 160;
pub const MAX_CHECKLIST_ITEMS: usize = 20;
pub const MAX_CHECKLIST_LABEL_CHARS: usize = 140;
pub const MAX_CHECKLIST_DETAIL_CHARS: usize = 300;
pub const MAX_CODE_TITLE_CHARS: usize = 100;
pub const MAX_CODE_LANGUAGE_CHARS: usize = 32;
pub const MAX_CODE_CONTENT_CHARS: usize = 8_000;
pub const MAX_FILES_ITEMS: usize = 24;
pub const MAX_FILE_BADGE_CHARS: usize = 32;
pub const MAX_PATH_CHARS: usize = 300;
pub const MAX_LINKS: usize = 6;
pub const MAX_LINK_LABEL_CHARS: usize = 80;
pub const MAX_QUESTIONS: usize = 4;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 4;
pub const MAX_HEADER_CHARS: usize = 24;
pub const MAX_QUESTION_CHARS: usize = 600;
pub const MAX_OPTION_LABEL_CHARS: usize = 80;
pub const MAX_OPTION_DESCRIPTION_CHARS: usize = 240;
pub const MAX_PREVIEW_CHARS: usize = 2_000;
pub const MAX_CONTEXT_SCOPE_CHARS: usize = 120;
pub const MAX_DONT_ASK_LABEL_CHARS: usize = 100;

const RESERVED_OPTION_LABELS: &[&str] = &[
    "other",
    "other...",
    "type something",
    "type something.",
    "next",
    "submit",
    "cancel",
    "\u{5176}\u{4ed6}",
    "\u{5176}\u{5b83}",
    "\u{81ea}\u{5b9a}\u{4e49}",
    "\u{4e0b}\u{4e00}\u{6b65}",
    "\u{63d0}\u{4ea4}",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum MessageTone {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum MessageCalloutTone {
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum MessageFactTone {
    Default,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum MessageFindingSeverity {
    High,
    Medium,
    Low,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum MessageChecklistStatus {
    Pass,
    Fail,
    Warn,
    Skip,
    Running,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageFactItem {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<MessageFactTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageFindingItem {
    pub severity: MessageFindingSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub title: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MessageSkippedFileItem {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageChecklistItem {
    pub status: MessageChecklistStatus,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageFileChipItem {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MessageBlock {
    Markdown {
        content: String,
    },
    Callout {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<MessageCalloutTone>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        content: String,
    },
    Facts {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
        items: Vec<MessageFactItem>,
    },
    Findings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
        items: Vec<MessageFindingItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        empty_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skipped: Option<Vec<MessageSkippedFileItem>>,
    },
    Checklist {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
        items: Vec<MessageChecklistItem>,
    },
    Code {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        collapsed: Option<bool>,
    },
    Files {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
        items: Vec<MessageFileChipItem>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageUserCard {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<MessageTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<MessageBlock>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<CustomActionLink>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageQuestionOption {
    #[serde(default)]
    pub id: String,
    pub label: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageQuestionItem {
    #[serde(default)]
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<MessageQuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageQuestionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_file_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<CustomActionLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_dont_ask_again: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dont_ask_again_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MessageUserRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<MessageTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<MessageBlock>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<CustomActionLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<MessageQuestionItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<MessageQuestionContext>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageUserAnswer {
    pub question_index: usize,
    pub question_id: String,
    pub header: String,
    #[serde(default)]
    pub selected_option_ids: Vec<String>,
    #[serde(default)]
    pub selected_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageUserResponse {
    pub cancelled: bool,
    #[serde(default)]
    pub answers: Vec<MessageUserAnswer>,
    #[serde(default)]
    pub unanswered: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dont_ask_again: Option<bool>,
}

fn trim_truncate(input: &str, max_chars: usize) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(max_chars).collect())
}

fn sanitize_repo_path(raw: &str) -> Result<String, String> {
    let mut normalized = raw.trim().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_owned();
    }
    if normalized.is_empty()
        || normalized.chars().count() > MAX_PATH_CHARS
        || normalized.starts_with('/')
        || normalized.contains('\0')
    {
        return Err(format!("Invalid repository-relative path `{raw}`."));
    }
    let mut clean_segments = Vec::new();
    for seg in normalized.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.eq_ignore_ascii_case(".git") {
            return Err(format!(
                "Path `{raw}` must stay inside the repository worktree and cannot reference `.git` or `..`."
            ));
        }
        clean_segments.push(seg);
    }
    Ok(clean_segments.join("/"))
}

fn sanitize_links(links: Option<Vec<CustomActionLink>>) -> Option<Vec<CustomActionLink>> {
    let list = links?;
    let mut out: Vec<CustomActionLink> = Vec::new();
    for link in list {
        if out.len() >= MAX_LINKS {
            break;
        }
        let Some(label) = trim_truncate(&link.label, MAX_LINK_LABEL_CHARS) else {
            continue;
        };
        let url = link.url.trim().to_owned();
        let lower = url.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            continue;
        }
        if !out.iter().any(|existing| existing.url == url) {
            out.push(CustomActionLink { label, url });
        }
    }
    (!out.is_empty()).then_some(out)
}

fn sanitize_block(block: MessageBlock, index: usize) -> Result<MessageBlock, String> {
    match block {
        MessageBlock::Markdown { content } => {
            let content = trim_truncate(&content, MAX_MARKDOWN_CHARS)
                .ok_or_else(|| format!("blocks[{index}] (markdown) requires non-empty content."))?;
            Ok(MessageBlock::Markdown { content })
        }
        MessageBlock::Callout {
            tone,
            title,
            content,
        } => {
            let content = trim_truncate(&content, MAX_CALLOUT_CONTENT_CHARS)
                .ok_or_else(|| format!("blocks[{index}] (callout) requires non-empty content."))?;
            let title = title.and_then(|t| trim_truncate(&t, MAX_CALLOUT_TITLE_CHARS));
            Ok(MessageBlock::Callout {
                tone,
                title,
                content,
            })
        }
        MessageBlock::Facts { heading, items } => {
            if items.is_empty() || items.len() > MAX_FACTS_ITEMS {
                return Err(format!(
                    "blocks[{index}] (facts) must contain 1 to {MAX_FACTS_ITEMS} items."
                ));
            }
            let mut clean_items = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                let label = trim_truncate(&item.label, MAX_FACT_LABEL_CHARS).ok_or_else(|| {
                    format!("blocks[{index}].items[{i}] requires a non-empty label.")
                })?;
                let value = trim_truncate(&item.value, MAX_FACT_VALUE_CHARS).ok_or_else(|| {
                    format!("blocks[{index}].items[{i}] requires a non-empty value.")
                })?;
                clean_items.push(MessageFactItem {
                    label,
                    value,
                    tone: item.tone,
                    mono: item.mono.filter(|&m| m),
                });
            }
            Ok(MessageBlock::Facts {
                heading: heading.and_then(|h| trim_truncate(&h, MAX_BLOCK_HEADING_CHARS)),
                items: clean_items,
            })
        }
        MessageBlock::Findings {
            heading,
            items,
            empty_text,
            skipped,
        } => {
            if items.len() > MAX_FINDINGS_ITEMS {
                return Err(format!(
                    "blocks[{index}] (findings) accepts at most {MAX_FINDINGS_ITEMS} items."
                ));
            }
            let mut clean_items = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                let title =
                    trim_truncate(&item.title, MAX_FINDING_TITLE_CHARS).ok_or_else(|| {
                        format!("blocks[{index}].items[{i}] requires a non-empty title.")
                    })?;
                let detail =
                    trim_truncate(&item.detail, MAX_FINDING_DETAIL_CHARS).ok_or_else(|| {
                        format!("blocks[{index}].items[{i}] requires a non-empty detail.")
                    })?;
                let path = match item.path {
                    Some(raw) if !raw.trim().is_empty() => Some(sanitize_repo_path(&raw)?),
                    _ => None,
                };
                if item.line == Some(0) {
                    return Err(format!(
                        "blocks[{index}].items[{i}].line must be a positive integer."
                    ));
                }
                clean_items.push(MessageFindingItem {
                    severity: item.severity,
                    tag: item
                        .tag
                        .and_then(|t| trim_truncate(&t, MAX_FINDING_TAG_CHARS)),
                    title,
                    detail,
                    line: if path.is_some() { item.line } else { None },
                    path,
                });
            }
            let empty_text = empty_text.and_then(|t| trim_truncate(&t, MAX_FINDING_DETAIL_CHARS));
            if clean_items.is_empty() && empty_text.is_none() {
                return Err(format!(
                    "blocks[{index}] (findings) must provide at least one item or non-empty emptyText."
                ));
            }
            let clean_skipped = skipped.and_then(|list| {
                let items: Vec<MessageSkippedFileItem> = list
                    .into_iter()
                    .take(MAX_SKIPPED_ITEMS)
                    .filter_map(|entry| {
                        let path = sanitize_repo_path(&entry.path).ok()?;
                        let reason = trim_truncate(&entry.reason, MAX_SKIPPED_REASON_CHARS)?;
                        Some(MessageSkippedFileItem { path, reason })
                    })
                    .collect();
                (!items.is_empty()).then_some(items)
            });
            Ok(MessageBlock::Findings {
                heading: heading.and_then(|h| trim_truncate(&h, MAX_BLOCK_HEADING_CHARS)),
                items: clean_items,
                empty_text,
                skipped: clean_skipped,
            })
        }
        MessageBlock::Checklist { heading, items } => {
            if items.is_empty() || items.len() > MAX_CHECKLIST_ITEMS {
                return Err(format!(
                    "blocks[{index}] (checklist) must contain 1 to {MAX_CHECKLIST_ITEMS} items."
                ));
            }
            let mut clean_items = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                let label =
                    trim_truncate(&item.label, MAX_CHECKLIST_LABEL_CHARS).ok_or_else(|| {
                        format!("blocks[{index}].items[{i}] requires a non-empty label.")
                    })?;
                clean_items.push(MessageChecklistItem {
                    status: item.status,
                    label,
                    detail: item
                        .detail
                        .and_then(|d| trim_truncate(&d, MAX_CHECKLIST_DETAIL_CHARS)),
                });
            }
            Ok(MessageBlock::Checklist {
                heading: heading.and_then(|h| trim_truncate(&h, MAX_BLOCK_HEADING_CHARS)),
                items: clean_items,
            })
        }
        MessageBlock::Code {
            title,
            language,
            content,
            collapsed,
        } => {
            let normalized = content.replace("\r\n", "\n");
            let trimmed_end = normalized.trim_end();
            if trimmed_end.trim().is_empty() {
                return Err(format!(
                    "blocks[{index}] (code) requires non-empty content."
                ));
            }
            let truncated: String = trimmed_end.chars().take(MAX_CODE_CONTENT_CHARS).collect();
            Ok(MessageBlock::Code {
                title: title.and_then(|t| trim_truncate(&t, MAX_CODE_TITLE_CHARS)),
                language: language.and_then(|l| trim_truncate(&l, MAX_CODE_LANGUAGE_CHARS)),
                content: truncated,
                collapsed,
            })
        }
        MessageBlock::Files { heading, items } => {
            if items.is_empty() || items.len() > MAX_FILES_ITEMS {
                return Err(format!(
                    "blocks[{index}] (files) must contain 1 to {MAX_FILES_ITEMS} items."
                ));
            }
            let mut clean_items = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                let path = sanitize_repo_path(&item.path)?;
                if item.line == Some(0) {
                    return Err(format!(
                        "blocks[{index}].items[{i}].line must be a positive integer."
                    ));
                }
                clean_items.push(MessageFileChipItem {
                    path,
                    line: item.line,
                    badge: item
                        .badge
                        .and_then(|b| trim_truncate(&b, MAX_FILE_BADGE_CHARS)),
                });
            }
            Ok(MessageBlock::Files {
                heading: heading.and_then(|h| trim_truncate(&h, MAX_BLOCK_HEADING_CHARS)),
                items: clean_items,
            })
        }
    }
}

fn is_reserved_option_label(label: &str) -> bool {
    let normalized = label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    RESERVED_OPTION_LABELS.contains(&normalized.as_str())
}

fn sanitize_questions(
    raw_questions: Vec<MessageQuestionItem>,
) -> Result<Vec<MessageQuestionItem>, String> {
    if raw_questions.len() > MAX_QUESTIONS {
        return Err(format!(
            "message_user supports at most {MAX_QUESTIONS} questions (received {}).",
            raw_questions.len()
        ));
    }
    let mut clean_questions = Vec::with_capacity(raw_questions.len());
    let mut seen_question_ids = std::collections::HashSet::new();

    for (q_index, item) in raw_questions.into_iter().enumerate() {
        let header = item.header.trim().to_owned();
        if header.is_empty() || header.chars().count() > MAX_HEADER_CHARS {
            return Err(format!(
                "questions[{q_index}].header must be a non-empty short label (max {MAX_HEADER_CHARS} chars)."
            ));
        }
        let question = item.question.trim().to_owned();
        if question.is_empty() || question.chars().count() > MAX_QUESTION_CHARS {
            return Err(format!(
                "questions[{q_index}].question must be non-empty (max {MAX_QUESTION_CHARS} chars)."
            ));
        }
        let multi_select = item.multi_select == Some(true);
        if item.options.len() < MIN_OPTIONS || item.options.len() > MAX_OPTIONS {
            return Err(format!(
                "questions[{q_index}].options must contain {MIN_OPTIONS} to {MAX_OPTIONS} options."
            ));
        }
        let q_id = trim_truncate(&item.id, 64).unwrap_or_else(|| format!("q-{}", q_index + 1));
        if !seen_question_ids.insert(q_id.clone()) {
            return Err(format!("Duplicate question id \"{q_id}\"."));
        }

        let mut clean_options = Vec::with_capacity(item.options.len());
        let mut seen_opt_ids = std::collections::HashSet::new();
        let mut seen_opt_labels = std::collections::HashSet::new();
        for (o_index, opt) in item.options.into_iter().enumerate() {
            let label = opt.label.trim().to_owned();
            if label.is_empty() || label.chars().count() > MAX_OPTION_LABEL_CHARS {
                return Err(format!(
                    "questions[{q_index}].options[{o_index}].label must be non-empty (max {MAX_OPTION_LABEL_CHARS} chars)."
                ));
            }
            if is_reserved_option_label(&label) {
                return Err(format!(
                    "Option label \"{label}\" is reserved because the UI always provides its own free-text reply and submit controls. Omit it and list only concrete choices."
                ));
            }
            let normalized_label = label.to_lowercase();
            if !seen_opt_labels.insert(normalized_label) {
                return Err(format!(
                    "Duplicate option label \"{label}\" in questions[{q_index}]."
                ));
            }
            let description = opt.description.trim().to_owned();
            if description.is_empty() || description.chars().count() > MAX_OPTION_DESCRIPTION_CHARS
            {
                return Err(format!(
                    "questions[{q_index}].options[{o_index}].description must explain the option concisely (max {MAX_OPTION_DESCRIPTION_CHARS} chars)."
                ));
            }
            let preview = opt
                .preview
                .as_deref()
                .and_then(|p| trim_truncate(p, MAX_PREVIEW_CHARS));
            if preview.is_some() && multi_select {
                return Err(format!(
                    "questions[{q_index}] cannot combine option `preview` with `multiSelect: true`."
                ));
            }
            let opt_id =
                trim_truncate(&opt.id, 64).unwrap_or_else(|| format!("{q_id}-opt-{}", o_index + 1));
            if !seen_opt_ids.insert(opt_id.clone()) {
                return Err(format!(
                    "Duplicate option id \"{opt_id}\" in questions[{q_index}]."
                ));
            }
            clean_options.push(MessageQuestionOption {
                id: opt_id,
                label,
                description,
                recommended: opt.recommended,
                preview,
            });
        }

        clean_questions.push(MessageQuestionItem {
            id: q_id,
            header,
            question,
            options: clean_options,
            multi_select: multi_select.then_some(true),
        });
    }

    Ok(clean_questions)
}

fn sanitize_context(ctx: MessageQuestionContext) -> Option<MessageQuestionContext> {
    let scope = ctx
        .scope
        .as_deref()
        .and_then(|s| trim_truncate(s, MAX_CONTEXT_SCOPE_CHARS));
    let affected_file_ids = ctx.affected_file_ids.and_then(|ids| {
        let mut seen = std::collections::HashSet::new();
        let clean: Vec<String> = ids
            .into_iter()
            .filter_map(|id| {
                let trimmed = id.trim();
                (!trimmed.is_empty() && seen.insert(trimmed.to_owned())).then(|| trimmed.to_owned())
            })
            .collect();
        (!clean.is_empty()).then_some(clean)
    });
    let links = sanitize_links(ctx.links);
    let allow_dont_ask_again = ctx.allow_dont_ask_again.filter(|&allow| allow);
    let dont_ask_again_label = ctx
        .dont_ask_again_label
        .as_deref()
        .and_then(|l| trim_truncate(l, MAX_DONT_ASK_LABEL_CHARS));
    if scope.is_some()
        || affected_file_ids.is_some()
        || links.is_some()
        || allow_dont_ask_again.is_some()
    {
        Some(MessageQuestionContext {
            scope,
            affected_file_ids,
            links,
            allow_dont_ask_again,
            dont_ask_again_label,
        })
    } else {
        None
    }
}

impl MessageUserRequest {
    pub fn has_questions(&self) -> bool {
        self.questions.as_ref().is_some_and(|q| !q.is_empty())
    }

    pub fn questions_slice(&self) -> &[MessageQuestionItem] {
        self.questions.as_deref().unwrap_or(&[])
    }

    /// Authoritative entry point for raw `message_user` JSON payloads from Pi Core tool calls.
    pub fn parse_and_validate(raw: serde_json::Value) -> Result<Self, String> {
        if !raw.is_object() {
            return Err(
                "Invalid message_user arguments: expected an object with `title` and optional `blocks` and/or `questions`.".into(),
            );
        }
        let parsed: Self = serde_json::from_value(raw)
            .map_err(|err| format!("Invalid message_user arguments: {err}"))?;
        parsed.sanitize_and_validate()
    }

    pub fn sanitize_and_validate(mut self) -> Result<Self, String> {
        self.title = self.title.and_then(|t| trim_truncate(&t, MAX_TITLE_CHARS));
        self.subtitle = self
            .subtitle
            .and_then(|s| trim_truncate(&s, MAX_SUBTITLE_CHARS));
        self.links = sanitize_links(self.links);
        self.context = self.context.take().and_then(sanitize_context);
        if let Some(blocks) = self.blocks.take() {
            if blocks.len() > MAX_BLOCKS {
                return Err(format!(
                    "message_user supports at most {MAX_BLOCKS} blocks (received {}).",
                    blocks.len()
                ));
            }
            if !blocks.is_empty() {
                let mut clean = Vec::with_capacity(blocks.len());
                for (index, block) in blocks.into_iter().enumerate() {
                    clean.push(sanitize_block(block, index)?);
                }
                self.blocks = Some(clean);
            }
        }
        if let Some(questions) = self.questions.take()
            && !questions.is_empty()
        {
            self.questions = Some(sanitize_questions(questions)?);
        }
        if self.title.is_none() && !self.has_questions() {
            return Err(
                "message_user requires a non-empty `title` when `questions` is omitted.".into(),
            );
        }
        if self.title.is_none() {
            self.title = self.questions_slice().first().map(|q| q.header.clone());
        }
        Ok(self)
    }

    pub fn merged_links(&self) -> Vec<CustomActionLink> {
        let mut merged: Vec<CustomActionLink> = Vec::new();
        for list in [
            self.links.as_deref(),
            self.context.as_ref().and_then(|c| c.links.as_deref()),
        ]
        .into_iter()
        .flatten()
        {
            for link in list {
                if merged.len() >= MAX_LINKS {
                    break;
                }
                if !merged.iter().any(|existing| existing.url == link.url) {
                    merged.push(link.clone());
                }
            }
        }
        merged
    }

    pub fn to_card(&self, fallback_title: &str) -> MessageUserCard {
        let title = self
            .title
            .clone()
            .or_else(|| self.questions_slice().first().map(|q| q.header.clone()))
            .unwrap_or_else(|| fallback_title.to_owned());
        let merged_links = self.merged_links();
        MessageUserCard {
            title,
            subtitle: self.subtitle.clone(),
            tone: self.tone,
            blocks: self.blocks.clone(),
            links: (!merged_links.is_empty()).then_some(merged_links),
        }
    }

    pub fn card_if_present(&self, fallback_title: &str) -> Option<MessageUserCard> {
        let has_card_content = self.title.is_some()
            || self.subtitle.is_some()
            || self.tone.is_some()
            || self.blocks.as_ref().is_some_and(|b| !b.is_empty())
            || self.links.as_ref().is_some_and(|l| !l.is_empty());
        has_card_content.then(|| self.to_card(fallback_title))
    }
}

/// Formats a compact plain-text digest of a persisted `MessageUserCard` for follow-up runs
/// so Pi Core knows the exact findings, checks, or facts it previously presented.
pub fn format_card_for_checkpoint(card: &MessageUserCard) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("Last published card title: {}", card.title));
    if let Some(ref subtitle) = card.subtitle {
        lines.push(format!("Last published card subtitle: {subtitle}"));
    }
    if let Some(ref blocks) = card.blocks {
        for block in blocks {
            match block {
                MessageBlock::Findings { items, .. } => {
                    for (idx, item) in items.iter().take(12).enumerate() {
                        let loc = match (&item.path, item.line) {
                            (Some(path), Some(line)) => format!(" ({path}:{line})"),
                            (Some(path), None) => format!(" ({path})"),
                            _ => String::new(),
                        };
                        lines.push(format!(
                            "  - Finding #{} [{:?}]: {}{} — {}",
                            idx + 1,
                            item.severity,
                            item.title,
                            loc,
                            item.detail
                        ));
                    }
                }
                MessageBlock::Checklist { items, .. } => {
                    for item in items.iter().take(12) {
                        let detail = item
                            .detail
                            .as_deref()
                            .map(|d| format!(" — {d}"))
                            .unwrap_or_default();
                        lines.push(format!(
                            "  - Check [{:?}]: {}{}",
                            item.status, item.label, detail
                        ));
                    }
                }
                MessageBlock::Facts { items, .. } => {
                    let pairs: Vec<String> = items
                        .iter()
                        .take(8)
                        .map(|f| format!("{}={}", f.label, f.value))
                        .collect();
                    if !pairs.is_empty() {
                        lines.push(format!("  - Facts: {}", pairs.join(", ")));
                    }
                }
                MessageBlock::Callout { title, content, .. } => {
                    if let Some(t) = title {
                        lines.push(format!("  - Callout ({t}): {content}"));
                    } else {
                        lines.push(format!("  - Callout: {content}"));
                    }
                }
                MessageBlock::Files { items, .. } => {
                    let paths: Vec<String> =
                        items.iter().take(12).map(|f| f.path.clone()).collect();
                    if !paths.is_empty() {
                        lines.push(format!("  - Referenced files: {}", paths.join(", ")));
                    }
                }
                MessageBlock::Markdown { content } => {
                    let excerpt: String = content.chars().take(240).collect();
                    lines.push(format!("  - Note: {excerpt}"));
                }
                MessageBlock::Code { title, .. } => {
                    if let Some(t) = title {
                        lines.push(format!("  - Code snippet shown: {t}"));
                    }
                }
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_question_constraints_reserved_labels_preview_and_defaults() {
        // Rejects completely empty message_user calls:
        let err = MessageUserRequest::parse_and_validate(json!({})).unwrap_err();
        assert!(err.contains("title"));

        // Rejects reserved labels (EN & ZH):
        for reserved in [
            "Other",
            "Type something.",
            "\u{5176}\u{4ed6}",
            "\u{81ea}\u{5b9a}\u{4e49}",
        ] {
            let err = MessageUserRequest::parse_and_validate(json!({
                "questions": [{
                    "header": "Scope",
                    "question": "How should we group these changes?",
                    "options": [
                        { "label": "Single PR", "description": "Combine into one PR" },
                        { "label": reserved, "description": "Custom choice" }
                    ]
                }]
            }))
            .unwrap_err();
            assert!(
                err.contains("reserved"),
                "expected reserved error for {reserved}, got: {err}"
            );
        }

        // Rejects preview combined with multiSelect:
        let err = MessageUserRequest::parse_and_validate(json!({
            "questions": [{
                "header": "Labels",
                "question": "Pick labels",
                "multiSelect": true,
                "options": [
                    { "label": "bug", "description": "Bug fix", "preview": "preview text" },
                    { "label": "feat", "description": "Feature" }
                ]
            }]
        }))
        .unwrap_err();
        assert!(err.contains("preview") && err.contains("multiSelect"));

        // Rejects duplicate option labels (case-insensitive) and too few options:
        assert!(
            MessageUserRequest::parse_and_validate(json!({
                "questions": [{
                    "header": "Mode",
                    "question": "Pick one",
                    "options": [
                        { "label": "Split", "description": "Two commits" },
                        { "label": "split", "description": "Duplicate label" }
                    ]
                }]
            }))
            .is_err()
        );
        assert!(
            MessageUserRequest::parse_and_validate(json!({
                "questions": [{
                    "header": "Mode",
                    "question": "Only one option",
                    "options": [{ "label": "Only", "description": "Not enough" }]
                }]
            }))
            .is_err()
        );

        // Synthesizes missing question/option IDs and default title from first question header:
        let valid = MessageUserRequest::parse_and_validate(json!({
            "questions": [{
                "header": "Grouping",
                "question": "Group payment and docs separately?",
                "options": [
                    { "label": "Split into 2 PRs", "description": "Cleaner review", "recommended": true, "preview": "PR 1: pay\nPR 2: docs" },
                    { "label": "Single PR", "description": "Ship together" }
                ]
            }],
            "context": {
                "scope": "2 feature areas",
                "allowDontAskAgain": true
            }
        }))
        .unwrap();
        assert_eq!(valid.title.as_deref(), Some("Grouping"));
        assert!(valid.has_questions());
        let q = &valid.questions_slice()[0];
        assert_eq!(q.id, "q-1");
        assert_eq!(q.options[0].id, "q-1-opt-1");
        assert_eq!(q.options[1].id, "q-1-opt-2");
    }

    #[test]
    fn validates_all_seven_blocks_and_blocks_path_escapes_and_unsafe_links() {
        // Rejects ../ escape in findings and .git in files:
        assert!(
            MessageUserRequest::parse_and_validate(json!({
                "title": "Unsafe finding path",
                "blocks": [{
                    "type": "findings",
                    "items": [{
                        "severity": "low",
                        "title": "Escape attempt",
                        "detail": "Should reject ../ path.",
                        "path": "../outside/secret.env",
                        "line": 1
                    }]
                }]
            }))
            .is_err()
        );
        assert!(
            MessageUserRequest::parse_and_validate(json!({
                "title": "Unsafe file chip path",
                "blocks": [{
                    "type": "files",
                    "items": [{ "path": ".git/config", "badge": "blocked" }]
                }]
            }))
            .is_err()
        );

        // Validates all 7 block kinds and filters unsafe javascript: links:
        let req = MessageUserRequest::parse_and_validate(json!({
            "tone": "warning",
            "title": "Code review finished",
            "subtitle": "2 findings across 3 files",
            "blocks": [
                { "type": "markdown", "content": "Review completed on `feat/pay`." },
                { "type": "callout", "tone": "warning", "title": "Action suggested", "content": "Address P1 before merging." },
                { "type": "facts", "items": [{ "label": "Scope", "value": "3 files", "mono": true }] },
                {
                    "type": "findings",
                    "items": [
                        { "severity": "high", "tag": "P1", "title": "Plaintext secret logged", "detail": "Redact payload.", "path": "src/api/pay.ts", "line": 46 },
                        { "severity": "low", "title": "General note", "detail": "No file path." }
                    ],
                    "skipped": [{ "path": "pnpm-lock.yaml", "reason": "Lockfile" }]
                },
                { "type": "checklist", "items": [{ "status": "pass", "label": "TypeScript check" }] },
                { "type": "code", "title": "tsc output", "content": "No errors", "collapsed": false },
                { "type": "files", "items": [{ "path": "src/api/pay.ts", "line": 46, "badge": "M" }] }
            ],
            "context": {
                "links": [
                    { "label": "PR #42", "url": "https://github.com/acme/demo/pull/42" },
                    { "label": "XSS", "url": "javascript:alert(1)" }
                ]
            }
        }))
        .unwrap();
        assert!(!req.has_questions());
        let card = req.to_card("Fallback");
        assert_eq!(card.blocks.as_ref().map(Vec::len), Some(7));
        assert_eq!(
            card.links.as_deref(),
            Some(
                &[CustomActionLink {
                    label: "PR #42".into(),
                    url: "https://github.com/acme/demo/pull/42".into(),
                }][..]
            )
        );
    }
}
