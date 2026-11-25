import { useEffect, useRef, useState } from 'react';
import '../../splash/splash.css';
import { WhaleSplashStage } from '../../splash/WhaleSplashStage';
import { computeParabolicProgress, type SplashLaunchPlan, type SplashPhase } from '../../splash/whaleMath';

interface Preset {
  id: string;
  label: string;
  compact: boolean;
  /** Target box position inside the 960x600 simulated desktop stage. */
  winX: number;
  winY: number;
  winW: number;
  winH: number;
}

const STAGE_W = 960;
const STAGE_H = 600;
const WHALE_SIZE = 220;
const ANCHOR_X = 110;
const ANCHOR_Y = 132;

const PRESETS: Preset[] = [
  { id: 'top-right-compact', label: 'Top-Right Compact', compact: true, winX: 690, winY: 44, winW: 230, winH: 54 },
  { id: 'top-left-compact', label: 'Top-Left Compact', compact: true, winX: 44, winY: 54, winW: 230, winH: 54 },
  { id: 'left-full', label: 'Left Full Workbench', compact: false, winX: 48, winY: 96, winW: 230, winH: 404 },
  { id: 'center-first', label: 'First Launch Center', compact: false, winX: 365, winY: 96, winW: 230, winH: 404 },
];

function computePreviewPlan(preset: Preset, slowLoad: boolean): {
  plan: SplashLaunchPlan;
  startX: number;
  startY: number;
  endX: number;
  endY: number;
} {
  const startX = STAGE_W / 2 - ANCHOR_X;
  const startY = STAGE_H / 2 - ANCHOR_Y;
  const anchorX = preset.winX + preset.winW / 2;
  const anchorY = preset.compact ? preset.winY + preset.winH / 2 : preset.winY + preset.winH * 0.44;
  const endX = anchorX - ANCHOR_X;
  const endY = anchorY - ANCHOR_Y;
  const dx = endX - startX;
  const dy = endY - startY;
  const distance = Math.hypot(dx, dy);
  const rawArc = distance < 36 ? 72 : Math.min(155, Math.max(74, distance * 0.28 + 64));
  const midY = (startY + endY) * 0.5;
  const arcHeight = Math.round(Math.min(rawArc, Math.max(36, midY + 30)));
  const leapDurationMs = distance < 36 ? 440 : Math.round(Math.min(580, Math.max(440, 400 + distance * 0.16)));
  return {
    plan: {
      dx,
      dy,
      distance,
      arcHeight,
      leapDurationMs,
      compactMode: preset.compact,
      workbenchAlreadyReady: !slowLoad,
    },
    startX,
    startY,
    endX,
    endY,
  };
}

export function SplashPreview() {
  const [presetId, setPresetId] = useState(PRESETS[0].id);
  const [wallpaper, setWallpaper] = useState<'dark' | 'light'>('dark');
  const [slowLoad, setSlowLoad] = useState(false);
  const [runKey, setRunKey] = useState(0);
  const [phase, setPhase] = useState<SplashPhase>('entering');
  const [windowVisible, setWindowVisible] = useState(false);

  const whaleHostRef = useRef<HTMLDivElement>(null);
  const preset = PRESETS.find((item) => item.id === presetId) ?? PRESETS[0];

  useEffect(() => {
    const host = whaleHostRef.current;
    if (!host) return;
    host.innerHTML = '';
    setWindowVisible(false);

    const { plan, startX, startY, endX, endY } = computePreviewPlan(preset, slowLoad);
    host.style.transform = `translate(${startX}px, ${startY}px)`;

    let flightRaf: number | null = null;
    let loadTimer: number | null = null;

    const stage = new WhaleSplashStage(plan, {
      onPhaseChange: (next) => setPhase(next),
      onLaunchJump: () => new Promise<void>((resolve) => {
        const started = performance.now();
        let revealed = false;
        const step = (now: number) => {
          const t = Math.min(1, (now - started) / plan.leapDurationMs);
          const u = computeParabolicProgress(t);
          const arcLift = 4 * plan.arcHeight * t * (1 - t);
          const x = startX + (endX - startX) * u;
          const y = startY + (endY - startY) * u - arcLift;
          host.style.transform = `translate(${x.toFixed(2)}px, ${y.toFixed(2)}px)`;
          if (!revealed && t >= 0.9) {
            revealed = true;
            setWindowVisible(true);
          }
          if (t < 1) {
            flightRaf = requestAnimationFrame(step);
          } else {
            resolve();
          }
        };
        flightRaf = requestAnimationFrame(step);
      }),
      onComplete: () => {
        setWindowVisible(true);
      },
    });

    host.append(stage.element);
    stage.start();

    if (slowLoad) {
      loadTimer = window.setTimeout(() => stage.markWorkbenchReady(), 1650);
    }

    return () => {
      if (flightRaf !== null) cancelAnimationFrame(flightRaf);
      if (loadTimer !== null) window.clearTimeout(loadTimer);
      stage.dispose();
    };
  }, [preset, slowLoad, runKey]);

  const isDark = wallpaper === 'dark';

  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '14px',
        padding: '20px',
        background: '#090b0f',
        color: '#e4e4e7',
        fontFamily: 'Inter, -apple-system, BlinkMacSystemFont, sans-serif',
      }}
    >
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', alignItems: 'center' }}>
        {PRESETS.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => { setPresetId(item.id); setRunKey((k) => k + 1); }}
            style={{
              padding: '6px 10px',
              borderRadius: '6px',
              border: '1px solid rgba(255,255,255,0.14)',
              background: item.id === preset.id ? '#f4f4f5' : '#151922',
              color: item.id === preset.id ? '#0b0d10' : '#e4e4e7',
              fontSize: '12px',
              cursor: 'pointer',
            }}
          >
            {item.label}
          </button>
        ))}
        <button
          type="button"
          onClick={() => setRunKey((k) => k + 1)}
          style={{
            padding: '6px 10px',
            borderRadius: '6px',
            border: '1px solid rgba(129,140,248,0.45)',
            background: 'rgba(129,140,248,0.16)',
            color: '#e0e7ff',
            fontSize: '12px',
            cursor: 'pointer',
          }}
        >
          Replay
        </button>
        <button
          type="button"
          onClick={() => setWallpaper((w) => (w === 'dark' ? 'light' : 'dark'))}
          style={{
            padding: '6px 10px',
            borderRadius: '6px',
            border: '1px solid rgba(255,255,255,0.14)',
            background: '#151922',
            color: '#e4e4e7',
            fontSize: '12px',
            cursor: 'pointer',
          }}
        >
          Wallpaper: {isDark ? 'Dark' : 'Light'}
        </button>
        <label style={{ display: 'inline-flex', alignItems: 'center', gap: '6px', fontSize: '12px' }}>
          <input
            type="checkbox"
            checked={slowLoad}
            onChange={(e) => { setSlowLoad(e.target.checked); setRunKey((k) => k + 1); }}
          />
          Slow repo scan (1.65s)
        </label>
        <span style={{ fontSize: '12px', color: '#a1a1aa' }}>Phase: {phase}</span>
      </div>

      <div
        style={{
          position: 'relative',
          width: `${STAGE_W}px`,
          height: `${STAGE_H}px`,
          borderRadius: '14px',
          overflow: 'hidden',
          border: '1px solid rgba(255,255,255,0.12)',
          background: isDark
            ? 'radial-gradient(circle at 25% 20%, #1d2438 0%, #0f131d 55%, #090b10 100%)'
            : 'radial-gradient(circle at 25% 20%, #f8fafc 0%, #e2e8f0 55%, #cbd5e1 100%)',
        }}
      >
        {/* Simulated target Picket window that smoothly fades in as the whale touches down */}
        <div
          style={{
            position: 'absolute',
            left: `${preset.winX}px`,
            top: `${preset.winY}px`,
            width: `${preset.winW}px`,
            height: `${preset.winH}px`,
            borderRadius: '10px',
            background: '#0b0d10',
            border: '1px solid rgba(255,255,255,0.14)',
            boxShadow: '0 18px 42px rgba(0,0,0,0.48)',
            opacity: windowVisible ? 1 : 0,
            transition: 'opacity 240ms cubic-bezier(0.22, 1, 0.36, 1)',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            padding: '10px 12px',
            color: '#e4e4e7',
            fontSize: '11px',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            <span style={{ width: '8px', height: '8px', borderRadius: '99px', background: '#ff5f57' }} />
            <span style={{ width: '8px', height: '8px', borderRadius: '99px', background: '#febc2e' }} />
            <span style={{ width: '8px', height: '8px', borderRadius: '99px', background: '#28c840' }} />
            <span style={{ marginLeft: '6px', fontWeight: 500 }}>Picket</span>
          </div>
          {!preset.compact && (
            <div style={{ color: '#71717a', fontSize: '11px' }}>Hydrated Workbench Ready</div>
          )}
        </div>

        {/* Floating transparent whale window container */}
        <div
          ref={whaleHostRef}
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            width: `${WHALE_SIZE}px`,
            height: `${WHALE_SIZE}px`,
            pointerEvents: 'none',
            zIndex: 10,
          }}
        />
      </div>
    </div>
  );
}
