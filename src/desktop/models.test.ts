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
  decodeModelSaveReply,
  decodeModelTestReply,
  decodeProviderRevealKeyReply,
  decodeProviderSaveReply,
  decodeReviewScopeReply,
  MODEL_PROTOCOL_VERSION,
} from '../../shared/model-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import { emptyWorkbench, workbenchReducer } from '../features/workbench/state/workbench';

describe('M3 desktop model protocol, provider connections, and security boundaries', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('rejects any provider, model, or restore reply that leaks apiKey or credentialRef', () => {
    expect(() => decodeModelSaveReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      activeModelId: 'm1',
      connections: [{ id: 'c1', name: 'Anthropic', provider: 'anthropic', keyHint: 'sk-a…1111' }],
      model: { id: 'm1', connectionId: 'c1', name: 'Claude', provider: 'anthropic', modelId: 'claude', apiKey: 'sk-leak' },
      models: [{ id: 'm1', connectionId: 'c1', name: 'Claude', provider: 'anthropic', modelId: 'claude' }],
    })).toThrow();
    expect(() => decodeModelMutateReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      activeModelId: 'm1',
      connections: [{ id: 'c1', name: 'Anthropic', provider: 'anthropic', credentialRef: 'cred-1' }],
      models: [{ id: 'm1', connectionId: 'c1', name: 'Claude', provider: 'anthropic', modelId: 'claude' }],
    })).toThrow();
    expect(() => decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
      connections: [{ id: 'c1', name: 'OpenAI', provider: 'openai', apiKey: 'sk-leak' }],
      models: [{ id: 'm1', connectionId: 'c1', name: 'GPT', provider: 'openai', modelId: 'gpt-4.1' }],
      activeModelId: 'm1',
    })).toThrow();
    expect(() => decodeProviderRevealKeyReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      connectionId: 'c1',
      outcome: 'cancelled',
      message: 'Cancelled',
      apiKey: 'fake-leak-on-cancel',
    })).toThrow();
    expect(() => decodeProviderRevealKeyReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      connectionId: 'c1',
      outcome: 'authenticated',
      apiKey: 'fake-revealed-key-0001',
      keyHint: 'fake…0001',
      credentialRef: 'cred-leak',
    })).toThrow();
    expect(decodeProviderRevealKeyReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      connectionId: 'c1',
      outcome: 'authenticated',
      apiKey: 'fake-revealed-key-0001',
      keyHint: 'fake…0001',
    })).toEqual({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      connectionId: 'c1',
      outcome: 'authenticated',
      apiKey: 'fake-revealed-key-0001',
      keyHint: 'fake…0001',
    });
    expect(decodeProviderSaveReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      activeModelId: 'm1',
      connection: { id: 'c1', name: 'Company Gateway', provider: 'custom', baseUrl: 'https://gw.example.com/v1', keyHint: 'fake…0001' },
      model: { id: 'm1', connectionId: 'c1', name: 'Fast Model', provider: 'custom', modelId: 'team-fast', baseUrl: 'https://gw.example.com/v1' },
      connections: [{ id: 'c1', name: 'Company Gateway', provider: 'custom', baseUrl: 'https://gw.example.com/v1', keyHint: 'fake…0001' }],
      models: [
        { id: 'm1', connectionId: 'c1', name: 'Fast Model', provider: 'custom', modelId: 'team-fast', baseUrl: 'https://gw.example.com/v1' },
        { id: 'm2', connectionId: 'c1', name: 'Review Model', provider: 'custom', modelId: 'team-review', baseUrl: 'https://gw.example.com/v1' },
      ],
    }).models).toHaveLength(2);
  });

  it('strictly verifies request identity, prompt/locale/commitLanguage forwarding, and scope tokens', async () => {
    invokeMock.mockResolvedValueOnce({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      provider: 'anthropic',
      modelId: 'claude-sonnet-4-20250514',
      summary: 'Connected: Anthropic · claude-sonnet-4-20250514 (85 ms)',
      latencyMs: 85,
    });
    const testSummary = await desktopWorkbenchBackend.testModel({
      kind: 'newConnection',
      provider: 'anthropic',
      modelId: 'claude-sonnet-4-20250514',
      baseUrl: '',
      apiKey: 'sk-ant-1',
      locale: 'en',
    });
    expect(testSummary).toContain('Connected');

    // Mismatched provider on testModel is rejected:
    invokeMock.mockResolvedValueOnce({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      provider: 'openai',
      modelId: 'claude-sonnet-4-20250514',
      summary: 'wrong',
      latencyMs: 10,
    });
    await expect(desktopWorkbenchBackend.testModel({
      kind: 'newConnection',
      provider: 'anthropic',
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
      protocolVersion: MODEL_PROTOCOL_VERSION,
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
      protocolVersion: MODEL_PROTOCOL_VERSION,
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
      protocolVersion: MODEL_PROTOCOL_VERSION,
      provider: 'ollama',
      modelId: 'qwen2.5-coder:14b',
      summary: 'ok',
      latencyMs: 5,
    }).provider).toBe('ollama');
    expect(() => decodeReviewScopeReply({
      protocolVersion: MODEL_PROTOCOL_VERSION,
      kind: 'review',
      requestId: 'r1',
      target,
      modelId: 'm1',
      findings: [{ id: '1', fileId: 'f1', path: 'a.ts', line: 0, severity: 'high', title: 't', reason: 'r' }],
      skippedPaths: [],
      reviewedFileIds: ['f1'],
    })).toThrow();
  });

  it('restores saved connections, models, and activeModelId on startup hydration', () => {
    const restored = decodeWorkbenchRestoreReply({
      protocolVersion: 2,
      editor: 'idea',
      activeWorktreeId: null,
      emptyPrompt: '',
      worktrees: [],
      snapshots: [],
      warning: null,
      connections: [{ id: 'conn-1', name: 'Anthropic', provider: 'anthropic', keyHint: 'sk-a…9999' }],
      models: [{ id: 'm-claude', connectionId: 'conn-1', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' }],
      activeModelId: 'm-claude',
    });
    const hydrated = workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored });
    expect(hydrated.connections).toHaveLength(1);
    expect(hydrated.models).toHaveLength(1);
    expect(hydrated.activeModelId).toBe('m-claude');
  });
});
