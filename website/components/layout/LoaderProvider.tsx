"use client";

import { createContext, useContext, useEffect, useState } from "react";

const LoaderContext = createContext({ progress: 100, done: true });

export function useLoader() {
  return useContext(LoaderContext);
}

const DURATION_MS = 5000;

export function LoaderProvider({ children }: { children: React.ReactNode }) {
  const [progress, setProgress] = useState(0);
  const [done, setDone] = useState(false);

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

  return <LoaderContext.Provider value={{ progress, done }}>{children}</LoaderContext.Provider>;
}
