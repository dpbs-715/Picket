import { act, createElement } from 'react';
import type { Root } from 'react-dom/client';
import type { WindowController } from '../../../../desktop/window';
import { getMessages, LANGUAGE_OPTIONS } from '../../../../i18n/messages';
import { Workbench } from '../../Workbench';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchBackend } from '../../commands';
import type { WorkbenchState } from '../../state/workbench';
import { repositoryReply } from '../repository-fixture';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

export const tick = () => act(async () => {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
});

export function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

export function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent?.trim() === label,
  );
  assert(found, `Missing button: ${label}`);
  return found;
}

export async function clickElement(target: HTMLElement) {
  await act(async () => { target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); });
  await act(async () => { target.click(); });
}

export async function click(label: string) {
  const target = button(label);
  await act(async () => { target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); });
  await act(async () => { button(label).click(); });
}

export async function openSettingsTab(tab: 'general' | 'models' | 'skills') {
  const el = document.querySelector<HTMLButtonElement>(`[data-settings-tab="${tab}"]`);
  assert(el, `Missing settings tab: ${tab}`);
  await clickElement(el);
}

export function type(element: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value);
    element.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

export const text = () => document.querySelector('.workbench')!.textContent ?? '';

export function createControlledWindow(
  initialPos = { x: 180, y: 120 },
  initialSize = { width: 400, height: 700 },
) {
  let winPinned = false;
  let winPos = { ...initialPos };
  let winSize = { ...initialSize };
  let geometryListener: ((snap: { position: { x: number; y: number }; size: { width: number; height: number } }) => void) | undefined;
  let pendingGate: { promise: Promise<void>; release: () => void } | null = null;
  const notifications: { title: string; body: string }[] = [];
  const history: { pinned: boolean; compactMode: boolean; pos: { x: number; y: number }; size: { width: number; height: number } }[] = [];
  const controller: WindowController = {
    isDesktopWindow: true,
    async readSnapshot() {
      return { pinned: winPinned, focused: true, scaleFactor: 2, position: { ...winPos }, size: { ...winSize } };
    },
    async setPinned(nextPinned) {
      winPinned = nextPinned;
      return winPinned;
    },
    async applyWindowMode(opts) {
      const previousPosition = { ...winPos };
      const previousSize = { ...winSize };
      if (pendingGate) {
        const gate = pendingGate;
        pendingGate = null;
        await gate.promise;
      }
      winPinned = opts.pinned;
      winSize = opts.compactMode
        ? { width: opts.compactWidth ?? 400, height: opts.compactHeight ?? 86 }
        : { width: opts.fullSize.width, height: opts.fullSize.height };
      if (opts.placement === 'restorePosition' && opts.targetPosition) {
        winPos = { ...opts.targetPosition };
      }
      // placement === 'anchorTopLeft' keeps winPos (top-left) unchanged!
      history.push({ pinned: winPinned, compactMode: opts.compactMode, pos: { ...winPos }, size: { ...winSize } });
      return {
        pinned: winPinned,
        previousPosition,
        previousSize,
        position: { ...winPos },
        size: { ...winSize },
      };
    },
    async adjustCompactHeight(height) {
      winSize = { width: Math.max(400, winSize.width), height };
    },
    subscribeGeometry(listener) {
      geometryListener = listener;
      return () => { geometryListener = undefined; };
    },
    async notifyBackgroundAttention(payload) {
      notifications.push(payload);
      return true;
    },
  };
  return {
    controller,
    notifications,
    history,
    getPos: () => ({ ...winPos }),
    getSize: () => ({ ...winSize }),
    getPinned: () => winPinned,
    pauseNextApply() {
      let release!: () => void;
      const promise = new Promise<void>((resolve) => { release = resolve; });
      pendingGate = { promise, release };
      return release;
    },
    dragTo(x: number, y: number) {
      winPos = { x, y };
      geometryListener?.({ position: { ...winPos }, size: { ...winSize } });
    },
    resizeTo(width: number, height: number) {
      winSize = { width, height };
      geometryListener?.({ position: { ...winPos }, size: { ...winSize } });
    },
  };
}

export interface BrowserCheckHarness {
  root: Root;
  checks: string[];
  zh: ReturnType<typeof getMessages>;
  zhLanguageLabel: string;
  nextKey: () => number;
  nextReadSequence: () => number;
  createReadsBackend: () => WorkbenchBackend;
  setInstantMotion: (enabled: boolean) => void;
  waitUntil: (predicate: () => boolean, label: string | (() => string), timeoutMs?: number) => Promise<void>;
  mount: (
    initialState: WorkbenchState,
    backend?: WorkbenchBackend,
    windowController?: WindowController,
  ) => Promise<void>;
}

export function createBrowserCheckHarness(root: Root): BrowserCheckHarness {
  const checks: string[] = [];
  const zh = getMessages('zh');
  const zhLanguageLabel = LANGUAGE_OPTIONS.find((item) => item.id === 'zh')!.label;
  let key = 0;
  let readSequence = 0;
  const nextKey = () => ++key;
  const nextReadSequence = () => ++readSequence;

  const createReadsBackend = (): WorkbenchBackend => ({
    ...disconnectedBackend,
    repositoryReadsAvailable: true,
    async execute(command) {
      if (command.kind !== 'openRepository' && command.kind !== 'refresh') {
        return disconnectedBackend.execute(command);
      }
      const reply = repositoryReply(command.requestId, nextReadSequence());
      if (command.kind === 'refresh') {
        reply.target = command.target;
        reply.snapshot!.activeWorktreeId = command.target.worktreeId;
      }
      return reply;
    },
  });

  const setInstantMotion = (enabled: boolean) => {
    (globalThis as { __PICKET_INSTANT_MOTION__?: boolean }).__PICKET_INSTANT_MOTION__ = enabled;
  };

  const waitUntil = async (predicate: () => boolean, label: string | (() => string), timeoutMs = 1500) => {
    const start = Date.now();
    while (!predicate()) {
      if (Date.now() - start > timeoutMs) {
        const detail = typeof label === 'function' ? label() : label;
        throw new Error(`Timed out waiting for motion check: ${detail}`);
      }
      await act(async () => {
        await new Promise<void>((resolve) => setTimeout(resolve, 25));
      });
    }
  };

  async function mount(
    initialState: WorkbenchState,
    backend: WorkbenchBackend = disconnectedBackend,
    windowController?: WindowController,
  ) {
    await act(async () => {
      root.render(
        createElement(Workbench, {
          key: nextKey(),
          initialState,
          backend,
          ...(windowController ? { windowController } : {}),
        }),
      );
    });
  }

  return {
    root,
    checks,
    zh,
    zhLanguageLabel,
    nextKey,
    nextReadSequence,
    createReadsBackend,
    setInstantMotion,
    waitUntil,
    mount,
  };
}
