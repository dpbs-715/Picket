import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { I18nProvider } from '../../../i18n';
import { MessageDock } from '../components/MessageDock';
import { fixture } from '../testing/fixtures';
import { workbenchReducer, type Message } from './workbench';

describe('Phase 1 MessageDock dismiss and ask_user_question rendering', () => {
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

  it('renders inline dismiss button on collapsed done bar and ask_user_question multi-question UI', () => {
    const doneState = fixture('done');
    doneState.worktrees[0].dockExpanded = false;
    const collapsedHtml = renderToStaticMarkup(
      createElement(I18nProvider, {
        locale: 'zh',
        children: createElement(MessageDock, {
          worktree: doneState.worktrees[0],
          busy: false,
          editorShort: 'IDEA',
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onReview: () => {},
        }),
      }),
    );
    expect(collapsedHtml).toContain('message-collapsed-dismiss');
    expect(collapsedHtml).toContain('关闭已完成提示');

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
          onExpanded: () => {},
          onDismiss: () => {},
          onReply: () => {},
          onRespond: () => {},
          onOpenFile: () => {},
          onReview: () => {},
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
  });
});
