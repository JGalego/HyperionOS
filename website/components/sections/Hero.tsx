"use client";

import { useState } from "react";
import { hero } from "@/content/hero";
import { Button } from "@/components/ui/Button";
import { Container } from "@/components/ui/Container";
import { Starfield } from "@/components/ui/Starfield";
import { useLoader } from "@/components/layout/LoaderProvider";
import { HeroMark } from "./HeroMark";

// A full lap of the ring, in the same 0-100 units as its viewBox -- lets the dash values below
// read as "how far around" rather than raw SVG path units. Sized close to the viewBox's own
// edge (unlike a smaller radius, which leaves the ring's true stroke diameter well short of
// its box) so "100%" -- the widest the counter ever gets -- has room to sit inside it.
const RING_RADIUS = 47;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

export function Hero() {
  const { progress, done } = useLoader();
  const [revealed, setRevealed] = useState(false);

  return (
    <section
      id="top"
      aria-label="Introduction"
      className="relative flex min-h-[calc(100dvh-4rem)] items-center overflow-hidden py-14"
    >
      <Starfield />

      <Container className="relative z-10 grid place-items-center text-center">
        {/* Same grid cell as the content below, so the counter and the headline/subhead/CTAs
            crossfade in place instead of one pushing the other around as it appears. */}
        <div
          aria-hidden={done}
          className={`relative col-start-1 row-start-1 flex flex-col items-center justify-center gap-3 transition-opacity duration-300 ${
            done ? "pointer-events-none opacity-0" : "opacity-100"
          }`}
        >
          <div className="relative flex h-36 w-36 items-center justify-center sm:h-44 sm:w-44">
            {/* Rotated -90deg so the fill starts at 12 o'clock instead of SVG's default 3
                o'clock, echoing the same clockwise sweep as the swirl in the Hyperion mark
                below. Absolutely-positioned elements paint above static ones regardless of DOM
                order, so without pointer-events-none this decorative ring -- not the button
                inside it -- would be what actually receives the click. */}
            <svg
              viewBox="0 0 100 100"
              className="pointer-events-none absolute h-36 w-36 -rotate-90 sm:h-44 sm:w-44"
            >
              <circle cx="50" cy="50" r={RING_RADIUS} fill="none" stroke="var(--color-border)" strokeWidth="2" />
              <circle
                cx="50"
                cy="50"
                r={RING_RADIUS}
                fill="none"
                stroke="var(--color-accent)"
                strokeWidth="2"
                strokeLinecap="round"
                strokeDasharray={RING_CIRCUMFERENCE}
                strokeDashoffset={RING_CIRCUMFERENCE * (1 - progress / 100)}
              />
            </svg>
            {/* For whoever's wondering whether this number tracks anything real -- it doesn't.
                Click it. */}
            <button
              type="button"
              tabIndex={done ? -1 : 0}
              aria-expanded={revealed}
              title="Yes, we know."
              aria-label="Reveal what this counter actually tracks"
              onClick={() => setRevealed((v) => !v)}
              className="font-mono text-3xl tabular-nums cursor-help sm:text-4xl"
              style={{ color: `color-mix(in srgb, var(--color-accent) ${progress}%, var(--color-fg-muted))` }}
            >
              {Math.round(progress)}%
            </button>
          </div>

          {revealed && (
            <p className="max-w-72 text-center font-mono text-xs text-fg-muted">{hero.progressEasterEgg}</p>
          )}
        </div>

        <div
          className={`col-start-1 row-start-1 flex flex-col items-center transition-all duration-700 ease-out ${
            done ? "opacity-100" : "pointer-events-none translate-y-3 opacity-0"
          }`}
        >
          <HeroMark />

          <h1 className="mt-8 text-4xl font-medium tracking-tight text-balance sm:text-5xl md:text-6xl">
            {hero.headline}
          </h1>

          <p className="mt-6 inline-block rounded-lg bg-accent px-3 py-1 text-xl text-accent-fg text-pretty sm:text-2xl">
            {hero.subhead}
          </p>

          <div className="mt-8 flex flex-col gap-3 sm:flex-row">
            <Button href={hero.primaryCta.href}>{hero.primaryCta.label}</Button>
            <Button href={hero.secondaryCta.href} variant="secondary">
              {hero.secondaryCta.label}
            </Button>
          </div>
        </div>
      </Container>
    </section>
  );
}
