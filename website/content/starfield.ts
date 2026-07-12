// The same starfield as assets/generate-banner.py (seed 42, 55 stars), converted from that
// script's 1200x260 banner canvas into percentage coordinates so it can stretch to fill any
// container here. Keeping the exact same seed/positions is deliberate: this is the same
// intergalactic backdrop as the README banner, not a fresh random field.
export const stars = [
  { x: 63.71, y: 5.42, r: 1.0, o: 0.3 },
  { x: 14.56, y: 12.69, r: 1.6, o: 0.69 },
  { x: 9.38, y: 42.65, r: 0.6, o: 0.21 },
  { x: 23.71, y: 59.58, r: 1.4, o: 0.27 },
  { x: 64.74, y: 54.23, r: 0.8, o: 0.42 },
  { x: 28.19, y: 84.65, r: 1.9, o: 0.63 },
  { x: 69.48, y: 35.0, r: 0.8, o: 0.28 },
  { x: 75.91, y: 12.65, r: 1.2, o: 0.21 },
  { x: 84.17, y: 59.73, r: 1.9, o: 0.18 },
  { x: 46.01, y: 14.81, r: 1.2, o: 0.2 },
  { x: 29.67, y: 62.08, r: 1.9, o: 0.37 },
  { x: 19.74, y: 9.62, r: 1.6, o: 0.29 },
  { x: 29.29, y: 10.58, r: 0.8, o: 0.67 },
  { x: 38.21, y: 45.62, r: 1.9, o: 0.37 },
  { x: 37.23, y: 22.73, r: 1.0, o: 0.57 },
  { x: 68.05, y: 9.77, r: 1.6, o: 0.25 },
  { x: 72.53, y: 18.42, r: 1.2, o: 0.31 },
  { x: 91.83, y: 67.65, r: 0.8, o: 0.56 },
  { x: 83.72, y: 75.88, r: 0.8, o: 0.64 },
  { x: 80.0, y: 40.73, r: 0.6, o: 0.28 },
  { x: 93.55, y: 85.31, r: 1.0, o: 0.28 },
  { x: 49.93, y: 86.12, r: 1.6, o: 0.43 },
  { x: 26.88, y: 26.23, r: 1.4, o: 0.47 },
  { x: 74.29, y: 43.27, r: 1.4, o: 0.39 },
  { x: 22.4, y: 96.69, r: 1.4, o: 0.45 },
  { x: 75.15, y: 83.88, r: 0.8, o: 0.53 },
  { x: 78.72, y: 42.69, r: 0.6, o: 0.38 },
  { x: 59.43, y: 47.0, r: 1.0, o: 0.73 },
  { x: 85.47, y: 4.15, r: 1.6, o: 0.22 },
  { x: 87.84, y: 73.54, r: 1.9, o: 0.53 },
  { x: 11.8, y: 43.88, r: 1.2, o: 0.15 },
  { x: 71.84, y: 70.62, r: 1.4, o: 0.61 },
  { x: 50.76, y: 13.08, r: 1.6, o: 0.33 },
  { x: 63.67, y: 60.23, r: 0.8, o: 0.37 },
  { x: 16.72, y: 92.58, r: 1.4, o: 0.7 },
  { x: 59.73, y: 48.92, r: 0.6, o: 0.71 },
  { x: 87.24, y: 81.12, r: 1.0, o: 0.29 },
  { x: 24.52, y: 56.31, r: 0.6, o: 0.2 },
  { x: 48.62, y: 9.58, r: 1.9, o: 0.47 },
  { x: 13.2, y: 65.0, r: 1.4, o: 0.25 },
  { x: 52.73, y: 60.0, r: 0.8, o: 0.71 },
  { x: 75.1, y: 67.81, r: 1.6, o: 0.34 },
  { x: 98.69, y: 64.08, r: 1.2, o: 0.69 },
  { x: 45.23, y: 26.35, r: 0.6, o: 0.35 },
  { x: 58.68, y: 24.65, r: 0.8, o: 0.15 },
  { x: 70.44, y: 8.62, r: 0.6, o: 0.69 },
  { x: 85.37, y: 9.73, r: 0.8, o: 0.32 },
  { x: 48.57, y: 53.69, r: 1.6, o: 0.71 },
  { x: 56.98, y: 47.42, r: 1.9, o: 0.43 },
  { x: 40.86, y: 11.92, r: 1.6, o: 0.41 },
  { x: 42.48, y: 46.92, r: 1.6, o: 0.18 },
  { x: 65.09, y: 63.73, r: 0.6, o: 0.39 },
  { x: 34.2, y: 83.92, r: 0.8, o: 0.26 },
  { x: 53.57, y: 16.23, r: 0.8, o: 0.32 },
  { x: 25.4, y: 89.73, r: 1.2, o: 0.63 },
] as const;

// The banner's hand-placed brighter stars, same positions, converted the same way.
export const twinkleStars = [
  { x: 17.08, y: 21.15, r: 1.5, dur: "3.2s", delay: "0s" },
  { x: 88.33, y: 30.77, r: 1.6, dur: "4.1s", delay: "0.6s" },
  { x: 83.33, y: 23.08, r: 1.3, dur: "2.7s", delay: "1.1s" },
  { x: 11.67, y: 76.92, r: 1.4, dur: "3.6s", delay: "1.7s" },
] as const;

const STAR_RADII = [0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 1.9];

// A small seeded PRNG (mulberry32) so every other section gets its own fixed starfield --
// different from the hero's and from each other, but stable across rebuilds instead of
// reshuffling on every deploy.
function mulberry32(seed: number) {
  return function random() {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hashSeed(input: string) {
  let hash = 0;
  for (let i = 0; i < input.length; i++) {
    hash = (Math.imul(hash, 31) + input.charCodeAt(i)) | 0;
  }
  return hash >>> 0;
}

export function generateStars(seed: string, count = 26) {
  const random = mulberry32(hashSeed(seed));
  return Array.from({ length: count }, () => {
    // Most of the width is covered by the card column, so a star placed anywhere across the
    // full 0-100% range mostly lands underneath one and never shows. Weight most stars into
    // the margins (still visible the whole way down) and leave the rest fully scattered, so
    // they still show up in the thin gaps between cards too.
    const inMargin = random() < 0.75;
    const onLeft = random() < 0.5;
    const x = inMargin
      ? onLeft
        ? random() * 18
        : 82 + random() * 18
      : random() * 92 + 4;
    return {
      x: Math.round(x * 100) / 100,
      y: Math.round((random() * 92 + 4) * 100) / 100,
      r: STAR_RADII[Math.floor(random() * STAR_RADII.length)],
      o: Math.round((random() * 0.55 + 0.15) * 100) / 100,
    };
  });
}

// Shooting stars for the hero. Hand-placed fixed positions made the same handful of streaks
// noticeably repeat from the same few spots once they were firing often -- generated instead,
// like every other section's starfield, so each one gets its own position and timing (the
// travel angle stays fixed in Starfield.tsx, matching every streak's flight path).
// Durations are randomized (not hand-picked coprime integers) specifically because arbitrary
// decimal periods essentially never share an exact common cycle, which is a stronger, free
// guarantee against the combined pattern ever feeling mechanical than picking coprime integers
// by hand.
//
// Plain independent random draws for x and for first-ignition time both have the same failure
// mode: with only a handful of stars, pure uniform randomness clusters more often than it
// spreads out -- and since this is a single fixed seed (not re-rolled per page load), an unlucky
// draw doesn't average out on refresh, it repeats forever. So both x and each star's first
// ignition are stratified into `count` non-overlapping bands (one star per band, jittered
// within it) instead of drawn freely across the whole range -- this guarantees the six starting
// positions stay spread across the width and the six *first* sightings land at least ~6s apart,
// while durations (and therefore every ignition after the first) stay independently random so
// the long-run pattern still doesn't lock into a visible rhythm.
export function generateShootingStars(seed: string, count = 6) {
  const random = mulberry32(hashSeed(seed));
  const xBandWidth = 80 / count;
  // Kept well under the shortest possible duration (18s) so a target ignition time can never
  // exceed a star's own cycle length -- if it did, the modulo below would wrap it around to
  // some earlier, unplanned moment, potentially colliding with a different star's own band
  // (this was a real bug: an early version let a late band exceed a short-duration star's
  // cycle, and two streaks ended up igniting within a second of each other on every load).
  // The 0.7 jitter fraction leaves a guaranteed gap between every consecutive band, too.
  const igniteBandWidth = 2.5;
  const igniteJitterFraction = 0.7;
  return Array.from({ length: count }, (_, index) => {
    const duration = Math.round((random() * 22 + 18) * 10) / 10;
    const ignitionPoint = duration * 0.96;
    const targetFirstIgnition = index * igniteBandWidth + random() * igniteBandWidth * igniteJitterFraction;
    const delayMagnitude = ((ignitionPoint - targetFirstIgnition) % duration + duration) % duration;
    return {
      x: Math.round((5 + index * xBandWidth + random() * xBandWidth) * 100) / 100,
      y: Math.round((random() * 25 + 4) * 100) / 100,
      duration: `${duration}s`,
      delay: `-${Math.round(delayMagnitude * 10) / 10}s`,
    };
  });
}
