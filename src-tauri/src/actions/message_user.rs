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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageQuestionOption {
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageQuestionItem {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<MessageQuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

impl MessageUserRequest {
    pub fn has_questions(&self) -> bool {
        self.questions.as_ref().is_some_and(|q| !q.is_empty())
    }

    pub fn questions_slice(&self) -> &[MessageQuestionItem] {
        self.questions.as_deref().unwrap_or(&[])
    }

    pub fn sanitize_and_validate(mut self) -> Result<Self, String> {
        self.title = self.title.and_then(|t| trim_truncate(&t, MAX_TITLE_CHARS));
        self.subtitle = self
            .subtitle
            .and_then(|s| trim_truncate(&s, MAX_SUBTITLE_CHARS));
        self.links = sanitize_links(self.links);
        if let Some(ref mut ctx) = self.context {
            ctx.links = sanitize_links(ctx.links.take());
        }
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
            if questions.len() > MAX_QUESTIONS {
                return Err(format!(
                    "message_user supports at most {MAX_QUESTIONS} questions (received {}).",
                    questions.len()
                ));
            }
            self.questions = Some(questions);
        }
        if self.title.is_none() && !self.has_questions() {
            return Err(
                "message_user requires a non-empty `title` when `questions` is omitted.".into(),
            );
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
