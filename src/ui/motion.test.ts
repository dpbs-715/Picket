import { afterEach, describe, expect, it } from 'vitest';
import { MOTION_DURATION, MOTION_EASE, isInstantMotionEnabled } from './motion';

describe('shared motion configuration', () => {
  afterEach(() => {
    delete (globalThis as { __PICKET_INSTANT_MOTION__?: boolean }).__PICKET_INSTANT_MOTION__;
  });

  it('keeps crisp deceleration curve and restrained durations under 200ms', () => {
    expect(MOTION_EASE).toEqual([0.22, 1, 0.36, 1]);
    expect(MOTION_DURATION.fast).toBeLessThan(0.15);
    expect(MOTION_DURATION.base).toBeLessThanOrEqual(0.16);
    expect(MOTION_DURATION.overlay).toBeLessThanOrEqual(0.18);
    expect(MOTION_DURATION.window).toBeLessThanOrEqual(0.2);
  });

  it('supports instant motion override and falls back to instant when document is hidden (occluded WebView)', () => {
    expect(isInstantMotionEnabled()).toBe(false);
    (globalThis as { __PICKET_INSTANT_MOTION__?: boolean }).__PICKET_INSTANT_MOTION__ = true;
    expect(isInstantMotionEnabled()).toBe(true);
    delete (globalThis as { __PICKET_INSTANT_MOTION__?: boolean }).__PICKET_INSTANT_MOTION__;

    const originalDoc = globalThis.document;
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: { visibilityState: 'hidden' },
    });
    try {
      expect(isInstantMotionEnabled()).toBe(true);
    } finally {
      Object.defineProperty(globalThis, 'document', {
        configurable: true,
        value: originalDoc,
      });
    }
  });
});
