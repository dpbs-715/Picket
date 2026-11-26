import { createInterface } from 'node:readline';
import {
  PI_CORE_VERSION,
  PI_PROTOCOL_VERSION,
  decodeCommand,
  type PiEvent,
  type PiToolResultCommand,
} from '../../shared/pi-protocol';
import { createTaskRunner } from './tasks';

const emit = (event: PiEvent) => { process.stdout.write(`${JSON.stringify(event)}\n`); };
const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
const seenRequests = new Set<string>();
let active: {
  requestId: string;
  cancel: () => void;
  deliverToolResult?: (result: PiToolResultCommand) => void;
  finished: Promise<void>;
} | undefined;
let closing = false;
const SHUTDOWN_DEADLINE_MS = 2_000;

async function shutdown() {
  if (closing) return;
  closing = true;
  input.close();
  process.stdin.pause();
  // Parent EOF must also bound shutdown when a future provider ignores cancellation.
  const deadline = setTimeout(() => process.exit(1), SHUTDOWN_DEADLINE_MS);
  deadline.unref();
  active?.cancel();
  await active?.finished;
  clearTimeout(deadline);
}

input.on('line', (line) => {
  if (closing) return;
  try {
    const command = decodeCommand(line);
    if (command.type === 'shutdown') { void shutdown(); return; }
    if (command.type === 'cancel') {
      if (active?.requestId === command.requestId) active.cancel();
      return;
    }
    if (command.type === 'toolResult') {
      if (active?.requestId === command.requestId) {
        active.deliverToolResult?.(command);
      }
      return;
    }
    if (active || seenRequests.has(command.requestId)) throw new Error('Probe is busy or requestId was already accepted');
    seenRequests.add(command.requestId);
    const task = createTaskRunner(command, emit);
    // Defer execution until the cancellation and tool-result handles are installed.
    const finished = Promise.resolve().then(() => task.run()).finally(() => { active = undefined; });
    active = {
      requestId: command.requestId,
      cancel: task.cancel,
      deliverToolResult: task.deliverToolResult,
      finished,
    };
  } catch (error) {
    emit({ type: 'protocolError', message: error instanceof Error ? error.message : 'Invalid command' });
  }
});
input.on('close', () => { void shutdown(); });
process.on('SIGTERM', () => { void shutdown(); });
process.on('SIGINT', () => { void shutdown(); });
process.stdout.on('error', () => { void shutdown(); });

emit({ type: 'ready', protocolVersion: PI_PROTOCOL_VERSION, piCoreVersion: PI_CORE_VERSION,
  nodeVersion: process.versions.node, pid: process.pid });
