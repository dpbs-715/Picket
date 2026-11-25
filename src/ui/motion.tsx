import { AnimatePresence, LazyMotion, MotionConfig, domAnimation, m } from 'motion/react';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { prefersReducedMotion } from '../desktop/window';

export const MOTION_EASE = [0.22, 1, 0.36, 1] as const;

export const MOTION_DURATION = {
  fast: 0.11,
  base: 0.15,
  overlay: 0.17,
  window: 0.185,
} as const;

function isDocumentHidden(): boolean {
  return typeof document !== 'undefined' && document.visibilityState === 'hidden';
}

export function isInstantMotionEnabled(): boolean {
  if (
    typeof globalThis !== 'undefined'
    && Boolean((globalThis as { __PICKET_INSTANT_MOTION__?: boolean }).__PICKET_INSTANT_MOTION__)
  ) {
    return true;
  }
  return prefersReducedMotion() || isDocumentHidden();
}

export function useInstantMotion(): boolean {
  const [reduced, setReduced] = useState<boolean>(() => prefersReducedMotion());
  const [hidden, setHidden] = useState<boolean>(() => isDocumentHidden());

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const query = window.matchMedia('(prefers-reduced-motion: reduce)');
    const sync = () => setReduced(query.matches);
    sync();
    query.addEventListener('change', sync);
    return () => query.removeEventListener('change', sync);
  }, []);

  useEffect(() => {
    if (typeof document === 'undefined') return;
    const sync = () => setHidden(document.visibilityState === 'hidden');
    sync();
    document.addEventListener('visibilitychange', sync);
    return () => document.removeEventListener('visibilitychange', sync);
  }, []);

  if (
    typeof globalThis !== 'undefined'
    && Boolean((globalThis as { __PICKET_INSTANT_MOTION__?: boolean }).__PICKET_INSTANT_MOTION__)
  ) {
    return true;
  }
  return reduced || hidden;
}

export function MotionRoot({ children }: { children: ReactNode }) {
  const instant = useInstantMotion();
  return (
    <MotionConfig reducedMotion={instant ? 'always' : 'user'}>
      <LazyMotion features={domAnimation} strict>
        {children}
      </LazyMotion>
    </MotionConfig>
  );
}

export function MotionPresence({
  children,
  initial = true,
  mode,
}: {
  children: ReactNode;
  initial?: boolean;
  mode?: 'sync' | 'wait' | 'popLayout';
}) {
  const instant = useInstantMotion();
  if (instant) return <>{children}</>;
  return (
    <AnimatePresence initial={initial} mode={mode}>
      {children}
    </AnimatePresence>
  );
}

export function Collapse({
  open,
  children,
  className = '',
  id,
}: {
  open: boolean;
  children: ReactNode;
  className?: string;
  id?: string;
}) {
  const instant = useInstantMotion();
  const [overflowVisible, setOverflowVisible] = useState(open);

  useEffect(() => {
    if (!open || instant) {
      setOverflowVisible(open && instant);
    }
  }, [open, instant]);

  if (instant) {
    if (!open) return null;
    return <div id={id} className={className || undefined}>{children}</div>;
  }

  return (
    <AnimatePresence initial={false}>
      {open && (
        <m.div
          key="collapse-body"
          id={id}
          className={`motion-collapse ${className}`.trim()}
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: 'auto', opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{
            height: { duration: MOTION_DURATION.base, ease: MOTION_EASE },
            opacity: { duration: MOTION_DURATION.fast, ease: MOTION_EASE },
          }}
          style={{ overflow: overflowVisible ? 'visible' : 'hidden' }}
          onAnimationStart={() => setOverflowVisible(false)}
          onAnimationComplete={(definition) => {
            if (open && typeof definition === 'object' && definition !== null && 'height' in definition) {
              setOverflowVisible(true);
            }
          }}
        >
          {children}
        </m.div>
      )}
    </AnimatePresence>
  );
}

export function PresenceNotice({
  show,
  motionKey,
  children,
}: {
  show: boolean;
  motionKey: string;
  children: ReactNode;
}) {
  const instant = useInstantMotion();
  if (instant) {
    return show ? <>{children}</> : null;
  }
  return (
    <AnimatePresence initial={false}>
      {show && (
        <m.div
          key={motionKey}
          className="motion-notice-shell"
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: 'auto', opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{
            height: { duration: MOTION_DURATION.base, ease: MOTION_EASE },
            opacity: { duration: MOTION_DURATION.fast, ease: MOTION_EASE },
          }}
          style={{ overflow: 'hidden' }}
        >
          {children}
        </m.div>
      )}
    </AnimatePresence>
  );
}

export { m };
