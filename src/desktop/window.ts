import { isTauri } from '@tauri-apps/api/core';
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
  compactHeight?: number;
  fullSize: WindowSize;
  placement: 'anchorTopLeft' | 'restorePosition';
  targetPosition?: WindowPosition;
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
}

export interface WindowController {
  readonly isDesktopWindow: boolean;
  readSnapshot(): Promise<WindowGeometrySnapshot | null>;
  applyWindowMode(options: ApplyWindowModeOptions): Promise<AppliedWindowGeometry>;
  adjustCompactHeight(height: number, locale?: AppLocale): Promise<void>;
  subscribeGeometry?(listener: (snapshot: { position: WindowPosition; size: WindowSize }) => void): () => void;
  notifyBackgroundAttention(params: { title: string; body: string }): Promise<boolean>;
  subscribeNotificationTap?(onTap: () => void): () => void;
}

const TITLEBAR_VISIBLE_HEIGHT = 38;
const MIN_VISIBLE_HORIZONTAL_OVERLAP = 80;

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
};

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

  if (options.compactMode) {
    const compactHeight = clampCompactHeight(options.compactHeight ?? WINDOW_DIMENSIONS.compactDefaultHeight);
    const targetSize: WindowSize = {
      width: WINDOW_DIMENSIONS.compactWidth,
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
  const win = env.window();
  const targetHeight = clampCompactHeight(height);
  const [scaleRaw, beforeOuterPos, beforeInnerSize] = await Promise.all([
    win.scaleFactor(),
    win.outerPosition(),
    win.innerSize(),
  ]);
  const scale = scaleRaw > 0 ? scaleRaw : 1;
  const currentHeight = Math.round(beforeInnerSize.height / scale);
  if (Math.abs(currentHeight - targetHeight) <= 1) return;
  await win.setMinSize(new LogicalSize(WINDOW_DIMENSIONS.compactWidth, WINDOW_DIMENSIONS.compactMinHeight));
  await win.setSize(new LogicalSize(WINDOW_DIMENSIONS.compactWidth, targetHeight));
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
        void getCurrentWindow().setFocus().catch(() => {});
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
  const window = getCurrentWindow();
  await window.setAlwaysOnTop(pinned);
  return window.isAlwaysOnTop();
}
