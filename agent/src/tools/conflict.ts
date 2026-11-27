import type { AgentTool } from '@earendil-works/pi-agent-core';
import type { ToolBridge } from './bridge';

const CONFLICT_SECTIONS = new Set(['conflicted', 'base', 'upstream', 'local']);

export function createReadConflictFileTool(
  bridge: ToolBridge,
): AgentTool<any, { path: string; section: string }> {
  return {
    name: 'read_conflict_file',
    label: 'Read Conflicted File',
    description: 'Read a specific section (`conflicted`, `base`, `upstream`, or `local`) of an authorized conflicted file during rebase conflict resolution. Supports optional 1-based `startLine` and `endLine` pagination for large files.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['path'],
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative path (or exact `fileId`) of one of the authorized conflicted files.',
        },
        section: {
          type: 'string',
          enum: ['conflicted', 'base', 'upstream', 'local'],
          description: 'Which version to read: `conflicted` (working tree with conflict markers, default), `base` (common ancestor), `upstream` (onto branch), or `local` (local commit being rebased).',
        },
        startLine: {
          type: 'integer',
          minimum: 1,
          description: 'Optional 1-based starting line number.',
        },
        endLine: {
          type: 'integer',
          minimum: 1,
          description: 'Optional 1-based inclusive ending line number.',
        },
      },
    },
    executionMode: 'parallel',
    async execute(_toolCallId, params, signal) {
      const path = typeof params?.path === 'string' ? params.path.trim() : '';
      const rawSection = typeof params?.section === 'string' ? params.section.trim() : 'conflicted';
      const section = CONFLICT_SECTIONS.has(rawSection) ? rawSection : 'conflicted';
      if (!path) {
        return {
          content: [{ type: 'text', text: 'read_conflict_file requires a non-empty `path`.' }],
          details: { path, section },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = { path, section };
      if (typeof params?.startLine === 'number' && Number.isSafeInteger(params.startLine) && params.startLine >= 1) {
        payload.startLine = params.startLine;
      }
      if (typeof params?.endLine === 'number' && Number.isSafeInteger(params.endLine) && params.endLine >= 1) {
        payload.endLine = params.endLine;
      }
      const response = await bridge.invokeHostTool('read_conflict_file', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { path, section },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createWriteConflictResolutionTool(
  bridge: ToolBridge,
): AgentTool<any, { path: string; stopAgent?: boolean }> {
  return {
    name: 'write_conflict_resolution',
    label: 'Write Conflict Resolution',
    description: 'Submit the complete resolved UTF-8 content for an authorized conflicted file. Rust validates zero leftover conflict markers and non-empty content; once every conflicted file in the active batch is resolved, Rust applies all resolutions and continues the rebase automatically.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['path', 'content'],
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative path (or exact `fileId`) of the authorized conflicted file.',
        },
        content: {
          type: 'string',
          description: 'Complete resolved file content with zero leftover conflict markers.',
        },
        summary: {
          type: 'string',
          description: 'Optional concise merge summary describing how the conflicts were resolved.',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const path = typeof params?.path === 'string' ? params.path.trim() : '';
      const content = typeof params?.content === 'string' ? params.content : null;
      if (!path || content === null) {
        return {
          content: [{ type: 'text', text: 'write_conflict_resolution requires `path` and string `content`.' }],
          details: { path },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = { path, content };
      if (typeof params?.summary === 'string' && params.summary.trim()) {
        payload.summary = params.summary.trim();
      }
      const response = await bridge.invokeHostTool('write_conflict_resolution', payload, signal);
      const stopAgent = response.ok
        && !response.isError
        && typeof response.details === 'object'
        && response.details !== null
        && (response.details as Record<string, unknown>).stopAgent === true;
      return {
        content: [{ type: 'text', text: response.content }],
        details: { path, ...(stopAgent ? { stopAgent: true } : {}) },
        isError: response.isError || !response.ok,
        ...(stopAgent ? { terminate: true } : {}),
      };
    },
  };
}
