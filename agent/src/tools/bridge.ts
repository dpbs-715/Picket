import type { PiActionLink, PiToolResultCommand, ProbeEvent, RequestContext } from '../../../shared/pi-protocol';

export interface HostSettledOutcome {
  summary: string;
  links: PiActionLink[];
}

interface PendingToolCall {
  resolve: (result: PiToolResultCommand) => void;
  reject: (error: Error) => void;
}

function extractHostSettledOutcome(result: PiToolResultCommand): HostSettledOutcome | undefined {
  if (!result.ok || result.isError || typeof result.details !== 'object' || result.details === null) {
    return undefined;
  }
  const details = result.details as Record<string, unknown>;
  if (details.stopAgent !== true) return undefined;
  const summary = typeof details.summary === 'string' && details.summary.trim()
    ? details.summary.trim().slice(0, 1200)
    : result.content.trim().slice(0, 1200);
  const links: PiActionLink[] = [];
  if (Array.isArray(details.links)) {
    for (const item of details.links) {
      if (typeof item !== 'object' || item === null || Array.isArray(item)) continue;
      const entry = item as Record<string, unknown>;
      const label = typeof entry.label === 'string' ? entry.label.trim().slice(0, 80) : '';
      const url = typeof entry.url === 'string' ? entry.url.trim() : '';
      if (label && /^https?:\/\//i.test(url)) {
        links.push({ label, url });
      }
    }
  }
  return { summary, links };
}

export class ToolBridge {
  private readonly pending = new Map<string, PendingToolCall>();
  private counter = 0;
  private aborted = false;
  private settledOutcome: HostSettledOutcome | undefined;

  constructor(
    private readonly envelope: { requestId: string; context: RequestContext },
    private readonly emit: (event: ProbeEvent) => void,
  ) {}

  getSettledOutcome(): HostSettledOutcome | undefined {
    return this.settledOutcome;
  }

  invokeHostTool(
    toolName: string,
    args: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<PiToolResultCommand> {
    if (this.aborted || signal?.aborted) {
      return Promise.reject(new Error('Tool invocation aborted'));
    }
    this.counter += 1;
    const callId = `tc-${this.counter}`;
    return new Promise<PiToolResultCommand>((resolve, reject) => {
      const onAbort = () => {
        if (this.pending.delete(callId)) {
          reject(new Error('Tool invocation aborted'));
        }
      };
      if (signal) {
        signal.addEventListener('abort', onAbort, { once: true });
      }
      this.pending.set(callId, {
        resolve: (result) => {
          signal?.removeEventListener('abort', onAbort);
          resolve(result);
        },
        reject: (err) => {
          signal?.removeEventListener('abort', onAbort);
          reject(err);
        },
      });
      this.emit({
        ...this.envelope,
        type: 'toolCall',
        callId,
        toolName,
        args,
      });
    });
  }

  emitProgress(callId: string, toolName: string, status: string): void {
    if (this.aborted) return;
    this.emit({
      ...this.envelope,
      type: 'toolProgress',
      callId,
      toolName,
      status,
    });
  }

  deliverToolResult(result: PiToolResultCommand): boolean {
    const entry = this.pending.get(result.callId);
    if (!entry) return false;
    this.pending.delete(result.callId);
    const settled = extractHostSettledOutcome(result);
    if (settled) {
      this.settledOutcome = settled;
    }
    entry.resolve(result);
    return true;
  }

  abortAll(reason = 'Task cancelled'): void {
    this.aborted = true;
    const error = new Error(reason);
    for (const entry of this.pending.values()) {
      entry.reject(error);
    }
    this.pending.clear();
  }
}
