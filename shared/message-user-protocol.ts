export const MESSAGE_USER_LIMITS = {
  maxTitleChars: 120,
  maxSubtitleChars: 200,
  maxBlocks: 12,
  maxMarkdownChars: 6000,
  maxCalloutTitleChars: 120,
  maxCalloutContentChars: 1200,
  maxBlockHeadingChars: 80,
  maxFactsItems: 12,
  maxFactLabelChars: 48,
  maxFactValueChars: 160,
  maxFindingsItems: 24,
  maxFindingTagChars: 24,
  maxFindingTitleChars: 160,
  maxFindingDetailChars: 1200,
  maxSkippedItems: 24,
  maxSkippedReasonChars: 160,
  maxChecklistItems: 20,
  maxChecklistLabelChars: 140,
  maxChecklistDetailChars: 300,
  maxCodeTitleChars: 100,
  maxCodeLanguageChars: 32,
  maxCodeContentChars: 8000,
  maxFilesItems: 24,
  maxFileBadgeChars: 32,
  maxPathChars: 300,
  maxLinks: 6,
  maxLinkLabelChars: 80,
  minQuestions: 1,
  maxQuestions: 4,
  minOptions: 2,
  maxOptions: 4,
  maxHeaderChars: 24,
  maxQuestionChars: 600,
  maxOptionLabelChars: 80,
  maxOptionDescriptionChars: 240,
  maxPreviewChars: 2000,
} as const;

const RESERVED_OPTION_LABELS = new Set([
  'other',
  'other...',
  'type something',
  'type something.',
  'next',
  'submit',
  'cancel',
  '\u5176\u4ed6',
  '\u5176\u5b83',
  '\u81ea\u5b9a\u4e49',
  '\u4e0b\u4e00\u6b65',
  '\u63d0\u4ea4',
]);

import type {
  MessageBlock,
  MessageCalloutTone,
  MessageChecklistItem,
  MessageChecklistStatus,
  MessageFactItem,
  MessageFactTone,
  MessageFileChipItem,
  MessageFindingItem,
  MessageFindingSeverity,
  MessageQuestionContext,
  MessageQuestionItem,
  MessageQuestionOption,
  MessageSkippedFileItem,
  MessageTone,
  MessageUserAnswer,
  MessageUserCard,
  MessageUserRequest,
  MessageUserResponse,
  WorkflowLink as MessageLink,
} from './generated';

export type {
  MessageBlock,
  MessageCalloutTone,
  MessageChecklistItem,
  MessageChecklistStatus,
  MessageFactItem,
  MessageFactTone,
  MessageFileChipItem,
  MessageFindingItem,
  MessageFindingSeverity,
  MessageLink,
  MessageQuestionContext,
  MessageQuestionItem,
  MessageQuestionOption,
  MessageSkippedFileItem,
  MessageTone,
  MessageUserAnswer,
  MessageUserCard,
  MessageUserRequest,
  MessageUserResponse,
};

export type MessageMarkdownBlock = Extract<MessageBlock, { type: 'markdown' }>;
export type MessageCalloutBlock = Extract<MessageBlock, { type: 'callout' }>;
export type MessageFactsBlock = Extract<MessageBlock, { type: 'facts' }>;
export type MessageFindingsBlock = Extract<MessageBlock, { type: 'findings' }>;
export type MessageChecklistBlock = Extract<MessageBlock, { type: 'checklist' }>;
export type MessageCodeBlock = Extract<MessageBlock, { type: 'code' }>;
export type MessageFilesBlock = Extract<MessageBlock, { type: 'files' }>;

export type MessageUserValidationResult =
  | { ok: true; request: MessageUserRequest }
  | { ok: false; code: string; message: string };

const VALID_TONES = new Set<MessageTone>(['info', 'success', 'warning', 'error']);
const VALID_CALLOUT_TONES = new Set<MessageCalloutTone>(['info', 'success', 'warning', 'danger']);
const VALID_FACT_TONES = new Set<MessageFactTone>(['default', 'success', 'warning', 'danger']);
const VALID_FINDING_SEVERITIES = new Set<MessageFindingSeverity>(['high', 'medium', 'low', 'info']);
const VALID_CHECKLIST_STATUSES = new Set<MessageChecklistStatus>(['pass', 'fail', 'warn', 'skip', 'running']);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isReservedLabel(label: string): boolean {
  const normalized = label.trim().toLowerCase().replace(/\s+/g, ' ');
  return RESERVED_OPTION_LABELS.has(normalized);
}

function trimOptional(value: unknown, maxChars: number): string | undefined {
  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim();
  return trimmed ? trimmed.slice(0, maxChars) : undefined;
}

export function sanitizeMessageRepoPath(raw: unknown): string | null {
  if (typeof raw !== 'string') return null;
  let normalized = raw.trim().replace(/\\/g, '/');
  while (normalized.startsWith('./')) {
    normalized = normalized.slice(2);
  }
  if (!normalized || normalized.length > MESSAGE_USER_LIMITS.maxPathChars) return null;
  if (normalized.startsWith('/') || normalized.includes('\0')) return null;
  const segments = normalized.split('/');
  if (segments.some((seg) => !seg || seg === '.' || seg === '..' || seg === '.git')) {
    return null;
  }
  return segments.join('/');
}

function parseOptionalLine(raw: unknown): { ok: true; line?: number } | { ok: false } {
  if (raw === undefined || raw === null) return { ok: true };
  if (typeof raw !== 'number' || !Number.isSafeInteger(raw) || raw < 1) {
    return { ok: false };
  }
  return { ok: true, line: raw };
}

function sanitizeLinks(raw: unknown): MessageLink[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const links: MessageLink[] = [];
  const seenUrls = new Set<string>();
  for (const item of raw) {
    if (links.length >= MESSAGE_USER_LIMITS.maxLinks) break;
    if (!isRecord(item) || typeof item.label !== 'string' || typeof item.url !== 'string') continue;
    const label = item.label.trim().slice(0, MESSAGE_USER_LIMITS.maxLinkLabelChars);
    const url = item.url.trim();
    if (!label || !/^https?:\/\//i.test(url) || seenUrls.has(url)) continue;
    seenUrls.add(url);
    links.push({ label, url });
  }
  return links.length > 0 ? links : undefined;
}

function validateBlock(raw: unknown, index: number): { ok: true; block: MessageBlock } | { ok: false; code: string; message: string } {
  if (!isRecord(raw) || typeof raw.type !== 'string') {
    return { ok: false, code: 'invalid_block', message: `blocks[${index}] must be an object with a valid \`type\`.` };
  }

  switch (raw.type) {
    case 'markdown': {
      const content = trimOptional(raw.content, MESSAGE_USER_LIMITS.maxMarkdownChars);
      if (!content) {
        return { ok: false, code: 'invalid_markdown_block', message: `blocks[${index}] (markdown) requires non-empty \`content\`.` };
      }
      return { ok: true, block: { type: 'markdown', content } };
    }
    case 'callout': {
      const content = trimOptional(raw.content, MESSAGE_USER_LIMITS.maxCalloutContentChars);
      if (!content) {
        return { ok: false, code: 'invalid_callout_block', message: `blocks[${index}] (callout) requires non-empty \`content\`.` };
      }
      const tone = typeof raw.tone === 'string' && VALID_CALLOUT_TONES.has(raw.tone as MessageCalloutTone)
        ? (raw.tone as MessageCalloutTone)
        : undefined;
      if (raw.tone !== undefined && raw.tone !== null && !tone) {
        return { ok: false, code: 'invalid_callout_tone', message: `blocks[${index}].tone must be one of info, success, warning, danger.` };
      }
      const title = trimOptional(raw.title, MESSAGE_USER_LIMITS.maxCalloutTitleChars);
      return {
        ok: true,
        block: {
          type: 'callout',
          ...(tone ? { tone } : {}),
          ...(title ? { title } : {}),
          content,
        },
      };
    }
    case 'facts': {
      if (!Array.isArray(raw.items) || raw.items.length === 0 || raw.items.length > MESSAGE_USER_LIMITS.maxFactsItems) {
        return {
          ok: false,
          code: 'invalid_facts_items',
          message: `blocks[${index}] (facts) requires 1 to ${MESSAGE_USER_LIMITS.maxFactsItems} items.`,
        };
      }
      const items: MessageFactItem[] = [];
      for (let i = 0; i < raw.items.length; i += 1) {
        const entry = raw.items[i];
        if (!isRecord(entry)) {
          return { ok: false, code: 'invalid_fact_item', message: `blocks[${index}].items[${i}] must be an object.` };
        }
        const label = trimOptional(entry.label, MESSAGE_USER_LIMITS.maxFactLabelChars);
        const value = trimOptional(entry.value, MESSAGE_USER_LIMITS.maxFactValueChars);
        if (!label || !value) {
          return { ok: false, code: 'invalid_fact_item', message: `blocks[${index}].items[${i}] requires non-empty \`label\` and \`value\`.` };
        }
        const tone = typeof entry.tone === 'string' && VALID_FACT_TONES.has(entry.tone as MessageFactTone)
          ? (entry.tone as MessageFactTone)
          : undefined;
        if (entry.tone !== undefined && entry.tone !== null && !tone) {
          return { ok: false, code: 'invalid_fact_tone', message: `blocks[${index}].items[${i}].tone must be default, success, warning, or danger.` };
        }
        items.push({
          label,
          value,
          ...(tone ? { tone } : {}),
          ...(entry.mono === true ? { mono: true } : {}),
        });
      }
      const heading = trimOptional(raw.heading, MESSAGE_USER_LIMITS.maxBlockHeadingChars);
      return {
        ok: true,
        block: {
          type: 'facts',
          ...(heading ? { heading } : {}),
          items,
        },
      };
    }
    case 'findings': {
      if (!Array.isArray(raw.items) || raw.items.length > MESSAGE_USER_LIMITS.maxFindingsItems) {
        return {
          ok: false,
          code: 'invalid_findings_items',
          message: `blocks[${index}] (findings) requires an \`items\` array (up to ${MESSAGE_USER_LIMITS.maxFindingsItems} items).`,
        };
      }
      const items: MessageFindingItem[] = [];
      for (let i = 0; i < raw.items.length; i += 1) {
        const entry = raw.items[i];
        if (!isRecord(entry)) {
          return { ok: false, code: 'invalid_finding_item', message: `blocks[${index}].items[${i}] must be an object.` };
        }
        if (typeof entry.severity !== 'string' || !VALID_FINDING_SEVERITIES.has(entry.severity as MessageFindingSeverity)) {
          return { ok: false, code: 'invalid_finding_severity', message: `blocks[${index}].items[${i}].severity must be high, medium, low, or info.` };
        }
        const title = trimOptional(entry.title, MESSAGE_USER_LIMITS.maxFindingTitleChars);
        const detail = trimOptional(entry.detail, MESSAGE_USER_LIMITS.maxFindingDetailChars);
        if (!title || !detail) {
          return { ok: false, code: 'invalid_finding_item', message: `blocks[${index}].items[${i}] requires non-empty \`title\` and \`detail\`.` };
        }
        let path: string | undefined;
        if (entry.path !== undefined && entry.path !== null && String(entry.path).trim() !== '') {
          const cleanPath = sanitizeMessageRepoPath(entry.path);
          if (!cleanPath) {
            return { ok: false, code: 'invalid_finding_path', message: `blocks[${index}].items[${i}].path must be a safe repository-relative path without \`..\` or \`.git\`.` };
          }
          path = cleanPath;
        }
        const lineParsed = parseOptionalLine(entry.line);
        if (!lineParsed.ok) {
          return { ok: false, code: 'invalid_finding_line', message: `blocks[${index}].items[${i}].line must be a positive integer.` };
        }
        const tag = trimOptional(entry.tag, MESSAGE_USER_LIMITS.maxFindingTagChars);
        items.push({
          severity: entry.severity as MessageFindingSeverity,
          ...(tag ? { tag } : {}),
          title,
          detail,
          ...(path ? { path } : {}),
          ...(path && lineParsed.line ? { line: lineParsed.line } : {}),
        });
      }
      const emptyText = trimOptional(raw.emptyText, MESSAGE_USER_LIMITS.maxFindingDetailChars);
      if (items.length === 0 && !emptyText) {
        return {
          ok: false,
          code: 'empty_findings_block',
          message: `blocks[${index}] (findings) has no items; provide at least one item or a non-empty \`emptyText\`.`,
        };
      }
      let skipped: MessageSkippedFileItem[] | undefined;
      if (Array.isArray(raw.skipped) && raw.skipped.length > 0) {
        skipped = [];
        for (const item of raw.skipped.slice(0, MESSAGE_USER_LIMITS.maxSkippedItems)) {
          if (!isRecord(item)) continue;
          const cleanPath = sanitizeMessageRepoPath(item.path);
          const reason = trimOptional(item.reason, MESSAGE_USER_LIMITS.maxSkippedReasonChars);
          if (cleanPath && reason) {
            skipped.push({ path: cleanPath, reason });
          }
        }
        if (skipped.length === 0) skipped = undefined;
      }
      const heading = trimOptional(raw.heading, MESSAGE_USER_LIMITS.maxBlockHeadingChars);
      return {
        ok: true,
        block: {
          type: 'findings',
          ...(heading ? { heading } : {}),
          items,
          ...(emptyText ? { emptyText } : {}),
          ...(skipped ? { skipped } : {}),
        },
      };
    }
    case 'checklist': {
      if (!Array.isArray(raw.items) || raw.items.length === 0 || raw.items.length > MESSAGE_USER_LIMITS.maxChecklistItems) {
        return {
          ok: false,
          code: 'invalid_checklist_items',
          message: `blocks[${index}] (checklist) requires 1 to ${MESSAGE_USER_LIMITS.maxChecklistItems} items.`,
        };
      }
      const items: MessageChecklistItem[] = [];
      for (let i = 0; i < raw.items.length; i += 1) {
        const entry = raw.items[i];
        if (!isRecord(entry)) {
          return { ok: false, code: 'invalid_checklist_item', message: `blocks[${index}].items[${i}] must be an object.` };
        }
        if (typeof entry.status !== 'string' || !VALID_CHECKLIST_STATUSES.has(entry.status as MessageChecklistStatus)) {
          return { ok: false, code: 'invalid_checklist_status', message: `blocks[${index}].items[${i}].status must be pass, fail, warn, skip, or running.` };
        }
        const label = trimOptional(entry.label, MESSAGE_USER_LIMITS.maxChecklistLabelChars);
        if (!label) {
          return { ok: false, code: 'invalid_checklist_item', message: `blocks[${index}].items[${i}] requires a non-empty \`label\`.` };
        }
        const detail = trimOptional(entry.detail, MESSAGE_USER_LIMITS.maxChecklistDetailChars);
        items.push({
          status: entry.status as MessageChecklistStatus,
          label,
          ...(detail ? { detail } : {}),
        });
      }
      const heading = trimOptional(raw.heading, MESSAGE_USER_LIMITS.maxBlockHeadingChars);
      return {
        ok: true,
        block: {
          type: 'checklist',
          ...(heading ? { heading } : {}),
          items,
        },
      };
    }
    case 'code': {
      if (typeof raw.content !== 'string' || !raw.content.trim()) {
        return { ok: false, code: 'invalid_code_block', message: `blocks[${index}] (code) requires non-empty \`content\`.` };
      }
      const content = raw.content.replace(/\r\n/g, '\n').trimEnd().slice(0, MESSAGE_USER_LIMITS.maxCodeContentChars);
      const title = trimOptional(raw.title, MESSAGE_USER_LIMITS.maxCodeTitleChars);
      const language = trimOptional(raw.language, MESSAGE_USER_LIMITS.maxCodeLanguageChars);
      return {
        ok: true,
        block: {
          type: 'code',
          ...(title ? { title } : {}),
          ...(language ? { language } : {}),
          content,
          ...(typeof raw.collapsed === 'boolean' ? { collapsed: raw.collapsed } : {}),
        },
      };
    }
    case 'files': {
      if (!Array.isArray(raw.items) || raw.items.length === 0 || raw.items.length > MESSAGE_USER_LIMITS.maxFilesItems) {
        return {
          ok: false,
          code: 'invalid_files_items',
          message: `blocks[${index}] (files) requires 1 to ${MESSAGE_USER_LIMITS.maxFilesItems} items.`,
        };
      }
      const items: MessageFileChipItem[] = [];
      for (let i = 0; i < raw.items.length; i += 1) {
        const entry = raw.items[i];
        if (!isRecord(entry)) {
          return { ok: false, code: 'invalid_file_item', message: `blocks[${index}].items[${i}] must be an object.` };
        }
        const cleanPath = sanitizeMessageRepoPath(entry.path);
        if (!cleanPath) {
          return { ok: false, code: 'invalid_file_path', message: `blocks[${index}].items[${i}].path must be a safe repository-relative path without \`..\` or \`.git\`.` };
        }
        const lineParsed = parseOptionalLine(entry.line);
        if (!lineParsed.ok) {
          return { ok: false, code: 'invalid_file_line', message: `blocks[${index}].items[${i}].line must be a positive integer.` };
        }
        const badge = trimOptional(entry.badge, MESSAGE_USER_LIMITS.maxFileBadgeChars);
        items.push({
          path: cleanPath,
          ...(lineParsed.line ? { line: lineParsed.line } : {}),
          ...(badge ? { badge } : {}),
        });
      }
      const heading = trimOptional(raw.heading, MESSAGE_USER_LIMITS.maxBlockHeadingChars);
      return {
        ok: true,
        block: {
          type: 'files',
          ...(heading ? { heading } : {}),
          items,
        },
      };
    }
    default:
      return {
        ok: false,
        code: 'unknown_block_type',
        message: `blocks[${index}].type "${raw.type}" is not supported. Use one of: markdown, callout, facts, findings, checklist, code, files.`,
      };
  }
}

export function validateMessageUserRequest(raw: unknown): MessageUserValidationResult {
  if (!isRecord(raw)) {
    return {
      ok: false,
      code: 'invalid_root',
      message: 'message_user arguments must be an object with `title` and optional `blocks` and/or `questions`.',
    };
  }

  const title = trimOptional(raw.title, MESSAGE_USER_LIMITS.maxTitleChars);
  const subtitle = trimOptional(raw.subtitle, MESSAGE_USER_LIMITS.maxSubtitleChars);
  const tone = typeof raw.tone === 'string' && VALID_TONES.has(raw.tone as MessageTone)
    ? (raw.tone as MessageTone)
    : undefined;
  if (raw.tone !== undefined && raw.tone !== null && !tone) {
    return {
      ok: false,
      code: 'invalid_tone',
      message: '`tone` must be one of info, success, warning, or error.',
    };
  }

  let blocks: MessageBlock[] | undefined;
  if (raw.blocks !== undefined && raw.blocks !== null) {
    if (!Array.isArray(raw.blocks)) {
      return { ok: false, code: 'invalid_blocks', message: '`blocks` must be an array when provided.' };
    }
    if (raw.blocks.length > MESSAGE_USER_LIMITS.maxBlocks) {
      return {
        ok: false,
        code: 'too_many_blocks',
        message: `Expected at most ${MESSAGE_USER_LIMITS.maxBlocks} blocks, received ${raw.blocks.length}.`,
      };
    }
    if (raw.blocks.length > 0) {
      blocks = [];
      for (let i = 0; i < raw.blocks.length; i += 1) {
        const res = validateBlock(raw.blocks[i], i);
        if (!res.ok) return res;
        blocks.push(res.block);
      }
    }
  }

  let questions: MessageQuestionItem[] | undefined;
  if (raw.questions !== undefined && raw.questions !== null) {
    if (!Array.isArray(raw.questions)) {
      return {
        ok: false,
        code: 'invalid_questions',
        message: '`questions` must be an array of 1 to 4 question objects when provided.',
      };
    }
    if (raw.questions.length > MESSAGE_USER_LIMITS.maxQuestions) {
      return {
        ok: false,
        code: 'invalid_question_count',
        message: `Expected between ${MESSAGE_USER_LIMITS.minQuestions} and ${MESSAGE_USER_LIMITS.maxQuestions} questions, received ${raw.questions.length}.`,
      };
    }
    if (raw.questions.length > 0) {
      questions = [];
      const seenQuestionIds = new Set<string>();
      for (let qIndex = 0; qIndex < raw.questions.length; qIndex += 1) {
        const entry = raw.questions[qIndex];
        if (!isRecord(entry)) {
          return {
            ok: false,
            code: 'invalid_question_item',
            message: `questions[${qIndex}] must be an object.`,
          };
        }
        const header = typeof entry.header === 'string' ? entry.header.trim() : '';
        const question = typeof entry.question === 'string' ? entry.question.trim() : '';
        if (!header || header.length > MESSAGE_USER_LIMITS.maxHeaderChars) {
          return {
            ok: false,
            code: 'invalid_header',
            message: `questions[${qIndex}].header must be a non-empty short label (max ${MESSAGE_USER_LIMITS.maxHeaderChars} chars).`,
          };
        }
        if (!question || question.length > MESSAGE_USER_LIMITS.maxQuestionChars) {
          return {
            ok: false,
            code: 'invalid_question_text',
            message: `questions[${qIndex}].question must be non-empty (max ${MESSAGE_USER_LIMITS.maxQuestionChars} chars).`,
          };
        }
        const multiSelect = typeof entry.multiSelect === 'boolean' ? entry.multiSelect : false;
        if (!Array.isArray(entry.options)
          || entry.options.length < MESSAGE_USER_LIMITS.minOptions
          || entry.options.length > MESSAGE_USER_LIMITS.maxOptions) {
          return {
            ok: false,
            code: 'invalid_option_count',
            message: `questions[${qIndex}].options must contain ${MESSAGE_USER_LIMITS.minOptions} to ${MESSAGE_USER_LIMITS.maxOptions} options.`,
          };
        }

        const qId = typeof entry.id === 'string' && entry.id.trim()
          ? entry.id.trim().slice(0, 64)
          : `q-${qIndex + 1}`;
        if (seenQuestionIds.has(qId)) {
          return {
            ok: false,
            code: 'duplicate_question_id',
            message: `Duplicate question id "${qId}".`,
          };
        }
        seenQuestionIds.add(qId);

        const options: MessageQuestionOption[] = [];
        const seenOptionIds = new Set<string>();
        const seenOptionLabels = new Set<string>();

        for (let oIndex = 0; oIndex < entry.options.length; oIndex += 1) {
          const rawOpt = entry.options[oIndex];
          if (!isRecord(rawOpt)) {
            return {
              ok: false,
              code: 'invalid_option_item',
              message: `questions[${qIndex}].options[${oIndex}] must be an object.`,
            };
          }
          const label = typeof rawOpt.label === 'string' ? rawOpt.label.trim() : '';
          const description = typeof rawOpt.description === 'string' ? rawOpt.description.trim() : '';
          if (!label || label.length > MESSAGE_USER_LIMITS.maxOptionLabelChars) {
            return {
              ok: false,
              code: 'invalid_option_label',
              message: `questions[${qIndex}].options[${oIndex}].label must be non-empty (max ${MESSAGE_USER_LIMITS.maxOptionLabelChars} chars).`,
            };
          }
          if (isReservedLabel(label)) {
            return {
              ok: false,
              code: 'reserved_option_label',
              message: `Option label "${label}" is reserved because the UI always provides its own free-text reply and submit controls. Omit it and list only concrete choices.`,
            };
          }
          const normalizedLabel = label.toLowerCase();
          if (seenOptionLabels.has(normalizedLabel)) {
            return {
              ok: false,
              code: 'duplicate_option_label',
              message: `Duplicate option label "${label}" in questions[${qIndex}].`,
            };
          }
          seenOptionLabels.add(normalizedLabel);

          if (!description || description.length > MESSAGE_USER_LIMITS.maxOptionDescriptionChars) {
            return {
              ok: false,
              code: 'invalid_option_description',
              message: `questions[${qIndex}].options[${oIndex}].description must explain the option concisely (max ${MESSAGE_USER_LIMITS.maxOptionDescriptionChars} chars).`,
            };
          }
          const preview = typeof rawOpt.preview === 'string' && rawOpt.preview.trim()
            ? rawOpt.preview.trim().slice(0, MESSAGE_USER_LIMITS.maxPreviewChars)
            : undefined;
          if (preview && multiSelect) {
            return {
              ok: false,
              code: 'preview_with_multiselect',
              message: `questions[${qIndex}] cannot combine option \`preview\` with \`multiSelect: true\`.`,
            };
          }
          const optId = typeof rawOpt.id === 'string' && rawOpt.id.trim()
            ? rawOpt.id.trim().slice(0, 64)
            : `${qId}-opt-${oIndex + 1}`;
          if (seenOptionIds.has(optId)) {
            return {
              ok: false,
              code: 'duplicate_option_id',
              message: `Duplicate option id "${optId}" in questions[${qIndex}].`,
            };
          }
          seenOptionIds.add(optId);

          options.push({
            id: optId,
            label,
            description,
            ...(typeof rawOpt.recommended === 'boolean' ? { recommended: rawOpt.recommended } : {}),
            ...(preview ? { preview } : {}),
          });
        }

        questions.push({
          id: qId,
          header,
          question,
          options,
          ...(multiSelect ? { multiSelect: true } : {}),
        });
      }
    }
  }

  if (!title && (!questions || questions.length === 0)) {
    return {
      ok: false,
      code: 'missing_title',
      message: '`title` is required when calling `message_user` without `questions`.',
    };
  }

  let context: MessageQuestionContext | undefined;
  if (raw.context !== undefined && raw.context !== null) {
    if (!isRecord(raw.context)) {
      return {
        ok: false,
        code: 'invalid_context',
        message: '`context` must be an object when provided.',
      };
    }
    const scope = trimOptional(raw.context.scope, 120);
    const affectedFileIds = Array.isArray(raw.context.affectedFileIds)
      ? [...new Set(raw.context.affectedFileIds.filter((item): item is string => typeof item === 'string' && item.trim().length > 0).map((item) => item.trim()))]
      : undefined;
    const contextLinks = sanitizeLinks(raw.context.links);
    const allowDontAskAgain = typeof raw.context.allowDontAskAgain === 'boolean' ? raw.context.allowDontAskAgain : undefined;
    const dontAskAgainLabel = trimOptional(raw.context.dontAskAgainLabel, 100);
    if (scope || (affectedFileIds && affectedFileIds.length > 0) || contextLinks || allowDontAskAgain) {
      context = {
        ...(scope ? { scope } : {}),
        ...(affectedFileIds && affectedFileIds.length > 0 ? { affectedFileIds } : {}),
        ...(contextLinks ? { links: contextLinks } : {}),
        ...(allowDontAskAgain ? { allowDontAskAgain: true } : {}),
        ...(dontAskAgainLabel ? { dontAskAgainLabel } : {}),
      };
    }
  }

  const links = sanitizeLinks(raw.links);
  const resolvedTitle = title ?? questions?.[0]?.header;

  return {
    ok: true,
    request: {
      ...(resolvedTitle ? { title: resolvedTitle } : {}),
      ...(subtitle ? { subtitle } : {}),
      ...(tone ? { tone } : {}),
      ...(blocks ? { blocks } : {}),
      ...(links ? { links } : {}),
      ...(questions ? { questions } : {}),
      ...(context ? { context } : {}),
    },
  };
}

export function hasMessageQuestions(request: MessageUserRequest): boolean {
  return Boolean(request.questions && request.questions.length > 0);
}

export function extractMessageUserCard(request: MessageUserRequest): MessageUserCard | undefined {
  const title = request.title?.trim() || request.questions?.[0]?.header?.trim() || '';
  if (!title) return undefined;
  const mergedLinks: MessageLink[] = [];
  const seen = new Set<string>();
  for (const list of [request.links, request.context?.links]) {
    if (!list) continue;
    for (const link of list) {
      if (mergedLinks.length >= MESSAGE_USER_LIMITS.maxLinks) break;
      if (seen.has(link.url)) continue;
      seen.add(link.url);
      mergedLinks.push(link);
    }
  }
  return {
    title,
    ...(request.subtitle ? { subtitle: request.subtitle } : {}),
    ...(request.tone ? { tone: request.tone } : {}),
    ...(request.blocks && request.blocks.length > 0 ? { blocks: request.blocks } : {}),
    ...(mergedLinks.length > 0 ? { links: mergedLinks } : {}),
  };
}

export function decodeMessageUserCard(raw: unknown): MessageUserCard | undefined {
  if (raw === undefined || raw === null) return undefined;
  if (!isRecord(raw)) return undefined;
  const validated = validateMessageUserRequest({
    title: raw.title,
    subtitle: raw.subtitle,
    tone: raw.tone,
    blocks: raw.blocks,
    links: raw.links,
  });
  if (!validated.ok) return undefined;
  return extractMessageUserCard(validated.request);
}

export function formatMessageUserResponseForModel(
  request: MessageUserRequest,
  response: MessageUserResponse,
): string {
  const questions = request.questions ?? [];
  if (response.cancelled) {
    const note = response.globalNote?.trim();
    return note
      ? `User declined to answer questions. Note: ${note}`
      : 'User declined to answer questions.';
  }
  const lines: string[] = [];
  const answerByIndex = new Map(response.answers.map((ans) => [ans.questionIndex, ans]));
  for (let i = 0; i < questions.length; i += 1) {
    const q = questions[i];
    const ans = answerByIndex.get(i);
    if (!ans || (ans.selectedLabels.length === 0 && !ans.customText?.trim())) {
      lines.push(`- [${q.header}] (unanswered)`);
      continue;
    }
    const parts: string[] = [];
    if (ans.selectedLabels.length > 0) {
      const formatted = ans.selectedLabels.map((label, idx) => {
        const optId = ans.selectedOptionIds[idx];
        return optId ? `${label} (id=${optId})` : label;
      });
      parts.push(formatted.join(', '));
    }
    if (ans.customText?.trim()) {
      parts.push(`Custom: ${ans.customText.trim()}`);
    }
    lines.push(`- [${q.header}] ${parts.join(' | ')}`);
  }
  if (response.dontAskAgain) {
    lines.push('- Preference: user checked "do not ask again for this action"');
  }
  if (response.globalNote?.trim()) {
    lines.push(`- Note: ${response.globalNote.trim()}`);
  }
  return `User responses:\n${lines.join('\n')}`;
}
