import type { AgentTool } from '@earendil-works/pi-agent-core';
import type { ToolBridge } from './bridge';

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
