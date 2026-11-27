import { describe, expect, it } from 'vitest';
import { SPLASH_THEME_PALETTES, resolveSplashTheme } from './WhaleSplashStage';
import {
  computeEyeLookTarget,
  computeParabolicProgress,
  evaluateAimPose,
  evaluateBouncePose,
  evaluateEntrancePose,
  evaluateLandingPose,
  evaluateLeapPose,
  targetFacingForDelta,
  type SplashLaunchPlan,
} from './whaleMath';

const rightCompactPlan: SplashLaunchPlan = {
  dx: 420,
  dy: -240,
  distance: Math.hypot(420, -240),
  arcHeight: 140,
  leapDurationMs: 480,
  compactMode: true,
  workbenchAlreadyReady: false,
};

const leftCompactPlan: SplashLaunchPlan = {
  dx: -480,
  dy: -180,
  distance: Math.hypot(-480, -180),
  arcHeight: 145,
  leapDurationMs: 490,
  compactMode: true,
  workbenchAlreadyReady: false,
};

describe('whaleMath', () => {
  it('flips facing left only when the target window is meaningfully to the left', () => {
    expect(targetFacingForDelta(300)).toBe(1);
    expect(targetFacingForDelta(0)).toBe(1);
    expect(targetFacingForDelta(-20)).toBe(1);
    expect(targetFacingForDelta(-120)).toBe(-1);
  });

  it('computes parabolic progress monotonically matching Rust trajectory endpoints', () => {
    expect(computeParabolicProgress(0)).toBeCloseTo(0, 6);
    expect(computeParabolicProgress(0.5)).toBeCloseTo(0.5, 6);
    expect(computeParabolicProgress(1)).toBeCloseTo(1, 6);
    expect(computeParabolicProgress(0.25)).toBeGreaterThan(0.15);
    expect(computeParabolicProgress(0.25)).toBeLessThan(0.35);
  });

  it('animates entrance from emerging water ripple to settled upright pose with a blink', () => {
    const start = evaluateEntrancePose(0);
    const peak = evaluateEntrancePose(0.44);
    const blink = evaluateEntrancePose(0.85);
    const end = evaluateEntrancePose(1);

    expect(start.opacity).toBe(0);
    expect(start.offsetY).toBeGreaterThan(15);
    expect(peak.offsetY).toBeLessThan(-10);
    expect(blink.eyeScaleY).toBeLessThan(0.3);
    expect(end.opacity).toBe(1);
    expect(end.offsetY).toBeCloseTo(0, 2);
    expect(end.scaleX).toBeCloseTo(1, 2);
    expect(end.scaleY).toBeCloseTo(1, 2);
  });

  it('bounces with crouch anticipation, high apex, shadow reduction, and cushioned landing', () => {
    const crouch = evaluateBouncePose(0.16, 0);
    const apex = evaluateBouncePose(0.52, 0);
    const landed = evaluateBouncePose(0.79, 0);
    const reset = evaluateBouncePose(1, 0);

    expect(crouch.offsetY).toBeGreaterThan(2);
    expect(crouch.scaleX).toBeGreaterThan(1.08);
    expect(crouch.scaleY).toBeLessThan(0.92);

    expect(apex.offsetY).toBeLessThan(-32);
    expect(apex.shadowScale).toBeLessThan(crouch.shadowScale);
    expect(apex.shadowOpacity).toBeLessThan(crouch.shadowOpacity);

    expect(landed.scaleX).toBeGreaterThan(1.08);
    expect(landed.scaleY).toBeLessThan(0.9);
    expect(reset.offsetY).toBeCloseTo(0, 4);
  });

  it('turns smoothly toward left-side targets during aim and shifts gaze in local facing space', () => {
    const startAim = evaluateAimPose(0, leftCompactPlan);
    const endAim = evaluateAimPose(1, leftCompactPlan);
    const look = computeEyeLookTarget(leftCompactPlan.dx, leftCompactPlan.dy, -1);

    expect(startAim.facing).toBe(1);
    expect(endAim.facing).toBe(-1);
    expect(endAim.scaleX).toBeGreaterThan(1.1);
    expect(endAim.scaleY).toBeLessThan(0.86);
    // In local mirrored coordinates, looking forward toward the target has positive eyeOffsetX:
    expect(look.x).toBeGreaterThan(8);
    expect(look.y).toBeLessThan(0);
  });

  it('pitches nose upward at leap takeoff, curves over apex, and melts into landing ripples', () => {
    const takeoff = evaluateLeapPose(0.22, rightCompactPlan);
    const dive = evaluateLeapPose(0.98, rightCompactPlan);
    const landMid = evaluateLandingPose(0.3, rightCompactPlan);
    const landEnd = evaluateLandingPose(1, rightCompactPlan);

    expect(takeoff.rotateDeg).toBeLessThan(-10);
    expect(dive.rotateDeg).toBeGreaterThan(10);
    expect(dive.shadowOpacity).toBe(0);

    expect(landMid.primaryRipple.opacity).toBeGreaterThan(0.3);
    expect(landMid.droplets.some((drop) => drop.opacity > 0.2)).toBe(true);
    expect(landEnd.opacity).toBe(0);
    expect(landEnd.primaryRipple.opacity).toBeCloseTo(0, 4);
  });

  it('resolves splash theme palettes distinctly for dark and light themes', () => {
    expect(resolveSplashTheme('light')).toBe('light');
    expect(resolveSplashTheme('dark')).toBe('dark');
    expect(resolveSplashTheme(undefined)).toBe('dark');
    expect(resolveSplashTheme('unknown')).toBe('dark');

    expect(SPLASH_THEME_PALETTES.dark.bodyStops).toEqual(['#FFFFFF', '#F1F5FF', '#DCE6FA']);
    expect(SPLASH_THEME_PALETTES.light.bodyStops).toEqual(['#5B7DB1', '#3B5988', '#253D63']);
    expect(SPLASH_THEME_PALETTES.light.bodyStops[0]).not.toBe(SPLASH_THEME_PALETTES.dark.bodyStops[0]);
  });
});
