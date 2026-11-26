import type { AgentTool, AgentToolResult } from '@earendil-works/pi-agent-core';
import {
  formatAskUserQuestionResponseForModel,
  validateAskUserQuestionRequest,
  type AskUserQuestionResponse,
} from '../../shared/ask-user-protocol';
import type { PiToolResultCommand, ProbeEvent, RequestContext } from '../../shared/pi-protocol';
import type { SkillRegistry } from './skills';

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

export function createAskUserQuestionTool(bridge: ToolBridge): AgentTool<any, AskUserQuestionResponse | { cancelled: true; answers: []; errorCode: string }> {
  return {
    name: 'ask_user_question',
    label: 'Ask User Question',
    description: 'Present 1 to 4 structured questions in Picket\'s unified dialog when user input, scope grouping, or explicit authorization is required. Do not include an "Other" option; the dialog always provides free-text input.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['questions'],
      properties: {
        questions: {
          type: 'array',
          minItems: 1,
          maxItems: 4,
          items: {
            type: 'object',
            additionalProperties: false,
            required: ['header', 'question', 'options'],
            properties: {
              id: { type: 'string' },
              header: { type: 'string', description: 'Short tab/chip label (max 24 chars).' },
              question: { type: 'string', description: 'Full question text.' },
              multiSelect: { type: 'boolean', description: 'Allow selecting multiple options (default false).' },
              options: {
                type: 'array',
                minItems: 2,
                maxItems: 4,
                items: {
                  type: 'object',
                  additionalProperties: false,
                  required: ['label', 'description'],
                  properties: {
                    id: { type: 'string' },
                    label: { type: 'string', description: 'Concise choice label (never "Other" or "Cancel").' },
                    description: { type: 'string', description: 'Explanation of the choice or its trade-off.' },
                    recommended: { type: 'boolean' },
                    preview: { type: 'string', description: 'Optional inline preview text (single-select only).' },
                  },
                },
              },
            },
          },
        },
        context: {
          type: 'object',
          additionalProperties: false,
          properties: {
            scope: { type: 'string' },
            affectedFileIds: { type: 'array', items: { type: 'string' } },
            links: {
              type: 'array',
              items: {
                type: 'object',
                additionalProperties: false,
                required: ['label', 'url'],
                properties: {
                  label: { type: 'string' },
                  url: { type: 'string' },
                },
              },
            },
            allowDontAskAgain: { type: 'boolean' },
            dontAskAgainLabel: { type: 'string' },
          },
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const validation = validateAskUserQuestionRequest(params);
      if (!validation.ok) {
        return {
          content: [{ type: 'text', text: `Invalid ask_user_question arguments (${validation.code}): ${validation.message}` }],
          details: { cancelled: true, answers: [], errorCode: validation.code },
          isError: true,
        };
      }
      const response = await bridge.invokeHostTool('ask_user_question', validation.request as unknown as Record<string, unknown>, signal);
      if (!response.ok || response.isError) {
        return {
          content: [{ type: 'text', text: response.content || 'User dialog was unavailable.' }],
          details: { cancelled: true, answers: [], errorCode: 'host_error' },
          isError: true,
        };
      }
      let parsed: AskUserQuestionResponse | undefined;
      if (response.details && typeof response.details === 'object') {
        parsed = response.details as AskUserQuestionResponse;
      } else {
        try {
          parsed = JSON.parse(response.content) as AskUserQuestionResponse;
        } catch {
          parsed = undefined;
        }
      }
      if (!parsed || typeof parsed.cancelled !== 'boolean' || !Array.isArray(parsed.answers)) {
        return {
          content: [{ type: 'text', text: response.content }],
          details: { cancelled: true, answers: [], errorCode: 'invalid_host_response' },
          isError: true,
        };
      }
      const modelText = formatAskUserQuestionResponseForModel(validation.request, parsed);
      return {
        content: [{ type: 'text', text: modelText }],
        details: parsed,
      };
    },
  };
}

export function createCheckCliEnvTool(bridge: ToolBridge): AgentTool<any, { checked: boolean }> {
  return {
    name: 'check_cli_env',
    label: 'Check Local CLI Environment',
    description: 'Inspect local `git`, `gh`, `glab`, and `brew` installation paths, versions, head-SHA guard flag support, authentication status (tokens redacted), and configured repository remotes/platform.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      properties: {},
    },
    executionMode: 'parallel',
    async execute(_toolCallId, _params, signal) {
      const response = await bridge.invokeHostTool('check_cli_env', {}, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { checked: response.ok },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createInstallPlatformCliTool(bridge: ToolBridge): AgentTool<any, { tool: string }> {
  return {
    name: 'install_platform_cli',
    label: 'Install Platform CLI',
    description: 'Install `gh` or `glab` via Homebrew (`brew install gh` / `brew install glab`). Rust strictly requires that the user already approved this installation via `ask_user_question` in the current session.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['tool'],
      properties: {
        tool: {
          type: 'string',
          enum: ['gh', 'glab'],
          description: 'Whitelisted platform CLI to install (`gh` or `glab`).',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const tool = typeof params?.tool === 'string' ? params.tool.trim() : '';
      if (tool !== 'gh' && tool !== 'glab') {
        return {
          content: [{ type: 'text', text: 'Only `gh` and `glab` can be installed.' }],
          details: { tool },
          isError: true,
        };
      }
      const response = await bridge.invokeHostTool('install_platform_cli', { tool }, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { tool },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createRunGitTool(bridge: ToolBridge, readOnlyOnly = false): AgentTool<any, { args: string[]; worktreePath?: string }> {
  return {
    name: 'run_git',
    label: readOnlyOnly ? 'Run Read-Only Git Command' : 'Run Controlled Git Command',
    description: readOnlyOnly
      ? 'Execute a read-only Tier-0 Git inspection command (such as `status`, `branch --list`, `tag -l`, `log -n 10 --oneline`, `remote -v`) with discrete arguments (no shell).'
      : 'Execute a controlled Git command with discrete arguments (no shell). Enforces Picket\'s 3-tier policy and permanent redlines. Optionally pass `worktreePath` to run inside a Picket-owned isolated submit worktree. Use `commit_exact_scope` or `manage_submit_worktree` instead of raw `git add`/`git commit`.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['args'],
      properties: {
        args: {
          type: 'array',
          minItems: 1,
          items: { type: 'string' },
          description: 'Discrete argument vector passed directly to `git` (for example `["tag", "-a", "v0.2.0", "-m", "Release v0.2.0"]`).',
        },
        ...(readOnlyOnly
          ? {}
          : {
              worktreePath: {
                type: 'string',
                description: 'Optional absolute path of a Picket-owned isolated submit worktree returned by `manage_submit_worktree(create)`. Omit to run in the active checkout.',
              },
            }),
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const args = Array.isArray(params?.args)
        ? params.args.filter((item: unknown): item is string => typeof item === 'string')
        : [];
      if (args.length === 0) {
        return {
          content: [{ type: 'text', text: 'Missing required non-empty `args` array.' }],
          details: { args: [] },
          isError: true,
        };
      }
      const worktreePath = !readOnlyOnly && typeof params?.worktreePath === 'string' && params.worktreePath.trim()
        ? params.worktreePath.trim()
        : undefined;
      const payload: Record<string, unknown> = {
        args,
        ...(worktreePath ? { worktreePath } : {}),
      };
      const response = await bridge.invokeHostTool('run_git', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { args, ...(worktreePath ? { worktreePath } : {}) },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createRunPlatformCliTool(bridge: ToolBridge): AgentTool<any, { cli: string; args: string[] }> {
  return {
    name: 'run_platform_cli',
    label: 'Run GitHub/GitLab CLI',
    description: 'Execute local `gh` or `glab` with discrete arguments (no shell). For multiline Issue/PR/MR descriptions, supply `bodyText` (and optional `bodyFlag` such as `--body-file` or `--description-file`) so Rust writes a temporary Markdown file safely. Enforces head-SHA merge/approve guards and repository scope checks.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['cli', 'args'],
      properties: {
        cli: {
          type: 'string',
          enum: ['gh', 'glab'],
          description: 'Hosting CLI binary (`gh` for GitHub, `glab` for GitLab).',
        },
        args: {
          type: 'array',
          minItems: 1,
          items: { type: 'string' },
          description: 'Discrete argv array passed directly to `gh` or `glab`.',
        },
        bodyText: {
          type: 'string',
          description: 'Optional Markdown body content written by Rust to a temporary file and passed via `bodyFlag`.',
        },
        bodyFlag: {
          type: 'string',
          enum: ['--body-file', '--description-file', '-F', '--notes-file'],
          description: 'Flag name for `bodyText` (defaults to `--body-file` for `gh`, `--description-file` for `glab`).',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const cli = typeof params?.cli === 'string' ? params.cli.trim() : '';
      const args = Array.isArray(params?.args)
        ? params.args.filter((item: unknown): item is string => typeof item === 'string')
        : [];
      if ((cli !== 'gh' && cli !== 'glab') || args.length === 0) {
        return {
          content: [{ type: 'text', text: 'Expected `cli` ("gh" or "glab") and non-empty `args` array.' }],
          details: { cli, args },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = { cli, args };
      if (typeof params?.bodyText === 'string') payload.bodyText = params.bodyText;
      if (typeof params?.bodyFlag === 'string') payload.bodyFlag = params.bodyFlag;
      const response = await bridge.invokeHostTool('run_platform_cli', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { cli, args },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createCommitExactScopeTool(bridge: ToolBridge): AgentTool<any, { fileIds: string[] }> {
  return {
    name: 'commit_exact_scope',
    label: 'Commit Exact Scope',
    description: 'Create a local Git commit containing ONLY the specified `fileIds` from the user\'s authorized selected scope (preserving partial staging, unselected files, and Local Keep byte-for-byte).',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['fileIds', 'message'],
      properties: {
        fileIds: {
          type: 'array',
          minItems: 1,
          items: { type: 'string' },
          description: 'Exact `fileId`s from `Authorized selected files` to include in this commit.',
        },
        message: {
          type: 'string',
          description: 'Two-part Conventional Commit message (`<type>(<scope>): <subject>\\n\\n- bullet 1`).',
        },
        push: {
          type: 'boolean',
          description: 'If true, immediately push the current branch after committing (default false).',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const fileIds = Array.isArray(params?.fileIds)
        ? params.fileIds.filter((id: unknown): id is string => typeof id === 'string' && id.trim().length > 0)
        : [];
      const message = typeof params?.message === 'string' ? params.message.trim() : '';
      if (fileIds.length === 0 || !message) {
        return {
          content: [{ type: 'text', text: 'commit_exact_scope requires non-empty `fileIds` and `message`.' }],
          details: { fileIds },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = {
        fileIds,
        message,
        ...(typeof params?.push === 'boolean' ? { push: params.push } : {}),
      };
      const response = await bridge.invokeHostTool('commit_exact_scope', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { fileIds },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createSyncAndPushTool(bridge: ToolBridge): AgentTool<any, { mode: string }> {
  return {
    name: 'sync_and_push',
    label: 'Sync and Push',
    description: 'Run Picket\'s lossless pull-with-rebase or fast-forward-only push on the current branch without risking dirty working-tree changes.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['mode'],
      properties: {
        mode: {
          type: 'string',
          enum: ['push', 'pullRebase'],
          description: '`push` for fast-forward-only branch push; `pullRebase` for lossless dirty-worktree rebase onto upstream.',
        },
        setUpstreamRemote: {
          type: 'string',
          description: 'Optional remote name (e.g. `origin`) when pushing a new branch with no upstream yet.',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const mode = params?.mode === 'pullRebase' ? 'pullRebase' : 'push';
      const payload: Record<string, unknown> = { mode };
      if (typeof params?.setUpstreamRemote === 'string' && params.setUpstreamRemote.trim()) {
        payload.setUpstreamRemote = params.setUpstreamRemote.trim();
      }
      const response = await bridge.invokeHostTool('sync_and_push', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { mode },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createManageSubmitWorktreeTool(bridge: ToolBridge): AgentTool<any, { op: string }> {
  return {
    name: 'manage_submit_worktree',
    label: 'Manage Isolated Submit Worktree',
    description: 'Create, commit/push, list, or safely remove Picket-owned isolated Git worktrees for `submit` multi-group workflows. Transfers ONLY authorized selected files (and ONLY staged index content for partially staged files) while recording exact preflight hashes for Step 5 duplicate cleanup. Never touches unselected files, Local Keep, or original working-tree drafts. Removal enforces ownership, unlocked state, verified HEAD SHA, zero in-progress Git ops, empty status (including untracked/ignored), non-force `git worktree remove`, and optional safe `git branch -d`.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['op'],
      properties: {
        op: {
          type: 'string',
          enum: ['create', 'commit', 'list', 'remove'],
          description: 'Operation: `create` isolated worktree & transfer exact selected files; `commit` (& optionally push) inside owned isolated worktree; `list` worktrees with ownership flags; `remove` verified clean temporary worktree.',
        },
        groupId: {
          type: 'string',
          description: 'Stable group identifier (e.g. `group-1`). Required for `create`; optional lookup key for `commit` / `remove`.',
        },
        title: {
          type: 'string',
          description: 'Short human-readable group title (optional for `create`).',
        },
        branch: {
          type: 'string',
          description: 'Development branch name for `create` (never `main`/`master`/`HEAD`).',
        },
        baseRef: {
          type: 'string',
          description: 'Base Git ref or commit for `create` (e.g. `origin/main` or `origin/v1.9.0`; defaults to `HEAD`).',
        },
        reuseExistingBranch: {
          type: 'boolean',
          description: 'Set true in `create` to check out an existing issue branch instead of creating `-b`.',
        },
        fileIds: {
          type: 'array',
          items: { type: 'string' },
          description: 'Exact `fileId`s from `Authorized selected files` to transfer into the isolated worktree (`create` only).',
        },
        worktreePath: {
          type: 'string',
          description: 'Owned isolated worktree path for `commit` or `remove`.',
        },
        message: {
          type: 'string',
          description: 'Two-part Conventional Commit message (`commit` only; real Git hooks run).',
        },
        push: {
          type: 'boolean',
          description: 'If true in `commit`, immediately push the isolated branch (`git push -u <remote> <branch>`).',
        },
        remote: {
          type: 'string',
          description: 'Optional remote name for `commit` push (defaults to `origin`).',
        },
        expectedHeadSha: {
          type: 'string',
          description: 'Optional 40-char verified commit SHA guard before `remove`.',
        },
        deleteLocalBranch: {
          type: 'boolean',
          description: 'If true in `remove`, safely run `git branch -d <branch>` (never `-D`) after the worktree is removed.',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const op = typeof params?.op === 'string' ? params.op.trim() : '';
      if (!['create', 'commit', 'list', 'remove'].includes(op)) {
        return {
          content: [{ type: 'text', text: 'Invalid `op`: expected one of create, commit, list, remove.' }],
          details: { op },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = { op };
      for (const key of ['groupId', 'title', 'branch', 'baseRef', 'worktreePath', 'message', 'remote', 'expectedHeadSha'] as const) {
        if (typeof params?.[key] === 'string' && params[key].trim()) {
          payload[key] = params[key].trim();
        }
      }
      for (const key of ['reuseExistingBranch', 'push', 'deleteLocalBranch'] as const) {
        if (typeof params?.[key] === 'boolean') {
          payload[key] = params[key];
        }
      }
      if (Array.isArray(params?.fileIds)) {
        payload.fileIds = params.fileIds.filter((id: unknown): id is string => typeof id === 'string' && id.trim().length > 0);
      }
      const response = await bridge.invokeHostTool('manage_submit_worktree', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { op },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createCleanupSubmitDuplicatesTool(bridge: ToolBridge): AgentTool<any, { userConfirmed: boolean }> {
  return {
    name: 'cleanup_submit_duplicates',
    label: 'Backup & Clean Submitted Duplicates',
    description: 'Execute `submit` Step 5 verified local duplicate backup & cleanup on the original checkout ONLY AFTER the user explicitly approved cleanup via `ask_user_question`. Verifies every group has a pushed remote tip + PR/MR URL, retains partial-staged or edited files, writes a verified backup under `<git-dir>/submit-backup/<UTC-timestamp>/` before any write, and cleans only verified literal paths.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['userConfirmed'],
      properties: {
        userConfirmed: {
          type: 'boolean',
          description: 'Must be `true` only after the user explicitly approved Step 5 duplicate cleanup via `ask_user_question`.',
        },
        groupIds: {
          type: 'array',
          items: { type: 'string' },
          description: 'Optional subset of group IDs to verify and clean; defaults to all recorded groups.',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const userConfirmed = params?.userConfirmed === true;
      const payload: Record<string, unknown> = { userConfirmed };
      if (Array.isArray(params?.groupIds)) {
        payload.groupIds = params.groupIds.filter((id: unknown): id is string => typeof id === 'string' && id.trim().length > 0);
      }
      const response = await bridge.invokeHostTool('cleanup_submit_duplicates', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { userConfirmed },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createRecordActionCheckpointTool(bridge: ToolBridge): AgentTool<any, { saved: boolean }> {
  return {
    name: 'record_action_checkpoint',
    label: 'Record Action Checkpoint',
    description: 'Persist verified workflow milestones (Issue URLs/numbers, branch names, verified commit SHAs, PR/MR URLs/states, promotion PR/MR state, and clickable links) into `~/.picket/action-sessions.json` so crashes or restarts resume cleanly without duplicating Issues, branches, or PRs.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      properties: {
        workflowKind: {
          type: 'string',
          enum: ['custom', 'submit', 'submitMerge', 'submitPromote'],
        },
        firstTarget: { type: 'string' },
        finalTarget: { type: 'string' },
        group: {
          type: 'object',
          additionalProperties: false,
          required: ['groupId'],
          properties: {
            groupId: { type: 'string' },
            title: { type: 'string' },
            issueUrl: { type: 'string' },
            issueNumber: { type: 'string' },
            issueClosed: { type: 'boolean' },
            branch: { type: 'string' },
            baseRef: { type: 'string' },
            remote: { type: 'string' },
            verifiedHeadSha: { type: 'string' },
            prUrl: { type: 'string' },
            prNumber: { type: 'string' },
            prState: { type: 'string', enum: ['open', 'merged', 'closed'] },
          },
        },
        promotion: {
          type: 'object',
          additionalProperties: false,
          required: ['sourceBranch', 'targetBranch'],
          properties: {
            sourceBranch: { type: 'string' },
            targetBranch: { type: 'string' },
            prUrl: { type: 'string' },
            prNumber: { type: 'string' },
            prState: { type: 'string', enum: ['open', 'merged', 'closed'] },
            verifiedHeadSha: { type: 'string' },
          },
        },
        links: {
          type: 'array',
          items: {
            type: 'object',
            additionalProperties: false,
            required: ['label', 'url'],
            properties: {
              label: { type: 'string' },
              url: { type: 'string' },
            },
          },
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const payload: Record<string, unknown> = params && typeof params === 'object' ? { ...params } : {};
      const response = await bridge.invokeHostTool('record_action_checkpoint', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { saved: response.ok && !response.isError },
        isError: response.isError || !response.ok,
      };
    },
  };
}
