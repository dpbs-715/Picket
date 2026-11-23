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
  decodeGenerateCommitReply,
  decodeModelMutateReply,
  decodeModelSaveReply,
  decodeModelTestReply,
  decodeReviewScopeReply,
} from '../../shared/model-protocol';
import { decodeWorkbenchRestoreReply } from '../../shared/workbench-persistence-protocol';
import { emptyWorkbench, workbenchReducer } from '../features/workbench/state/workbench';

describe('M3 模型桌面协议与安全边界', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('拒绝在任何模型或恢复回执中夹带 apiKey 或 credentialRef', () => {
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
      protocolVersion: 1,
      editor: 'idea',
      activeWorktreeId: null,
      emptyDraft: '',
      emptyEditing: false,
      worktrees: [],
      snapshots: [],
      warning: null,
      models: [{ id: 'm1', name: 'GPT', provider: 'openai', modelId: 'gpt-4.1', apiKey: 'sk-leak' }],
      activeModelId: 'm1',
    })).toThrow();
  });

  it('连接测试、保存、切换、删除及生成与审查严格校验请求身份与范围凭证', async () => {
    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      provider: 'anthropic',
      modelId: 'claude-sonnet-4-20250514',
      summary: '连接成功：Anthropic · claude-sonnet-4-20250514（85 ms）',
      latencyMs: 85,
    });
    const testSummary = await desktopWorkbenchBackend.testModel({
      provider: 'anthropic',
      name: 'Claude',
      modelId: 'claude-sonnet-4-20250514',
      baseUrl: '',
      apiKey: 'sk-ant-1',
    });
    expect(testSummary).toContain('连接成功');

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
    })).rejects.toThrow('不匹配');

    // Missing per-file commitTokens on generate/review is rejected before IPC:
    const target = { repositoryId: 'r1', worktreeId: 'w1', sessionId: 's1', scopeVersion: 'v1' };
    await expect(desktopWorkbenchBackend.execute({
      kind: 'generate',
      requestId: 'gen-no-token',
      target,
      fileIds: ['f1'],
      modelId: 'm1',
      draft: '',
    })).rejects.toThrow('范围凭证');
    expect(invokeMock).toHaveBeenCalledTimes(2);

    // Valid generate & review round-trips verify target & modelId:
    invokeMock.mockResolvedValueOnce({
      protocolVersion: 1,
      kind: 'generate',
      requestId: 'gen-ok',
      target,
      modelId: 'm1',
      text: 'feat(pay): 增加幂等校验',
      skippedPaths: [],
    });
    const genReply = await desktopWorkbenchBackend.execute({
      kind: 'generate',
      requestId: 'gen-ok',
      target,
      fileIds: ['f1'],
      selections: [{ fileId: 'f1', commitToken: 'tok-1' }],
      modelId: 'm1',
      draft: '',
    });
    expect(genReply).toMatchObject({ kind: 'generate', text: 'feat(pay): 增加幂等校验' });

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
      draft: '',
    })).rejects.toThrow('不匹配');

    expect(decodeModelTestReply({
      protocolVersion: 1,
      provider: 'ollama',
      modelId: 'qwen2.5-coder:14b',
      summary: 'ok',
      latencyMs: 5,
    }).provider).toBe('ollama');
    expect(() => decodeGenerateCommitReply({
      protocolVersion: 1,
      kind: 'generate',
      requestId: 'g1',
      target,
      modelId: 'm1',
      text: '   ',
      skippedPaths: [],
    })).toThrow();
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

  it('启动水合恢复已保存的模型列表与当前选中模型', () => {
    const restored = decodeWorkbenchRestoreReply({
      protocolVersion: 1,
      editor: 'idea',
      activeWorktreeId: null,
      emptyDraft: '',
      emptyEditing: false,
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
