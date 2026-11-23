export const PI_PROTOCOL_VERSION = 1;
export const PI_CORE_VERSION = '1.6.0';
export const FOUNDATION_SCOPE_VERSION = 'foundation-v1';
export const MAX_PROTOCOL_LINE_BYTES = 64 * 1024;

export interface RequestContext {
  repositoryId: string;
  worktreeId: string;
  sessionId: string;
  scopeVersion: string;
}

export interface ProbeRequest {
  type: 'probe';
  requestId: string;
  context: RequestContext;
}

export type PiCommand = ProbeRequest
  | { type: 'cancel'; requestId: string }
  | { type: 'shutdown' };

export type ProbeEvent = {
  requestId: string;
  context: RequestContext;
} & (
  | { type: 'started' }
  | { type: 'textDelta'; delta: string }
  | { type: 'completed'; text: string }
  | { type: 'cancelled' }
  | { type: 'failed'; message: string }
);

export type PiEvent = ProbeEvent
  | { type: 'ready'; protocolVersion: number; piCoreVersion: string; nodeVersion: string; pid: number }
  | { type: 'protocolError'; message: string };

export function decodeCommand(line: string): PiCommand {
  if (Buffer.byteLength(line) > MAX_PROTOCOL_LINE_BYTES) throw new Error('Protocol line is too large');
  const command: unknown = JSON.parse(line);
  if (!isRecord(command)) throw new Error('Expected a command object');
  if (command.type === 'shutdown') return { type: 'shutdown' };
  if (typeof command.requestId !== 'string' || !/^[a-zA-Z0-9-]{1,128}$/.test(command.requestId)) {
    throw new Error('Invalid requestId');
  }
  if (command.type === 'cancel') return { type: 'cancel', requestId: command.requestId };
  if (command.type !== 'probe' || !isRecord(command.context)) throw new Error('Unknown command');
  const context = command.context;
  if (context.repositoryId !== 'foundation' || context.worktreeId !== 'temporary'
    || context.sessionId !== command.requestId || context.scopeVersion !== FOUNDATION_SCOPE_VERSION) {
    throw new Error('M0 accepts only the isolated foundation context');
  }
  return {
    type: 'probe', requestId: command.requestId,
    context: { repositoryId: 'foundation', worktreeId: 'temporary', sessionId: command.requestId,
      scopeVersion: FOUNDATION_SCOPE_VERSION },
  };
}

function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === 'object' && input !== null && !Array.isArray(input);
}
