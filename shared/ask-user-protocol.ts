export const ASK_USER_LIMITS = {
  minQuestions: 1,
  maxQuestions: 4,
  minOptions: 2,
  maxOptions: 4,
  maxHeaderChars: 24,
  maxQuestionChars: 600,
  maxOptionLabelChars: 80,
  maxOptionDescriptionChars: 240,
  maxPreviewChars: 2000,
  maxLinks: 6,
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

export interface AskUserQuestionOption {
  id: string;
  label: string;
  description: string;
  recommended?: boolean;
  preview?: string;
}

export interface AskUserQuestionItem {
  id: string;
  header: string;
  question: string;
  options: AskUserQuestionOption[];
  multiSelect?: boolean;
}

export interface AskUserQuestionLink {
  label: string;
  url: string;
}

export interface AskUserQuestionContext {
  scope?: string;
  affectedFileIds?: string[];
  links?: AskUserQuestionLink[];
  allowDontAskAgain?: boolean;
  dontAskAgainLabel?: string;
}

export interface AskUserQuestionRequest {
  questions: AskUserQuestionItem[];
  context?: AskUserQuestionContext;
}

export interface AskUserQuestionAnswer {
  questionIndex: number;
  questionId: string;
  header: string;
  selectedOptionIds: string[];
  selectedLabels: string[];
  customText?: string;
}

export interface AskUserQuestionResponse {
  cancelled: boolean;
  answers: AskUserQuestionAnswer[];
  unanswered: number[];
  globalNote?: string;
  dontAskAgain?: boolean;
}

export type AskUserValidationResult =
  | { ok: true; request: AskUserQuestionRequest }
  | { ok: false; code: string; message: string };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isReservedLabel(label: string): boolean {
  const normalized = label.trim().toLowerCase().replace(/\s+/g, ' ');
  return RESERVED_OPTION_LABELS.has(normalized);
}

export function validateAskUserQuestionRequest(raw: unknown): AskUserValidationResult {
  if (!isRecord(raw)) {
    return {
      ok: false,
      code: 'invalid_root',
      message: 'ask_user_question arguments must be an object with a `questions` array (1 to 4 items).',
    };
  }
  if (!Array.isArray(raw.questions)) {
    return {
      ok: false,
      code: 'missing_questions',
      message: '`questions` must be an array containing 1 to 4 question objects.',
    };
  }
  if (raw.questions.length < ASK_USER_LIMITS.minQuestions || raw.questions.length > ASK_USER_LIMITS.maxQuestions) {
    return {
      ok: false,
      code: 'invalid_question_count',
      message: `Expected between ${ASK_USER_LIMITS.minQuestions} and ${ASK_USER_LIMITS.maxQuestions} questions, received ${raw.questions.length}.`,
    };
  }

  const questions: AskUserQuestionItem[] = [];
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
    if (!header || header.length > ASK_USER_LIMITS.maxHeaderChars) {
      return {
        ok: false,
        code: 'invalid_header',
        message: `questions[${qIndex}].header must be a non-empty short label (max ${ASK_USER_LIMITS.maxHeaderChars} chars).`,
      };
    }
    if (!question || question.length > ASK_USER_LIMITS.maxQuestionChars) {
      return {
        ok: false,
        code: 'invalid_question_text',
        message: `questions[${qIndex}].question must be non-empty (max ${ASK_USER_LIMITS.maxQuestionChars} chars).`,
      };
    }
    const multiSelect = typeof entry.multiSelect === 'boolean' ? entry.multiSelect : false;
    if (!Array.isArray(entry.options)
      || entry.options.length < ASK_USER_LIMITS.minOptions
      || entry.options.length > ASK_USER_LIMITS.maxOptions) {
      return {
        ok: false,
        code: 'invalid_option_count',
        message: `questions[${qIndex}].options must contain ${ASK_USER_LIMITS.minOptions} to ${ASK_USER_LIMITS.maxOptions} options.`,
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

    const options: AskUserQuestionOption[] = [];
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
      if (!label || label.length > ASK_USER_LIMITS.maxOptionLabelChars) {
        return {
          ok: false,
          code: 'invalid_option_label',
          message: `questions[${qIndex}].options[${oIndex}].label must be non-empty (max ${ASK_USER_LIMITS.maxOptionLabelChars} chars).`,
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

      if (!description || description.length > ASK_USER_LIMITS.maxOptionDescriptionChars) {
        return {
          ok: false,
          code: 'invalid_option_description',
          message: `questions[${qIndex}].options[${oIndex}].description must explain the option concisely (max ${ASK_USER_LIMITS.maxOptionDescriptionChars} chars).`,
        };
      }
      const preview = typeof rawOpt.preview === 'string' && rawOpt.preview.trim()
        ? rawOpt.preview.trim().slice(0, ASK_USER_LIMITS.maxPreviewChars)
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

  let context: AskUserQuestionContext | undefined;
  if (raw.context !== undefined && raw.context !== null) {
    if (!isRecord(raw.context)) {
      return {
        ok: false,
        code: 'invalid_context',
        message: '`context` must be an object when provided.',
      };
    }
    const scope = typeof raw.context.scope === 'string' && raw.context.scope.trim()
      ? raw.context.scope.trim().slice(0, 120)
      : undefined;
    const affectedFileIds = Array.isArray(raw.context.affectedFileIds)
      ? [...new Set(raw.context.affectedFileIds.filter((item): item is string => typeof item === 'string' && item.trim().length > 0).map((item) => item.trim()))]
      : undefined;
    let links: AskUserQuestionLink[] | undefined;
    if (Array.isArray(raw.context.links)) {
      links = [];
      for (const item of raw.context.links.slice(0, ASK_USER_LIMITS.maxLinks)) {
        if (!isRecord(item) || typeof item.label !== 'string' || typeof item.url !== 'string') continue;
        const label = item.label.trim().slice(0, 80);
        const url = item.url.trim();
        if (!label || !/^https?:\/\//i.test(url)) continue;
        links.push({ label, url });
      }
      if (links.length === 0) links = undefined;
    }
    const allowDontAskAgain = typeof raw.context.allowDontAskAgain === 'boolean' ? raw.context.allowDontAskAgain : undefined;
    const dontAskAgainLabel = typeof raw.context.dontAskAgainLabel === 'string' && raw.context.dontAskAgainLabel.trim()
      ? raw.context.dontAskAgainLabel.trim().slice(0, 100)
      : undefined;
    if (scope || (affectedFileIds && affectedFileIds.length > 0) || links || allowDontAskAgain) {
      context = {
        ...(scope ? { scope } : {}),
        ...(affectedFileIds && affectedFileIds.length > 0 ? { affectedFileIds } : {}),
        ...(links ? { links } : {}),
        ...(allowDontAskAgain ? { allowDontAskAgain: true } : {}),
        ...(dontAskAgainLabel ? { dontAskAgainLabel } : {}),
      };
    }
  }

  return {
    ok: true,
    request: {
      questions,
      ...(context ? { context } : {}),
    },
  };
}

export function formatAskUserQuestionResponseForModel(
  request: AskUserQuestionRequest,
  response: AskUserQuestionResponse,
): string {
  if (response.cancelled) {
    const note = response.globalNote?.trim();
    return note
      ? `User declined to answer questions. Note: ${note}`
      : 'User declined to answer questions.';
  }
  const lines: string[] = [];
  const answerByIndex = new Map(response.answers.map((ans) => [ans.questionIndex, ans]));
  for (let i = 0; i < request.questions.length; i += 1) {
    const q = request.questions[i];
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
