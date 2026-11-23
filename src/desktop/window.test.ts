import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const config = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const capability = JSON.parse(readFileSync(new URL('../../src-tauri/capabilities/main.json', import.meta.url), 'utf8'));

describe('macOS 原生合并标题栏', () => {
  it('保留原生窗口按钮，内容延伸到标题栏，不再显示独立标题', () => {
    expect(config.app.windows.find((window: { label: string }) => window.label === 'main')).toMatchObject({
      title: 'Picket', decorations: true, titleBarStyle: 'Overlay', hiddenTitle: true,
      // AppKit's inset is not the button's center; the screenshot showed y=16
      // sitting 5 logical pixels above the 38px header's content.
      trafficLightPosition: { x: 12, y: 21 },
    });
  });
  it('只补充窗口拖动权限，不开放前端文件或 shell 权限', () => {
    expect(capability.permissions).toEqual(['core:default', 'core:window:allow-set-always-on-top', 'core:window:allow-start-dragging']);
  });
});
