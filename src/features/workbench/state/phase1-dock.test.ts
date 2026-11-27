import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { I18nProvider } from '../../../i18n';
import {
  DOCK_MIN_HEIGHT_PX,
  computeDraggedDockPreference,
  resolveDockSplit,
} from '../components/dockResize';
import { MessageDock } from '../components/MessageDock';
import { fixture } from '../testing/fixtures';
import { workbenchReducer, type Message } from './workbench';

describe('Direction 2 MessageDock dismiss, rich message_user blocks, and interactive Q&A rendering', () => {
  it('dismissMessage truly removes completed done message and returns session to idle while keeping history', () => {
    const state = fixture('done');
    const tree = state.worktrees[0];
    expect(tree.sessionStatus).toBe('done');
    expect(tree.messages.some((m) => m.kind === 'done')).toBe(true);
    const doneMsg = tree.messages.find((m) => m.kind === 'done')!;

    const next = workbenchReducer(state, {
      type: 'dismissMessage',
      worktreeId: tree.worktreeId,
      messageId: doneMsg.id,
    });
    const nextTree = next.worktrees[0];
    expect(nextTree.messages.some((m) => m.id === doneMsg.id)).toBe(false);
    expect(nextTree.dockExpanded).toBe(false);
    expect(nextTree.sessionStatus).toBe('idle');
  });

  it('renders inline dismiss button on collapsed done bar, Direction 2 rich card blocks, and multi-question UI', () => {
    const doneState = fixture('done');
    doneState.worktrees[0].dockExpanded = false;
    const collapsedHtml = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: doneState.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
        }),
      }),
    );
    expect(collapsedHtml).toContain('message-collapsed-dismiss');
    expect(collapsedHtml).toContain('关闭已完成提示');

    // Direction 2 rich card renders facts, findings with IDE jump, checklist, auto-collapsed long code, files, and header dismiss ×:
    const richState = fixture('rich-card');
    const richHtml = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: richState.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
          onFollowUpCustomAction: () => {},
        }),
      }),
    );
    expect(richHtml).toContain('d2-sheet');
    expect(richHtml).toContain('d2-grab-zone');
    expect(richHtml).toContain('role="separator"');
    expect(richHtml).not.toContain('撑满阅读');
    expect(richHtml).toContain('d2-facts-bar');
    expect(richHtml).toContain('Signature failure log prints full request body');
    expect(richHtml).toContain('在 IDEA 打开');
    expect(richHtml).toContain('d2-checklist-row');
    expect(richHtml).toContain('d2-code-well');
    expect(richHtml).toContain('10 行');
    expect(richHtml).toContain('关闭已完成提示');
    expect(richHtml).toContain('继续追问或补充下一步指令');
    expect(richHtml).not.toContain('知道了');

    const askState = fixture('question');
    const questionnaireMsg: Message = {
      id: 'ask-1',
      kind: 'question',
      title: 'Confirm submission options',
      description: '',
      questionnaire: {
        questions: [
          {
            id: 'q-1',
            header: 'Grouping',
            question: 'How should we group these changes?',
            options: [
              { id: 'split', label: 'Split into 2 PRs', description: 'Keep features isolated', recommended: true, preview: 'PR 1: pay\nPR 2: docs' },
              { id: 'single', label: 'Single PR', description: 'Ship together' },
            ],
          },
          {
            id: 'q-2',
            header: 'Labels',
            question: 'Choose labels to apply',
            multiSelect: true,
            options: [
              { id: 'feat', label: 'feat', description: 'New feature' },
              { id: 'docs', label: 'docs', description: 'Documentation update' },
            ],
          },
        ],
        context: {
          scope: '2 feature areas',
          allowDontAskAgain: true,
          dontAskAgainLabel: '此按钮不再提示',
        },
      },
    };
    askState.worktrees[0].messages = [questionnaireMsg];
    askState.worktrees[0].dockExpanded = true;

    const askHtml = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: askState.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
        }),
      }),
    );
    expect(askHtml).toContain('questionnaire-tabs');
    expect(askHtml).toContain('Grouping');
    expect(askHtml).toContain('Labels');
    expect(askHtml).toContain('推荐');
    expect(askHtml).toContain('预览');
    expect(askHtml).toContain('此按钮不再提示');
    expect(askHtml).toContain('提交选择');
    expect(askHtml).toContain('取消');

    // Single-select quick confirmation renders refined primary option shell + Recommended badge:
    const confirmState = fixture('question');
    confirmState.worktrees[0].dockExpanded = true;
    confirmState.worktrees[0].messages = [
      {
        id: 'confirm-write-1',
        kind: 'question',
        title: 'Confirm Write',
        description: 'Run write command?',
        questionnaire: {
          questions: [
            {
              id: 'confirm-write',
              header: 'Confirm Write',
              question: 'Run write command?',
              options: [
                { id: 'approve-write', label: 'Run command', description: 'Execute previewed command', recommended: true, preview: 'git push origin :feat/tmp' },
                { id: 'cancel-write', label: 'Cancel', description: 'Decline operation' },
              ],
            },
          ],
        },
      },
    ];
    const confirmHtml = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'en',
        children: createElement(MessageDock, {
          worktree: confirmState.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
        }),
      }),
    );
    expect(confirmHtml).toContain('d2-opt-shell is-primary');
    expect(confirmHtml).toContain('option-recommended-badge');
    expect(confirmHtml).toContain('Recommended');
  });

  it('omits header dismiss button on error cards with options and shows header dismiss on standalone errors', () => {
    const withOptions = fixture('question');
    withOptions.worktrees[0].sessionStatus = 'needsDecision';
    withOptions.worktrees[0].dockExpanded = true;
    withOptions.worktrees[0].messages = [
      {
        id: 'err-step-1',
        kind: 'error',
        title: '提交 1/1 暂停',
        description: '存在阻断错误',
        options: [{ id: 'dismiss-session', label: '关闭本次会话', recommended: true }],
      },
    ];
    const expandedWithOpts = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: withOptions.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
        }),
      }),
    );
    expect(expandedWithOpts).toContain('关闭本次会话');
    expect(expandedWithOpts).not.toContain('关闭已完成提示');

    withOptions.worktrees[0].dockExpanded = false;
    const collapsedWithOpts = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: withOptions.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
        }),
      }),
    );
    expect(collapsedWithOpts).not.toContain('message-collapsed-dismiss');

    const standaloneError = fixture('question');
    standaloneError.worktrees[0].dockExpanded = true;
    standaloneError.worktrees[0].messages = [
      {
        id: 'err-standalone',
        kind: 'error',
        title: '操作失败',
        description: '单次错误提示',
      },
    ];
    const expandedStandalone = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: standaloneError.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          fullHeight: false,
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onOpenRepoPath: () => {},
        }),
      }),
    );
    expect(expandedStandalone).toContain('关闭已完成提示');
  });

  it('computes and clamps draggable MessageDock split heights and full-height snapping cleanly', () => {
    expect(resolveDockSplit(null, 580)).toEqual({
      fullHeight: false,
      changesHeightPx: null,
      dockHeightPx: null,
    });
    expect(resolveDockSplit('full', 580)).toEqual({
      fullHeight: true,
      changesHeightPx: 0,
      dockHeightPx: 580,
    });
    expect(resolveDockSplit(300, 580)).toEqual({
      fullHeight: false,
      changesHeightPx: 280,
      dockHeightPx: 300,
    });
    expect(resolveDockSplit(90, 580)).toEqual({
      fullHeight: false,
      changesHeightPx: 580 - DOCK_MIN_HEIGHT_PX,
      dockHeightPx: DOCK_MIN_HEIGHT_PX,
    });
    expect(computeDraggedDockPreference(550, 580)).toBe('full');
    expect(computeDraggedDockPreference(260, 580)).toBe(260);
    expect(computeDraggedDockPreference(60, 580)).toBe(DOCK_MIN_HEIGHT_PX);
  });
});
