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
    description: 'Install `gh` or `glab` via Homebrew (`brew install gh` / `brew install glab`). Rust strictly requires that the user already approved this installation via `message_user` (with `questions`) in the current session.',
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

export function createCommitExactScopeTool(
  bridge: ToolBridge,
  options?: { cpushMode?: boolean },
): AgentTool<any, { fileIds: string[]; editedPaths: string[]; groupCount?: number; stopAgent?: boolean }> {
  const cpushMode = options?.cpushMode === true;
  return {
    name: 'commit_exact_scope',
    label: 'Commit Exact Scope',
    description: cpushMode
      ? 'Commit all authorized selected files as one or more cohesive exact-scope commits (preserving partial staging, unselected files, and Local Keep byte-for-byte) and optionally push the current branch. Reference authorized files by their "@1".."@N" alias or exact `fileId`. Every authorized file must appear once across `groups[].fileIds` and user-confirmed `skippedFileIds`.'
      : 'Create one or more local Git commits containing ONLY authorized files: pre-selected `fileIds` from `Authorized selected files` and/or `editedPaths` created or modified via `edit_repo_file`, `write_repo_file`, or `run_repo_command` during this run (preserving partial staging, unselected files, and Local Keep byte-for-byte). Accepts either a single commit (`fileIds`/`editedPaths` + `message`) or multiple cohesive commits via `groups`. When `push: true` fails or hits a conflict, returns a `recoveryHandle` so you can either handle it in-workflow or call `handoff_to_recovery`.',
    parameters: cpushMode
      ? {
          type: 'object',
          additionalProperties: false,
          required: ['groups'],
          properties: {
            groups: {
              type: 'array',
              description: 'Ordered list of cohesive commit groups. Default to 1 group unless the scope mixes genuinely independent intents or the user asked to split. May be empty ONLY when every authorized file is listed in `skippedFileIds` after user confirmation.',
              items: {
                type: 'object',
                additionalProperties: false,
                required: ['fileIds', 'message'],
                properties: {
                  id: { type: 'string', description: 'Optional short group ID (e.g. `group-1`).' },
                  fileIds: {
                    type: 'array',
                    minItems: 1,
                    items: { type: 'string' },
                    description: 'Authorized file references (`"@1"`, `"@2"`, ... or exact `fileId`s) belonging to this commit.',
                  },
                  message: {
                    type: 'string',
                    description: 'Two-part Conventional Commit message (`<type>(<optional scope>): <subject>\\n\\n- bullet 1\\n- bullet 2`).',
                  },
                },
              },
            },
            skippedFileIds: {
              type: 'array',
              items: { type: 'string' },
              description: 'Authorized file references explicitly excluded AFTER user confirmation via `message_user` (must be `[]` if not confirmed).',
            },
            push: {
              type: 'boolean',
              description: 'Whether to sync and push after committing (default `true`; set `false` ONLY when the user explicitly asked to commit locally without pushing).',
            },
            summary: {
              type: 'string',
              description: 'Concise 1-line summary in the UI language describing the commit/push plan.',
            },
          },
        }
      : {
          type: 'object',
          additionalProperties: false,
          properties: {
            groups: {
              type: 'array',
              description: 'Optional ordered list of cohesive commit groups for multi-commit splits (each group has `message` and `fileIds` and/or `editedPaths`). Omit to create a single commit using top-level `fileIds`/`editedPaths` + `message`.',
              items: {
                type: 'object',
                additionalProperties: false,
                required: ['message'],
                properties: {
                  id: { type: 'string', description: 'Optional short group ID (e.g. `group-1`).' },
                  fileIds: {
                    type: 'array',
                    items: { type: 'string' },
                    description: 'Authorized file references (`"@1"`, `"@2"`, ... or exact `fileId`s) from `Authorized selected files`.',
                  },
                  editedPaths: {
                    type: 'array',
                    items: { type: 'string' },
                    description: 'Repository-relative paths created or modified via `edit_repo_file`, `write_repo_file`, or `run_repo_command` during this run.',
                  },
                  message: {
                    type: 'string',
                    description: 'Two-part Conventional Commit message (`<type>(<scope>): <subject>\\n\\n- bullet 1`).',
                  },
                },
              },
            },
            fileIds: {
              type: 'array',
              items: { type: 'string' },
              description: 'Exact `fileId`s (or `"@1"`..`"@N"` aliases) from `Authorized selected files` to include in a single commit.',
            },
            editedPaths: {
              type: 'array',
              items: { type: 'string' },
              description: 'Repository-relative paths created or modified via `edit_repo_file`, `write_repo_file`, or `run_repo_command` during this run (or verified from the previous run checkpoint) to include in a single commit.',
            },
            message: {
              type: 'string',
              description: 'Two-part Conventional Commit message (`<type>(<scope>): <subject>\\n\\n- bullet 1`) for a single commit.',
            },
            push: {
              type: 'boolean',
              description: 'If true, immediately push the current branch after committing (default false).',
            },
            summary: {
              type: 'string',
              description: 'Optional concise 1-line summary in the UI language.',
            },
          },
        },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const skippedFileIds = Array.isArray(params?.skippedFileIds)
        ? params.skippedFileIds.filter((id: unknown): id is string => typeof id === 'string' && id.trim().length > 0)
        : [];
      const summary = typeof params?.summary === 'string' && params.summary.trim()
        ? params.summary.trim().slice(0, 300)
        : undefined;

      if (Array.isArray(params?.groups) || (cpushMode && !Array.isArray(params?.fileIds))) {
        const rawGroups = Array.isArray(params?.groups) ? params.groups : [];
        const groups: Array<{ id: string; fileIds: string[]; editedPaths?: string[]; message: string }> = [];
        for (let idx = 0; idx < rawGroups.length; idx += 1) {
          const entry = rawGroups[idx];
          if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
            return {
              content: [{ type: 'text', text: `Invalid commit group at index ${idx}: expected an object with \`fileIds\`/\`editedPaths\` and \`message\`.` }],
              details: { fileIds: [], editedPaths: [] },
              isError: true,
            };
          }
          const rec = entry as Record<string, unknown>;
          const gFileIds = Array.isArray(rec.fileIds)
            ? rec.fileIds.filter((id: unknown): id is string => typeof id === 'string' && id.trim().length > 0)
            : [];
          const gEditedPaths = !cpushMode && Array.isArray(rec.editedPaths)
            ? rec.editedPaths.filter((p: unknown): p is string => typeof p === 'string' && p.trim().length > 0)
            : [];
          const gMessage = typeof rec.message === 'string' ? rec.message.trim() : '';
          if ((gFileIds.length === 0 && gEditedPaths.length === 0) || !gMessage) {
            return {
              content: [{ type: 'text', text: `Commit group ${idx + 1} requires non-empty \`message\` and at least one entry in \`fileIds\`${cpushMode ? '' : ' or `editedPaths`'}.` }],
              details: { fileIds: [], editedPaths: [] },
              isError: true,
            };
          }
          const gId = typeof rec.id === 'string' && rec.id.trim()
            ? rec.id.trim().slice(0, 64)
            : `group-${idx + 1}`;
          groups.push({
            id: gId,
            fileIds: gFileIds,
            ...(gEditedPaths.length > 0 ? { editedPaths: gEditedPaths } : {}),
            message: gMessage,
          });
        }
        if (groups.length === 0 && skippedFileIds.length === 0) {
          return {
            content: [{ type: 'text', text: 'commit_exact_scope requires at least one commit group in `groups` (or all authorized files in confirmed `skippedFileIds`).' }],
            details: { fileIds: [], editedPaths: [] },
            isError: true,
          };
        }
        const payload: Record<string, unknown> = {
          groups,
          ...(skippedFileIds.length > 0 ? { skippedFileIds } : {}),
          ...(typeof params?.push === 'boolean' ? { push: params.push } : {}),
          ...(summary ? { summary } : {}),
        };
        const response = await bridge.invokeHostTool('commit_exact_scope', payload, signal);
        const stopAgent = response.ok
          && !response.isError
          && typeof response.details === 'object'
          && response.details !== null
          && (response.details as Record<string, unknown>).stopAgent === true;
        return {
          content: [{ type: 'text', text: response.content }],
          details: {
            fileIds: groups.flatMap((g) => g.fileIds),
            editedPaths: groups.flatMap((g) => g.editedPaths ?? []),
            groupCount: groups.length,
            ...(stopAgent ? { stopAgent: true } : {}),
          },
          isError: response.isError || !response.ok,
          ...(stopAgent ? { terminate: true } : {}),
        };
      }

      const fileIds = Array.isArray(params?.fileIds)
        ? params.fileIds.filter((id: unknown): id is string => typeof id === 'string' && id.trim().length > 0)
        : [];
      const editedPaths = Array.isArray(params?.editedPaths)
        ? params.editedPaths.filter((p: unknown): p is string => typeof p === 'string' && p.trim().length > 0)
        : [];
      const message = typeof params?.message === 'string' ? params.message.trim() : '';
      if ((fileIds.length === 0 && editedPaths.length === 0) || !message) {
        return {
          content: [{ type: 'text', text: 'commit_exact_scope requires non-empty `message` and at least one entry in `fileIds` or `editedPaths`.' }],
          details: { fileIds, editedPaths },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = cpushMode
        ? {
            groups: [{ id: 'group-1', fileIds, message }],
            ...(skippedFileIds.length > 0 ? { skippedFileIds } : {}),
            ...(typeof params?.push === 'boolean' ? { push: params.push } : {}),
            ...(summary ? { summary } : {}),
          }
        : {
            ...(fileIds.length > 0 ? { fileIds } : {}),
            ...(editedPaths.length > 0 ? { editedPaths } : {}),
            message,
            ...(typeof params?.push === 'boolean' ? { push: params.push } : {}),
            ...(summary ? { summary } : {}),
          };
      const response = await bridge.invokeHostTool('commit_exact_scope', payload, signal);
      const stopAgent = response.ok
        && !response.isError
        && typeof response.details === 'object'
        && response.details !== null
        && (response.details as Record<string, unknown>).stopAgent === true;
      return {
        content: [{ type: 'text', text: response.content }],
        details: {
          fileIds,
          editedPaths,
          ...(stopAgent ? { stopAgent: true } : {}),
        },
        isError: response.isError || !response.ok,
        ...(stopAgent ? { terminate: true } : {}),
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
