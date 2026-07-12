"use client";

import { useEffect, useState } from "react";

/**
 * Framer Motion's own useReducedMotion() resolves matchMedia synchronously on the client's
 * first render, which diverges from the server's render for any visitor who actually has
 * reduced motion enabled: a real hydration mismatch. Deferring the read to an effect keeps
 * the first client render identical to the server's.
 */
export function useReducedMotionSafe(): boolean {
  const [reduced, setReduced] = useState(false);

  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    // Deliberately synchronous: this reads a browser-only API that doesn't exist during SSR,
    // so there is no earlier point at which the real value could be known.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setReduced(query.matches);
    const listener = (event: MediaQueryListEvent) => setReduced(event.matches);
    query.addEventListener("change", listener);
    return () => query.removeEventListener("change", listener);
  }, []);

  return reduced;
}
