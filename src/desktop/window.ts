import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

export async function readWindowPinned(): Promise<boolean> {
  return isTauri() ? getCurrentWindow().isAlwaysOnTop() : false;
}
export async function setWindowPinned(pinned: boolean): Promise<boolean> {
  if (!isTauri()) throw new Error('浏览器预览不能设置窗口置顶。请使用 pnpm desktop dev 打开桌面应用。');
  const window = getCurrentWindow();
  await window.setAlwaysOnTop(pinned);
  return window.isAlwaysOnTop();
}
