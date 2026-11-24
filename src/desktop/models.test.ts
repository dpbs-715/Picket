import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage?: (event: unknown) => void;
  },
}));

import { desktopWorkbenchBackend } from './repositories';
import {
  decodeModelMutateReply,
  decodeModelRevealKeyReply,
  decodeModelSaveReply,
  decodeModelTestReply,
  decodeReviewScopeReply,
} from '../../shared/model-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import { emptyWorkbench, workbenchReducer } from '../features/workbench/state/workbench';

describe('M3 desktop model protocol, C-Push plan preview, and security boundaries', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('rejects any model or restore reply that leaks apiKey or credentialRef', () => {
    expect(() => decodeModelSaveReply({
      protocolVersion: 1,
      activeModelId: 'm1',
      model: { id: 'm1', name: 'Claude', provider: 'anthropic', modelId: 'claude', apiKey: 'sk-leak' },
    })).toThrow();
    expect(() => decodeModelMutateReply({
      protocolVersion: 1,
      activeModelId: 'm1',
      models: [{ id: 'm1', name: 'Claude', provider: 'anthropic', modelId: 'claude', credentialRef: 'cred-1' }],
    })).toThrow();
    expect(() => decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
      models: [{ id: 'm1', name: 'GPT', provider: 'openai', modelId: 'gpt-4.1', apiKey: 'sk-leak' }],
      activeModelId: 'm1',
    })).toThrow();
    expect(() => decodeModelRevealKeyReply({
      protocolVersion: 1,
      modelId: 'm1',
      outcome: 'cancelled',
      message: 'Cancelled',
      apiKey: 'fake-leak-on-cancel',
    })).toThrow();
    expect(() => decodeModelRevealKeyReply({
      protocolVersion: 1,
      modelId: 'm1',
      outcome: 'authenticated',
      apiKey: 'fake-revealed-key-0001',
      keyHint: 'fake…0001',
      credentialRef: 'cred-leak',
    })).toThrow();
    expect(decodeModelRevealKeyReply({
      protocolVersion: 1,
      modelId: 'm1',
      outcome: 'authenticated',
      apiKey: 'fake-revealed-key-0001',
      keyHint: 'fake…0001',
    })).toEqual({
      protocolVersion: 1,
      modelId: 'm1',
      outcome: 'authenticated',
      apiKey: 'fake-revealed-key-0001',
      keyHint: 'fake…0001',
    });
    expect(decodeModelSaveReply({
      protocolVersion: 1,
      activeModelId: 'm1',
      model: {
        id: 'm2',
        name: 'GPT Copy',
        provider: 'openai',
        modelId: 'gpt-4.1-mini',
        keyHint: 'fake…0001',
        sharedKeyCount: 2,
      },
      models: [
        { id: 'm1', name: 'GPT', provider: 'openai', modelId: 'gpt-4.1', keyHint: 'fake…0001', sharedKeyCount: 2 },
        { id: 'm2', name: 'GPT Copy', provider: 'openai', modelId: 'gpt-4.1-mini', keyHint: 'fake…0001', sharedKeyCount: 2 },
      ],
    }).models).toHaveLength(2);
  });

  it('strictly verifies request identity, prompt/locale/commitLanguage forwarding, and scope tokens', async () => {
    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      provider: 'anthropic',
      modelId: 'claude-sonnet-4-20250514',
      summary: 'Connected: Anthropic · claude-sonnet-4-20250514 (85 ms)',
      latencyMs: 85,
    });
    const testSummary = await desktopWorkbenchBackend.testModel({
      provider: 'anthropic',
      name: 'Claude',
      modelId: 'claude-sonnet-4-20250514',
      baseUrl: '',
      apiKey: 'sk-ant-1',
      locale: 'en',
    });
    expect(testSummary).toContain('Connected');

    // Mismatched provider on testModel is rejected:
    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      provider: 'openai',
      modelId: 'claude-sonnet-4-20250514',
      summary: 'wrong',
      latencyMs: 10,
    });
    await expect(desktopWorkbenchBackend.testModel({
      provider: 'anthropic',
      name: 'Claude',
      modelId: 'claude-sonnet-4-20250514',
      baseUrl: '',
      apiKey: 'sk-ant-1',
    })).rejects.toThrow('match');

    // Missing per-file commitTokens on planCPush and review is rejected before IPC:
    const target = { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' };
    await expect(desktopWorkbenchBackend.execute({
      kind: 'planCPush',
      requestId: 'plan-no-token',
      target,
      fileIds: ['f1'],
      modelId: 'm1',
      prompt: 'split lockfile',
    })).rejects.toThrow('scope tokens');
    await expect(desktopWorkbenchBackend.execute({
      kind: 'review',
      requestId: 'rev-no-token',
      target,
      fileIds: ['f1'],
      modelId: 'm1',
      prompt: 'check error paths',
    })).rejects.toThrow('scope tokens');
    expect(invokeMock).toHaveBeenCalledTimes(2);

    // Valid planCPush & review round-trips verify target, modelId, prompt, locale, and commitLanguage:
    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      kind: 'cPush',
      requestId: 'plan-ok',
      preparedPlan: {
        summary: 'Prepared 1 commit',
        groups: [{ id: 'g1', fileIds: ['f1'], message: 'feat(pay): add idempotency check\n\n- guard duplicate callbacks' }],
        skippedFileIds: [],
      },
      session: {
        runId: 'plan-ok',
        repositoryId: 'r1',
        worktreeId: 'w1',
        sessionId: 's1',
        frozenScopeVersion: 'v1',
        modelId: 'm1',
        frozenFileIds: ['f1'],
        committedFileIds: [],
        skippedFileIds: [],
        sessionStatus: 'idle',
        message: null,
        timeline: [],
        qaLog: [],
        commits: [],
        canRetryRemaining: false,
      },
    });
    const planReply = await desktopWorkbenchBackend.execute({
      kind: 'planCPush',
      requestId: 'plan-ok',
      target,
      fileIds: ['f1'],
      selections: [{ fileId: 'f1', commitToken: 'tok-1' }],
      modelId: 'm1',
      prompt: 'write detailed bullets',
      locale: 'en',
      commitLanguage: 'zh',
    });
    expect(planReply).toMatchObject({
      kind: 'cPush',
      preparedPlan: {
        summary: 'Prepared 1 commit',
        groups: [{ id: 'g1', fileIds: ['f1'] }],
      },
    });
    expect(invokeMock.mock.calls[2][1]).toMatchObject({
      request: { prompt: 'write detailed bullets', locale: 'en', commitLanguage: 'zh' },
    });

    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      kind: 'review',
      requestId: 'rev-ok',
      target,
      modelId: 'm1',
      findings: [],
      skippedPaths: [],
      reviewedFileIds: ['f1'],
    });
    await desktopWorkbenchBackend.execute({
      kind: 'review',
      requestId: 'rev-ok',
      target,
      fileIds: ['f1'],
      selections: [{ fileId: 'f1', commitToken: 'tok-1' }],
      modelId: 'm1',
      prompt: 'focus on concurrency',
      locale: 'zh',
    });
    expect(invokeMock.mock.calls[3][1]).toMatchObject({
      request: { prompt: 'focus on concurrency', locale: 'zh' },
    });

    // Wrong target on review reply is rejected:
    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      kind: 'review',
      requestId: 'rev-bad-target',
      target: { ...target, worktreeId: 'other-wt' },
      modelId: 'm1',
      findings: [],
      skippedPaths: [],
      reviewedFileIds: ['f1'],
    });
    await expect(desktopWorkbenchBackend.execute({
      kind: 'review',
      requestId: 'rev-bad-target',
      target,
      fileIds: ['f1'],
      selections: [{ fileId: 'f1', commitToken: 'tok-1' }],
      modelId: 'm1',
      prompt: '',
    })).rejects.toThrow('mismatch');

    expect(decodeModelTestReply({
      protocolVersion: 1,
      provider: 'ollama',
      modelId: 'qwen2.5-coder:14b',
      summary: 'ok',
      latencyMs: 5,
    }).provider).toBe('ollama');
    expect(() => decodeReviewScopeReply({
      protocolVersion: 1,
      kind: 'review',
      requestId: 'r1',
      target,
      modelId: 'm1',
      findings: [{ id: '1', fileId: 'f1', path: 'a.ts', line: 0, severity: 'high', title: 't', reason: 'r' }],
      skippedPaths: [],
      reviewedFileIds: ['f1'],
    })).toThrow();
  });

  it('restores saved model catalog and activeModelId on startup hydration', () => {
    const restored = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
      models: [{ id: 'm-claude', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }],
      activeModelId: 'm-claude',
    });
    const hydrated = workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored });
    expect(hydrated.models).toHaveLength(1);
    expect(hydrated.activeModelId).toBe('m-claude');
  });
});
