"use client";

import { useMemo } from "react";
import { stars as heroStars, twinkleStars, generateStars, generateShootingStars, hashSeed, mulberry32 } from "@/content/starfield";
import { useLoader } from "@/components/layout/LoaderProvider";

const shootingStars = generateShootingStars("hero-shooting");

// Same skew DenseStarfield uses: pulls most reveal points late into the 0-100 range, so only
// a trickle of stars pop in early and the rest arrive in a flood near completion.
const REVEAL_SKEW = 0.22;

function revealThresholds(length: number, seedKey: string) {
  const random = mulberry32(hashSeed(seedKey));
  return Array.from({ length }, () => 100 * random() ** REVEAL_SKEW);
}

// Dark-mode-only starfield. Plain positioned dots (not an SVG stretched to fill the box) so
// each star stays perfectly round no matter the container's aspect ratio. With no seed, this
// is the hero's own field -- the same one as the README banner. With a seed, it generates a
// distinct fixed field of `count` stars instead (e.g. the page-wide backdrop behind every card).
export function Starfield({ seed, count }: { seed?: string; count?: number } = {}) {
  const dots = seed ? generateStars(seed, count) : heroStars;
  const { progress } = useLoader();

  const dotReveals = useMemo(() => revealThresholds(dots.length, `${seed ?? "hero"}-reveal`), [dots.length, seed]);
  const twinkleReveals = useMemo(
    () => revealThresholds(twinkleStars.length, `${seed ?? "hero"}-twinkle-reveal`),
    [seed]
  );

  return (
    <div className="pointer-events-none absolute inset-0 hidden overflow-hidden dark:block" aria-hidden="true">
      {dots.map((star, index) => (
        <span
          key={index}
          className="absolute rounded-full bg-fg transition-opacity duration-150 ease-out"
          style={{
            left: `${star.x}%`,
            top: `${star.y}%`,
            width: `${star.r}px`,
            height: `${star.r}px`,
            opacity: progress >= dotReveals[index] ? star.o : 0,
          }}
        />
      ))}
      {!seed &&
        twinkleStars.map((star, index) => {
          const revealed = progress >= twinkleReveals[index];
          return (
            <span
              key={index}
              // The twinkle keyframes drive their own opacity, which wins over a plain inline
              // opacity while active -- so hiding one pre-reveal means withholding the
              // animation class itself, not fighting it with a static opacity value.
              className={`absolute rounded-full bg-fg transition-opacity duration-150 ease-out ${
                revealed ? "motion-safe:animate-twinkle" : ""
              }`}
              style={{
                left: `${star.x}%`,
                top: `${star.y}%`,
                width: `${star.r}px`,
                height: `${star.r}px`,
                animationDuration: star.dur,
                animationDelay: star.delay,
                opacity: revealed ? undefined : 0,
              }}
            />
          );
        })}
      {!seed &&
        shootingStars.map((star, index) => (
          <span
            key={index}
            className="absolute h-[1.5px] w-[92px] rounded-full motion-safe:animate-shooting-star"
            style={{
              left: `${star.x}%`,
              top: `${star.y}%`,
              rotate: "19deg",
              opacity: 0,
              background: "linear-gradient(to right, transparent, var(--color-fg) 65%, var(--color-accent-strong) 100%)",
              animationDuration: star.duration,
              animationDelay: star.delay,
            }}
          />
        ))}
    </div>
  );
}
