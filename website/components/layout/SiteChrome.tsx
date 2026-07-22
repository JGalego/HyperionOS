"use client";

import { useMinimalMode } from "@/components/layout/MinimalModeProvider";
import { LoaderProvider } from "@/components/layout/LoaderProvider";
import { NavBar } from "@/components/layout/NavBar";
import { Footer } from "@/components/layout/Footer";
import { Starfield } from "@/components/ui/Starfield";
import { DenseStarfield } from "@/components/ui/DenseStarfield";

// Hidden outright (display: none) rather than unmounted when minimal mode is on -- see
// `.site-chrome` in globals.css. The starfields are the exception: they're still unmounted here
// so their canvas/rAF loop actually stops running behind the scenes, instead of animating
// invisibly and burning battery for no visible effect.
export function SiteChrome({ children }: { children: React.ReactNode }) {
  const { minimal } = useMinimalMode();

  return (
    <div className="site-chrome relative flex min-h-screen flex-1 flex-col">
      <LoaderProvider>
        {!minimal && (
          <>
            <DenseStarfield seed="page" />
            <Starfield seed="page" count={150} />
          </>
        )}

        <a
          href="#main"
          className="sr-only focus:not-sr-only focus:fixed focus:top-4 focus:left-4 focus:z-[100] focus:rounded-full focus:bg-accent focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-accent-fg"
        >
          Skip to content
        </a>
        <NavBar />
        <main id="main" className="relative z-10 flex-1">
          {children}
        </main>
        <Footer />
      </LoaderProvider>
    </div>
  );
}
