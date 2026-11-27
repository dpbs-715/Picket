import {
  decodeMessageUserCard,
  type MessageUserCard,
  type MessageUserRequest,
  type MessageUserResponse,
} from './message-user-protocol';
import type { CPushLandedCommit, CPushMessageKind } from './cpush-protocol';
import { decodeGitReadReply, GIT_READ_PROTOCOL_VERSION } from './git-read-protocol';
import type { GitReadTarget, GitRepositorySnapshot } from './git-read-protocol';
import type { AppLocale, CommitLanguage } from './language-protocol';
import type { ModelFileSelection } from './model-protocol';

export const CUSTOM_ACTION_PROTOCOL_VERSION = 1;

export const CUSTOM_ACTION_LIMITS = {
  maxPerRepo: 30,
  maxIdChars: 64,
  maxLabelChars: 48,
  maxPromptChars: 4000,
  maxGenerateRequestChars: 2000,
} as const;

export interface CustomActionConfig {
  id: string;
  templateKey?: string;
  label: string;
  prompt: string;
  skipWriteConfirm?: boolean;
  trustedPromptHash?: string;
}

export interface CustomActionLink {
  label: string;
  url: string;
}

export interface CustomActionTimelineItem {
  state: 'done' | 'active' | 'failed';
  label: string;
}

export interface CustomActionUiMessage {
  id: string;
  kind: CPushMessageKind;
  title: string;
  description: string;
  scope?: string;
  files?: { id: string; label: string; badge?: string }[];
  commits?: { hash: string; title: string }[];
  links?: CustomActionLink[];
  card?: MessageUserCard;
  questionnaire?: MessageUserRequest;
  progress?: { completed: number; total: number; label: string };
}

export interface TrustedActionUpdate {
  repositoryId: string;
  actionId: string;
  trustedPromptHash: string;
}

export interface StartCustomActionRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  action: CustomActionConfig;
  selections: ModelFileSelection[];
  extraPrompt: string;
  followUpPrompt?: string;
  resumeFromRunId?: string;
  locale?: AppLocale;
  commitLanguage?: CommitLanguage;
}

export interface RespondCustomActionRequest {
  requestId: string;
  runId: string;
  callId: string;
  response: MessageUserResponse;
  locale?: AppLocale;
}

export interface DismissCustomActionDoneRequest {
  requestId: string;
  target: GitReadTarget;
  runId?: string;
  messageId: string;
  locale?: AppLocale;
}

export interface ActionSessionSnapshot {
  runId: string;
  repositoryId: string;
  worktreeId: string;
  actionId: string;
  actionLabel: string;
  status: 'interrupted' | 'done' | 'cancelled' | 'failed';
  summary: string;
  message: CustomActionUiMessage | null;
  timeline: CustomActionTimelineItem[];
  commits: CPushLandedCommit[];
  links: CustomActionLink[];
}

export interface CustomActionProgressEvent {
  protocolVersion: typeof CUSTOM_ACTION_PROTOCOL_VERSION;
  runId: string;
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  actionId: string;
  actionLabel: string;
  status: 'running' | 'needsDecision' | 'done' | 'cancelled' | 'failed';
  pendingCallId?: string;
  message: CustomActionUiMessage | null;
  timeline: CustomActionTimelineItem[];
  commits: CPushLandedCommit[];
  links: CustomActionLink[];
  trustedActionUpdate?: TrustedActionUpdate;
  repositorySnapshot?: GitRepositorySnapshot;
}

export interface CustomActionCommandReply {
  protocolVersion: typeof CUSTOM_ACTION_PROTOCOL_VERSION;
  kind: 'customAction';
  requestId: string;
  runId: string;
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  actionId: string;
  actionLabel: string;
  status: 'done' | 'cancelled' | 'failed';
  summary: string;
  message: CustomActionUiMessage | null;
  timeline: CustomActionTimelineItem[];
  commits: CPushLandedCommit[];
  links: CustomActionLink[];
  trustedActionUpdate?: TrustedActionUpdate;
  repositorySnapshot?: GitRepositorySnapshot;
}

export interface GenerateCustomActionRequest {
  requestId: string;
  target: GitReadTarget;
  modelId: string;
  userRequest: string;
  existingLabels: string[];
  locale?: AppLocale;
}

export interface CustomActionDraft {
  label: string;
  prompt: string;
  summary: string;
}

export interface GenerateCustomActionProgressEvent {
  protocolVersion: typeof CUSTOM_ACTION_PROTOCOL_VERSION;
  runId: string;
  status: 'running' | 'needsDecision' | 'done' | 'cancelled' | 'failed';
  statusLabel: string;
  pendingCallId?: string;
  card?: MessageUserCard;
  questionnaire?: MessageUserRequest;
}

export interface GenerateCustomActionReply {
  protocolVersion: typeof CUSTOM_ACTION_PROTOCOL_VERSION;
  kind: 'generateCustomAction';
  requestId: string;
  status: 'done' | 'cancelled';
  draft?: CustomActionDraft;
}

export interface CustomActionSyncResult {
  added: number;
  updated: number;
  skipped: number;
}

// Pure synchronous SHA-256 (UTF-8) producing lowercase hex matching Rust `sha2::Sha256`.
const SHA256_K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

function sha256Hex(input: string): string {
  const bytes = new TextEncoder().encode(input);
  const bitLen = bytes.length * 8;
  const paddedLen = Math.ceil((bytes.length + 9) / 64) * 64;
  const buf = new Uint8Array(paddedLen);
  buf.set(bytes);
  buf[bytes.length] = 0x80;
  const view = new DataView(buf.buffer);
  view.setUint32(paddedLen - 8, Math.floor(bitLen / 0x100000000), false);
  view.setUint32(paddedLen - 4, bitLen >>> 0, false);

  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;
  const w = new Uint32Array(64);

  for (let offset = 0; offset < paddedLen; offset += 64) {
    for (let i = 0; i < 16; i += 1) {
      w[i] = view.getUint32(offset + i * 4, false);
    }
    for (let i = 16; i < 64; i += 1) {
      const s0 = ((w[i - 15] >>> 7) | (w[i - 15] << 25))
        ^ ((w[i - 15] >>> 18) | (w[i - 15] << 14))
        ^ (w[i - 15] >>> 3);
      const s1 = ((w[i - 2] >>> 17) | (w[i - 2] << 15))
        ^ ((w[i - 2] >>> 19) | (w[i - 2] << 13))
        ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let a = h0;
    let b = h1;
    let c = h2;
    let d = h3;
    let e = h4;
    let f = h5;
    let g = h6;
    let h = h7;
    for (let i = 0; i < 64; i += 1) {
      const S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7));
      const ch = (e & f) ^ (~e & g);
      const temp1 = (h + S1 + ch + SHA256_K[i] + w[i]) >>> 0;
      const S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10));
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (S0 + maj) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }
    h0 = (h0 + a) >>> 0;
    h1 = (h1 + b) >>> 0;
    h2 = (h2 + c) >>> 0;
    h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0;
    h5 = (h5 + f) >>> 0;
    h6 = (h6 + g) >>> 0;
    h7 = (h7 + h) >>> 0;
  }

  return [h0, h1, h2, h3, h4, h5, h6, h7]
    .map((word) => word.toString(16).padStart(8, '0'))
    .join('');
}

export function normalizeCustomActionPrompt(prompt: string): string {
  return prompt.replace(/\r\n/g, '\n').trim();
}

export function computeCustomActionPromptHash(prompt: string): string {
  return sha256Hex(normalizeCustomActionPrompt(prompt));
}

export function isCustomActionWriteTrusted(action: CustomActionConfig): boolean {
  return Boolean(
    action.skipWriteConfirm
    && action.trustedPromptHash
    && action.trustedPromptHash === computeCustomActionPromptHash(action.prompt),
  );
}

export function sanitizeCustomActionConfig(raw: CustomActionConfig): CustomActionConfig {
  const id = raw.id.trim().slice(0, CUSTOM_ACTION_LIMITS.maxIdChars);
  const label = raw.label.trim().slice(0, CUSTOM_ACTION_LIMITS.maxLabelChars);
  const prompt = normalizeCustomActionPrompt(raw.prompt).slice(0, CUSTOM_ACTION_LIMITS.maxPromptChars);
  const templateKey = raw.templateKey?.trim()
    ? raw.templateKey.trim().slice(0, CUSTOM_ACTION_LIMITS.maxIdChars)
    : undefined;
  const expectedHash = computeCustomActionPromptHash(prompt);
  const trusted = Boolean(raw.skipWriteConfirm && raw.trustedPromptHash === expectedHash);
  return {
    id,
    ...(templateKey ? { templateKey } : {}),
    label,
    prompt,
    ...(trusted ? { skipWriteConfirm: true, trustedPromptHash: expectedHash } : {}),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function decodeCustomActionConfig(value: unknown): CustomActionConfig {
  if (!isRecord(value)
    || typeof value.id !== 'string' || !value.id.trim() || value.id.trim().length > CUSTOM_ACTION_LIMITS.maxIdChars
    || typeof value.label !== 'string' || !value.label.trim() || value.label.trim().length > CUSTOM_ACTION_LIMITS.maxLabelChars
    || typeof value.prompt !== 'string' || !value.prompt.trim() || value.prompt.trim().length > CUSTOM_ACTION_LIMITS.maxPromptChars
    || !(value.templateKey === undefined || value.templateKey === null || (typeof value.templateKey === 'string' && value.templateKey.trim().length <= CUSTOM_ACTION_LIMITS.maxIdChars))
    || !(value.skipWriteConfirm === undefined || value.skipWriteConfirm === null || typeof value.skipWriteConfirm === 'boolean')
    || !(value.trustedPromptHash === undefined || value.trustedPromptHash === null || typeof value.trustedPromptHash === 'string')) {
    throw new Error('Invalid custom action configuration.');
  }
  return sanitizeCustomActionConfig({
    id: value.id,
    ...(typeof value.templateKey === 'string' && value.templateKey.trim() ? { templateKey: value.templateKey } : {}),
    label: value.label,
    prompt: value.prompt,
    ...(typeof value.skipWriteConfirm === 'boolean' ? { skipWriteConfirm: value.skipWriteConfirm } : {}),
    ...(typeof value.trustedPromptHash === 'string' && value.trustedPromptHash ? { trustedPromptHash: value.trustedPromptHash } : {}),
  });
}

function decodeOptionalSnapshot(raw: unknown): GitRepositorySnapshot | undefined {
  if (raw === undefined || raw === null) return undefined;
  const decoded = decodeGitReadReply({
    protocolVersion: GIT_READ_PROTOCOL_VERSION,
    requestId: 'custom-action-snapshot',
    snapshot: raw,
  });
  if (!decoded.snapshot) throw new Error('Invalid custom action repository snapshot.');
  return decoded.snapshot;
}

function decodeLinks(raw: unknown): CustomActionLink[] {
  if (!Array.isArray(raw)) return [];
  const links: CustomActionLink[] = [];
  for (const item of raw) {
    if (!isRecord(item) || typeof item.label !== 'string' || typeof item.url !== 'string') continue;
    const label = item.label.trim();
    const url = item.url.trim();
    if (label && /^https?:\/\//i.test(url)) {
      links.push({ label, url });
    }
  }
  return links;
}

function decodeTimeline(raw: unknown): CustomActionTimelineItem[] {
  if (!Array.isArray(raw)) throw new Error('Invalid custom action timeline.');
  return raw.map((item) => {
    if (!isRecord(item)
      || (item.state !== 'done' && item.state !== 'active' && item.state !== 'failed')
      || typeof item.label !== 'string') {
      throw new Error('Invalid custom action timeline item.');
    }
    return { state: item.state, label: item.label };
  });
}

function decodeCommits(raw: unknown): CPushLandedCommit[] {
  if (!Array.isArray(raw)) throw new Error('Invalid custom action commits list.');
  return raw.map((item) => {
    if (!isRecord(item)
      || typeof item.oid !== 'string'
      || typeof item.shortOid !== 'string'
      || typeof item.title !== 'string'
      || typeof item.branch !== 'string'
      || !Array.isArray(item.fileIds)
      || !item.fileIds.every((id) => typeof id === 'string')) {
      throw new Error('Invalid custom action commit item.');
    }
    return {
      oid: item.oid,
      shortOid: item.shortOid,
      title: item.title,
      branch: item.branch,
      fileIds: item.fileIds as string[],
    };
  });
}

function decodeTrustedActionUpdate(raw: unknown): TrustedActionUpdate | undefined {
  if (raw === undefined || raw === null) return undefined;
  if (!isRecord(raw)
    || typeof raw.repositoryId !== 'string' || !raw.repositoryId
    || typeof raw.actionId !== 'string' || !raw.actionId
    || typeof raw.trustedPromptHash !== 'string' || !raw.trustedPromptHash) {
    throw new Error('Invalid trustedActionUpdate field.');
  }
  return {
    repositoryId: raw.repositoryId,
    actionId: raw.actionId,
    trustedPromptHash: raw.trustedPromptHash,
  };
}

function decodeUiMessage(raw: unknown): CustomActionUiMessage | null {
  if (raw === undefined || raw === null) return null;
  if (!isRecord(raw)
    || typeof raw.id !== 'string' || !raw.id
    || typeof raw.kind !== 'string'
    || typeof raw.title !== 'string'
    || typeof raw.description !== 'string') {
    throw new Error('Invalid custom action message payload.');
  }
  const base = raw as unknown as CustomActionUiMessage;
  const card = decodeMessageUserCard(raw.card);
  return {
    ...base,
    ...(card ? { card } : { card: undefined }),
  };
}

export function decodeCustomActionProgressEvent(value: unknown): CustomActionProgressEvent {
  if (!isRecord(value)
    || value.protocolVersion !== CUSTOM_ACTION_PROTOCOL_VERSION
    || typeof value.runId !== 'string' || !value.runId
    || typeof value.repositoryId !== 'string' || !value.repositoryId
    || typeof value.worktreeId !== 'string' || !value.worktreeId
    || typeof value.sessionId !== 'string' || !value.sessionId
    || typeof value.actionId !== 'string' || !value.actionId
    || typeof value.actionLabel !== 'string'
    || !['running', 'needsDecision', 'done', 'cancelled', 'failed'].includes(String(value.status))) {
    throw new Error('Custom action progress event protocol mismatch.');
  }
  const repositorySnapshot = decodeOptionalSnapshot(value.repositorySnapshot);
  const trustedActionUpdate = decodeTrustedActionUpdate(value.trustedActionUpdate);
  return {
    protocolVersion: CUSTOM_ACTION_PROTOCOL_VERSION,
    runId: value.runId,
    repositoryId: value.repositoryId,
    worktreeId: value.worktreeId,
    sessionId: value.sessionId,
    actionId: value.actionId,
    actionLabel: value.actionLabel,
    status: value.status as CustomActionProgressEvent['status'],
    ...(typeof value.pendingCallId === 'string' && value.pendingCallId ? { pendingCallId: value.pendingCallId } : {}),
    message: decodeUiMessage(value.message),
    timeline: decodeTimeline(value.timeline),
    commits: decodeCommits(value.commits),
    links: decodeLinks(value.links),
    ...(trustedActionUpdate ? { trustedActionUpdate } : {}),
    ...(repositorySnapshot ? { repositorySnapshot } : {}),
  };
}

export function decodeCustomActionCommandReply(value: unknown): CustomActionCommandReply {
  if (!isRecord(value)
    || value.protocolVersion !== CUSTOM_ACTION_PROTOCOL_VERSION
    || value.kind !== 'customAction'
    || typeof value.requestId !== 'string' || !value.requestId
    || typeof value.runId !== 'string' || !value.runId
    || typeof value.repositoryId !== 'string' || !value.repositoryId
    || typeof value.worktreeId !== 'string' || !value.worktreeId
    || typeof value.sessionId !== 'string' || !value.sessionId
    || typeof value.actionId !== 'string' || !value.actionId
    || typeof value.actionLabel !== 'string'
    || typeof value.summary !== 'string'
    || !['done', 'cancelled', 'failed'].includes(String(value.status))) {
    throw new Error('Custom action reply protocol mismatch.');
  }
  const repositorySnapshot = decodeOptionalSnapshot(value.repositorySnapshot);
  const trustedActionUpdate = decodeTrustedActionUpdate(value.trustedActionUpdate);
  return {
    protocolVersion: CUSTOM_ACTION_PROTOCOL_VERSION,
    kind: 'customAction',
    requestId: value.requestId,
    runId: value.runId,
    repositoryId: value.repositoryId,
    worktreeId: value.worktreeId,
    sessionId: value.sessionId,
    actionId: value.actionId,
    actionLabel: value.actionLabel,
    status: value.status as CustomActionCommandReply['status'],
    summary: value.summary,
    message: decodeUiMessage(value.message),
    timeline: decodeTimeline(value.timeline),
    commits: decodeCommits(value.commits),
    links: decodeLinks(value.links),
    ...(trustedActionUpdate ? { trustedActionUpdate } : {}),
    ...(repositorySnapshot ? { repositorySnapshot } : {}),
  };
}

export function decodeGenerateCustomActionProgressEvent(value: unknown): GenerateCustomActionProgressEvent {
  if (!isRecord(value)
    || value.protocolVersion !== CUSTOM_ACTION_PROTOCOL_VERSION
    || typeof value.runId !== 'string' || !value.runId
    || typeof value.statusLabel !== 'string'
    || !['running', 'needsDecision', 'done', 'cancelled', 'failed'].includes(String(value.status))) {
    throw new Error('Generate custom action progress event mismatch.');
  }
  const card = decodeMessageUserCard(value.card);
  return {
    protocolVersion: CUSTOM_ACTION_PROTOCOL_VERSION,
    runId: value.runId,
    status: value.status as GenerateCustomActionProgressEvent['status'],
    statusLabel: value.statusLabel,
    ...(typeof value.pendingCallId === 'string' && value.pendingCallId ? { pendingCallId: value.pendingCallId } : {}),
    ...(card ? { card } : {}),
    ...(value.questionnaire !== undefined && value.questionnaire !== null
      ? { questionnaire: value.questionnaire as MessageUserRequest }
      : {}),
  };
}

export function decodeGenerateCustomActionReply(value: unknown): GenerateCustomActionReply {
  if (!isRecord(value)
    || value.protocolVersion !== CUSTOM_ACTION_PROTOCOL_VERSION
    || value.kind !== 'generateCustomAction'
    || typeof value.requestId !== 'string' || !value.requestId
    || (value.status !== 'done' && value.status !== 'cancelled')) {
    throw new Error('Generate custom action reply protocol mismatch.');
  }
  let draft: CustomActionDraft | undefined;
  if (value.draft !== undefined && value.draft !== null) {
    if (!isRecord(value.draft)
      || typeof value.draft.label !== 'string' || !value.draft.label.trim()
      || typeof value.draft.prompt !== 'string' || !value.draft.prompt.trim()
      || typeof value.draft.summary !== 'string') {
      throw new Error('Invalid generated custom action draft.');
    }
    draft = {
      label: value.draft.label.trim().slice(0, CUSTOM_ACTION_LIMITS.maxLabelChars),
      prompt: normalizeCustomActionPrompt(value.draft.prompt).slice(0, CUSTOM_ACTION_LIMITS.maxPromptChars),
      summary: value.draft.summary.trim(),
    };
  }
  return {
    protocolVersion: CUSTOM_ACTION_PROTOCOL_VERSION,
    kind: 'generateCustomAction',
    requestId: value.requestId,
    status: value.status,
    ...(draft ? { draft } : {}),
  };
}

export function decodeActionSessionSnapshot(value: unknown): ActionSessionSnapshot {
  if (!isRecord(value)
    || typeof value.runId !== 'string' || !value.runId
    || typeof value.repositoryId !== 'string' || !value.repositoryId
    || typeof value.worktreeId !== 'string' || !value.worktreeId
    || typeof value.actionId !== 'string' || !value.actionId
    || typeof value.actionLabel !== 'string'
    || typeof value.summary !== 'string'
    || !['interrupted', 'done', 'cancelled', 'failed'].includes(String(value.status))) {
    throw new Error('Action session snapshot protocol mismatch.');
  }
  return {
    runId: value.runId,
    repositoryId: value.repositoryId,
    worktreeId: value.worktreeId,
    actionId: value.actionId,
    actionLabel: value.actionLabel,
    status: value.status as ActionSessionSnapshot['status'],
    summary: value.summary,
    message: decodeUiMessage(value.message),
    timeline: decodeTimeline(value.timeline),
    commits: decodeCommits(value.commits),
    links: decodeLinks(value.links),
  };
}
