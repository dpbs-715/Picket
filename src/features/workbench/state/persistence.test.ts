import { describe, expect, it } from 'vitest';
import type { WorkbenchRestoreReply } from '../../../../shared/workbench-persistence-protocol';
import { repositorySnapshot } from '../testing/repository-fixture';
import { readScopeVersion, serializeWorkbenchPersistence } from './repositories';
import { emptyWorkbench, workbenchReducer } from './workbench';

describe('工作台跨重启持久化投影与水合', () => {
  it('恢复首选编辑器、活动工作树、各工作树草稿与空选择，并检测关停期间的范围变化', () => {
    const snapshot = repositorySnapshot(1);
    const unchangedScope = readScopeVersion({
      ...snapshot.worktrees[1],
      selectedIds: [],
      scopeVersion: snapshot.worktrees[1].statusVersion,
    });
    const restored: WorkbenchRestoreReply = {
      protocolVersion: 1,
      editor: 'webstorm',
      activeWorktreeId: 'fix-tree',
      emptyDraft: '无仓库草稿',
      emptyEditing: true,
      snapshots: [snapshot],
      warning: null,
      worktrees: [
        {
          repositoryId: 'orders',
          worktreeId: 'pay-tree',
          draftText: '关闭前写好的提交说明',
          draftEditing: true,
          draftOutdated: false,
          selectedIds: ['order'],
          scopeVersion: 'old-scope-before-external-edit',
          tab: 'review',
        },
        {
          repositoryId: 'orders',
          worktreeId: 'fix-tree',
          draftText: '保持未过期的说明',
          draftEditing: true,
          draftOutdated: false,
          selectedIds: [],
          scopeVersion: unchangedScope,
          tab: 'commit',
        },
      ],
    };

    const hydrated = workbenchReducer(emptyWorkbench(), { type: 'workbenchHydrated', restored });
    expect(hydrated.editor).toBe('webstorm');
    expect(hydrated.activeWorktreeId).toBe('fix-tree');
    expect(hydrated.emptyDraft).toBe('无仓库草稿');

    const payTree = hydrated.worktrees.find((tree) => tree.worktreeId === 'pay-tree')!;
    expect(payTree.selectedIds).toEqual(['order']);
    expect(payTree.tab).toBe('review');
    expect(payTree.draft).toMatchObject({ text: '关闭前写好的提交说明', editing: true, outdated: true });

    const fixTree = hydrated.worktrees.find((tree) => tree.worktreeId === 'fix-tree')!;
    expect(fixTree.selectedIds).toEqual([]);
    expect(fixTree.draft).toMatchObject({ text: '保持未过期的说明', editing: true, outdated: false });
  });

  it('异步水合晚于用户操作到达时，不覆盖用户在启动后已修改的编辑器和草稿', () => {
    let state = emptyWorkbench();
    state = workbenchReducer(state, { type: 'editor', editor: 'cursor' });
    state = workbenchReducer(state, { type: 'draft', worktreeId: null, text: '刚输入的新草稿' });
    state = workbenchReducer(state, {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 1,
        editor: 'idea',
        activeWorktreeId: null,
        emptyDraft: '旧草稿',
        emptyEditing: false,
        snapshots: [],
        worktrees: [],
        warning: null,
      },
    });
    expect(state.editor).toBe('cursor');
    expect(state.emptyDraft).toBe('刚输入的新草稿');
  });

  it('保存投影只输出白名单字段，不携带路径、写凭证或模型信息', () => {
    const snapshot = repositorySnapshot(1);
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 1,
        editor: 'vscode',
        activeWorktreeId: 'pay-tree',
        emptyDraft: '',
        emptyEditing: false,
        snapshots: [snapshot],
        worktrees: [],
        warning: null,
      },
    });
    const payload = serializeWorkbenchPersistence(hydrated, 5);
    const serialized = JSON.stringify(payload);
    expect(payload.revision).toBe(5);
    expect(payload.editor).toBe('vscode');
    expect(serialized).not.toMatch(/commitToken|branchToken|path|apiKey/);
  });

  it('根据本机已安装编辑器列表过滤可用项，并在原保存编辑器未安装时自动回退至首个已安装项', () => {
    const hydrated = workbenchReducer(emptyWorkbench(), {
      type: 'workbenchHydrated',
      restored: {
        protocolVersion: 1,
        editor: 'cursor',
        installedEditors: ['idea', 'rustrover', 'webstorm'],
        activeWorktreeId: null,
        emptyDraft: '',
        emptyEditing: false,
        snapshots: [],
        worktrees: [],
        warning: null,
      },
    });
    expect(hydrated.installedEditors).toEqual(['idea', 'rustrover', 'webstorm']);
    expect(hydrated.editor).toBe('idea');
  });
});
