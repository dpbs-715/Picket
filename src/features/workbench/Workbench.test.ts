import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { getMessages } from '../../i18n/messages';
import { Workbench } from './Workbench';
import { fixture, scenarios } from './testing/fixtures';

const render = (scenario: typeof scenarios[number]) => renderToStaticMarkup(createElement(Workbench, { initialState: fixture(scenario) }));

describe('Workbench static rendering', () => {
  it.each(scenarios)('renders %s scenario without rehearsal controls, manual commit dropdowns, or demo secrets', (scenario) => {
    const html = render(scenario);
    expect(html).toContain('aria-label="Picket Workbench"');
    if (scenario === 'compact') {
      expect(html).toContain('aria-label="Compact Workbench"');
      expect(html).toContain('aria-label="Expand Workbench"');
      expect(html).toContain('aria-label="Settings"');
      expect(html).not.toContain('prompt-textarea');
      expect(html).not.toContain('aria-label="Changed Files"');
      expect(html).not.toContain('role="tablist"');
      expect(html).not.toContain('aria-label="C-Push Session History"');
    } else {
      expect(html).toContain('aria-label="Changed Files"');
      expect(html).toContain('aria-label="Collapse to Compact Mode"');
      expect(html).toContain('aria-label="Settings"');
      expect(html).toContain('role="tablist"');
    }
    expect(html).not.toContain('action-dropdown');
    expect(html).not.toMatch(/\u6a21\u62df\u4fee\u590d|\u6a21\u62df\u8fd4\u56de|\u6f14\u7ec3\u63a7\u5236\u53f0|\u63a8\u8fdb\u81f3\u63d0\u95ee|Simulate Fix|Simulation Console|sk-demo/);
  });

  it('marks only header blank space as drag region and never buttons or the entire workbench in both full and compact modes', () => {
    const html = render('typing');
    expect(html).toMatch(/class="context-row context-top" data-tauri-drag-region="true"/);
    expect(html).toMatch(/class="menu-anchor repository-anchor" data-tauri-drag-region="true"/);
    expect(html).not.toMatch(/data-tauri-drag-region="deep"|<button[^>]*data-tauri-drag-region/);
    expect(html).not.toMatch(/class="context-row context-bottom" data-tauri-drag-region/);

    const compactHtml = render('compact');
    expect(compactHtml).toMatch(/class="compact-top" data-tauri-drag-region="true"/);
    expect(compactHtml).not.toMatch(/data-tauri-drag-region="deep"|<button[^>]*data-tauri-drag-region/);
  });

  it('starts in English zero-model empty state with collapsed + Prompt toggle and minimalist Configure Model button', () => {
    const html = renderToStaticMarkup(createElement(Workbench));
    expect(html).toContain('Open Local Repository…');
    expect(html).toContain('Configure Model');
    expect(html).toContain('Add Model');
    expect(html).toContain('+ Prompt');
    expect(html).not.toContain('prompt-textarea');
    expect(html).not.toContain('Or click C-Push below');
    expect(html).not.toContain('Write Manually');
    expect(html).not.toMatch(/my-order-service|Order\.ts|Claude Sonnet 4|upstream\/feat\/pay|a19e7c2/);
  });

  it('renders localized Chinese UI when state.locale is zh', () => {
    const zhMessages = getMessages('zh');
    const zhState = { ...fixture('empty'), locale: 'zh' as const };
    const html = renderToStaticMarkup(createElement(Workbench, { initialState: zhState }));
    expect(html).toContain(`aria-label="${zhMessages.workbench.ariaLabel}"`);
    expect(html).toContain(zhMessages.changes.openLocalRepository);
    expect(html).toContain(zhMessages.commit.configureModelButton);
    expect(html).toContain(zhMessages.commit.promptToggle);
  });

  it('renders distinct structures for running, planning, question, editable plan, done, zero-model, and recovery states', () => {
    expect(render('running')).toContain('Task progress');
    expect(render('generating')).toContain('Generating commit plan…');
    expect(render('generating')).toContain('+ Prompt');
    expect(render('question')).toContain('Commit separately &amp; push together');
    expect(render('typing')).toContain('Plan · 1 commit');
    expect(render('typing')).toContain('plan-group-textarea');
    expect(render('typing')).toContain('prompt-textarea');
    expect(render('no-model')).toContain('Configure Model');
    expect(render('done')).toContain('Got it');
    expect(render('push-failed')).toContain('Committed locally, push failed');
    expect(render('push-failed')).toContain('a19e7c2');
    expect(render('conflict')).toContain('Let Agent Merge');
    expect(render('manual')).toContain('Open conflicted file');
    expect(render('resolved')).toContain('Continue Push');
    expect(render('terminal-done')).not.toContain('aria-label="Active Session"');
  });
});
