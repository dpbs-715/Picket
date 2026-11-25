import { invoke, isTauri } from '@tauri-apps/api/core';
import { LogicalPosition, LogicalSize, PhysicalPosition } from '@tauri-apps/api/dpi';
import { availableMonitors, getCurrentWindow, primaryMonitor, type Monitor } from '@tauri-apps/api/window';
import { isPermissionGranted, onAction, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { DEFAULT_APP_LOCALE, type AppLocale } from '../../shared/language-protocol';
import {
  WINDOW_DIMENSIONS,
  type WindowPosition,
  type WindowSize,
} from '../../shared/workbench-persistence-protocol';
import { getMessages } from '../i18n/messages';

export const WINDOW_MODE_ANIMATION_MS = 185;
export const COMPACT_DRAWER_ANIMATION_MS = 145;

export interface AnimateWindowFrameRequest {
  width?: number;
  height: number;
  minWidth: number;
  minHeight: number;
  placement: 'anchorTopLeft' | 'restorePosition';
  targetX?: number;
  targetY?: number;
  durationMs: number;
  pinned?: boolean;
}

export interface AnimateWindowFrameReply {
  pinned: boolean;
  previousPosition: WindowPosition;
  previousSize: WindowSize;
  position: WindowPosition;
  size: WindowSize;
  animated: boolean;
}

export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export interface MonitorWorkArea {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface WindowGeometrySnapshot {
  pinned: boolean;
  focused: boolean;
  position: WindowPosition;
  size: WindowSize;
  scaleFactor: number;
}

export interface ApplyWindowModeOptions {
  pinned: boolean;
  compactMode: boolean;
  compactWidth?: number;
  compactHeight?: number;
  fullSize: WindowSize;
  placement: 'anchorTopLeft' | 'restorePosition';
  targetPosition?: WindowPosition;
  /** Skip native frame transition (e.g. cold startup hydration). */
  instant?: boolean;
  locale?: AppLocale;
}

export interface AppliedWindowGeometry {
  pinned: boolean;
  previousPosition: WindowPosition;
  previousSize: WindowSize;
  position: WindowPosition;
  size: WindowSize;
}

export interface NativeWindowHandle {
  isAlwaysOnTop(): Promise<boolean>;
  setAlwaysOnTop(alwaysOnTop: boolean): Promise<void>;
  setVisibleOnAllWorkspaces?(visible: boolean): Promise<void>;
  isFocused(): Promise<boolean>;
  setFocus(): Promise<void>;
  scaleFactor(): Promise<number>;
  outerPosition(): Promise<{ x: number; y: number }>;
  innerSize(): Promise<{ width: number; height: number }>;
  setMinSize(size: LogicalSize): Promise<void>;
  setSize(size: LogicalSize): Promise<void>;
  setPosition(position: LogicalPosition | PhysicalPosition): Promise<void>;
  onMoved?(handler: () => void): Promise<() => void>;
  onResized?(handler: () => void): Promise<() => void>;
}

export interface NativeWindowEnvironment {
  window(): NativeWindowHandle;
  monitors(): Promise<{ available: MonitorWorkArea[]; primary?: MonitorWorkArea }>;
  setPinned?(pinned: boolean): Promise<boolean>;
  animateFrame?(request: AnimateWindowFrameRequest): Promise<AnimateWindowFrameReply>;
  prefersReducedMotion?(): boolean;
}

export interface WindowController {
  readonly isDesktopWindow: boolean;
  readSnapshot(): Promise<WindowGeometrySnapshot | null>;
  setPinned(pinned: boolean, locale?: AppLocale): Promise<boolean>;
  applyWindowMode(options: ApplyWindowModeOptions): Promise<AppliedWindowGeometry>;
  adjustCompactHeight(height: number, locale?: AppLocale): Promise<void>;
  notifyWorkbenchReady?(): Promise<void>;
  subscribeGeometry?(listener: (snapshot: { position: WindowPosition; size: WindowSize }) => void): () => void;
  notifyBackgroundAttention(params: { title: string; body: string }): Promise<boolean>;
  subscribeNotificationTap?(onTap: () => void): () => void;
}

const TITLEBAR_VISIBLE_HEIGHT = 38;
const MIN_VISIBLE_HORIZONTAL_OVERLAP = 80;

export function normalizeCompactWidth(width?: number): number {
  if (!width || !Number.isFinite(width)) return WINDOW_DIMENSIONS.compactWidth;
  return Math.min(
    WINDOW_DIMENSIONS.maxStoredDimension,
    Math.max(WINDOW_DIMENSIONS.compactWidth, Math.round(width)),
  );
}

export function clampCompactHeight(height: number): number {
  if (!Number.isFinite(height)) return WINDOW_DIMENSIONS.compactDefaultHeight;
  return Math.min(
    WINDOW_DIMENSIONS.compactMaxHeight,
    Math.max(WINDOW_DIMENSIONS.compactMinHeight, Math.round(height)),
  );
}

export function normalizeFullWindowSize(size?: WindowSize): WindowSize {
  if (!size || !Number.isFinite(size.width) || !Number.isFinite(size.height)) {
    return {
      width: WINDOW_DIMENSIONS.defaultFullWidth,
      height: WINDOW_DIMENSIONS.defaultFullHeight,
    };
  }
  return {
    width: Math.min(WINDOW_DIMENSIONS.maxStoredDimension, Math.max(WINDOW_DIMENSIONS.minFullWidth, Math.round(size.width))),
    height: Math.min(WINDOW_DIMENSIONS.maxStoredDimension, Math.max(WINDOW_DIMENSIONS.minFullHeight, Math.round(size.height))),
  };
}

export function resolveVisibleWindowPosition(
  desired: WindowPosition | undefined,
  size: WindowSize,
  monitors: readonly MonitorWorkArea[],
  primary?: MonitorWorkArea,
): WindowPosition | undefined {
  if (!desired || !Number.isFinite(desired.x) || !Number.isFinite(desired.y)) {
    return undefined;
  }
  const pool = monitors.length > 0 ? monitors : primary ? [primary] : [];
  if (pool.length === 0) {
    return { x: Math.round(desired.x), y: Math.round(desired.y) };
  }
  const requiredOverlap = Math.min(MIN_VISIBLE_HORIZONTAL_OVERLAP, Math.max(40, size.width));
  const visibleOnSomeMonitor = pool.some((monitor) => {
    const overlapLeft = Math.max(desired.x, monitor.x);
    const overlapRight = Math.min(desired.x + size.width, monitor.x + monitor.width);
    const horizontalOk = overlapRight - overlapLeft >= requiredOverlap;
    const verticalOk = desired.y >= monitor.y && desired.y + TITLEBAR_VISIBLE_HEIGHT <= monitor.y + monitor.height;
    return horizontalOk && verticalOk;
  });
  if (visibleOnSomeMonitor) {
    return { x: Math.round(desired.x), y: Math.round(desired.y) };
  }
  const fallback = primary ?? pool[0];
  const maxX = Math.max(fallback.x, fallback.x + fallback.width - size.width);
  const maxY = Math.max(fallback.y, fallback.y + fallback.height - size.height);
  return {
    x: Math.min(Math.max(Math.round(desired.x), fallback.x), maxX),
    y: Math.min(Math.max(Math.round(desired.y), fallback.y), maxY),
  };
}

function monitorToLogicalWorkArea(monitor: Monitor): MonitorWorkArea {
  const factor = monitor.scaleFactor > 0 ? monitor.scaleFactor : 1;
  const area = monitor.workArea ?? { position: monitor.position, size: monitor.size };
  return {
    x: Math.round(area.position.x / factor),
    y: Math.round(area.position.y / factor),
    width: Math.round(area.size.width / factor),
    height: Math.round(area.size.height / factor),
  };
}

const tauriEnvironment: NativeWindowEnvironment = {
  window: () => getCurrentWindow(),
  async monitors() {
    const [all, primary] = await Promise.all([
      availableMonitors().catch(() => [] as Monitor[]),
      primaryMonitor().catch(() => null),
    ]);
    return {
      available: all.map(monitorToLogicalWorkArea),
      primary: primary ? monitorToLogicalWorkArea(primary) : undefined,
    };
  },
  setPinned: (pinned) => invoke<boolean>('set_window_pinned', { pinned }),
  animateFrame: (request) => invoke<AnimateWindowFrameReply>('animate_window_frame', { request }),
  prefersReducedMotion,
};

export async function setWindowPinnedWith(
  env: NativeWindowEnvironment,
  pinned: boolean,
): Promise<boolean> {
  if (env.setPinned) {
    return env.setPinned(pinned);
  }
  const win = env.window();
  await win.setAlwaysOnTop(pinned);
  if (win.setVisibleOnAllWorkspaces) {
    await win.setVisibleOnAllWorkspaces(pinned);
  }
  return win.isAlwaysOnTop();
}

export async function readWindowSnapshotWith(env: NativeWindowEnvironment): Promise<WindowGeometrySnapshot> {
  const win = env.window();
  const [pinned, focused, scaleFactorRaw, outerPos, innerSize] = await Promise.all([
    win.isAlwaysOnTop(),
    win.isFocused(),
    win.scaleFactor(),
    win.outerPosition(),
    win.innerSize(),
  ]);
  const scaleFactor = scaleFactorRaw > 0 ? scaleFactorRaw : 1;
  return {
    pinned,
    focused,
    scaleFactor,
    position: {
      x: Math.round(outerPos.x / scaleFactor),
      y: Math.round(outerPos.y / scaleFactor),
    },
    size: {
      width: Math.round(innerSize.width / scaleFactor),
      height: Math.round(innerSize.height / scaleFactor),
    },
  };
}

export async function applyWindowModeWith(
  env: NativeWindowEnvironment,
  options: ApplyWindowModeOptions,
): Promise<AppliedWindowGeometry> {
  if (env.animateFrame) {
    const targetSize: WindowSize = options.compactMode
      ? {
          width: normalizeCompactWidth(options.compactWidth),
          height: clampCompactHeight(options.compactHeight ?? WINDOW_DIMENSIONS.compactDefaultHeight),
        }
      : normalizeFullWindowSize(options.fullSize);
    const minSize: WindowSize = options.compactMode
      ? { width: WINDOW_DIMENSIONS.compactWidth, height: WINDOW_DIMENSIONS.compactMinHeight }
      : { width: WINDOW_DIMENSIONS.minFullWidth, height: WINDOW_DIMENSIONS.minFullHeight };
    let placement: 'anchorTopLeft' | 'restorePosition' = options.placement;
    let targetX: number | undefined;
    let targetY: number | undefined;
    if (options.placement === 'restorePosition') {
      if (options.targetPosition) {
        const { available, primary } = await env.monitors();
        const visible = resolveVisibleWindowPosition(options.targetPosition, targetSize, available, primary);
        if (visible) {
          targetX = visible.x;
          targetY = visible.y;
        } else {
          placement = 'anchorTopLeft';
        }
      } else {
        placement = 'anchorTopLeft';
      }
    }
    const reduced = env.prefersReducedMotion ? env.prefersReducedMotion() : prefersReducedMotion();
    const durationMs = options.instant || reduced ? 0 : WINDOW_MODE_ANIMATION_MS;
    const reply = await env.animateFrame({
      width: targetSize.width,
      height: targetSize.height,
      minWidth: minSize.width,
      minHeight: minSize.height,
      placement,
      ...(targetX !== undefined && targetY !== undefined ? { targetX, targetY } : {}),
      durationMs,
      pinned: options.pinned,
    });
    return {
      pinned: reply.pinned,
      previousPosition: reply.previousPosition,
      previousSize: reply.previousSize,
      position: reply.position,
      size: reply.size,
    };
  }

  const win = env.window();
  const [beforeScaleRaw, beforeOuterPos, beforeInnerSize] = await Promise.all([
    win.scaleFactor(),
    win.outerPosition(),
    win.innerSize(),
  ]);
  const beforeScale = beforeScaleRaw > 0 ? beforeScaleRaw : 1;
  const previousPosition: WindowPosition = {
    x: Math.round(beforeOuterPos.x / beforeScale),
    y: Math.round(beforeOuterPos.y / beforeScale),
  };
  const previousSize: WindowSize = {
    width: Math.round(beforeInnerSize.width / beforeScale),
    height: Math.round(beforeInnerSize.height / beforeScale),
  };

  await win.setAlwaysOnTop(options.pinned);
  if (win.setVisibleOnAllWorkspaces) {
    await win.setVisibleOnAllWorkspaces(options.pinned);
  }

  if (options.compactMode) {
    const compactWidth = normalizeCompactWidth(options.compactWidth);
    const compactHeight = clampCompactHeight(options.compactHeight ?? WINDOW_DIMENSIONS.compactDefaultHeight);
    const targetSize: WindowSize = {
      width: compactWidth,
      height: compactHeight,
    };
    await win.setMinSize(new LogicalSize(WINDOW_DIMENSIONS.compactWidth, WINDOW_DIMENSIONS.compactMinHeight));
    await win.setSize(new LogicalSize(targetSize.width, targetSize.height));
    if (options.placement === 'anchorTopLeft') {
      await win.setPosition(new PhysicalPosition(beforeOuterPos.x, beforeOuterPos.y));
    } else if (options.targetPosition) {
      const { available, primary } = await env.monitors();
      const visible = resolveVisibleWindowPosition(options.targetPosition, targetSize, available, primary);
      if (visible) {
        await win.setPosition(new LogicalPosition(visible.x, visible.y));
      }
    }
  } else {
    const fullSize = normalizeFullWindowSize(options.fullSize);
    await win.setSize(new LogicalSize(fullSize.width, fullSize.height));
    await win.setMinSize(new LogicalSize(WINDOW_DIMENSIONS.minFullWidth, WINDOW_DIMENSIONS.minFullHeight));
    if (options.placement === 'anchorTopLeft') {
      await win.setPosition(new PhysicalPosition(beforeOuterPos.x, beforeOuterPos.y));
    } else if (options.targetPosition) {
      const { available, primary } = await env.monitors();
      const visible = resolveVisibleWindowPosition(options.targetPosition, fullSize, available, primary);
      if (visible) {
        await win.setPosition(new LogicalPosition(visible.x, visible.y));
      }
    }
  }

  const [afterPinned, afterScaleRaw, afterOuterPos, afterInnerSize] = await Promise.all([
    win.isAlwaysOnTop(),
    win.scaleFactor(),
    win.outerPosition(),
    win.innerSize(),
  ]);
  const afterScale = afterScaleRaw > 0 ? afterScaleRaw : 1;
  return {
    pinned: afterPinned,
    previousPosition,
    previousSize,
    position: {
      x: Math.round(afterOuterPos.x / afterScale),
      y: Math.round(afterOuterPos.y / afterScale),
    },
    size: {
      width: Math.round(afterInnerSize.width / afterScale),
      height: Math.round(afterInnerSize.height / afterScale),
    },
  };
}

export async function adjustCompactHeightWith(
  env: NativeWindowEnvironment,
  height: number,
): Promise<void> {
  const targetHeight = clampCompactHeight(height);
  if (env.animateFrame) {
    const reduced = env.prefersReducedMotion ? env.prefersReducedMotion() : prefersReducedMotion();
    await env.animateFrame({
      height: targetHeight,
      minWidth: WINDOW_DIMENSIONS.compactWidth,
      minHeight: WINDOW_DIMENSIONS.compactMinHeight,
      placement: 'anchorTopLeft',
      durationMs: reduced ? 0 : COMPACT_DRAWER_ANIMATION_MS,
    });
    return;
  }
  const win = env.window();
  const [scaleRaw, beforeOuterPos, beforeInnerSize] = await Promise.all([
    win.scaleFactor(),
    win.outerPosition(),
    win.innerSize(),
  ]);
  const scale = scaleRaw > 0 ? scaleRaw : 1;
  const currentWidth = normalizeCompactWidth(Math.round(beforeInnerSize.width / scale));
  const currentHeight = Math.round(beforeInnerSize.height / scale);
  if (Math.abs(currentHeight - targetHeight) <= 1) return;
  await win.setMinSize(new LogicalSize(WINDOW_DIMENSIONS.compactWidth, WINDOW_DIMENSIONS.compactMinHeight));
  await win.setSize(new LogicalSize(currentWidth, targetHeight));
  await win.setPosition(new PhysicalPosition(beforeOuterPos.x, beforeOuterPos.y));
}

function isUiSmokeSuppressingSystemNotifications(): boolean {
  return typeof globalThis !== 'undefined'
    && Boolean((globalThis as { __PICKET_UI_SMOKE__?: boolean }).__PICKET_UI_SMOKE__);
}

export function createDesktopWindowController(): WindowController {
  let programmaticDepth = 0;
  let suppressUntil = 0;

  const runProgrammatic = async <T>(work: () => Promise<T>): Promise<T> => {
    programmaticDepth += 1;
    try {
      return await work();
    } finally {
      programmaticDepth -= 1;
      suppressUntil = Date.now() + 350;
    }
  };

  return {
    get isDesktopWindow() {
      return isTauri();
    },
    async readSnapshot() {
      if (!isTauri()) return null;
      return readWindowSnapshotWith(tauriEnvironment);
    },
    async setPinned(pinned, locale = DEFAULT_APP_LOCALE) {
      if (!isTauri()) {
        throw new Error(getMessages(locale).desktop.browserPinUnsupported);
      }
      return runProgrammatic(() => setWindowPinned(pinned, locale));
    },
    async applyWindowMode(options) {
      if (!isTauri()) {
        throw new Error(getMessages(options.locale ?? DEFAULT_APP_LOCALE).desktop.browserWindowUnsupported);
      }
      return runProgrammatic(() => applyWindowModeWith(tauriEnvironment, options));
    },
    async adjustCompactHeight(height, locale = DEFAULT_APP_LOCALE) {
      if (!isTauri()) {
        throw new Error(getMessages(locale).desktop.browserWindowUnsupported);
      }
      await runProgrammatic(() => adjustCompactHeightWith(tauriEnvironment, height));
    },
    async notifyWorkbenchReady() {
      if (!isTauri()) return;
      await invoke('notify_workbench_ready').catch(() => {});
    },
    subscribeGeometry(listener) {
      if (!isTauri()) return () => {};
      const win = getCurrentWindow();
      let disposed = false;
      let timer: ReturnType<typeof setTimeout> | undefined;
      let unlistenMove: (() => void) | undefined;
      let unlistenResize: (() => void) | undefined;

      const scheduleRead = () => {
        if (disposed || programmaticDepth > 0 || Date.now() < suppressUntil) return;
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
          if (disposed || programmaticDepth > 0 || Date.now() < suppressUntil) return;
          void readWindowSnapshotWith(tauriEnvironment)
            .then((snap) => {
              if (!disposed && programmaticDepth === 0 && Date.now() >= suppressUntil) {
                listener({ position: snap.position, size: snap.size });
              }
            })
            .catch(() => {});
        }, 120);
      };

      void win.onMoved(scheduleRead).then((fn) => {
        if (disposed) fn();
        else unlistenMove = fn;
      }).catch(() => {});
      void win.onResized(scheduleRead).then((fn) => {
        if (disposed) fn();
        else unlistenResize = fn;
      }).catch(() => {});

      return () => {
        disposed = true;
        if (timer) clearTimeout(timer);
        unlistenMove?.();
        unlistenResize?.();
      };
    },
    async notifyBackgroundAttention({ title, body }) {
      if (!isTauri() || isUiSmokeSuppressingSystemNotifications()) return false;
      if (typeof document !== 'undefined' && document.hasFocus()) return false;
      try {
        const win = getCurrentWindow();
        if (await win.isFocused()) return false;
        let granted = await isPermissionGranted();
        if (!granted) {
          granted = (await requestPermission()) === 'granted';
        }
        if (!granted) return false;
        sendNotification({ title, body });
        return true;
      } catch {
        return false;
      }
    },
    subscribeNotificationTap(onTap) {
      if (!isTauri() || isUiSmokeSuppressingSystemNotifications()) return () => {};
      let disposed = false;
      let cleanup: (() => void) | undefined;
      void onAction(() => {
        if (disposed) return;
        void invoke('tray_open_main_window', { action: { kind: 'show' } }).catch(() => {
          void getCurrentWindow().setFocus().catch(() => {});
        });
        onTap();
      })
        .then((listener) => {
          if (disposed) {
            void listener.unregister().catch(() => {});
          } else {
            cleanup = () => {
              void listener.unregister().catch(() => {});
            };
          }
        })
        .catch(() => {});
      return () => {
        disposed = true;
        cleanup?.();
      };
    },
  };
}

export const desktopWindowController: WindowController = createDesktopWindowController();

export async function readWindowPinned(): Promise<boolean> {
  return isTauri() ? getCurrentWindow().isAlwaysOnTop() : false;
}

export async function setWindowPinned(pinned: boolean, locale: AppLocale = DEFAULT_APP_LOCALE): Promise<boolean> {
  if (!isTauri()) throw new Error(getMessages(locale).desktop.browserPinUnsupported);
  return setWindowPinnedWith(tauriEnvironment, pinned);
}
