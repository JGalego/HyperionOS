"use client";

import { createContext, useContext, useEffect, useRef, useState } from "react";

const LoaderContext = createContext({ progress: 100, done: true });

export function useLoader() {
  return useContext(LoaderContext);
}

// Deliberately a separate context from LoaderContext, not just a throttled value computed
// inside whatever consumes it -- a component that calls useLoader() directly re-renders on
// every 60fps tick regardless of what it does with the number afterward (React re-runs the
// whole function body whenever a context it subscribes to changes). Splitting it out means a
// component that only needs the throttled copy only re-renders when *this* context's value
// changes -- i.e. at THROTTLE_INTERVAL_MS, not 60fps. That's what star fields use: re-rendering
// 200+ DOM nodes (or redrawing a canvas) every single animation frame was heavy enough to
// visibly stall unrelated main-thread work -- a smooth-scroll kicked off by clicking
// "Get started" while the intro was still playing, in one observed case.
const ThrottledProgressContext = createContext(100);

export function useThrottledProgress() {
  return useContext(ThrottledProgressContext);
}

const DURATION_MS = 5000;
const THROTTLE_INTERVAL_MS = 80;

export function LoaderProvider({ children }: { children: React.ReactNode }) {
  const [progress, setProgress] = useState(0);
  const [done, setDone] = useState(false);
  const [throttledProgress, setThrottledProgress] = useState(0);
  const progressRef = useRef(0);

  useEffect(() => {
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    // A zero-length "animation" for the reduced-motion case, rather than setting state directly
    // in the effect body -- keeps every update flowing through the same rAF-scheduled callback
    // below instead of firing synchronously during the effect itself.
    const duration = reduceMotion ? 0 : DURATION_MS;

    const start = performance.now();
    let frame: number;

    const tick = (now: number) => {
      const t = duration === 0 ? 1 : Math.min((now - start) / duration, 1);
      const eased = 1 - (1 - t) ** 3; // ease-out cubic: quick start, gentle settle at 100%
      setProgress(eased * 100);

      if (t < 1) {
        frame = requestAnimationFrame(tick);
      } else {
        setDone(true);
      }
    };

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);

  useEffect(() => {
    progressRef.current = progress;
  }, [progress]);

  useEffect(() => {
    const interval = setInterval(() => {
      setThrottledProgress(progressRef.current);
      // Nothing left to poll for once the intro's actually finished.
      if (progressRef.current >= 100) clearInterval(interval);
    }, THROTTLE_INTERVAL_MS);
    return () => clearInterval(interval);
  }, []);

  return (
    <LoaderContext.Provider value={{ progress, done }}>
      <ThrottledProgressContext.Provider value={throttledProgress}>{children}</ThrottledProgressContext.Provider>
    </LoaderContext.Provider>
  );
}
