import { readFileSync } from 'node:fs';
import { LogicalPosition, PhysicalPosition } from '@tauri-apps/api/dpi';
import { describe, expect, it } from 'vitest';
import {
  COMPACT_DRAWER_ANIMATION_MS,
  WINDOW_MODE_ANIMATION_MS,
  adjustCompactHeightWith,
  applyWindowModeWith,
  clampCompactHeight,
  desktopWindowController,
  normalizeCompactWidth,
  resolveVisibleWindowPosition,
  setWindowPinned,
  setWindowPinnedWith,
  type AnimateWindowFrameRequest,
  type MonitorWorkArea,
  type NativeWindowEnvironment,
} from './window';

const config = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const capability = JSON.parse(readFileSync(new URL('../../src-tauri/capabilities/main.json', import.meta.url), 'utf8'));

function createMockWindowEnv(initial?: {
  pinned?: boolean;
  scaleFactor?: number;
  outerPos?: { x: number; y: number };
  innerSize?: { width: number; height: number };
  monitors?: MonitorWorkArea[];
  primary?: MonitorWorkArea;
}) {
  let pinned = initial?.pinned ?? false;
  let visibleOnAllWorkspaces = initial?.pinned ?? false;
  const scaleFactor = initial?.scaleFactor ?? 2;
  let outerPos = { ...(initial?.outerPos ?? { x: 240, y: 180 }) };
  let innerSize = { ...(initial?.innerSize ?? { width: 800, height: 1400 }) };
  let minSize = { width: 400, height: 480 };
  const calls: string[] = [];

  const env: NativeWindowEnvironment = {
    window: () => ({
      isAlwaysOnTop: async () => pinned,
      setAlwaysOnTop: async (next) => {
        calls.push(`pin:${next}`);
        pinned = next;
      },
      setVisibleOnAllWorkspaces: async (visible) => {
        calls.push(`allSpaces:${visible}`);
        visibleOnAllWorkspaces = visible;
      },
      isFocused: async () => true,
      setFocus: async () => {
        calls.push('focus');
      },
      scaleFactor: async () => scaleFactor,
      outerPosition: async () => ({ ...outerPos }),
      innerSize: async () => ({ ...innerSize }),
      setMinSize: async (size) => {
        calls.push(`minSize:${size.width}x${size.height}`);
        minSize = { width: size.width, height: size.height };
      },
      setSize: async (size) => {
        calls.push(`size:${size.width}x${size.height}`);
        // Simulate macOS bottom-left origin shift during setContentSize unless top-left is re-anchored:
        const nextPhysHeight = size.height * scaleFactor;
        outerPos = {
          x: outerPos.x,
          y: outerPos.y + (innerSize.height - nextPhysHeight),
        };
        innerSize = {
          width: size.width * scaleFactor,
          height: nextPhysHeight,
        };
      },
      setPosition: async (pos) => {
        if (pos instanceof PhysicalPosition) {
          calls.push(`physPos:${pos.x},${pos.y}`);
          outerPos = { x: pos.x, y: pos.y };
        } else if (pos instanceof LogicalPosition) {
          calls.push(`logPos:${pos.x},${pos.y}`);
          outerPos = { x: pos.x * scaleFactor, y: pos.y * scaleFactor };
        }
      },
    }),
    monitors: async () => ({
      available: initial?.monitors ?? [{ x: 0, y: 25, width: 1512, height: 945 }],
      primary: initial?.primary ?? { x: 0, y: 25, width: 1512, height: 945 },
    }),
  };

  return {
    env,
    calls,
    getOuterPos: () => outerPos,
    getInnerSize: () => innerSize,
    getMinSize: () => minSize,
    isVisibleOnAllWorkspaces: () => visibleOnAllWorkspaces,
  };
}

describe('macOS native overlaid titlebar and compact window configuration', () => {
  it('keeps native window controls, 400x700 default size, and 400x480 full-mode minimum size', () => {
    expect(config.app.windows.find((window: { label: string }) => window.label === 'main')).toMatchObject({
      title: 'Picket',
      width: 400,
      height: 700,
      minWidth: 400,
      minHeight: 480,
      decorations: true,
      titleBarStyle: 'Overlay',
      hiddenTitle: true,
      trafficLightPosition: { x: 12, y: 21 },
      dragDropEnabled: false,
    });
  });

  it('grants only minimal window geometry, focus, pin, drag, and notification permissions without shell or filesystem access', () => {
    expect(capability.permissions).toEqual([
      'core:default',
      'core:window:allow-set-always-on-top',
      'core:window:allow-start-dragging',
      'core:window:allow-set-size',
      'core:window:allow-set-min-size',
      'core:window:allow-set-position',
      'core:window:allow-set-focus',
      'notification:default',
    ]);
  });

  it('keeps top-left physical coordinates fixed when collapsing and expanding on macOS Retina displays', async () => {
    const mock = createMockWindowEnv({ outerPos: { x: 241, y: 183 }, innerSize: { width: 800, height: 1400 }, scaleFactor: 2 });

    // Collapse to 2-row compact mode (~86px): lowers minSize BEFORE setSize and re-anchors exact physical top-left:
    const collapsed = await applyWindowModeWith(mock.env, {
      pinned: true,
      compactMode: true,
      compactHeight: 86,
      fullSize: { width: 400, height: 700 },
      placement: 'anchorTopLeft',
    });
    expect(collapsed.pinned).toBe(true);
    expect(collapsed.size).toEqual({ width: 400, height: 86 });
    expect(mock.getOuterPos()).toEqual({ x: 241, y: 183 });
    expect(mock.calls.indexOf('minSize:400x82')).toBeLessThan(mock.calls.indexOf('size:400x86'));

    // Temporarily expand compact height for project/files drawer (~240px) while preserving physical top-left:
    await adjustCompactHeightWith(mock.env, 240);
    expect(mock.getInnerSize()).toEqual({ width: 800, height: 480 });
    expect(mock.getOuterPos()).toEqual({ x: 241, y: 183 });

    // Shrink back to 86px when drawer closes while keeping exact physical top-left:
    await adjustCompactHeightWith(mock.env, 86);
    expect(mock.getInnerSize()).toEqual({ width: 800, height: 172 });
    expect(mock.getOuterPos()).toEqual({ x: 241, y: 183 });

    // Expand back to full mode: restores 400x700 and 480 minHeight while keeping exact top-left:
    const expanded = await applyWindowModeWith(mock.env, {
      pinned: true,
      compactMode: false,
      fullSize: { width: 400, height: 700 },
      placement: 'anchorTopLeft',
    });
    expect(expanded.size).toEqual({ width: 400, height: 700 });
    expect(mock.getMinSize()).toEqual({ width: 400, height: 480 });
    expect(mock.getOuterPos()).toEqual({ x: 241, y: 183 });
  });

  it('restores custom compact width, height, and saved position after temporarily expanding for standalone Settings', async () => {
    const mock = createMockWindowEnv({
      outerPos: { x: 420, y: 1200 },
      innerSize: { width: 920, height: 180 },
      scaleFactor: 2,
    });

    // 1. Expand from compact (460x90 logical at 210,600) to full size for Settings:
    const opened = await applyWindowModeWith(mock.env, {
      pinned: false,
      compactMode: false,
      fullSize: { width: 400, height: 700 },
      placement: 'anchorTopLeft',
    });
    expect(opened.previousPosition).toEqual({ x: 210, y: 600 });
    expect(opened.previousSize).toEqual({ width: 460, height: 90 });
    expect(opened.size).toEqual({ width: 400, height: 700 });

    // 2. Restore exact pre-settings compact width, height, and logical position:
    const restored = await applyWindowModeWith(mock.env, {
      pinned: false,
      compactMode: true,
      compactWidth: normalizeCompactWidth(opened.previousSize.width),
      compactHeight: clampCompactHeight(opened.previousSize.height),
      fullSize: { width: 400, height: 700 },
      placement: 'restorePosition',
      targetPosition: opened.previousPosition,
    });
    expect(restored.size).toEqual({ width: 460, height: 90 });
    expect(restored.position).toEqual({ x: 210, y: 600 });

    // 3. Drawer height adjustment preserves custom compact width (460) instead of snapping back to 400:
    await adjustCompactHeightWith(mock.env, 210);
    expect(mock.getInnerSize()).toEqual({ width: 920, height: 420 });
  });

  it('falls back to primary monitor work area when saved position belongs to a disconnected display', () => {
    const laptop: MonitorWorkArea = { x: 0, y: 25, width: 1512, height: 945 };
    const externalRight: MonitorWorkArea = { x: 1512, y: 0, width: 2560, height: 1440 };

    // Visible on connected secondary display -> preserved as-is:
    expect(
      resolveVisibleWindowPosition({ x: 2200, y: 120 }, { width: 400, height: 140 }, [laptop, externalRight], laptop),
    ).toEqual({ x: 2200, y: 120 });

    // External display unplugged -> clamped cleanly onto primary display workArea:
    expect(
      resolveVisibleWindowPosition({ x: 2200, y: -200 }, { width: 400, height: 700 }, [laptop], laptop),
    ).toEqual({ x: 1112, y: 25 });

    // Compact height is clamped safely within bounds (82..280):
    expect(clampCompactHeight(40)).toBe(82);
    expect(clampCompactHeight(300)).toBe(280);
  });

  it('delegates smooth frame transitions to native animateFrame and respects instant & reduced-motion flags', async () => {
    const requests: AnimateWindowFrameRequest[] = [];
    let reduced = false;
    const baseMock = createMockWindowEnv({
      monitors: [{ x: 0, y: 25, width: 1512, height: 945 }],
      primary: { x: 0, y: 25, width: 1512, height: 945 },
    });
    const animatedEnv: NativeWindowEnvironment = {
      ...baseMock.env,
      prefersReducedMotion: () => reduced,
      animateFrame: async (req) => {
        requests.push(req);
        return {
          pinned: req.pinned ?? true,
          previousPosition: { x: 200, y: 120 },
          previousSize: { width: 400, height: 700 },
          position: { x: req.targetX ?? 200, y: req.targetY ?? 120 },
          size: { width: req.width ?? 400, height: req.height },
          animated: req.durationMs > 0,
        };
      },
    };

    // Interactive collapse animates smoothly with top-left anchoring:
    const collapsed = await applyWindowModeWith(animatedEnv, {
      pinned: true,
      compactMode: true,
      compactHeight: 86,
      fullSize: { width: 400, height: 700 },
      placement: 'anchorTopLeft',
    });
    expect(collapsed.size).toEqual({ width: 400, height: 86 });
    expect(requests.at(-1)).toMatchObject({
      width: 400,
      height: 86,
      minWidth: 400,
      minHeight: 82,
      placement: 'anchorTopLeft',
      durationMs: WINDOW_MODE_ANIMATION_MS,
      pinned: true,
    });

    // Startup hydration with `instant: true` uses durationMs: 0 and clamps off-screen coordinates:
    await applyWindowModeWith(animatedEnv, {
      pinned: false,
      compactMode: false,
      fullSize: { width: 400, height: 700 },
      placement: 'restorePosition',
      targetPosition: { x: 2400, y: -100 },
      instant: true,
    });
    expect(requests.at(-1)).toMatchObject({
      width: 400,
      height: 700,
      minWidth: 400,
      minHeight: 480,
      placement: 'restorePosition',
      targetX: 1112,
      targetY: 25,
      durationMs: 0,
      pinned: false,
    });

    // Compact drawer height adjustment animates without overriding width or pin state:
    await adjustCompactHeightWith(animatedEnv, 240);
    expect(requests.at(-1)).toEqual({
      height: 240,
      minWidth: 400,
      minHeight: 82,
      placement: 'anchorTopLeft',
      durationMs: COMPACT_DRAWER_ANIMATION_MS,
    });

    // Reduced motion forces durationMs: 0 for both mode switch and compact drawer resize:
    reduced = true;
    await adjustCompactHeightWith(animatedEnv, 86);
    expect(requests.at(-1)?.durationMs).toBe(0);
    await applyWindowModeWith(animatedEnv, {
      pinned: false,
      compactMode: false,
      fullSize: { width: 400, height: 700 },
      placement: 'anchorTopLeft',
    });
    expect(requests.at(-1)?.durationMs).toBe(0);
  });

  it('syncs cross-workspace visibility with pin toggles via native setPinned or fallback handle', async () => {
    const delegated: boolean[] = [];
    const baseMock = createMockWindowEnv({ pinned: false });
    const nativeEnv: NativeWindowEnvironment = {
      ...baseMock.env,
      setPinned: async (next) => {
        delegated.push(next);
        return next;
      },
    };
    expect(await setWindowPinnedWith(nativeEnv, true)).toBe(true);
    expect(await setWindowPinnedWith(nativeEnv, false)).toBe(false);
    expect(delegated).toEqual([true, false]);
    expect(baseMock.calls).toEqual([]);

    // Fallback handle toggles always-on-top and all-workspaces visibility in lockstep:
    expect(await setWindowPinnedWith(baseMock.env, true)).toBe(true);
    expect(baseMock.isVisibleOnAllWorkspaces()).toBe(true);
    expect(baseMock.calls).toEqual(['pin:true', 'allSpaces:true']);

    expect(await setWindowPinnedWith(baseMock.env, false)).toBe(false);
    expect(baseMock.isVisibleOnAllWorkspaces()).toBe(false);
    expect(baseMock.calls).toEqual(['pin:true', 'allSpaces:true', 'pin:false', 'allSpaces:false']);

    await applyWindowModeWith(baseMock.env, {
      pinned: true,
      compactMode: true,
      compactHeight: 86,
      fullSize: { width: 400, height: 700 },
      placement: 'anchorTopLeft',
    });
    expect(baseMock.isVisibleOnAllWorkspaces()).toBe(true);
  });

  it('reports explicit unsupported errors instead of faking native window operations in browser preview', async () => {
    await expect(setWindowPinned(true, 'en')).rejects.toThrow(/unavailable in browser preview/i);
    await expect(desktopWindowController.setPinned(true, 'en')).rejects.toThrow(/unavailable in browser preview/i);
    await expect(
      desktopWindowController.applyWindowMode({
        pinned: true,
        compactMode: true,
        fullSize: { width: 400, height: 700 },
        placement: 'anchorTopLeft',
        locale: 'zh',
      }),
    ).rejects.toThrow(/\u6d4f\u89c8\u5668\u9884\u89c8\u4e0d\u652f\u6301/);
  });
});
