import type {
  CPushLandedCommit as WorkflowLandedCommit,
  WorkflowLink,
  WorkflowMessageCommitSummary,
  WorkflowMessageFileChip,
  WorkflowMessageOption,
  WorkflowMessageProgress,
  WorkflowTimelineItem,
  WorkflowUiMessage,
} from './generated';
import {
  decodeMessageUserCard,
  type MessageUserRequest,
} from './message-user-protocol';

export type WorkflowMessageKind = WorkflowUiMessage['kind'];
export type {
  WorkflowLandedCommit,
  WorkflowLink,
  WorkflowMessageCommitSummary,
  WorkflowMessageFileChip,
  WorkflowMessageOption,
  WorkflowMessageProgress,
  WorkflowTimelineItem,
  WorkflowUiMessage,
};

const VALID_WORKFLOW_MESSAGE_KINDS: readonly WorkflowMessageKind[] = [
  'progress',
  'question',
  'conflict',
  'waitingManual',
  'resolved',
  'pushFailed',
  'done',
  'error',
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

export function decodeWorkflowLinks(raw: unknown): WorkflowLink[] {
  if (!Array.isArray(raw)) return [];
  const links: WorkflowLink[] = [];
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

export function decodeWorkflowTimeline(raw: unknown, contextLabel = 'workflow'): WorkflowTimelineItem[] {
  if (!Array.isArray(raw)) throw new Error(`Invalid ${contextLabel} timeline.`);
  return raw.map((item) => {
    if (!isRecord(item)
      || (item.state !== 'done' && item.state !== 'active' && item.state !== 'failed')
      || typeof item.label !== 'string') {
      throw new Error(`Invalid ${contextLabel} timeline entry.`);
    }
    return { state: item.state, label: item.label };
  });
}

export function decodeWorkflowCommits(raw: unknown, contextLabel = 'workflow'): WorkflowLandedCommit[] {
  if (!Array.isArray(raw)) throw new Error(`Invalid ${contextLabel} commits list.`);
  return raw.map((entry) => {
    if (!isRecord(entry)
      || typeof entry.oid !== 'string' || !entry.oid
      || typeof entry.shortOid !== 'string' || !entry.shortOid
      || typeof entry.title !== 'string'
      || typeof entry.branch !== 'string'
      || !isStringArray(entry.fileIds)) {
      throw new Error(`Invalid ${contextLabel} landed commit entry.`);
    }
    return {
      oid: entry.oid,
      shortOid: entry.shortOid,
      title: entry.title,
      branch: entry.branch,
      fileIds: entry.fileIds,
    };
  });
}

export function decodeWorkflowUiMessage(value: unknown, contextLabel = 'workflow'): WorkflowUiMessage {
  if (!isRecord(value)
    || typeof value.id !== 'string' || !value.id
    || !VALID_WORKFLOW_MESSAGE_KINDS.includes(value.kind as WorkflowMessageKind)
    || typeof value.title !== 'string'
    || typeof value.description !== 'string'
    || !(value.scope === undefined || value.scope === null || typeof value.scope === 'string')) {
    throw new Error(`Desktop returned an invalid ${contextLabel} message structure.`);
  }
  const files = value.files === undefined || value.files === null ? undefined : (() => {
    if (!Array.isArray(value.files)) throw new Error(`Invalid ${contextLabel} file reference list.`);
    return value.files.map((file) => {
      if (!isRecord(file)
        || typeof file.id !== 'string' || !file.id
        || typeof file.label !== 'string'
        || !(file.badge === undefined || file.badge === null || typeof file.badge === 'string')) {
        throw new Error(`Invalid ${contextLabel} file reference item.`);
      }
      return {
        id: file.id,
        label: file.label,
        ...(typeof file.badge === 'string' && file.badge ? { badge: file.badge } : {}),
      };
    });
  })();
  const commits = value.commits === undefined || value.commits === null ? undefined : (() => {
    if (!Array.isArray(value.commits)) throw new Error(`Invalid ${contextLabel} commit summary list.`);
    return value.commits.map((item) => {
      if (!isRecord(item) || typeof item.hash !== 'string' || typeof item.title !== 'string') {
        throw new Error(`Invalid ${contextLabel} commit summary item.`);
      }
      return { hash: item.hash, title: item.title };
    });
  })();
  const options = value.options === undefined || value.options === null ? undefined : (() => {
    if (!Array.isArray(value.options)) throw new Error(`Invalid ${contextLabel} option list.`);
    return value.options.map((opt) => {
      if (!isRecord(opt)
        || typeof opt.id !== 'string' || !opt.id
        || typeof opt.label !== 'string' || !opt.label
        || !(opt.recommended === undefined || typeof opt.recommended === 'boolean')) {
        throw new Error(`Invalid ${contextLabel} option item.`);
      }
      return {
        id: opt.id,
        label: opt.label,
        ...(typeof opt.recommended === 'boolean' ? { recommended: opt.recommended } : {}),
      };
    });
  })();
  const progress = value.progress === undefined || value.progress === null ? undefined : (() => {
    if (!isRecord(value.progress)
      || typeof value.progress.completed !== 'number'
      || typeof value.progress.total !== 'number'
      || typeof value.progress.label !== 'string') {
      throw new Error(`Invalid ${contextLabel} progress field.`);
    }
    return {
      completed: value.progress.completed,
      total: value.progress.total,
      label: value.progress.label,
    };
  })();
  const decodedLinks = decodeWorkflowLinks(value.links);
  const links = decodedLinks.length > 0 ? decodedLinks : undefined;
  const card = decodeMessageUserCard(value.card);
  const questionnaire = value.questionnaire !== undefined && value.questionnaire !== null
    ? (value.questionnaire as MessageUserRequest)
    : undefined;
  return {
    id: value.id,
    kind: value.kind as WorkflowMessageKind,
    title: value.title,
    description: value.description,
    ...(typeof value.scope === 'string' && value.scope ? { scope: value.scope } : {}),
    ...(files ? { files } : {}),
    ...(commits ? { commits } : {}),
    ...(links ? { links } : {}),
    ...(options ? { options } : {}),
    ...(card ? { card } : {}),
    ...(questionnaire ? { questionnaire } : {}),
    ...(progress ? { progress } : {}),
  };
}
