import type { AgentTool, AgentToolResult } from '@earendil-works/pi-agent-core';
import type { SkillRegistry } from '../skills';
import type { ToolBridge } from './bridge';

export function createReadSkillDocTool(registry: SkillRegistry): AgentTool<any, { skill: string; path: string; source: string }> {
  return {
    name: 'read_skill_doc',
    label: 'Read Skill Document',
    description: 'Read a Picket skill specification (`SKILL.md`) or one of its reference documents (`references/*.md`) from the active skill registry.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['skill'],
      properties: {
        skill: {
          type: 'string',
          description: 'Skill name from the available skills index (for example `dcr`, `commit`, `submit`, `review-pr`).',
        },
        path: {
          type: 'string',
          description: 'Relative path inside the skill directory. Defaults to `SKILL.md`; use e.g. `references/merge-chain.md` for reference docs.',
        },
      },
    },
    executionMode: 'parallel',
    async execute(_toolCallId, params) {
      const skill = typeof params?.skill === 'string' ? params.skill : '';
      const path = typeof params?.path === 'string' ? params.path : 'SKILL.md';
      try {
        const doc = await registry.readDoc(skill, path);
        return {
          content: [{ type: 'text', text: doc.content }],
          details: { skill: doc.skill, path: doc.path, source: doc.source },
        };
      } catch (error) {
        return {
          content: [{ type: 'text', text: error instanceof Error ? error.message : 'Failed to read skill document' }],
          details: { skill, path, source: 'error' },
          isError: true,
        };
      }
    },
  };
}

export function createReadRepoFileTool(bridge: ToolBridge): AgentTool<any, { path: string; rev?: string }> {
  return {
    name: 'read_repo_file',
    label: 'Read Repository File',
    description: 'Read a bounded line slice from a repository-relative text file in the active worktree (or at a Git revision such as `HEAD`). Use this to inspect surrounding code, callers, types, or tests.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['path'],
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative path (no leading slash, no `..`, never inside `.git`).',
        },
        startLine: {
          type: 'integer',
          minimum: 1,
          description: '1-based starting line number (default 1).',
        },
        endLine: {
          type: 'integer',
          minimum: 1,
          description: '1-based inclusive ending line number (capped to 250 lines per call).',
        },
        rev: {
          type: 'string',
          description: 'Optional Git revision such as `HEAD` to read the committed version instead of the working tree.',
        },
      },
    },
    executionMode: 'parallel',
    async execute(_toolCallId, params, signal): Promise<AgentToolResult<{ path: string; rev?: string }>> {
      const path = typeof params?.path === 'string' ? params.path.trim() : '';
      if (!path) {
        return {
          content: [{ type: 'text', text: 'Missing required `path` argument.' }],
          details: { path: '' },
          isError: true,
        };
      }
      const args: Record<string, unknown> = { path };
      if (typeof params?.startLine === 'number' && Number.isFinite(params.startLine)) {
        args.startLine = Math.max(1, Math.trunc(params.startLine));
      }
      if (typeof params?.endLine === 'number' && Number.isFinite(params.endLine)) {
        args.endLine = Math.max(1, Math.trunc(params.endLine));
      }
      if (typeof params?.rev === 'string' && params.rev.trim()) {
        args.rev = params.rev.trim();
      }
      const response = await bridge.invokeHostTool('read_repo_file', args, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { path, ...(typeof args.rev === 'string' ? { rev: args.rev } : {}) },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createReadScopeDiffTool(bridge: ToolBridge): AgentTool<any, { path: string }> {
  return {
    name: 'read_scope_diff',
    label: 'Read Authorized Scope Diff',
    description: 'Read a bounded line slice from the exact authorized unified diff of a selected file in the current review scope (for partially staged files, this returns ONLY the staged diff hunks). Use this whenever an inline patch was truncated or omitted.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['path'],
      properties: {
        path: {
          type: 'string',
          description: 'Exact repository-relative path of a file in the current review scope.',
        },
        startLine: {
          type: 'integer',
          minimum: 1,
          description: '1-based starting line number inside the unified diff (default 1).',
        },
        endLine: {
          type: 'integer',
          minimum: 1,
          description: '1-based inclusive ending line number inside the unified diff (capped to 250 lines per call).',
        },
      },
    },
    executionMode: 'parallel',
    async execute(_toolCallId, params, signal): Promise<AgentToolResult<{ path: string }>> {
      const path = typeof params?.path === 'string' ? params.path.trim() : '';
      if (!path) {
        return {
          content: [{ type: 'text', text: 'Missing required `path` argument.' }],
          details: { path: '' },
          isError: true,
        };
      }
      const args: Record<string, unknown> = { path };
      if (typeof params?.startLine === 'number' && Number.isFinite(params.startLine)) {
        args.startLine = Math.max(1, Math.trunc(params.startLine));
      }
      if (typeof params?.endLine === 'number' && Number.isFinite(params.endLine)) {
        args.endLine = Math.max(1, Math.trunc(params.endLine));
      }
      const response = await bridge.invokeHostTool('read_scope_diff', args, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { path },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createSearchRepoTool(bridge: ToolBridge): AgentTool<any, { query: string; pathPrefix?: string }> {
  return {
    name: 'search_repo',
    label: 'Search Repository',
    description: 'Search tracked repository files for a symbol name, call site, type definition, or text pattern (fixed-string by default; capped results).',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['query'],
      properties: {
        query: {
          type: 'string',
          description: 'Non-empty text or regex pattern to search for.',
        },
        pathPrefix: {
          type: 'string',
          description: 'Optional repository-relative file or directory prefix to narrow the search.',
        },
        isRegex: {
          type: 'boolean',
          description: 'Interpret `query` as an extended regular expression instead of a literal string (default false).',
        },
        caseSensitive: {
          type: 'boolean',
          description: 'Use case-sensitive matching (default true).',
        },
        maxResults: {
          type: 'integer',
          minimum: 1,
          maximum: 50,
          description: 'Maximum matching lines to return (default 30, max 50).',
        },
      },
    },
    executionMode: 'parallel',
    async execute(_toolCallId, params, signal): Promise<AgentToolResult<{ query: string; pathPrefix?: string }>> {
      const query = typeof params?.query === 'string' ? params.query : '';
      if (!query.trim()) {
        return {
          content: [{ type: 'text', text: 'Missing required non-empty `query` argument.' }],
          details: { query: '' },
          isError: true,
        };
      }
      const args: Record<string, unknown> = { query };
      if (typeof params?.pathPrefix === 'string' && params.pathPrefix.trim()) {
        args.pathPrefix = params.pathPrefix.trim();
      }
      if (typeof params?.isRegex === 'boolean') args.isRegex = params.isRegex;
      if (typeof params?.caseSensitive === 'boolean') args.caseSensitive = params.caseSensitive;
      if (typeof params?.maxResults === 'number' && Number.isFinite(params.maxResults)) {
        args.maxResults = Math.min(50, Math.max(1, Math.trunc(params.maxResults)));
      }
      const response = await bridge.invokeHostTool('search_repo', args, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { query, ...(typeof args.pathPrefix === 'string' ? { pathPrefix: args.pathPrefix } : {}) },
        isError: response.isError || !response.ok,
      };
    },
  };
}
