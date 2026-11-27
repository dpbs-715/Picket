import type { ResolvedTheme } from '../../../shared/theme-protocol';
import {
  SPLASH_TIMING,
  evaluateAimPose,
  evaluateBouncePose,
  evaluateEntrancePose,
  evaluateLandingPose,
  evaluateLeapPose,
  type SplashLaunchPlan,
  type SplashPhase,
  type WhaleFramePose,
} from './whaleMath';

export interface SplashThemePalette {
  bodyStops: readonly [string, string, string];
  shadowColor: string;
  shadowOpacities: readonly [string, string, string];
}

export const SPLASH_THEME_PALETTES: Record<ResolvedTheme, SplashThemePalette> = {
  dark: {
    bodyStops: ['#FFFFFF', '#F1F5FF', '#DCE6FA'],
    shadowColor: '#0F172A',
    shadowOpacities: ['0.52', '0.22', '0'],
  },
  light: {
    bodyStops: ['#5B7DB1', '#3B5988', '#253D63'],
    shadowColor: '#1E3A5F',
    shadowOpacities: ['0.34', '0.14', '0'],
  },
};

export function resolveSplashTheme(theme: unknown): ResolvedTheme {
  return theme === 'light' ? 'light' : 'dark';
}

/** Exact vector subpaths extracted from `src-tauri/icons/icon.svg` (1024x1024 space). */
const WHALE_BODY_PATH =
  'M 320.25 694.63 C 302.67 694.59, 285.47 684.78, 280.67 666.99 C 278.10 657.45, 278.62 646.66, 279.24 636.89 C 281.05 608.41, 289.41 585.58, 304.64 561.58 C 318.29 540.08, 338.97 516.80, 338.12 489.68 C 337.60 473.21, 326.79 460.86, 315.77 449.74 C 304.34 438.22, 292.07 427.54, 280.18 416.50 C 272.02 408.93, 263.52 401.36, 257.07 392.21 C 246.62 377.40, 240.71 347.74, 251.06 331.65 C 261.48 315.46, 287.55 319.83, 304.24 319.78 C 312.31 319.76, 321.97 319.14, 327.76 325.91 C 335.71 335.18, 334.08 350.12, 336.41 361.37 C 339.87 378.01, 361.16 407.65, 379.43 408.99 C 400.91 410.56, 422.39 378.99, 427.08 360.59 C 429.82 349.83, 430.15 336.17, 437.09 327.06 C 439.52 323.87, 442.77 321.68, 446.61 320.55 C 452.48 318.82, 458.97 319.48, 465.00 319.47 C 473.96 319.46, 482.94 319.33, 491.90 319.53 C 515.23 320.07, 522.91 339.39, 519.78 359.97 C 516.24 383.22, 496.56 401.34, 482.27 418.62 C 476.99 425.01, 471.28 431.57, 467.56 439.04 C 466.34 441.50, 464.50 446.14, 468.06 447.66 C 472.59 449.61, 482.80 443.78, 487.07 441.84 C 502.98 434.61, 518.72 427.93, 535.90 424.33 C 579.21 415.26, 630.47 421.85, 670.53 440.50 C 740.53 473.09, 773.04 536.22, 781.60 610.59 C 783.44 626.63, 786.77 655.76, 781.07 670.65 C 777.49 680.03, 769.51 687.75, 760.22 691.41 C 747.99 696.22, 728.48 694.16, 715.19 694.13 C 685.50 694.07, 655.80 694.25, 626.11 694.25 C 524.16 694.25, 422.20 694.85, 320.25 694.63 Z';

const WHALE_LEFT_EYE_PATH =
  'M 599.46 587.74 C 606.72 587.70, 613.64 584.39, 617.06 577.74 C 620.71 570.64, 619.60 562.01, 619.58 554.27 C 619.57 548.41, 619.62 542.55, 619.61 536.68 C 619.61 532.58, 620.05 528.18, 619.30 524.14 C 617.52 514.47, 609.07 508.33, 599.50 508.27 C 591.68 508.22, 584.71 512.40, 581.39 519.50 C 578.25 526.20, 579.46 534.64, 579.44 541.84 C 579.43 547.70, 579.42 553.56, 579.45 559.43 C 579.47 563.57, 579.05 567.88, 579.75 571.97 C 581.42 581.74, 589.81 587.79, 599.46 587.74 Z';

const WHALE_RIGHT_EYE_PATH =
  'M 673.79 593.76 C 680.25 593.67, 686.15 590.49, 689.55 584.97 C 693.06 579.29, 692.89 571.93, 693.97 565.51 C 694.81 560.55, 695.54 555.57, 696.17 550.58 C 696.64 546.85, 697.39 543.09, 697.27 539.31 C 697.02 530.97, 691.35 524.17, 683.40 521.89 C 680.99 521.19, 678.41 520.98, 675.91 521.06 C 668.21 521.34, 661.40 526.63, 658.96 533.93 C 657.41 538.58, 657.17 543.96, 656.40 548.79 C 655.61 553.76, 654.95 558.75, 654.27 563.74 C 652.58 576.10, 651.43 588.29, 666.34 592.73 C 668.76 593.45, 671.27 593.79, 673.79 593.76 Z';

const SVG_NS = 'http://www.w3.org/2000/svg';
/** Anchors whale bottom baseline (y = 695 in 1024 space) at (110, 168) in the 220x220 stage. */
const BASE_ANCHOR_X = 110;
const BASE_ANCHOR_Y = 168;
const WHALE_SVG_SCALE = 0.2;
const WHALE_LOCAL_CENTER_X = 515;
const WHALE_LOCAL_BASE_Y = 695;
const LEFT_EYE_CENTER = { x: 599.5, y: 548 };
const RIGHT_EYE_CENTER = { x: 675.2, y: 557.4 };

function svgEl<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Record<string, string> = {},
): SVGElementTagNameMap[K] {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) {
    el.setAttribute(key, value);
  }
  return el;
}

export interface WhaleSplashHooks {
  onPhaseChange?: (phase: SplashPhase) => void;
  /** Called at the exact takeoff frame of the parabolic leap. */
  onLaunchJump?: () => Promise<void> | void;
  /** Called as soon as the landing splash ripple finishes. */
  onComplete?: () => void;
}

export class WhaleSplashStage {
  readonly element: HTMLDivElement;
  private readonly whaleFillStops: [SVGStopElement, SVGStopElement, SVGStopElement];
  private readonly shadowGradStops: [SVGStopElement, SVGStopElement, SVGStopElement];
  private readonly whaleMotionGroup: SVGGElement;
  private readonly whaleFacingGroup: SVGGElement;
  private readonly leftEyeGroup: SVGGElement;
  private readonly rightEyeGroup: SVGGElement;
  private readonly shadowEl: SVGEllipseElement;
  private readonly primaryRippleEl: SVGEllipseElement;
  private readonly secondaryRippleEl: SVGEllipseElement;
  private readonly dropletEls: [SVGCircleElement, SVGCircleElement, SVGCircleElement];

  private plan: SplashLaunchPlan;
  private hooks: WhaleSplashHooks;
  private theme: ResolvedTheme = 'dark';
  private phase: SplashPhase = 'entering';
  private phaseStartMs = 0;
  private bounceCount = 0;
  private readyToLand = false;
  private jumpTriggered = false;
  private jumpArrived = false;
  private rafId: number | null = null;
  private disposed = false;

  constructor(plan: SplashLaunchPlan, hooks: WhaleSplashHooks = {}) {
    this.plan = plan;
    this.hooks = hooks;
    this.readyToLand = plan.workbenchAlreadyReady;

    const stage = document.createElement('div');
    stage.className = 'whale-stage';
    stage.setAttribute('role', 'img');
    stage.setAttribute('aria-label', 'Picket launching');

    const svg = svgEl('svg', {
      class: 'whale-svg',
      viewBox: '0 0 220 220',
      width: '220',
      height: '220',
    });

    const defs = svgEl('defs');

    const whaleFill = svgEl('linearGradient', {
      id: 'picket-whale-fill',
      x1: '0',
      y1: '310',
      x2: '0',
      y2: '705',
      gradientUnits: 'userSpaceOnUse',
    });
    const fillStop0 = svgEl('stop', { offset: '0%' });
    const fillStop1 = svgEl('stop', { offset: '62%' });
    const fillStop2 = svgEl('stop', { offset: '100%' });
    this.whaleFillStops = [fillStop0, fillStop1, fillStop2];
    whaleFill.append(fillStop0, fillStop1, fillStop2);

    const shadowGrad = svgEl('radialGradient', {
      id: 'picket-whale-shadow',
      cx: '50%',
      cy: '50%',
      r: '50%',
    });
    const shadowStop0 = svgEl('stop', { offset: '0%' });
    const shadowStop1 = svgEl('stop', { offset: '55%' });
    const shadowStop2 = svgEl('stop', { offset: '100%' });
    this.shadowGradStops = [shadowStop0, shadowStop1, shadowStop2];
    shadowGrad.append(shadowStop0, shadowStop1, shadowStop2);

    defs.append(whaleFill, shadowGrad);
    svg.append(defs);

    // Water ripples & ground shadow live underneath the whale at baseline (110, 170):
    this.primaryRippleEl = svgEl('ellipse', {
      class: 'whale-ripple-primary',
      cx: String(BASE_ANCHOR_X),
      cy: String(BASE_ANCHOR_Y + 2),
      rx: '44',
      ry: '13',
      opacity: '0',
    });
    this.secondaryRippleEl = svgEl('ellipse', {
      class: 'whale-ripple-secondary',
      cx: String(BASE_ANCHOR_X),
      cy: String(BASE_ANCHOR_Y + 2),
      rx: '44',
      ry: '13',
      opacity: '0',
    });
    this.shadowEl = svgEl('ellipse', {
      class: 'whale-ground-shadow',
      cx: String(BASE_ANCHOR_X),
      cy: String(BASE_ANCHOR_Y + 5),
      rx: '46',
      ry: '10',
      opacity: '0',
    });

    // Motion group translates & applies bottom-anchored squash/stretch + pitch:
    this.whaleMotionGroup = svgEl('g', { class: 'whale-body-shadow' });
    this.whaleFacingGroup = svgEl('g');

    // Inner unit group maps 1024-space whale so its bottom-center (515, 695) sits at local (0, 0):
    const iconSpaceGroup = svgEl('g', {
      transform: `scale(${WHALE_SVG_SCALE}) translate(${-WHALE_LOCAL_CENTER_X} ${-WHALE_LOCAL_BASE_Y})`,
    });
    const bodyPath = svgEl('path', {
      class: 'whale-silhouette',
      d: WHALE_BODY_PATH,
    });

    this.leftEyeGroup = svgEl('g');
    this.leftEyeGroup.append(svgEl('path', { class: 'whale-eye', d: WHALE_LEFT_EYE_PATH }));
    this.rightEyeGroup = svgEl('g');
    this.rightEyeGroup.append(svgEl('path', { class: 'whale-eye', d: WHALE_RIGHT_EYE_PATH }));

    iconSpaceGroup.append(bodyPath, this.leftEyeGroup, this.rightEyeGroup);
    this.whaleFacingGroup.append(iconSpaceGroup);
    this.whaleMotionGroup.append(this.whaleFacingGroup);

    const drop0 = svgEl('circle', { class: 'whale-droplet', cx: String(BASE_ANCHOR_X), cy: String(BASE_ANCHOR_Y - 12), r: '3.2', opacity: '0' });
    const drop1 = svgEl('circle', { class: 'whale-droplet', cx: String(BASE_ANCHOR_X), cy: String(BASE_ANCHOR_Y - 12), r: '2.6', opacity: '0' });
    const drop2 = svgEl('circle', { class: 'whale-droplet', cx: String(BASE_ANCHOR_X), cy: String(BASE_ANCHOR_Y - 12), r: '3.0', opacity: '0' });
    this.dropletEls = [drop0, drop1, drop2];

    svg.append(
      this.secondaryRippleEl,
      this.primaryRippleEl,
      this.shadowEl,
      this.whaleMotionGroup,
      drop0,
      drop1,
      drop2,
    );
    stage.append(svg);
    this.element = stage;

    this.setTheme(resolveSplashTheme(plan.theme));
    this.applyPose(evaluateEntrancePose(0));
  }

  getTheme(): ResolvedTheme {
    return this.theme;
  }

  setTheme(theme: ResolvedTheme): void {
    this.theme = theme;
    this.element.dataset.theme = theme;
    const palette = SPLASH_THEME_PALETTES[theme];
    for (let i = 0; i < 3; i += 1) {
      this.whaleFillStops[i].setAttribute('stop-color', palette.bodyStops[i]);
      this.shadowGradStops[i].setAttribute('stop-color', palette.shadowColor);
      this.shadowGradStops[i].setAttribute('stop-opacity', palette.shadowOpacities[i]);
    }
  }

  start(): void {
    if (this.disposed) return;
    this.phase = 'entering';
    this.bounceCount = 0;
    this.jumpTriggered = false;
    this.jumpArrived = false;
    this.phaseStartMs = performance.now();
    this.hooks.onPhaseChange?.(this.phase);
    this.scheduleTick();
  }

  markWorkbenchReady(): void {
    this.readyToLand = true;
  }

  updatePlan(nextPlan: SplashLaunchPlan): void {
    this.plan = nextPlan;
    this.readyToLand = nextPlan.workbenchAlreadyReady;
    if (nextPlan.theme === 'dark' || nextPlan.theme === 'light') {
      this.setTheme(nextPlan.theme);
    }
  }

  /** Allows interactive preview tooling to jump straight to a phase or replay cleanly. */
  restart(nextPlan?: SplashLaunchPlan): void {
    if (nextPlan) {
      this.plan = nextPlan;
      if (nextPlan.theme === 'dark' || nextPlan.theme === 'light') {
        this.setTheme(nextPlan.theme);
      }
    }
    this.readyToLand = this.plan.workbenchAlreadyReady;
    this.stop();
    this.start();
  }

  stop(): void {
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
  }

  dispose(): void {
    this.disposed = true;
    this.stop();
  }

  private setPhase(next: SplashPhase, now: number): void {
    this.phase = next;
    this.phaseStartMs = now;
    this.hooks.onPhaseChange?.(next);
  }

  private scheduleTick(): void {
    if (this.disposed) return;
    this.rafId = requestAnimationFrame((now) => this.tick(now));
  }

  private tick(now: number): void {
    if (this.disposed) return;
    const elapsed = Math.max(0, now - this.phaseStartMs);

    if (this.phase === 'entering') {
      const t = elapsed / SPLASH_TIMING.entranceMs;
      if (t >= 1) {
        this.applyPose(evaluateEntrancePose(1));
        this.setPhase('bouncing', now);
      } else {
        this.applyPose(evaluateEntrancePose(t));
      }
      this.scheduleTick();
      return;
    }

    if (this.phase === 'bouncing') {
      const t = elapsed / SPLASH_TIMING.bounceCycleMs;
      if (t >= 1) {
        this.bounceCount += 1;
        if (this.readyToLand && this.bounceCount >= SPLASH_TIMING.minCompletedBounces) {
          this.applyPose(evaluateBouncePose(1, this.bounceCount - 1));
          this.setPhase('aiming', now);
        } else {
          // Carry sub-frame remainder seamlessly into the next bounce cycle:
          const overshootMs = elapsed - SPLASH_TIMING.bounceCycleMs;
          this.phaseStartMs = now - (overshootMs % SPLASH_TIMING.bounceCycleMs);
          this.applyPose(
            evaluateBouncePose(
              (now - this.phaseStartMs) / SPLASH_TIMING.bounceCycleMs,
              this.bounceCount,
            ),
          );
        }
      } else {
        this.applyPose(evaluateBouncePose(t, this.bounceCount));
      }
      this.scheduleTick();
      return;
    }

    if (this.phase === 'aiming') {
      const t = elapsed / SPLASH_TIMING.aimMs;
      if (t >= 1) {
        this.applyPose(evaluateAimPose(1, this.plan));
        this.setPhase('leaping', now);
        if (!this.jumpTriggered) {
          this.jumpTriggered = true;
          const jumpTask = this.hooks.onLaunchJump?.();
          if (jumpTask && typeof (jumpTask as Promise<void>).then === 'function') {
            void (jumpTask as Promise<void>)
              .catch(() => {})
              .finally(() => {
                this.jumpArrived = true;
              });
          } else {
            this.jumpArrived = true;
          }
        }
      } else {
        this.applyPose(evaluateAimPose(t, this.plan));
      }
      this.scheduleTick();
      return;
    }

    if (this.phase === 'leaping') {
      const duration = Math.max(120, this.plan.leapDurationMs);
      const t = elapsed / duration;
      const maxWaitElapsed = elapsed >= duration + 140;
      if (t >= 1 && (this.jumpArrived || maxWaitElapsed)) {
        this.applyPose(evaluateLeapPose(1, this.plan));
        this.setPhase('landing', now);
      } else {
        this.applyPose(evaluateLeapPose(Math.min(1, t), this.plan));
      }
      this.scheduleTick();
      return;
    }

    if (this.phase === 'landing') {
      const t = elapsed / SPLASH_TIMING.landingMs;
      if (t >= 1) {
        this.applyPose(evaluateLandingPose(1, this.plan));
        this.setPhase('done', now);
        this.rafId = null;
        this.hooks.onComplete?.();
        return;
      }
      this.applyPose(evaluateLandingPose(t, this.plan));
      this.scheduleTick();
    }
  }

  private applyPose(pose: WhaleFramePose): void {
    const tx = BASE_ANCHOR_X + pose.offsetX;
    const ty = BASE_ANCHOR_Y + pose.offsetY;
    this.whaleMotionGroup.setAttribute(
      'transform',
      `translate(${tx.toFixed(2)} ${ty.toFixed(2)})`,
    );
    this.whaleMotionGroup.setAttribute('opacity', pose.opacity.toFixed(3));

    // Apply facing flip first, then rotation in facing space, then bottom-anchored squash/stretch:
    this.whaleFacingGroup.setAttribute(
      'transform',
      `scale(${pose.facing.toFixed(3)} 1) rotate(${pose.rotateDeg.toFixed(2)} 0 -36) scale(${pose.scaleX.toFixed(3)} ${pose.scaleY.toFixed(3)})`,
    );

    this.leftEyeGroup.setAttribute(
      'transform',
      `translate(${(LEFT_EYE_CENTER.x + pose.eyeOffsetX).toFixed(2)} ${(LEFT_EYE_CENTER.y + pose.eyeOffsetY).toFixed(2)}) scale(1 ${pose.eyeScaleY.toFixed(3)}) translate(${(-LEFT_EYE_CENTER.x).toFixed(2)} ${(-LEFT_EYE_CENTER.y).toFixed(2)})`,
    );
    this.rightEyeGroup.setAttribute(
      'transform',
      `translate(${(RIGHT_EYE_CENTER.x + pose.eyeOffsetX).toFixed(2)} ${(RIGHT_EYE_CENTER.y + pose.eyeOffsetY).toFixed(2)}) scale(1 ${pose.eyeScaleY.toFixed(3)}) translate(${(-RIGHT_EYE_CENTER.x).toFixed(2)} ${(-RIGHT_EYE_CENTER.y).toFixed(2)})`,
    );

    this.shadowEl.setAttribute('rx', (46 * pose.shadowScale).toFixed(2));
    this.shadowEl.setAttribute('ry', (10 * Math.max(0.55, pose.shadowScale)).toFixed(2));
    this.shadowEl.setAttribute('opacity', pose.shadowOpacity.toFixed(3));

    this.primaryRippleEl.setAttribute('rx', (44 * pose.primaryRipple.scale).toFixed(2));
    this.primaryRippleEl.setAttribute('ry', (13 * pose.primaryRipple.scale).toFixed(2));
    this.primaryRippleEl.setAttribute('opacity', pose.primaryRipple.opacity.toFixed(3));

    this.secondaryRippleEl.setAttribute('rx', (44 * pose.secondaryRipple.scale).toFixed(2));
    this.secondaryRippleEl.setAttribute('ry', (13 * pose.secondaryRipple.scale).toFixed(2));
    this.secondaryRippleEl.setAttribute('opacity', pose.secondaryRipple.opacity.toFixed(3));

    for (let i = 0; i < 3; i += 1) {
      const drop = pose.droplets[i];
      const el = this.dropletEls[i];
      el.setAttribute('cx', (BASE_ANCHOR_X + drop.x).toFixed(2));
      el.setAttribute('cy', (BASE_ANCHOR_Y - 12 + drop.y).toFixed(2));
      el.setAttribute('r', (3.1 * drop.scale).toFixed(2));
      el.setAttribute('opacity', drop.opacity.toFixed(3));
    }
  }
}
