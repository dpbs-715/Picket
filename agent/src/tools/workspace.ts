import type { AgentTool } from '@earendil-works/pi-agent-core';
import type { ToolBridge } from './bridge';

export function createEditRepoFileTool(
  bridge: ToolBridge,
): AgentTool<any, { path: string }> {
  return {
    name: 'edit_repo_file',
    label: 'Edit Repository File',
    description: 'Apply an exact, unique text replacement to an existing UTF-8 text file inside the active repository worktree. Refuses `.git`, sensitive files (e.g. `.env`/keys), symlinks, `Local Keep`, and pre-existing unselected dirty drafts. Shows a diff preview for user confirmation unless the button is trusted, and records the edited file path so it can be committed via `commit_exact_scope({ editedPaths, message })`.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['path', 'oldText', 'newText'],
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative path of the existing text file to modify (no leading slash, no `..`, never inside `.git`).',
        },
        oldText: {
          type: 'string',
          description: 'Exact existing text snippet to replace. Must match exactly once in the file; read the file with `read_repo_file` first if unsure.',
        },
        newText: {
          type: 'string',
          description: 'Exact replacement text to substitute for `oldText`.',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const path = typeof params?.path === 'string' ? params.path.trim() : '';
      const oldText = typeof params?.oldText === 'string' ? params.oldText : '';
      const newText = typeof params?.newText === 'string' ? params.newText : '';
      if (!path || !oldText) {
        return {
          content: [{ type: 'text', text: 'edit_repo_file requires non-empty `path` and `oldText` strings.' }],
          details: { path },
          isError: true,
        };
      }
      const response = await bridge.invokeHostTool(
        'edit_repo_file',
        { path, oldText, newText },
        signal,
      );
      return {
        content: [{ type: 'text', text: response.content }],
        details: { path },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createWriteRepoFileTool(
  bridge: ToolBridge,
): AgentTool<any, { path: string }> {
  return {
    name: 'write_repo_file',
    label: 'Create Repository File',
    description: 'Create a brand-new UTF-8 text file inside the active repository worktree (never overwrites existing files; use `edit_repo_file` for existing files). Refuses `.git`, sensitive files (e.g. `.env`/keys), symlinked parent directories, and `Local Keep`. Shows a preview for user confirmation unless the button is trusted, and records the created path so it can be committed via `commit_exact_scope({ editedPaths, message })`.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['path', 'content'],
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative path of the new UTF-8 text file to create (parent directories inside the worktree are created automatically; must not already exist).',
        },
        content: {
          type: 'string',
          description: 'Complete UTF-8 text content for the new file (up to 256 KB, no NUL bytes).',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const path = typeof params?.path === 'string' ? params.path.trim() : '';
      const content = typeof params?.content === 'string' ? params.content : null;
      if (!path || content === null) {
        return {
          content: [{ type: 'text', text: 'write_repo_file requires non-empty `path` and string `content`.' }],
          details: { path },
          isError: true,
        };
      }
      const response = await bridge.invokeHostTool(
        'write_repo_file',
        { path, content },
        signal,
      );
      return {
        content: [{ type: 'text', text: response.content }],
        details: { path },
        isError: response.isError || !response.ok,
      };
    },
  };
}

export function createRunRepoCommandTool(
  bridge: ToolBridge,
): AgentTool<any, { command: string; args: string[] }> {
  return {
    name: 'run_repo_command',
    label: 'Run Local Repository Command',
    description: 'Run a local executable inside the active worktree with discrete `command` and `args` (zero shell interpolation; never pass `&&`, `|`, redirects, or shell scripts). Prompts the user with a command preview unless the button is trusted. Permanently blocks shells (`sh`/`bash`/`zsh`), destructive/privileged/network binaries (`sudo`/`rm`/`curl`/`wget`), and Picket-guarded CLIs (`git`/`gh`/`glab`/`brew`). Falls back to bundled Node automatically when `command` is `node`.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['command'],
      properties: {
        command: {
          type: 'string',
          description: 'Bare executable name on PATH (e.g. `node`, `pnpm`, `cargo`, `python3`) or a `./`-relative worktree executable.',
        },
        args: {
          type: 'array',
          items: { type: 'string' },
          description: 'Discrete argument vector passed directly to `command` without shell expansion (for example `["scripts/package-dmg.mjs", "--check-version-only", "--tag", "v0.2.1"]`).',
        },
        timeoutSecs: {
          type: 'integer',
          minimum: 5,
          maximum: 180,
          description: 'Optional execution timeout in seconds (default 90, clamped to 5..180).',
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      const command = typeof params?.command === 'string' ? params.command.trim() : '';
      const args = Array.isArray(params?.args)
        ? params.args.filter((item: unknown): item is string => typeof item === 'string')
        : [];
      if (!command) {
        return {
          content: [{ type: 'text', text: 'Missing required non-empty `command` string.' }],
          details: { command: '', args: [] },
          isError: true,
        };
      }
      const payload: Record<string, unknown> = { command, args };
      if (typeof params?.timeoutSecs === 'number' && Number.isFinite(params.timeoutSecs)) {
        payload.timeoutSecs = Math.min(180, Math.max(5, Math.trunc(params.timeoutSecs)));
      }
      const response = await bridge.invokeHostTool('run_repo_command', payload, signal);
      return {
        content: [{ type: 'text', text: response.content }],
        details: { command, args },
        isError: response.isError || !response.ok,
      };
    },
  };
}
