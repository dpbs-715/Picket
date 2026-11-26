import type { AgentTool } from '@earendil-works/pi-agent-core';
import type { ToolBridge } from './bridge';

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
