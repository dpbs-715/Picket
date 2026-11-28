import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import {
  CUSTOM_ACTION_LIMITS,
  computeCustomActionPromptHash,
  isCustomActionWriteTrusted,
  sanitizeCustomActionConfig,
  type CustomActionConfig,
} from '../../../../shared/custom-action-protocol';
import { decodeWorkbenchRestoreReply, WORKBENCH_PERSISTENCE_PROTOCOL_VERSION } from '../../../../shared/workbench-persistence-protocol';
import { fixture } from '../testing/fixtures';
import { hydrateWorkbench, serializeWorkbenchPersistence } from './repositories';
import {
  activeCustomActionFor,
  classifyCustomActionSyncTarget,
  computeSyncCustomActionAcrossRepos,
  customActionsFor,
  primaryAction,
  workbenchReducer,
  type WorkbenchState,
} from './workbench';

describe('Phase 2 custom actions persistence, prompt-hash trust invalidation, copy-sync, and primary action', () => {
  it('computes SHA-256 prompt fingerprints matching Node/Rust sha2 byte-for-byte', () => {
    const prompt = 'git tag -a v0.2.0 -m "Release"\r\ngit push origin v0.2.0\n';
    const expected = createHash('sha256')
      .update('git tag -a v0.2.0 -m "Release"\ngit push origin v0.2.0', 'utf8')
      .digest('hex');
    expect(computeCustomActionPromptHash(prompt)).toBe(expected);
  });

  it('auto-invalidates skipWriteConfirm when prompt changes while preserving trust on label-only edits', () => {
    let state = fixture();
    const initialPrompt = 'git tag v0.2.0 && git push origin v0.2.0';
    const hash = computeCustomActionPromptHash(initialPrompt);
    const action: CustomActionConfig = {
      id: 'act-tag',
      label: 'Release Tag',
      prompt: initialPrompt,
    };

    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action,
      selectAsActive: true,
    });
    state = workbenchReducer(state, {
      type: 'trustCustomActionWrites',
      repositoryId: 'orders',
      actionId: 'act-tag',
      trustedPromptHash: hash,
    });

    const trusted = activeCustomActionFor(state, 'orders');
    expect(trusted).toBeDefined();
    expect(isCustomActionWriteTrusted(trusted!)).toBe(true);

    // Label-only edit preserves write trust:
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action: { ...trusted!, label: 'Tag & Push' },
    });
    expect(isCustomActionWriteTrusted(activeCustomActionFor(state, 'orders')!)).toBe(true);

    // Editing prompt immediately revokes write trust:
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action: { ...trusted!, label: 'Tag & Push', prompt: 'git tag v0.3.0 && git push origin v0.3.0' },
    });
    const updated = activeCustomActionFor(state, 'orders')!;
    expect(updated.skipWriteConfirm).toBeUndefined();
    expect(updated.trustedPromptHash).toBeUndefined();
    expect(isCustomActionWriteTrusted(updated)).toBe(false);
  });

  it('syncs custom action as independent copies across repositories without silently overwriting customized entries', () => {
    let state = fixture();
    const sourceAction: CustomActionConfig = {
      id: 'act-release',
      label: 'Release Tag',
      prompt: 'git tag v0.2.0 && git push origin v0.2.0',
    };
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action: sourceAction,
      selectAsActive: true,
    });

    // First sync copies to `cli`:
    const first = computeSyncCustomActionAcrossRepos(state, 'orders', sourceAction, false);
    expect(first.result).toEqual({ added: 1, updated: 0, skipped: 0 });
    state = workbenchReducer(state, {
      type: 'syncCustomActionToAllRepos',
      sourceRepositoryId: 'orders',
      action: sourceAction,
      overwrite: false,
    });

    const cliActions = customActionsFor(state, 'cli');
    expect(cliActions).toHaveLength(1);
    expect(cliActions[0].id).not.toBe(sourceAction.id);
    expect(cliActions[0].templateKey).toBe('act-release');

    // Customize `cli`'s copy independently:
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'cli',
      action: { ...cliActions[0], prompt: 'git tag cli-v1.0.0 && git push origin cli-v1.0.0' },
    });

    // Re-syncing without overwrite skips `cli` and preserves its customized prompt:
    const second = computeSyncCustomActionAcrossRepos(
      state,
      'orders',
      { ...sourceAction, prompt: 'git tag v0.9.0 && git push origin v0.9.0' },
      false,
    );
    expect(second.result).toEqual({ added: 0, updated: 0, skipped: 1 });
    expect(customActionsFor(state, 'cli')[0].prompt).toBe('git tag cli-v1.0.0 && git push origin cli-v1.0.0');

    // Explicit overwrite updates `cli` when requested:
    const third = computeSyncCustomActionAcrossRepos(
      state,
      'orders',
      { ...sourceAction, prompt: 'git tag v0.9.0 && git push origin v0.9.0' },
      true,
    );
    expect(third.result).toEqual({ added: 0, updated: 1, skipped: 0 });
    state = workbenchReducer(state, {
      type: 'syncCustomActionToAllRepos',
      sourceRepositoryId: 'orders',
      action: { ...sourceAction, prompt: 'git tag v0.9.0 && git push origin v0.9.0' },
      overwrite: true,
    });
    expect(customActionsFor(state, 'cli')[0].prompt).toBe('git tag v0.9.0 && git push origin v0.9.0');
  });

  it('supports selective repo sync on saveCustomAction and classifies target repo states accurately', () => {
    const base = fixture();
    let state: WorkbenchState = {
      ...base,
      repositories: [
        ...base.repositories,
        {
          id: 'web',
          name: 'picket-web',
          branches: [{ name: 'main', worktreeId: 'web-tree' }],
        },
      ],
      repositorySettings: {
        ...base.repositorySettings,
        web: {
          editor: 'idea',
          commitLanguage: 'en',
          customActions: [{ id: 'act-web-same', label: 'Release Tag', prompt: 'npm version patch' }],
        },
      },
    };

    expect(classifyCustomActionSyncTarget([], { label: 'Release Tag' })).toBe('new');
    expect(
      classifyCustomActionSyncTarget(customActionsFor(state, 'web'), { label: 'Release Tag' }),
    ).toBe('sameLabel');
    expect(
      classifyCustomActionSyncTarget(
        Array.from({ length: CUSTOM_ACTION_LIMITS.maxPerRepo }, (_, i) => ({
          id: `a-${i}`,
          label: `Action ${i}`,
          prompt: `cmd ${i}`,
        })),
        { label: 'Unique Action' },
      ),
    ).toBe('full');

    const sourceAction: CustomActionConfig = {
      id: 'act-selective',
      label: 'Release Tag',
      prompt: 'git tag v0.2.0 && git push origin v0.2.0',
    };

    // Saving with only `cli` selected must sync to `cli` and leave `web` untouched:
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action: sourceAction,
      selectAsActive: true,
      syncTargetRepositoryIds: ['cli'],
    });

    const savedSource = customActionsFor(state, 'orders')[0];
    expect(savedSource.templateKey).toBe('act-selective');
    expect(customActionsFor(state, 'cli')).toHaveLength(1);
    expect(customActionsFor(state, 'cli')[0].templateKey).toBe('act-selective');
    expect(customActionsFor(state, 'cli')[0].prompt).toBe('git tag v0.2.0 && git push origin v0.2.0');
    expect(customActionsFor(state, 'web')[0].prompt).toBe('npm version patch');

    expect(
      classifyCustomActionSyncTarget(customActionsFor(state, 'cli'), savedSource),
    ).toBe('synced');
    expect(
      classifyCustomActionSyncTarget(customActionsFor(state, 'web'), savedSource),
    ).toBe('sameLabel');

    // Editing & saving with ONLY `web` selected updates `web` in place and preserves `cli`'s copy:
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action: { ...savedSource, prompt: 'git tag v0.3.0 && git push origin v0.3.0' },
      selectAsActive: true,
      syncTargetRepositoryIds: ['web'],
    });
    expect(customActionsFor(state, 'orders')[0].prompt).toBe('git tag v0.3.0 && git push origin v0.3.0');
    expect(customActionsFor(state, 'cli')[0].prompt).toBe('git tag v0.2.0 && git push origin v0.2.0');
    expect(customActionsFor(state, 'web')[0].id).toBe('act-web-same');
    expect(customActionsFor(state, 'web')[0].templateKey).toBe('act-selective');
    expect(customActionsFor(state, 'web')[0].prompt).toBe('git tag v0.3.0 && git push origin v0.3.0');
  });

  it('keeps custom actions clickable on clean trees / 0 selected files and persists across settings edits & restore', () => {
    let state: WorkbenchState = fixture();
    const reviewPr: CustomActionConfig = sanitizeCustomActionConfig({
      id: 'act-review-pr',
      label: 'Review PR',
      prompt: 'review-pr',
    });
    state = workbenchReducer(state, {
      type: 'saveCustomAction',
      repositoryId: 'orders',
      action: reviewPr,
      selectAsActive: true,
    });

    // Clean worktree with 0 files selected:
    const cleanTree = { ...state.worktrees[0], files: [], selectedIds: [] };
    expect(primaryAction(cleanTree, true, 'en')).toMatchObject({ label: 'C-Push', disabled: true });
    expect(primaryAction(cleanTree, true, 'en', reviewPr)).toEqual({
      label: 'Review PR',
      configureModel: false,
      resume: false,
      disabled: false,
    });

    // Changing per-repo editor or commitLanguage must never drop customActions/activeActionId:
    state = workbenchReducer(state, { type: 'editor', editor: 'vscode', repositoryId: 'orders' });
    state = workbenchReducer(state, { type: 'commitLanguage', commitLanguage: 'zh', repositoryId: 'orders' });
    expect(activeCustomActionFor(state, 'orders')?.id).toBe('act-review-pr');

    // Serialize & decode persistence roundtrip:
    const serialized = serializeWorkbenchPersistence(state, 1);
    const decoded = decodeWorkbenchRestoreReply({
      protocolVersion: WORKBENCH_PERSISTENCE_PROTOCOL_VERSION,
      locale: serialized.locale,
      editor: serialized.editor,
      defaultCommitLanguage: serialized.defaultCommitLanguage,
      repositorySettings: serialized.repositorySettings,
      activeWorktreeId: serialized.activeWorktreeId,
      emptyPrompt: serialized.emptyPrompt,
      worktrees: serialized.worktrees,
      snapshots: [],
      warning: null,
    });
    const restoredOrders = decoded.repositorySettings?.find((s) => s.repositoryId === 'orders');
    expect(restoredOrders?.customActions).toHaveLength(1);
    expect(restoredOrders?.activeActionId).toBe('act-review-pr');

    // Deleting active action falls back cleanly to C-Push:
    state = workbenchReducer(state, {
      type: 'deleteCustomAction',
      repositoryId: 'orders',
      actionId: 'act-review-pr',
    });
    expect(activeCustomActionFor(state, 'orders')).toBeUndefined();
  });

  it('decodes and cold-restores interrupted & done custom action sessions without clobbering active C-Push sessions', () => {
    const emptyState: WorkbenchState = {
      ...fixture(),
      repositories: [],
      worktrees: [],
      activeWorktreeId: null,
    };
    const decoded = decodeWorkbenchRestoreReply({
      protocolVersion: WORKBENCH_PERSISTENCE_PROTOCOL_VERSION,
      locale: 'en',
      editor: 'idea',
      defaultCommitLanguage: 'en',
      activeWorktreeId: 'wt-1',
      emptyPrompt: '',
      worktrees: [],
      snapshots: [{
        readSequence: 1,
        repository: {
          id: 'repo-1',
          name: 'demo-repo',
          branches: [{ name: 'main', worktreeId: 'wt-1' }],
          remoteBranches: [{ name: 'origin/main', remote: 'origin', localName: 'main' }],
        },
        activeWorktreeId: 'wt-1',
        worktrees: [{
          repositoryId: 'repo-1',
          worktreeId: 'wt-1',
          label: 'main',
          path: '/tmp/demo-repo',
          branch: 'main',
          detached: false,
          head: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          branchToken: null,
          readError: null,
          remote: { kind: 'tracked', branch: 'origin/main', ahead: 0, behind: 0, localOnly: false },
          statusVersion: 'sv-1',
          files: [],
        }],
      }],
      actionSessions: [{
        runId: 'run-submit-1',
        repositoryId: 'repo-1',
        worktreeId: 'wt-1',
        sessionId: 'sess-1',
        actionId: 'act-submit',
        actionLabel: 'Submit to v1.9.0 then test',
        profile: 'customAction',
        frozenScopeVersion: 'sv-1',
        modelId: 'm-1',
        frozenFileIds: [],
        committedFileIds: [],
        skippedFileIds: [],
        status: 'interrupted',
        summary: 'Submit to v1.9.0 then test was interrupted before completion. Created Issues, branches, and PRs were preserved.',
        message: {
          id: 'interrupted-action-run-submit-1',
          kind: 'error',
          title: 'Interrupted: Submit to v1.9.0 then test',
          description: 'Created Issues, branches, and PRs were preserved.',
          links: [{ label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' }],
        },
        timeline: [{ state: 'done', label: 'Created isolated worktree' }, { state: 'failed', label: 'gh pr merge' }],
        qaLog: [],
        commits: [{ oid: '1111111111111111111111111111111111111111', shortOid: '1111111', title: 'fix: auth', branch: 'fix/issue-42', fileIds: [] }],
        links: [{ label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' }],
        canRetryRemaining: false,
      }],
      warning: null,
    });

    expect(decoded.actionSessions).toHaveLength(1);
    const hydrated = hydrateWorkbench(emptyState, decoded);
    const wt = hydrated.worktrees[0];
    expect(wt.sessionStatus).toBe('idle');
    expect(wt.activeRun?.profile).toBe('customAction');
    expect(wt.activeRun?.runId).toBe('run-submit-1');
    expect(wt.activeRun?.links).toEqual([{ label: 'PR #101', url: 'https://github.com/acme/demo/pull/101' }]);
    expect(wt.messages).toHaveLength(1);
    expect(wt.messages[0].id).toBe('interrupted-action-run-submit-1');

    // Dismissing the restored interrupted message clears both message and activeRun state cleanly:
    const afterDismiss = workbenchReducer(hydrated, {
      type: 'dismissMessage',
      worktreeId: 'wt-1',
      messageId: 'interrupted-action-run-submit-1',
    });
    expect(afterDismiss.worktrees[0].messages).toHaveLength(0);
    expect(afterDismiss.worktrees[0].activeRun).toBeUndefined();
  });

  it('clears running state on customAction failure even with landed commits, honors suppressExpand on settle, and drops stale expectedRunId progress', () => {
    let state = fixture();
    const tree = state.worktrees[0];
    state = workbenchReducer(state, {
      type: 'actionStarted',
      worktreeId: tree.worktreeId,
      profile: 'customAction',
      runId: 'run-ca-commits',
      actionId: 'act-release',
      actionLabel: 'Release',
      frozenTarget: {
        repositoryId: tree.repositoryId,
        worktreeId: tree.worktreeId,
        sessionId: tree.sessionId,
        scopeVersion: tree.scopeVersion,
      },
      frozenFileIds: [],
      modelId: 'test-claude',
      expandDock: false,
    });
    // Simulate a commit having already landed mid-run:
    state = {
      ...state,
      worktrees: state.worktrees.map((wt) => (
        wt.worktreeId === tree.worktreeId && wt.activeRun
          ? {
              ...wt,
              activeRun: {
                ...wt.activeRun,
                commits: [{ oid: '111', shortOid: '111', title: 'chore: bump', branch: 'feat/pay', fileIds: [] }],
              },
            }
          : wt
      )),
    };

    // Custom Action start/IPC failure must still exit 'running' and surface error message:
    const afterFail = workbenchReducer(state, {
      type: 'actionStartFailed',
      worktreeId: tree.worktreeId,
      runId: 'run-ca-commits',
      message: 'Transport closed unexpectedly',
    });
    const failedTree = afterFail.worktrees[0];
    expect(failedTree.sessionStatus).toBe('idle');
    expect(failedTree.messages[0]).toMatchObject({
      kind: 'error',
      description: 'Transport closed unexpectedly',
    });

    // Settled failure with suppressExpand: true (e.g., user typing or compact mode) must NOT force dockExpanded:
    const suppressedSettle = workbenchReducer(afterFail, {
      type: 'actionSettled',
      expandDock: false,
      suppressExpand: true,
      reply: {
        protocolVersion: 2,
        kind: 'action',
        requestId: 'run-ca-commits',
        session: {
          runId: 'run-ca-commits',
          repositoryId: tree.repositoryId,
          worktreeId: tree.worktreeId,
          sessionId: tree.sessionId,
          actionId: 'act-release',
          actionLabel: 'Release',
          profile: 'customAction',
          frozenScopeVersion: tree.scopeVersion,
          modelId: 'test-claude',
          frozenFileIds: [],
          committedFileIds: [],
          skippedFileIds: [],
          status: 'failed',
          summary: 'Failed',
          message: {
            id: 'fail-suppressed',
            kind: 'error',
            title: 'Failed',
            description: 'Model error',
          },
          timeline: [],
          qaLog: [],
          commits: [],
          links: [],
          canRetryRemaining: false,
        },
      },
    });
    expect(suppressedSettle.worktrees[0].dockExpanded).toBe(false);

    // Stale reconcile event with mismatched expectedRunId must be dropped even when worktree is idle:
    const idleState = fixture();
    const afterStaleReconcile = workbenchReducer(idleState, {
      type: 'event',
      event: {
        type: 'actionProgress',
        expectedRunId: 'run-already-dismissed',
        progress: {
          protocolVersion: 2,
          session: {
            runId: 'run-already-dismissed',
            repositoryId: tree.repositoryId,
            worktreeId: tree.worktreeId,
            sessionId: tree.sessionId,
            actionId: 'act-old',
            actionLabel: 'Old Action',
            profile: 'customAction',
            frozenScopeVersion: tree.scopeVersion,
            modelId: 'test-claude',
            frozenFileIds: [],
            committedFileIds: [],
            skippedFileIds: [],
            status: 'conflict',
            summary: 'Stale conflict',
            message: {
              id: 'stale-conflict',
              kind: 'conflict',
              title: 'Stale conflict',
              description: 'Should be ignored',
            },
            timeline: [],
            qaLog: [],
            commits: [],
            links: [],
            canRetryRemaining: false,
          },
        },
      },
    });
    expect(afterStaleReconcile.worktrees[0].sessionStatus).toBe('idle');
    expect(afterStaleReconcile.worktrees[0].messages).toHaveLength(0);
  });
});
