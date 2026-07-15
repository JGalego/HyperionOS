"use client";

import { useEffect, useRef } from "react";
import { hashSeed, mulberry32 } from "@/content/starfield";
import { useLoader } from "@/components/layout/LoaderProvider";

// Loosely spectral-type-inspired: mostly blue-white and white (the hottest, most common naked-eye
// stars), a warm pale yellow band, and a rare cooler orange -- weights skew heavily toward the
// first two so the field still reads as neutral starlight, not a rainbow.
const STAR_COLORS = [
  { color: "#f5f3ee", weight: 0.42 }, // white
  { color: "#cfe0ff", weight: 0.33 }, // blue-white
  { color: "#fff4d6", weight: 0.17 }, // pale yellow
  { color: "#ffd9b3", weight: 0.08 }, // pale orange
];

function pickColor(random: () => number) {
  const roll = random();
  let cumulative = 0;
  for (const { color, weight } of STAR_COLORS) {
    cumulative += weight;
    if (roll < cumulative) return color;
  }
  return STAR_COLORS[0].color;
}

// One star per ~500px^2 of document. Scaling by area (not a flat count) keeps the field
// looking equally dense whether the page is one screen tall or ten, which a fixed count
// spread by percentage can't do -- that's what made the old fixed-count field look sparse
// on long pages. Capped so an extreme page height can't turn a single paint into jank.
const DENSITY_PER_PX2 = 1 / 500;
const MAX_STARS = 150000;

// Raising a uniform random to a power below 1 pulls its value up toward 1 -- so most stars'
// reveal points land late in the 0-100 range, with only a trickle earlier. 0.45 still let ~20%
// of a 150k-star field appear by the halfway mark, which read as "basically full" way too
// soon; this pushes the bulk of the field into roughly the last quarter of the progress.
const REVEAL_SKEW = 0.22;

type Star = {
  x: number;
  y: number;
  color: string;
  radius: number;
  alpha: number;
  haloRadius: number;
  haloAlpha: number;
  revealAt: number;
};

// Stars are sorted by revealAt ascending, so "how many are revealed at this progress" is a
// binary search and "draw the newly-revealed ones" is a slice -- neither costs anywhere near
// redrawing the whole field, which is what makes checking progress on every animation frame
// affordable even at up to 150,000 stars.
function revealedCount(stars: Star[], progress: number) {
  let lo = 0;
  let hi = stars.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (stars[mid].revealAt <= progress) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function drawNewlyRevealed(
  ctx: CanvasRenderingContext2D,
  stars: Star[],
  drawnCountRef: { current: number },
  progress: number
) {
  const target = revealedCount(stars, progress);
  if (target <= drawnCountRef.current) return;

  for (let i = drawnCountRef.current; i < target; i++) {
    const star = stars[i];

    // The rare brightest stars get a soft halo behind them (a second, much larger, much
    // dimmer circle) to suggest bloom -- real bright stars visually spill past their own
    // point size.
    if (star.haloRadius > 0) {
      ctx.globalAlpha = star.haloAlpha;
      ctx.fillStyle = star.color;
      ctx.beginPath();
      ctx.arc(star.x, star.y, star.haloRadius, 0, Math.PI * 2);
      ctx.fill();
    }

    ctx.globalAlpha = star.alpha;
    ctx.fillStyle = star.color;
    ctx.beginPath();
    ctx.arc(star.x, star.y, star.radius, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
  drawnCountRef.current = target;
}

// A plain DOM node per star (the approach the rest of Starfield.tsx uses) tops out in the
// low thousands before layout/paint cost becomes visible. Canvas has none of that per-node
// overhead, so it's the only way to render a field dense enough to actually look like
// "millions of stars" rather than a light dusting.
export function DenseStarfield({ seed = "dense-field" }: { seed?: string } = {}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const starsRef = useRef<Star[]>([]);
  const drawnCountRef = useRef(0);
  const progressRef = useRef(0);
  const { progress } = useLoader();

  // (Re)builds the star field and draws whatever's already revealed at the current progress.
  // Runs on mount and again on resize/theme-toggle -- both rare compared to the once-per-frame
  // reveal effect below, so regenerating the full field here (rather than trying to patch it)
  // stays cheap in practice.
  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    const setup = () => {
      if (!document.documentElement.classList.contains("dark")) {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        starsRef.current = [];
        drawnCountRef.current = 0;
        return;
      }

      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const width = document.documentElement.scrollWidth;
      const height = document.documentElement.scrollHeight;
      canvas.width = width * dpr;
      canvas.height = height * dpr;
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);

      const random = mulberry32(hashSeed(seed));
      const count = Math.min(Math.round(width * height * DENSITY_PER_PX2), MAX_STARS);
      const stars: Star[] = new Array(count);

      for (let i = 0; i < count; i++) {
        const x = random() * width;
        const y = random() * height;
        const color = pickColor(random);

        // Real skies have far more faint stars than bright ones (the luminosity function) --
        // cubing a uniform random skews the distribution toward 0 with a long tail reaching 1,
        // so most dots stay dim and only a rare few land bright.
        const brightness = random() ** 3;
        const radius = 0.7 + brightness * 1.5;
        const alpha = 0.1 + brightness * 0.8;
        const halo = brightness > 0.85;

        stars[i] = {
          x,
          y,
          color,
          radius,
          alpha,
          haloRadius: halo ? radius * 4 : 0,
          haloAlpha: halo ? (brightness - 0.85) * 0.4 : 0,
          revealAt: 100 * random() ** REVEAL_SKEW,
        };
      }

      stars.sort((a, b) => a.revealAt - b.revealAt);
      starsRef.current = stars;
      drawnCountRef.current = 0;
      drawNewlyRevealed(ctx, stars, drawnCountRef, progressRef.current);
    };

    setup();

    // The document only grows/shrinks when content or viewport width changes, so watching
    // body's box (not just window resize) catches page-length changes from content reflow too.
    const resizeObserver = new ResizeObserver(setup);
    resizeObserver.observe(document.body);

    // The field is dark-mode-only; redraw (or clear) when next-themes flips the class.
    const themeObserver = new MutationObserver(setup);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });

    return () => {
      resizeObserver.disconnect();
      themeObserver.disconnect();
    };
  }, [seed]);

  // Fires on every animation frame of the intro; only paints the stars that just crossed
  // their reveal threshold since the last frame, so however many frames the intro takes, this
  // stays a small, constant-ish amount of work rather than scaling with the field's size.
  useEffect(() => {
    progressRef.current = progress;
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return;
    drawNewlyRevealed(ctx, starsRef.current, drawnCountRef, progress);
  }, [progress]);

  return <canvas ref={canvasRef} aria-hidden="true" className="pointer-events-none absolute inset-0" />;
}
