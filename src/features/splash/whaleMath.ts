/**
 * Pure pose & choreography math for Picket's floating whale launch mascot.
 * Designed for a relaxed, expressive, bouncy personality with squash-and-stretch,
 * organic blinking, directional gaze anticipation, parabolic flight tilt, and
 * luminous water-ripple transitions.
 */

export type SplashPhase = 'entering' | 'bouncing' | 'aiming' | 'leaping' | 'landing' | 'done';

export interface SplashLaunchPlan {
  dx: number;
  dy: number;
  distance: number;
  arcHeight: number;
  leapDurationMs: number;
  compactMode: boolean;
  workbenchAlreadyReady: boolean;
}

export interface RipplePose {
  scale: number;
  opacity: number;
}

export interface DropletPose {
  x: number;
  y: number;
  scale: number;
  opacity: number;
}

export interface WhaleFramePose {
  /** Horizontal offset in logical px inside the 220x220 stage. */
  offsetX: number;
  /** Vertical offset in logical px (negative = upward). */
  offsetY: number;
  /** Horizontal squash/stretch multiplier (always positive; facing flips separately). */
  scaleX: number;
  /** Vertical squash/stretch multiplier. */
  scaleY: number;
  /** Facing multiplier interpolated smoothly between +1 (right) and -1 (left). */
  facing: number;
  /** Body rotation in degrees (applied in facing space so positive pitches nose down). */
  rotateDeg: number;
  /** Whale silhouette opacity [0, 1]. */
  opacity: number;
  /** Eye horizontal gaze offset in 1024-unit SVG space. */
  eyeOffsetX: number;
  /** Eye vertical gaze offset in 1024-unit SVG space. */
  eyeOffsetY: number;
  /** Eye vertical scale for blinking / happy leap squint [0.08, 1.05]. */
  eyeScaleY: number;
  /** Soft ground shadow horizontal scale & opacity. */
  shadowScale: number;
  shadowOpacity: number;
  /** Primary white water ripple and secondary indigo accent ripple. */
  primaryRipple: RipplePose;
  secondaryRipple: RipplePose;
  /** Three delicate micro-droplets on pop-out and landing splash. */
  droplets: [DropletPose, DropletPose, DropletPose];
}

export const SPLASH_TIMING = {
  entranceMs: 440,
  bounceCycleMs: 640,
  aimMs: 170,
  landingMs: 240,
  minCompletedBounces: 1,
} as const;

const LEFT_FLIP_THRESHOLD_PX = -36;
const IN_PLACE_DISTANCE_THRESHOLD_PX = 36;

export function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function easeOutCubic(t: number): number {
  const p = clamp01(t);
  return 1 - (1 - p) ** 3;
}

export function easeInOutSine(t: number): number {
  const p = clamp01(t);
  return 0.5 * (1 - Math.cos(Math.PI * p));
}

export function easeOutBack(t: number, overshoot = 1.45): number {
  const p = clamp01(t) - 1;
  return 1 + (overshoot + 1) * p ** 3 + overshoot * p ** 2;
}

/**
 * Matches Rust `compute_parabolic_jump_sample` horizontal blend so the native window flight
 * and SVG body pitch stay tightly synchronized.
 */
export function computeParabolicProgress(t: number): number {
  const p = clamp01(t);
  return 0.65 * p + 0.35 * easeInOutSine(p);
}

export function targetFacingForDelta(dx: number): 1 | -1 {
  return dx < LEFT_FLIP_THRESHOLD_PX ? -1 : 1;
}

export function computeEyeLookTarget(dx: number, dy: number, facing: 1 | -1): { x: number; y: number } {
  const dist = Math.hypot(dx, dy);
  if (dist < 12) {
    return { x: 0, y: -6 };
  }
  // Convert world dx into local facing space (because the whale group mirrors via scaleX(facing)).
  const localDx = dx * facing;
  const nx = localDx / dist;
  const ny = dy / dist;
  return {
    x: clampRange(nx * 16, -12, 18),
    y: clampRange(ny * 14, -14, 14),
  };
}

function clampRange(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function blinkScaleAt(t: number, start: number, end: number): number {
  if (t <= start || t >= end) return 1;
  const mid = (start + end) * 0.5;
  const half = (end - start) * 0.5;
  const normalized = Math.abs(t - mid) / half; // 0 at closed peak, 1 at open edges
  return lerp(0.08, 1, easeOutCubic(normalized));
}

function zeroDroplets(): [DropletPose, DropletPose, DropletPose] {
  return [
    { x: 0, y: 0, scale: 0, opacity: 0 },
    { x: 0, y: 0, scale: 0, opacity: 0 },
    { x: 0, y: 0, scale: 0, opacity: 0 },
  ];
}

function splashDroplets(t: number, spread = 1): [DropletPose, DropletPose, DropletPose] {
  const p = clamp01(t);
  if (p <= 0 || p >= 1) return zeroDroplets();
  const configs: ReadonlyArray<{ vx: number; vy: number; delay: number; size: number }> = [
    { vx: -34 * spread, vy: -30 * spread, delay: 0.0, size: 1.0 },
    { vx: 8 * spread, vy: -42 * spread, delay: 0.05, size: 0.85 },
    { vx: 38 * spread, vy: -26 * spread, delay: 0.08, size: 0.92 },
  ];
  return configs.map(({ vx, vy, delay, size }) => {
    const local = clamp01((p - delay) / (1 - delay));
    if (local <= 0 || local >= 1) {
      return { x: 0, y: 0, scale: 0, opacity: 0 };
    }
    const rise = easeOutCubic(local);
    const gravity = 28 * local * local;
    const fade = local < 0.2 ? local / 0.2 : 1 - (local - 0.2) / 0.8;
    return {
      x: vx * rise,
      y: vy * rise + gravity,
      scale: size * (1 - 0.35 * local),
      opacity: clamp01(fade) * 0.88,
    };
  }) as [DropletPose, DropletPose, DropletPose];
}

/**
 * Phase 1: Entrance ("pop out of luminous water ripple & say hello with a blink").
 */
export function evaluateEntrancePose(progress: number): WhaleFramePose {
  const t = clamp01(progress);

  // Subtle rise out of water with bouncy peak around t = 0.42, soft landing around t = 0.72:
  let offsetY = 0;
  let scaleX = 1;
  let scaleY = 1;
  let rotateDeg = 0;

  if (t < 0.44) {
    const p = t / 0.44;
    const eased = easeOutCubic(p);
    offsetY = lerp(26, -18, eased);
    scaleX = lerp(0.76, 0.96, eased);
    scaleY = lerp(1.22, 1.05, eased);
    rotateDeg = lerp(-9, -4, eased);
  } else if (t < 0.72) {
    const p = (t - 0.44) / 0.28;
    const eased = p * p;
    offsetY = lerp(-18, 2, eased);
    scaleX = lerp(0.96, 1.12, eased);
    scaleY = lerp(1.05, 0.88, eased);
    rotateDeg = lerp(-4, 2, eased);
  } else {
    const p = (t - 0.72) / 0.28;
    const eased = easeOutBack(p, 1.2);
    offsetY = lerp(2, 0, clamp01(eased));
    scaleX = lerp(1.12, 1, clamp01(eased));
    scaleY = lerp(0.88, 1, clamp01(eased));
    rotateDeg = lerp(2, 0, easeOutCubic(p));
  }

  const opacity = clamp01(t / 0.24);
  const heightNorm = clamp01(-offsetY / 40);
  const shadowOpacity = opacity * lerp(0.42, 0.2, heightNorm);
  const shadowScale = lerp(0.65, 1.0, opacity) * lerp(1.0, 0.68, heightNorm);

  // Welcoming water ripples & delicate droplets as the whale surfaces:
  const primaryRipple: RipplePose = {
    scale: lerp(0.35, 1.38, easeOutCubic(t)),
    opacity: t < 0.14 ? (t / 0.14) * 0.62 : (1 - (t - 0.14) / 0.86) * 0.62,
  };
  const secondaryT = clamp01((t - 0.12) / 0.88);
  const secondaryRipple: RipplePose = {
    scale: lerp(0.3, 1.55, easeOutCubic(secondaryT)),
    opacity: secondaryT > 0 ? Math.sin(secondaryT * Math.PI) * 0.42 : 0,
  };

  return {
    offsetX: 0,
    offsetY,
    scaleX,
    scaleY,
    facing: 1,
    rotateDeg,
    opacity,
    eyeOffsetX: 0,
    eyeOffsetY: lerp(-5, 0, easeOutCubic(t)),
    eyeScaleY: blinkScaleAt(t, 0.74, 0.96),
    shadowScale,
    shadowOpacity,
    primaryRipple,
    secondaryRipple,
    droplets: splashDroplets(clamp01(t / 0.78), 0.78),
  };
}

/**
 * Phase 2: Playful, buoyant center bounce loop while repositories load in the background.
 */
export function evaluateBouncePose(cycleProgress: number, cycleIndex: number): WhaleFramePose {
  const t = clamp01(cycleProgress);
  const swaySign = cycleIndex % 2 === 0 ? -1 : 1;

  let offsetY = 0;
  let scaleX = 1;
  let scaleY = 1;
  let rotateDeg = 0;
  let rippleOpacity = 0;
  let rippleScale = 0.5;

  if (t < 0.17) {
    // Cozy crouch anticipation:
    const p = easeInOutSine(t / 0.17);
    offsetY = lerp(0, 4.5, p);
    scaleX = lerp(1, 1.12, p);
    scaleY = lerp(1, 0.88, p);
    rotateDeg = lerp(0, 2.5 * swaySign, p);
  } else if (t < 0.54) {
    // Buoyant upward spring with floaty hang-time near the apex:
    const p = (t - 0.17) / 0.37;
    const rise = Math.sin(p * Math.PI * 0.5);
    offsetY = lerp(4.5, -38, rise);
    if (p < 0.38) {
      const stretch = easeOutCubic(p / 0.38);
      scaleX = lerp(1.12, 0.9, stretch);
      scaleY = lerp(0.88, 1.13, stretch);
      rotateDeg = lerp(2.5 * swaySign, -6.5, stretch);
    } else {
      const settle = easeInOutSine((p - 0.38) / 0.62);
      scaleX = lerp(0.9, 1.03, settle);
      scaleY = lerp(1.13, 0.97, settle);
      rotateDeg = lerp(-6.5, 2.0 * swaySign, settle);
    }
  } else if (t < 0.79) {
    // Accelerated drop into soft landing:
    const p = (t - 0.54) / 0.25;
    const fall = 1 - Math.cos(p * Math.PI * 0.5);
    offsetY = lerp(-38, 3, fall);
    if (p < 0.72) {
      const stretch = p / 0.72;
      scaleX = lerp(1.03, 0.93, stretch);
      scaleY = lerp(0.97, 1.09, stretch);
      rotateDeg = lerp(2.0 * swaySign, 4.5, stretch);
    } else {
      const impact = (p - 0.72) / 0.28;
      scaleX = lerp(0.93, 1.14, impact);
      scaleY = lerp(1.09, 0.86, impact);
      rotateDeg = lerp(4.5, 1.5, impact);
    }
  } else {
    // Soft cushioned rebound back to neutral:
    const p = (t - 0.79) / 0.21;
    const rebound = easeOutCubic(p);
    offsetY = lerp(3, 0, rebound);
    scaleX = lerp(1.14, 1, rebound);
    scaleY = lerp(0.86, 1, rebound);
    rotateDeg = lerp(1.5, 0, rebound);
    rippleScale = lerp(0.45, 1.1, rebound);
    rippleOpacity = (1 - rebound) * 0.26;
  }

  const heightNorm = clamp01(-offsetY / 40);
  const shadowScale = lerp(1.04, 0.56, heightNorm);
  const shadowOpacity = lerp(0.44, 0.16, heightNorm);

  // Cute periodic blink on rebound of every second bounce + tiny expressive gaze lift at apex:
  const blink = cycleIndex % 2 === 1 ? blinkScaleAt(t, 0.78, 0.98) : 1;
  const happyApexEye = t > 0.32 && t < 0.58 ? lerp(1, 0.88, Math.sin(((t - 0.32) / 0.26) * Math.PI)) : 1;

  return {
    offsetX: 0,
    offsetY,
    scaleX,
    scaleY,
    facing: 1,
    rotateDeg,
    opacity: 1,
    eyeOffsetX: swaySign * 3 * heightNorm,
    eyeOffsetY: -6 * heightNorm,
    eyeScaleY: Math.min(blink, happyApexEye),
    shadowScale,
    shadowOpacity,
    primaryRipple: { scale: rippleScale, opacity: rippleOpacity },
    secondaryRipple: { scale: 0.4, opacity: 0 },
    droplets: zeroDroplets(),
  };
}

/**
 * Phase 3: Aim & crouch anticipation right before jumping toward the restored Picket window.
 */
export function evaluateAimPose(progress: number, plan: SplashLaunchPlan): WhaleFramePose {
  const t = clamp01(progress);
  const targetFacing = targetFacingForDelta(plan.dx);
  // Flip smoothly during the first 65% of the crouch if jumping toward the left side of screen:
  const facingProgress = easeInOutSine(clamp01(t / 0.65));
  const facing = targetFacing === -1 ? lerp(1, -1, facingProgress) : 1;
  // Prevent zero-width flat line at exact 0 during flip by keeping minimum width factor:
  const safeFacing = Math.abs(facing) < 0.08 ? (facing >= 0 ? 0.08 : -0.08) : facing;

  const crouch = easeOutCubic(t);
  const eyeTarget = computeEyeLookTarget(plan.dx, plan.dy, targetFacing);

  return {
    offsetX: 0,
    offsetY: lerp(0, 7.5, crouch),
    scaleX: lerp(1, 1.16, crouch),
    scaleY: lerp(1, 0.83, crouch),
    facing: safeFacing,
    rotateDeg: lerp(0, -5, crouch),
    opacity: 1,
    eyeOffsetX: lerp(0, eyeTarget.x, crouch),
    eyeOffsetY: lerp(0, eyeTarget.y, crouch),
    eyeScaleY: lerp(1, 0.92, crouch),
    shadowScale: lerp(1.0, 1.12, crouch),
    shadowOpacity: lerp(0.42, 0.5, crouch),
    primaryRipple: { scale: 0.5, opacity: 0 },
    secondaryRipple: { scale: 0.5, opacity: 0 },
    droplets: zeroDroplets(),
  };
}

/**
 * Phase 4: In-flight parabolic leap pose synchronized with Rust's 120fps native window arc.
 */
export function evaluateLeapPose(progress: number, plan: SplashLaunchPlan): WhaleFramePose {
  const t = clamp01(progress);
  const facing = targetFacingForDelta(plan.dx);
  const inPlace = plan.distance < IN_PLACE_DISTANCE_THRESHOLD_PX;
  const eyeTarget = computeEyeLookTarget(plan.dx, plan.dy, facing);

  let scaleX = 1;
  let scaleY = 1;
  let rotateDeg = 0;

  if (t < 0.24) {
    // Energetic takeoff stretch:
    const p = easeOutCubic(t / 0.24);
    scaleX = lerp(1.16, 0.87, p);
    scaleY = lerp(0.83, 1.16, p);
    rotateDeg = lerp(-5, inPlace ? -9 : -17, p);
  } else if (t < 0.68) {
    // Graceful arcing glide over the apex:
    const p = easeInOutSine((t - 0.24) / 0.44);
    scaleX = lerp(0.87, 1.04, p);
    scaleY = lerp(1.16, 0.96, p);
    rotateDeg = lerp(inPlace ? -9 : -17, inPlace ? 6 : 9, p);
  } else {
    // Sleek dive tucked toward the target window:
    const p = easeInOutSine((t - 0.68) / 0.32);
    scaleX = lerp(1.04, 0.88, p);
    scaleY = lerp(0.96, 1.06, p);
    rotateDeg = lerp(inPlace ? 6 : 9, inPlace ? 10 : 19, p);
  }

  // Gently scale down slightly as it dives into the target window:
  const diveScale = t > 0.72 ? lerp(1, 0.86, easeInOutSine((t - 0.72) / 0.28)) : 1;
  // Cheerful squint near peak of the jump:
  const apexHappy = Math.sin(t * Math.PI);
  const eyeScaleY = lerp(1, 0.76, apexHappy * apexHappy);

  return {
    offsetX: 0,
    offsetY: 0,
    scaleX: scaleX * diveScale,
    scaleY: scaleY * diveScale,
    facing,
    rotateDeg,
    opacity: 1,
    eyeOffsetX: eyeTarget.x,
    eyeOffsetY: lerp(eyeTarget.y - 4, eyeTarget.y + 4, t),
    eyeScaleY,
    // Ground shadow vanishes as the whale leaps across the desktop:
    shadowScale: lerp(1.12, 0.3, clamp01(t / 0.25)),
    shadowOpacity: lerp(0.46, 0, clamp01(t / 0.22)),
    primaryRipple: { scale: 0.5, opacity: 0 },
    secondaryRipple: { scale: 0.5, opacity: 0 },
    droplets: zeroDroplets(),
  };
}

/**
 * Phase 5: Landing splash & melt-away right over the smoothly fading-in Picket window.
 */
export function evaluateLandingPose(progress: number, plan: SplashLaunchPlan): WhaleFramePose {
  const t = clamp01(progress);
  const facing = targetFacingForDelta(plan.dx);
  const ease = easeOutCubic(t);

  // Whale squashes into the spot and gently melts downward as the Picket window blooms behind it:
  const whaleFade = clamp01(1 - t / 0.68);
  const scaleX = lerp(0.96, 1.18, easeOutCubic(clamp01(t / 0.45))) * lerp(1, 0.78, ease);
  const scaleY = lerp(0.92, 0.62, ease);

  const primaryRipple: RipplePose = {
    scale: lerp(0.28, 1.62, ease),
    opacity: t < 0.16 ? (t / 0.16) * 0.78 : (1 - (t - 0.16) / 0.84) * 0.78,
  };
  const secondaryT = clamp01((t - 0.1) / 0.9);
  const secondaryRipple: RipplePose = {
    scale: lerp(0.24, 1.85, easeOutCubic(secondaryT)),
    opacity: secondaryT > 0 ? Math.sin(secondaryT * Math.PI) * 0.58 : 0,
  };

  return {
    offsetX: 0,
    offsetY: lerp(0, 12, ease),
    scaleX,
    scaleY,
    facing,
    rotateDeg: lerp(8, 0, ease),
    opacity: whaleFade,
    eyeOffsetX: 0,
    eyeOffsetY: 4 * ease,
    eyeScaleY: lerp(0.75, 0.12, ease),
    shadowScale: lerp(0.85, 1.3, ease),
    shadowOpacity: whaleFade * 0.22,
    primaryRipple,
    secondaryRipple,
    droplets: splashDroplets(t, 1.05),
  };
}
