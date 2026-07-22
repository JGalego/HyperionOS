"use client";

import { createContext, useContext, useEffect, useState } from "react";
import { MINIMAL_MODE_ATTRIBUTE, MINIMAL_MODE_STORAGE_KEY } from "@/lib/minimalMode";

type MinimalModeContextValue = {
  minimal: boolean;
  toggleMinimal: () => void;
};

const MinimalModeContext = createContext<MinimalModeContextValue>({
  minimal: false,
  toggleMinimal: () => {},
});

export function useMinimalMode() {
  return useContext(MinimalModeContext);
}

// Which tree is visible (the full site vs. MinimalHome) is decided purely by CSS keyed off the
// `data-minimal` attribute on <html> -- set before hydration by a blocking script in RootLayout,
// so there's no flash on load. `minimal` here starts false to match that first server-rendered
// pass, then syncs from the attribute on mount; it exists only so the toggle's icon and
// MinimalHome's own controls know which state they're in, not to decide what renders.
export function MinimalModeProvider({ children }: { children: React.ReactNode }) {
  const [minimal, setMinimal] = useState(false);

  useEffect(() => {
    // Guards against a server/client markup mismatch (the server never knows what the blocking
    // script already set); there is no external state to synchronize here, so the usual rule
    // doesn't apply.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setMinimal(document.documentElement.hasAttribute(MINIMAL_MODE_ATTRIBUTE));
  }, []);

  useEffect(() => {
    if (minimal) {
      document.documentElement.setAttribute(MINIMAL_MODE_ATTRIBUTE, "true");
    } else {
      document.documentElement.removeAttribute(MINIMAL_MODE_ATTRIBUTE);
    }
    try {
      localStorage.setItem(MINIMAL_MODE_STORAGE_KEY, String(minimal));
    } catch {
      // Private browsing / storage disabled: the toggle still works for this session.
    }
  }, [minimal]);

  return (
    <MinimalModeContext.Provider value={{ minimal, toggleMinimal: () => setMinimal((v) => !v) }}>
      {children}
    </MinimalModeContext.Provider>
  );
}
