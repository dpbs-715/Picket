import type { AgentTool } from '@earendil-works/pi-agent-core';
import type { ToolBridge } from './bridge';

export function createHandoffToRecoveryTool(
  bridge: ToolBridge,
): AgentTool<any, { handleId: string; stopAgent?: boolean }> {
  return {
    name: 'handoff_to_recovery',
    label: 'Hand Off to Host Recovery',
    description:
      'Hand off a verified push failure, missing upstream, or rebase conflict to Picket\'s zero-token Host Recovery UI (Retry Push / Set Upstream / Pull & Rebase / AI Conflict Merge / Manual Merge). Pass the exact `handleId` from `recoveryHandle` returned by `commit_exact_scope` or `sync_and_push` in this run. Rust verifies the handle against live Git state before terminating the run.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['handleId'],
      properties: {
        handleId: {
          type: 'string',
          description: 'Exact `recoveryHandle.handleId` issued by `commit_exact_scope` or `sync_and_push` in this run.',
        },
        summary: {
          type: 'string',
          description: 'Optional concise explanation in the UI language summarizing why recovery is being handed to the user.',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const handleId = typeof params?.handleId === 'string' ? params.handleId.trim() : '';
      if (!handleId) {
        return {
          content: [{ type: 'text', text: 'handoff_to_recovery requires a non-empty `handleId` from a previous tool result.' }],
          details: { handleId: '' },
          isError: true,
        };
      }
      const summary = typeof params?.summary === 'string' && params.summary.trim()
        ? params.summary.trim().slice(0, 300)
        : undefined;
      const payload: Record<string, unknown> = {
        handleId,
        ...(summary ? { summary } : {}),
      };
      const response = await bridge.invokeHostTool('handoff_to_recovery', payload, signal);
      const stopAgent = response.ok
        && !response.isError
        && typeof response.details === 'object'
        && response.details !== null
        && (response.details as Record<string, unknown>).stopAgent === true;
      return {
        content: [{ type: 'text', text: response.content }],
        details: {
          handleId,
          ...(stopAgent ? { stopAgent: true } : {}),
        },
        isError: response.isError || !response.ok,
        ...(stopAgent ? { terminate: true } : {}),
      };
    },
  };
}
