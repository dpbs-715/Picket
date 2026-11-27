import type { BeforeToolCallResult } from '@earendil-works/pi-agent-core';

export type ToolLoopGuardPolicy =
  | { kind: 'none' }
  | { kind: 'fixed'; maxCalls: number }
  | {
      kind: 'adaptive';
      maxIdenticalConsecutive: number;
      maxConsecutiveErrors: number;
      runawayCeiling: number;
    };

export const CUSTOM_ACTION_GUARD_DEFAULTS = {
  maxIdenticalConsecutive: 3,
  maxConsecutiveErrors: 8,
  runawayCeiling: 300,
} as const;

const OVERHEAD_TOOLS = new Set([
  'read_skill_doc',
  'record_action_checkpoint',
  'message_user',
]);

function normalizeValue(value: unknown, depth = 0): unknown {
  if (depth > 5) return '[…]';
  if (typeof value === 'string') {
    return value.length > 512 ? `${value.slice(0, 512)}…(${value.length})` : value;
  }
  if (Array.isArray(value)) {
    return value.slice(0, 40).map((item) => normalizeValue(item, depth + 1));
  }
  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
      .sort(([a], [b]) => a.localeCompare(b))
      .slice(0, 40)
      .map(([k, v]) => [k, normalizeValue(v, depth + 1)]);
    return Object.fromEntries(entries);
  }
  return value;
}

export function stableToolCallSignature(toolName: string, args: unknown): string {
  try {
    return `${toolName}:${JSON.stringify(normalizeValue(args))}`;
  } catch {
    return `${toolName}:<unserializable>`;
  }
}

export interface ToolLoopGuard {
  beforeToolCall: (ctx: { toolCall: { name: string }; args: unknown }) => Promise<BeforeToolCallResult | undefined>;
  afterToolCall: (ctx: { toolCall: { name: string }; isError: boolean }) => Promise<undefined>;
}

export function createToolLoopGuard(policy: ToolLoopGuardPolicy): ToolLoopGuard | undefined {
  if (policy.kind === 'none') return undefined;

  if (policy.kind === 'fixed') {
    let executed = 0;
    return {
      async beforeToolCall() {
        if (executed >= policy.maxCalls) {
          return {
            block: true,
            reason: `Tool call budget (${policy.maxCalls} calls) reached. Do not call more tools; return your final JSON response now using the evidence already collected.`,
          };
        }
        executed += 1;
        return undefined;
      },
      async afterToolCall() {
        return undefined;
      },
    };
  }

  let executionCalls = 0;
  let lastSignature = '';
  let identicalStreak = 0;
  let consecutiveErrors = 0;
  let identicalBlockCount = 0;

  return {
    async beforeToolCall(ctx) {
      const toolName = ctx.toolCall.name;

      // Always allow communicating with the user or recording verified checkpoints so the run can recover cleanly.
      if (toolName === 'message_user' || toolName === 'record_action_checkpoint') {
        lastSignature = '';
        identicalStreak = 0;
        identicalBlockCount = 0;
        return undefined;
      }

      if (executionCalls >= policy.runawayCeiling) {
        return {
          block: true,
          reason: `Safety ceiling (${policy.runawayCeiling} non-overhead tool calls) reached. Save a checkpoint with \`record_action_checkpoint\` if needed and return your concise final JSON summary now.`,
        };
      }

      if (consecutiveErrors >= policy.maxConsecutiveErrors) {
        return {
          block: true,
          reason: `Stopped after ${consecutiveErrors} consecutive failing tool calls. Do not keep retrying blindly; either ask the user for guidance via \`message_user\` or return your final JSON summary explaining the blocker.`,
        };
      }

      const signature = stableToolCallSignature(toolName, ctx.args);
      if (signature === lastSignature) {
        identicalStreak += 1;
      } else {
        lastSignature = signature;
        identicalStreak = 1;
        identicalBlockCount = 0;
      }

      if (identicalStreak > policy.maxIdenticalConsecutive) {
        identicalBlockCount += 1;
        if (identicalBlockCount >= 3) {
          return {
            block: true,
            reason: `Repeated loop guard violation on \`${toolName}\`. Stop calling tools and return your final JSON summary now (or ask the user via \`message_user\`).`,
          };
        }
        return {
          block: true,
          reason: `Loop detected: \`${toolName}\` was already called ${policy.maxIdenticalConsecutive} times in a row with identical arguments. Change your arguments or approach, inspect the previous output, or ask the user via \`message_user\`.`,
        };
      }

      if (!OVERHEAD_TOOLS.has(toolName)) {
        executionCalls += 1;
      }
      return undefined;
    },

    async afterToolCall(ctx) {
      const toolName = ctx.toolCall.name;
      if (toolName === 'message_user' || toolName === 'record_action_checkpoint') {
        if (!ctx.isError) {
          consecutiveErrors = 0;
        }
        return undefined;
      }
      if (ctx.isError) {
        consecutiveErrors += 1;
      } else {
        consecutiveErrors = 0;
      }
      return undefined;
    },
  };
}
