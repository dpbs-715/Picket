import type { EditorKind } from './generated';
import type { GitReadTarget } from './git-read-protocol';
import type { AppLocale } from './language-protocol';

export const IDE_PROTOCOL_VERSION = 1;

export type GitEditorId = EditorKind;
export type GitOpenFileAction = 'open' | 'goto' | 'diff' | 'stagedDiff' | 'merge';

export interface GitOpenFileRequest {
  requestId: string;
  target: GitReadTarget;
  fileId?: string;
  path?: string;
  editor: GitEditorId;
  line?: number;
  locale?: AppLocale;
}

export interface GitOpenFileReply {
  protocolVersion: typeof IDE_PROTOCOL_VERSION;
  kind: 'openFile';
  requestId: string;
  target: GitReadTarget;
  editor: GitEditorId;
  filePath: string;
  action: GitOpenFileAction;
  line?: number;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}
function nonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0;
}

export function isGitEditorId(value: unknown): value is GitEditorId {
  return value === 'idea' || value === 'rustrover' || value === 'webstorm' || value === 'vscode' || value === 'cursor';
}

export function decodeGitOpenFileReply(value: unknown): GitOpenFileReply {
  const fail = (): never => {
    throw new Error('Desktop editor protocol mismatch. Please restart the app and try again.');
  };
  const target = record(value) ? value.target : undefined;
  if (!record(value) || value.protocolVersion !== IDE_PROTOCOL_VERSION || value.kind !== 'openFile'
    || !nonEmptyString(value.requestId) || !record(target)
    || !(['repositoryId', 'worktreeId', 'sessionId', 'scopeVersion'] as const).every((key) => typeof target[key] === 'string')
    || !isGitEditorId(value.editor) || !nonEmptyString(value.filePath)
    || !['open', 'goto', 'diff', 'stagedDiff', 'merge'].includes(value.action as string)
    || (value.line !== undefined && (!Number.isSafeInteger(value.line) || (value.line as number) < 1))) {
    return fail();
  }
  return value as unknown as GitOpenFileReply;
}
