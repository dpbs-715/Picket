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

const VALID_TONES = new Set<MessageTone>(['info', 'success', 'warning', 'error']);
const VALID_BLOCK_TYPES = new Set<MessageBlock['type']>([
  'markdown',
  'callout',
  'facts',
  'findings',
  'checklist',
  'code',
  'files',
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Defensive repository-relative path check used when decoding UI cards before
 * offering click-to-open-in-IDE links. Full authoritative `message_user` schema
 * validation lives exclusively in Rust (`src-tauri/src/actions/message_user.rs`).
 */
export function sanitizeMessageRepoPath(raw: unknown): string | null {
  if (typeof raw !== 'string') return null;
  let normalized = raw.trim().replace(/\\/g, '/');
  while (normalized.startsWith('./')) {
    normalized = normalized.slice(2);
  }
  if (!normalized || normalized.length > 300 || normalized.startsWith('/') || normalized.includes('\0')) {
    return null;
  }
  const segments = normalized.split('/');
  if (segments.some((seg) => !seg || seg === '.' || seg === '..' || seg.toLowerCase() === '.git')) {
    return null;
  }
  return segments.join('/');
}

function decodeSafeLinks(raw: unknown): MessageLink[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const links: MessageLink[] = [];
  const seen = new Set<string>();
  for (const item of raw) {
    if (links.length >= 6) break;
    if (!isRecord(item) || typeof item.label !== 'string' || typeof item.url !== 'string') continue;
    const label = item.label.trim().slice(0, 80);
    const url = item.url.trim();
    if (!label || !/^https?:\/\//i.test(url) || seen.has(url)) continue;
    seen.add(url);
    links.push({ label, url });
  }
  return links.length > 0 ? links : undefined;
}

function decodeSafeBlock(raw: unknown): MessageBlock | undefined {
  if (!isRecord(raw) || typeof raw.type !== 'string' || !VALID_BLOCK_TYPES.has(raw.type as MessageBlock['type'])) {
    return undefined;
  }
  if (raw.type === 'findings') {
    if (!Array.isArray(raw.items)) return undefined;
    const items: MessageFindingItem[] = [];
    for (const entry of raw.items) {
      if (!isRecord(entry) || typeof entry.title !== 'string' || typeof entry.detail !== 'string') continue;
      const path = entry.path !== undefined && entry.path !== null
        ? (sanitizeMessageRepoPath(entry.path) ?? undefined)
        : undefined;
      if (entry.path !== undefined && entry.path !== null && String(entry.path).trim() !== '' && !path) {
        return undefined;
      }
      items.push({
        ...(entry as unknown as MessageFindingItem),
        ...(path ? { path } : { path: undefined, line: undefined }),
      });
    }
    const skipped = Array.isArray(raw.skipped)
      ? raw.skipped
          .filter((s): s is Record<string, unknown> => isRecord(s) && typeof s.reason === 'string')
          .map((s) => {
            const path = sanitizeMessageRepoPath(s.path);
            return path ? { path, reason: s.reason as string } : null;
          })
          .filter((s): s is MessageSkippedFileItem => s !== null)
      : undefined;
    return {
      ...(raw as unknown as MessageFindingsBlock),
      items,
      ...(skipped && skipped.length > 0 ? { skipped } : { skipped: undefined }),
    };
  }
  if (raw.type === 'files') {
    if (!Array.isArray(raw.items) || raw.items.length === 0) return undefined;
    const items: MessageFileChipItem[] = [];
    for (const entry of raw.items) {
      if (!isRecord(entry)) return undefined;
      const path = sanitizeMessageRepoPath(entry.path);
      if (!path) return undefined;
      items.push({ ...(entry as unknown as MessageFileChipItem), path });
    }
    return { ...(raw as unknown as MessageFilesBlock), items };
  }
  return raw as unknown as MessageBlock;
}

export function hasMessageQuestions(request: MessageUserRequest): boolean {
  return Boolean(request.questions && request.questions.length > 0);
}

export function extractMessageUserCard(request: MessageUserRequest): MessageUserCard | undefined {
  const title = request.title?.trim() || request.questions?.[0]?.header?.trim() || '';
  if (!title) return undefined;
  const mergedLinks = decodeSafeLinks([
    ...(request.links ?? []),
    ...(request.context?.links ?? []),
  ]);
  return {
    title,
    ...(request.subtitle ? { subtitle: request.subtitle } : {}),
    ...(request.tone ? { tone: request.tone } : {}),
    ...(request.blocks && request.blocks.length > 0 ? { blocks: request.blocks } : {}),
    ...(mergedLinks ? { links: mergedLinks } : {}),
  };
}

/**
 * Lightweight defensive decoder for `MessageUserCard` payloads already validated by Rust.
 */
export function decodeMessageUserCard(raw: unknown): MessageUserCard | undefined {
  if (!isRecord(raw) || typeof raw.title !== 'string') return undefined;
  const title = raw.title.trim().slice(0, 120);
  if (!title) return undefined;
  const subtitle = typeof raw.subtitle === 'string' && raw.subtitle.trim()
    ? raw.subtitle.trim().slice(0, 200)
    : undefined;
  const tone = typeof raw.tone === 'string' && VALID_TONES.has(raw.tone as MessageTone)
    ? (raw.tone as MessageTone)
    : undefined;
  let blocks: MessageBlock[] | undefined;
  if (Array.isArray(raw.blocks) && raw.blocks.length > 0) {
    const decoded: MessageBlock[] = [];
    for (const entry of raw.blocks.slice(0, 12)) {
      const block = decodeSafeBlock(entry);
      if (!block) return undefined;
      decoded.push(block);
    }
    if (decoded.length > 0) blocks = decoded;
  }
  const links = decodeSafeLinks(raw.links);
  return {
    title,
    ...(subtitle ? { subtitle } : {}),
    ...(tone ? { tone } : {}),
    ...(blocks ? { blocks } : {}),
    ...(links ? { links } : {}),
  };
}

export function formatMessageUserResponseForModel(
  request: Pick<MessageUserRequest, 'questions'>,
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
