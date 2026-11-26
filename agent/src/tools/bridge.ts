import type { PiToolResultCommand, ProbeEvent, RequestContext } from '../../../shared/pi-protocol';

interface PendingToolCall {
  resolve: (result: PiToolResultCommand) => void;
  reject: (error: Error) => void;
}

export class ToolBridge {
  private readonly pending = new Map<string, PendingToolCall>();
  private counter = 0;
  private aborted = false;

  constructor(
    private readonly envelope: { requestId: string; context: RequestContext },
    private readonly emit: (event: ProbeEvent) => void,
  ) {}

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
