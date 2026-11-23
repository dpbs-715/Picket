import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { Workbench } from './Workbench';
import { fixture, scenarios } from './testing/fixtures';

const render = (scenario: typeof scenarios[number]) => renderToStaticMarkup(createElement(Workbench, { initialState: fixture(scenario) }));

describe('定稿工作台展示', () => {
  it.each(scenarios)('可渲染 %s 样例，不含演练操作或凭据', (scenario) => {
    const html = render(scenario);
    expect(html).toContain('aria-label="Picket 工作台"');
    expect(html).toContain('aria-label="变更文件"');
    expect(html).toContain('role="tablist"');
    expect(html).not.toMatch(/模拟修复|模拟返回|演练控制台|推进至提问|sk-demo/);
  });
  it('仅头部空白是拖动区，不把按钮或整个工作台当作拖动区', () => {
    const html = render('typing');
    expect(html).toMatch(/class="context-row context-top" data-tauri-drag-region="true"/);
    expect(html).toMatch(/class="menu-anchor repository-anchor" data-tauri-drag-region="true"/);
    expect(html).not.toMatch(/data-tauri-drag-region="deep"|<button[^>]*data-tauri-drag-region/);
    expect(html).not.toMatch(/class="context-row context-bottom" data-tauri-drag-region/);
  });
  it('默认流程不预置示例数据，无仓库与零模型均可到达', () => {
    const html = renderToStaticMarkup(createElement(Workbench));
    expect(html).toContain('打开本机仓库…');
    expect(html).toContain('手动编写');
    expect(html).toContain('添加模型');
    expect(html).not.toMatch(/my-order-service|Order\.ts|Claude Sonnet 4|upstream\/feat\/pay|a19e7c2/);
  });
  it('执行中、提问、续写、完成四个时刻和核心异常有独立结构', () => {
    expect(render('running')).toContain('任务进度');
    expect(render('question')).toContain('单独提交并一起推送');
    expect(render('typing')).toContain('textarea');
    expect(render('done')).toContain('知道了');
    expect(render('push-failed')).toContain('已在本地提交，推送失败');
    expect(render('push-failed')).toContain('a19e7c2');
    expect(render('conflict')).toContain('让 Agent 合并');
    expect(render('manual')).toContain('打开冲突文件');
    expect(render('resolved')).toContain('继续推送');
    expect(render('terminal-done')).not.toContain('aria-label="当前事项"');
  });
});
